//! Window manager for the in-game windows (Character, Inventory, Abilities, Journal, Loot),
//! the `Esc` priority chain, the micro-menu next to the action bar, button hover art and
//! hint tooltips, and the cursor shape over units.
//!
//! - Windows are root UI nodes carrying [`UiWindow`]. The manager owns their `Visibility`,
//!   `GlobalZIndex` and position: open them with [`WindowCommand`] (or their hotkey), never by
//!   touching `Visibility` directly. Opening a window (or clicking it) brings it to the front;
//!   a newly opened window is placed where it overlaps the open ones least, cascading if it
//!   has to; dragging a window by its [`DragHandle`] (title bar) moves it, and the spot is
//!   remembered for the session.
//! - `Esc` is resolved once per frame in `PreUpdate` ([`EscSet`]) into an [`EscAction`]; every
//!   consumer acts only on its own variant, so one press does exactly one thing:
//!   text input (chat) > end card > cancel cast > close the most recently opened window >
//!   close the dialogue > clear the target > [`EscAction::Unhandled`] (pause menu).
//!   The micro-menu's menu button produces [`EscAction::MenuButton`]. A pause menu should open
//!   when [`pause_menu_requested`] is true (use it as a run condition in `Update`), and may call
//!   [`Windows::any_open`] to decide what to show.
//!
//! Debug: `DUSK_OPEN=character,inventory,abilities,journal` opens windows (in that order) after
//! `DUSK_OPEN_AT` seconds (default 3); `DUSK_ESC=n` then presses `Esc` n times, 0.3 s apart,
//! starting at `DUSK_ESC_AT` (default 5), logging what each press did.

use crate::{
    combat_ui::{UiFont, pick_npc},
    data::GameData,
    dialogue::{Dialogue, Modal},
    net::{Net, PlayerState},
    player::{MainCamera, Player},
    ui_input::{CapturesPointer, UiInputCaptured},
    unit::{Dead, Npc, Unit},
};
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::ui::{RelativeCursorPosition, UiSystems};
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};
use dusk_formats::db::faction;
use std::collections::HashMap;

pub struct WindowsPlugin;

impl Plugin for WindowsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Windows>()
            .init_resource::<EscAction>()
            .add_message::<WindowCommand>()
            .add_systems(OnEnter(crate::state::AppState::InGame), (spawn_micro_menu, spawn_hint))
            .add_systems(
                OnEnter(crate::state::AppState::Connecting),
                (crate::state::reset::<Windows>, crate::state::reset::<EscAction>),
            )
            .add_systems(
                PreUpdate,
                (debug_esc.run_if(|| std::env::var_os("DUSK_ESC").is_some()).after(InputSystems), resolve_esc)
                    .chain()
                    .in_set(EscSet)
                    .run_if(crate::state::in_game)
                    .after(UiSystems::Focus)
                    .after(crate::dialogue::capture_keyboard),
            )
            .add_systems(
                Update,
                (hotkeys, button_commands, apply_commands, focus_and_drag, apply_layout)
                    .chain()
                    .in_set(WindowSystems)
                    .run_if(crate::state::in_game),
            )
            .add_systems(Update, (button_art, hover_tint))
            .add_systems(Update, (micro_state, hints, cursor_icon).run_if(crate::state::in_game))
            .add_systems(
                Update,
                debug_open.run_if(|| std::env::var_os("DUSK_OPEN").is_some()).run_if(crate::state::in_game),
            );
    }
}

/// `PreUpdate` set that writes [`EscAction`]; read it in `Update` (or after this set).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EscSet;

/// `Update` set that applies [`WindowCommand`]s.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowSystems;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum WindowId {
    Character,
    Inventory,
    Abilities,
    Journal,
    Loot,
}

impl WindowId {
    /// Hotkey that toggles the window.
    pub fn key(self) -> Option<KeyCode> {
        match self {
            WindowId::Character => Some(KeyCode::KeyC),
            WindowId::Inventory => Some(KeyCode::KeyI),
            WindowId::Abilities => Some(KeyCode::KeyP),
            WindowId::Journal => Some(KeyCode::KeyJ),
            WindowId::Loot => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            WindowId::Character => "Character",
            WindowId::Inventory => "Inventory",
            WindowId::Abilities => "Abilities",
            WindowId::Journal => "Journal",
            WindowId::Loot => "Loot",
        }
    }

    fn key_label(self) -> &'static str {
        match self {
            WindowId::Character => "C",
            WindowId::Inventory => "I",
            WindowId::Abilities => "P",
            WindowId::Journal => "J",
            WindowId::Loot => "",
        }
    }

    const TOGGLEABLE: [WindowId; 4] =
        [WindowId::Character, WindowId::Inventory, WindowId::Abilities, WindowId::Journal];
}

/// Root node of a managed window. Give it `Node { position_type: Absolute, width, height }` in
/// pixels; the manager sets `left`/`top`, `Visibility` and `GlobalZIndex`.
#[derive(Component)]
#[require(CapturesPointer, RelativeCursorPosition, Visibility::Hidden)]
pub struct UiWindow(pub WindowId);

/// Dragging this node (a title bar) moves its window.
#[derive(Component)]
#[require(Interaction)]
pub struct DragHandle(pub WindowId);

/// Closes its window when clicked.
#[derive(Component)]
pub struct CloseButton(pub WindowId);

#[derive(Message, Clone, Copy, Debug)]
pub enum WindowCommand {
    Open(WindowId),
    Close(WindowId),
    Toggle(WindowId),
}

/// Open windows and where they sit.
#[derive(Resource, Default)]
pub struct Windows {
    /// Open windows, back to front (the last one is on top and closes first on `Esc`).
    stack: Vec<WindowId>,
    /// Top-left of each window this session (placed on open, or dragged).
    pos: HashMap<WindowId, Vec2>,
    /// Dragged by the player: keep the spot instead of re-placing on open.
    pinned: HashMap<WindowId, bool>,
    /// (window, cursor offset from its top-left) while dragging.
    dragging: Option<(WindowId, Vec2)>,
}

impl Windows {
    pub fn is_open(&self, id: WindowId) -> bool {
        self.stack.contains(&id)
    }

    #[allow(dead_code)] // for the pause menu
    pub fn any_open(&self) -> bool {
        !self.stack.is_empty()
    }

    /// The most recently opened / focused window.
    pub fn top(&self) -> Option<WindowId> {
        self.stack.last().copied()
    }

    fn raise(&mut self, id: WindowId) {
        self.stack.retain(|w| *w != id);
        self.stack.push(id);
    }
}

/// What this frame's `Esc` press does (at most one thing). See the module docs.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EscAction {
    #[default]
    None,
    /// Cancelled the chat input.
    TextInput,
    /// Dismissed the end card.
    Card,
    CancelCast,
    CloseWindow(WindowId),
    CloseDialogue,
    ClearTarget,
    /// Nothing else wanted it: open the pause menu.
    Unhandled,
    /// The micro-menu's menu button was clicked (not a key press): open the pause menu.
    MenuButton,
}

/// Run condition for the pause menu: `Esc` was not consumed by anything in game, or the
/// micro-menu's menu button was clicked.
pub fn pause_menu_requested(esc: Res<EscAction>) -> bool {
    matches!(*esc, EscAction::Unhandled | EscAction::MenuButton)
}

#[allow(clippy::too_many_arguments)]
fn resolve_esc(
    keys: Res<ButtonInput<KeyCode>>,
    modal: Res<Modal>,
    book: Res<crate::spells_ui::Spellbook>,
    windows: Res<Windows>,
    dialogue: Res<Dialogue>,
    state: Option<Res<PlayerState>>,
    menu: Query<(&Interaction, &MicroButton), Changed<Interaction>>,
    mut esc: ResMut<EscAction>,
) {
    let mut action = EscAction::None;
    if menu.iter().any(|(i, b)| *i == Interaction::Pressed && b.0.is_none()) {
        action = EscAction::MenuButton;
    }
    if keys.just_pressed(KeyCode::Escape) {
        action = if modal.chat_typing {
            EscAction::TextInput
        } else if modal.card {
            EscAction::Card
        } else if book.is_casting() && state.as_ref().is_some_and(|s| !s.dead) {
            EscAction::CancelCast
        } else if let Some(top) = windows.top() {
            EscAction::CloseWindow(top)
        } else if dialogue.is_open() {
            EscAction::CloseDialogue
        } else if state.as_ref().is_some_and(|s| s.target.is_some()) {
            EscAction::ClearTarget
        } else {
            EscAction::Unhandled
        };
        if std::env::var_os("DUSK_ESC").is_some() {
            info!("esc: {action:?}");
        }
    }
    esc.set_if_neq(action);
}

// ---------------------------------------------------------------- commands

fn hotkeys(keys: Res<ButtonInput<KeyCode>>, captured: Res<UiInputCaptured>, mut out: MessageWriter<WindowCommand>) {
    if captured.keyboard {
        return;
    }
    for id in WindowId::TOGGLEABLE {
        if id.key().is_some_and(|k| keys.just_pressed(k)) {
            out.write(WindowCommand::Toggle(id));
        }
    }
}

fn button_commands(
    close: Query<(&Interaction, &CloseButton), Changed<Interaction>>,
    micro: Query<(&Interaction, &MicroButton), Changed<Interaction>>,
    mut out: MessageWriter<WindowCommand>,
) {
    for (i, c) in &close {
        if *i == Interaction::Pressed {
            out.write(WindowCommand::Close(c.0));
        }
    }
    for (i, b) in &micro {
        if let (Interaction::Pressed, Some(id)) = (i, b.0) {
            out.write(WindowCommand::Toggle(id));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_commands(
    mut commands: MessageReader<WindowCommand>,
    esc: Res<EscAction>,
    mut windows: ResMut<Windows>,
    roots: Query<(&UiWindow, &Node)>,
    screen: Query<&bevy::window::Window, With<PrimaryWindow>>,
    mut sfx: MessageWriter<crate::audio::PlaySfx>,
) {
    let mut cmds: Vec<WindowCommand> = commands.read().copied().collect();
    if let EscAction::CloseWindow(id) = *esc {
        cmds.push(WindowCommand::Close(id));
    }
    if cmds.is_empty() {
        return;
    }
    let screen = screen.single().map(|w| Vec2::new(w.width(), w.height())).unwrap_or(Vec2::new(1280.0, 720.0));
    let size_of = |id: WindowId| {
        roots.iter().find(|(w, _)| w.0 == id).map(|(_, n)| px_size(n)).unwrap_or(Vec2::new(300.0, 300.0))
    };
    for cmd in cmds {
        let (id, open) = match cmd {
            WindowCommand::Open(id) => (id, true),
            WindowCommand::Close(id) => (id, false),
            WindowCommand::Toggle(id) => (id, !windows.is_open(id)),
        };
        if open == windows.is_open(id) {
            if open {
                windows.raise(id);
            }
            continue;
        }
        if open {
            let size = size_of(id);
            if !windows.pinned.get(&id).copied().unwrap_or(false) {
                let others: Vec<Rect> = windows
                    .stack
                    .iter()
                    .filter_map(|w| windows.pos.get(w).map(|p| Rect::from_corners(*p, *p + size_of(*w))))
                    .collect();
                let p = place(id, size, screen, &others);
                windows.pos.insert(id, p);
            }
            windows.raise(id);
            if id != WindowId::Loot {
                sfx.write(crate::audio::PlaySfx::ui(dusk_formats::sound::builtin::WINDOW_OPEN));
            }
        } else {
            windows.stack.retain(|w| *w != id);
            if windows.dragging.is_some_and(|d| d.0 == id) {
                windows.dragging = None;
            }
            if id != WindowId::Loot {
                sfx.write(crate::audio::PlaySfx::ui(dusk_formats::sound::builtin::WINDOW_CLOSE));
            }
        }
    }
}

fn px_size(n: &Node) -> Vec2 {
    let v = |v: Val| if let Val::Px(p) = v { p } else { 300.0 };
    Vec2::new(v(n.width), v(n.height))
}

/// Preferred top-left corners (1280x720 is the reference layout: unit frames top-left,
/// minimap top-right, chat bottom-left, action bar bottom-centre).
fn candidates(id: WindowId, size: Vec2, screen: Vec2) -> Vec<Vec2> {
    let left = Vec2::new(16.0, 118.0);
    let right = Vec2::new(screen.x - 250.0 - size.x, 70.0);
    let centre = Vec2::new(((screen.x - size.x) / 2.0).round(), 96.0);
    match id {
        WindowId::Character => vec![left, centre, right],
        WindowId::Inventory => vec![right, centre, left],
        WindowId::Abilities => vec![left, right, centre],
        WindowId::Journal => vec![centre, left, right],
        WindowId::Loot => vec![Vec2::new(screen.x / 2.0 - 200.0, 130.0), Vec2::new(screen.x / 2.0 + 60.0, 130.0)],
    }
}

fn clamp_pos(p: Vec2, size: Vec2, screen: Vec2) -> Vec2 {
    Vec2::new(p.x.clamp(0.0, (screen.x - size.x).max(0.0)), p.y.clamp(0.0, (screen.y - size.y).max(0.0)))
}

/// The candidate overlapping the open windows least; if every one overlaps a lot, the best one
/// cascaded down-right past any window sitting on the same corner.
fn place(id: WindowId, size: Vec2, screen: Vec2, open: &[Rect]) -> Vec2 {
    let overlap = |p: Vec2| -> f32 {
        let r = Rect::from_corners(p, p + size);
        open.iter().map(|o| o.intersect(r)).filter(|i| !i.is_empty()).map(|i| i.width() * i.height()).sum()
    };
    let mut best = candidates(id, size, screen)
        .into_iter()
        .map(|p| clamp_pos(p, size, screen))
        .min_by(|a, b| overlap(*a).total_cmp(&overlap(*b)))
        .unwrap_or_default();
    if overlap(best) > 0.25 * size.x * size.y {
        for _ in 0..8 {
            if !open.iter().any(|o| o.min.distance(best) < 12.0) {
                break;
            }
            best = clamp_pos(best + Vec2::splat(28.0), size, screen);
        }
    }
    best
}

#[allow(clippy::too_many_arguments)]
fn focus_and_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    mut windows: ResMut<Windows>,
    screen: Query<&bevy::window::Window, With<PrimaryWindow>>,
    roots: Query<(&UiWindow, &RelativeCursorPosition, &Node)>,
    handles: Query<(&Interaction, &DragHandle)>,
) {
    let Ok(screen) = screen.single() else { return };
    let size = Vec2::new(screen.width(), screen.height());
    let Some(cursor) = screen.cursor_position() else { return };
    if mouse.just_pressed(MouseButton::Left) {
        // Topmost open window under the cursor comes to the front.
        let hit = windows
            .stack
            .iter()
            .rev()
            .copied()
            .find(|id| roots.iter().any(|(w, rel, _)| w.0 == *id && rel.cursor_over()));
        if let Some(id) = hit {
            if windows.top() != Some(id) {
                windows.raise(id);
            }
            if handles.iter().any(|(i, h)| h.0 == id && *i == Interaction::Pressed) {
                let pos = windows.pos.get(&id).copied().unwrap_or_default();
                windows.dragging = Some((id, cursor - pos));
            }
        }
    }
    if !mouse.pressed(MouseButton::Left) && windows.dragging.is_some() {
        windows.dragging = None;
    }
    if let Some((id, grab)) = windows.dragging {
        let wsize = roots.iter().find(|(w, ..)| w.0 == id).map(|(.., n)| px_size(n)).unwrap_or_default();
        let p = clamp_pos((cursor - grab).round(), wsize, size);
        if windows.pos.get(&id) != Some(&p) {
            windows.pos.insert(id, p);
            windows.pinned.insert(id, true);
        }
    }
}

fn apply_layout(
    windows: Res<Windows>,
    mut roots: Query<(&UiWindow, &mut Node, &mut Visibility, Option<&mut GlobalZIndex>, Entity)>,
    mut commands: Commands,
) {
    if !windows.is_changed() {
        return;
    }
    for (w, mut node, mut vis, z, e) in &mut roots {
        match windows.stack.iter().position(|id| *id == w.0) {
            Some(rank) => {
                vis.set_if_neq(Visibility::Visible);
                let zi = GlobalZIndex(10 + 2 * rank as i32);
                match z {
                    Some(mut z) => {
                        z.set_if_neq(zi);
                    }
                    None => {
                        commands.entity(e).insert(zi);
                    }
                }
                if let Some(p) = windows.pos.get(&w.0) {
                    let (l, t) = (Val::Px(p.x), Val::Px(p.y));
                    if node.left != l || node.top != t || node.right != Val::Auto {
                        node.left = l;
                        node.top = t;
                        node.right = Val::Auto;
                    }
                }
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

// ---------------------------------------------------------------- chrome helpers

/// Swaps the button's image between idle / hover / pressed art.
#[derive(Component, Clone)]
pub struct ButtonArt {
    pub idle: Handle<Image>,
    pub hover: Handle<Image>,
    pub press: Handle<Image>,
}

impl ButtonArt {
    /// `<base>_idle.png`, `<base>_hover.png`, `<base>_press.png`.
    pub fn load(data: &GameData, assets: &AssetServer, base: &str) -> Self {
        let l = |s: &str| data.asset_path(&format!("{base}_{s}.png")).map(|p| assets.load(p)).unwrap_or_default();
        Self { idle: l("idle"), hover: l("hover"), press: l("press") }
    }
}

/// Invisible hot spot over art with a baked-in button (tabs, "Take All"...): tints on hover.
#[derive(Component, Default)]
#[require(Button, BackgroundColor, BorderColor)]
pub struct HoverTint;

/// Background a [`HoverTint`] node returns to when not hovered (e.g. a selected row).
#[derive(Component, Clone, Copy)]
pub struct TintBase(pub Color);

/// Tooltip shown after hovering a node for a moment: title line + optional body.
#[derive(Component, Clone, Default)]
#[require(Interaction)]
pub struct Hint {
    pub title: String,
    pub body: String,
}

impl Hint {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), body: String::new() }
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }
}

pub fn abs(left: f32, top: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    }
}

/// Close box (`ui_close*`, 26 px or 22 px when `small`) at (left, top) inside a window.
pub fn spawn_close_button(
    p: &mut ChildSpawnerCommands,
    data: &GameData,
    assets: &AssetServer,
    id: WindowId,
    left: f32,
    top: f32,
    small: bool,
) {
    let (base, size) = if small { ("ui_close_small", 22.0) } else { ("ui_close", 26.0) };
    let art = ButtonArt::load(data, assets, base);
    p.spawn((
        abs(left, top, size, size),
        ImageNode::new(art.idle.clone()),
        art,
        Button,
        CapturesPointer,
        CloseButton(id),
        Hint::new("Close").with_body("Esc closes the last opened window"),
    ));
}

/// Title-bar drag area of a window (spawn it before the close button so the button stays on top).
pub fn spawn_drag_handle(p: &mut ChildSpawnerCommands, id: WindowId, left: f32, top: f32, w: f32, h: f32) {
    p.spawn((abs(left, top, w, h), DragHandle(id), CapturesPointer));
}

fn button_art(mut buttons: Query<(&Interaction, &ButtonArt, &mut ImageNode), Changed<Interaction>>) {
    for (i, art, mut img) in &mut buttons {
        let h = match i {
            Interaction::None => &art.idle,
            Interaction::Hovered => &art.hover,
            Interaction::Pressed => &art.press,
        };
        if img.image != *h {
            img.image = h.clone();
        }
    }
}

const BRONZE: Color = Color::srgb(0.62, 0.47, 0.25);
const GOLD: Color = Color::srgb(0.95, 0.78, 0.42);
const BONE: Color = Color::srgb(0.84, 0.79, 0.68);

#[allow(clippy::type_complexity)]
fn hover_tint(
    mut q: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor, &mut Node, Option<&TintBase>),
        (Changed<Interaction>, With<HoverTint>),
    >,
) {
    for (i, mut bg, mut border, mut node, base) in &mut q {
        node.border = UiRect::all(Val::Px(1.0));
        let (b, c) = match i {
            Interaction::None => (base.map_or(Color::NONE, |b| b.0), Color::NONE),
            Interaction::Hovered => (Color::srgba(0.95, 0.70, 0.35, 0.10), BRONZE.with_alpha(0.8)),
            Interaction::Pressed => (Color::srgba(0.0, 0.0, 0.0, 0.25), GOLD),
        };
        bg.0 = b;
        *border = BorderColor::all(c);
    }
}

// ---------------------------------------------------------------- micro-menu

/// Micro-menu button: toggles a window, or opens the pause menu (`None`).
#[derive(Component)]
pub struct MicroButton(pub Option<WindowId>);

#[derive(Component)]
struct MicroPlate(Option<WindowId>);

const MICRO: [(Option<WindowId>, &str); 5] = [
    (Some(WindowId::Character), "character"),
    (Some(WindowId::Inventory), "inventory"),
    (Some(WindowId::Abilities), "abilities"),
    (Some(WindowId::Journal), "journal"),
    (None, "menu"),
];

fn spawn_micro_menu(mut commands: Commands, data: Res<GameData>, assets: Res<AssetServer>, font: Res<UiFont>) {
    let img = |n: &str| data.asset_path(n).map(|p| assets.load(p)).unwrap_or_default();
    let art = ButtonArt::load(&data, &assets, "ui_micro");
    let f = TextFont { font: font.0.clone().into(), font_size: 10.0.into(), ..default() };
    let w = 6.0 + 34.0 * MICRO.len() as f32 + 4.0;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(6.0),
                bottom: Val::Px(8.0),
                width: Val::Px(w),
                height: Val::Px(44.0),
                ..default()
            },
            ImageNode::new(img("ui_micro_bar.png")),
            CapturesPointer,
        ))
        .with_children(|bar| {
            for (i, (id, name)) in MICRO.iter().enumerate() {
                let hint = match id {
                    Some(id) => Hint::new(format!("{} ({})", id.title(), id.key_label())),
                    None => Hint::new("Menu (Esc)").with_body("Options, controls and quitting"),
                };
                bar.spawn((
                    abs(6.0 + 34.0 * i as f32, 6.0, 32.0, 32.0),
                    ImageNode::new(art.idle.clone()),
                    art.clone(),
                    Button,
                    CapturesPointer,
                    MicroButton(*id),
                    MicroPlate(*id),
                    hint,
                ))
                .with_children(|b| {
                    b.spawn((abs(5.0, 4.0, 22.0, 22.0), ImageNode::new(img(&format!("ui_micro_icon_{name}.png")))));
                    let key = id.map(|i| i.key_label()).unwrap_or("Esc");
                    b.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            right: Val::Px(3.0),
                            bottom: Val::Px(0.0),
                            ..default()
                        },
                        Text::new(key),
                        f.clone(),
                        TextColor(BONE),
                        TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK },
                    ));
                });
            }
        });
}

/// Micro buttons of open windows look pressed.
fn micro_state(windows: Res<Windows>, mut plates: Query<(&MicroPlate, &Interaction, &ButtonArt, &mut ImageNode)>) {
    for (p, i, art, mut img) in &mut plates {
        let open = p.0.is_some_and(|id| windows.is_open(id));
        let h = match i {
            Interaction::Pressed => &art.press,
            Interaction::Hovered => &art.hover,
            Interaction::None if open => &art.press,
            Interaction::None => &art.idle,
        };
        if img.image != *h {
            img.image = h.clone();
        }
    }
}

// ---------------------------------------------------------------- hint tooltip

#[derive(Component)]
struct HintBox;
#[derive(Component)]
struct HintTitle;
#[derive(Component)]
struct HintBody;

/// Show a hint after hovering this long.
const HINT_DELAY: f32 = 0.3;

fn spawn_hint(mut commands: Commands, font: Res<UiFont>) {
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                max_width: Val::Px(280.0),
                border: UiRect::all(Val::Px(1.0)),
                row_gap: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.05, 0.04, 0.95)),
            BorderColor::all(Color::srgb(0.45, 0.38, 0.25)),
            GlobalZIndex(130),
            Visibility::Hidden,
            HintBox,
        ))
        .with_children(|b| {
            b.spawn((Text::new(""), f(14.0), TextColor(GOLD), HintTitle));
            b.spawn((
                Text::new(""),
                f(12.0),
                TextColor(BONE),
                Node { max_width: Val::Px(250.0), ..default() },
                HintBody,
            ));
        });
}

#[allow(clippy::type_complexity)]
fn hints(
    time: Res<Time>,
    screen: Query<&bevy::window::Window, With<PrimaryWindow>>,
    hovered: Query<(Entity, &Interaction, &Hint, &InheritedVisibility)>,
    mut tip: Query<(&mut Node, &mut Visibility, &ComputedNode), With<HintBox>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<HintTitle>>,
        Query<(&mut Text, &mut Node), (With<HintBody>, Without<HintBox>)>,
    )>,
    mut since: Local<Option<(Entity, f32)>>,
) {
    let Ok((mut node, mut vis, computed)) = tip.single_mut() else { return };
    let now = time.elapsed_secs();
    // Debug aid: `DUSK_HINT=<title>` shows that hint as if hovered (cursor at the screen centre).
    let forced = std::env::var("DUSK_HINT").ok();
    let over = hovered.iter().find(|(_, i, h, v)| {
        v.get() && (**i == Interaction::Hovered || forced.as_ref().is_some_and(|f| *f == h.title))
    });
    let Some((e, _, hint, _)) = over else {
        *since = None;
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    if since.is_none_or(|(prev, _)| prev != e) {
        *since = Some((e, now));
        vis.set_if_neq(Visibility::Hidden);
        if let Ok(mut t) = texts.p0().single_mut() {
            t.0.clone_from(&hint.title);
        }
        if let Ok((mut t, mut n)) = texts.p1().single_mut() {
            t.0.clone_from(&hint.body);
            n.display = if hint.body.is_empty() { Display::None } else { Display::Flex };
        }
        return;
    }
    if since.is_some_and(|(_, t)| now - t < HINT_DELAY) {
        return;
    }
    let Ok(screen) = screen.single() else { return };
    let centre = Vec2::new(screen.width(), screen.height()) / 2.0;
    let Some(cursor) = screen.cursor_position().or(forced.map(|_| centre)) else { return };
    let size = computed.size() * computed.inverse_scale_factor();
    let x = if cursor.x + 18.0 + size.x > screen.width() { cursor.x - size.x - 8.0 } else { cursor.x + 18.0 };
    let y = if cursor.y + 22.0 + size.y > screen.height() { cursor.y - size.y - 6.0 } else { cursor.y + 22.0 };
    node.left = Val::Px(x.max(2.0));
    node.top = Val::Px(y.max(2.0));
    vis.set_if_neq(Visibility::Visible);
}

// ---------------------------------------------------------------- cursor

/// Hand over buttons and friendly NPCs (talk), crosshair over enemies, grab over lootable
/// corpses.
#[allow(clippy::too_many_arguments)]
fn cursor_icon(
    mut commands: Commands,
    window: Query<(Entity, &bevy::window::Window), With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    captured: Res<UiInputCaptured>,
    data: Res<GameData>,
    net: Option<Res<Net>>,
    buttons: Query<(&Interaction, &InheritedVisibility), With<Button>>,
    living: Query<(Entity, &Unit, &Npc, &Transform), (Without<Dead>, Without<Player>)>,
    corpses: Query<(Entity, &Unit, &Transform), With<Dead>>,
    loot: Res<crate::items_ui::ItemsState>,
    mut current: Local<Option<SystemCursorIcon>>,
) {
    let Ok((we, w)) = window.single() else { return };
    let mut icon = SystemCursorIcon::Default;
    if captured.pointer {
        if buttons.iter().any(|(i, v)| *i != Interaction::None && v.get()) {
            icon = SystemCursorIcon::Pointer;
        }
    } else if let (Some(c), Ok((cam, cam_tf))) = (w.cursor_position(), camera.single()) {
        if let Ok(at) = cam.viewport_to_world_2d(cam_tf, c) {
            if let Some((_, npc)) = pick_npc(at, living.iter()) {
                let friendly = data.npc_templates.get(&npc.entry).is_some_and(|t| t.faction == faction::FRIENDLY);
                icon = if friendly { SystemCursorIcon::Pointer } else { SystemCursorIcon::Crosshair };
            } else {
                let over_loot = corpses.iter().any(|(e, u, _)| {
                    let feet = crate::iso::to_screen(u.pos);
                    let inside = (at.x - feet.x).abs() <= 26.0 * u.scale
                        && at.y >= feet.y - 16.0
                        && at.y <= feet.y + 40.0 * u.scale;
                    inside && net.as_ref().and_then(|n| n.entity_id(e)).is_some_and(|id| loot.is_lootable(id))
                });
                if over_loot {
                    icon = SystemCursorIcon::Grab;
                }
            }
        }
    }
    if *current != Some(icon) {
        *current = Some(icon);
        commands.entity(we).insert(CursorIcon::from(icon));
    }
}

// ---------------------------------------------------------------- debug

fn parse_window(s: &str) -> Option<WindowId> {
    Some(match s.trim() {
        "character" | "char" | "c" => WindowId::Character,
        "inventory" | "inv" | "i" => WindowId::Inventory,
        "abilities" | "book" | "p" => WindowId::Abilities,
        "journal" | "j" => WindowId::Journal,
        _ => return None,
    })
}

/// `DUSK_OPEN=character,journal`: open windows one per frame after `DUSK_OPEN_AT` seconds.
fn debug_open(time: Res<Time>, mut next: Local<usize>, mut out: MessageWriter<WindowCommand>) {
    let at = std::env::var("DUSK_OPEN_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(3.0);
    if time.elapsed_secs() < at {
        return;
    }
    let list: Vec<WindowId> =
        std::env::var("DUSK_OPEN").unwrap_or_default().split(',').filter_map(parse_window).collect();
    if let Some(id) = list.get(*next) {
        out.write(WindowCommand::Open(*id));
        *next += 1;
    }
}

/// `DUSK_ESC=n`: press `Esc` n times, 0.3 s apart, from `DUSK_ESC_AT` seconds (default 5).
fn debug_esc(time: Res<Time>, mut keys: ResMut<ButtonInput<KeyCode>>, mut pressed: Local<(u32, bool)>) {
    let n: u32 = std::env::var("DUSK_ESC").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let at: f32 = std::env::var("DUSK_ESC_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(5.0);
    if pressed.1 {
        keys.release(KeyCode::Escape);
        pressed.1 = false;
        return;
    }
    if pressed.0 < n && time.elapsed_secs() >= at + 0.3 * pressed.0 as f32 {
        keys.press(KeyCode::Escape);
        *pressed = (pressed.0 + 1, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_avoids_open_windows() {
        let screen = Vec2::new(1280.0, 720.0);
        let inv = Vec2::new(364.0, 436.0);
        let first = place(WindowId::Inventory, inv, screen, &[]);
        assert_eq!(first, Vec2::new(1280.0 - 250.0 - 364.0, 70.0));
        // Character on the left doesn't push the inventory away.
        let ch = Rect::from_corners(Vec2::new(16.0, 118.0), Vec2::new(619.0, 689.0));
        assert_eq!(place(WindowId::Inventory, inv, screen, &[ch]), first);
        // Two windows of one size never land on the same corner.
        let book = Vec2::new(474.0, 592.0);
        let a = place(WindowId::Abilities, book, screen, &[ch]);
        assert_ne!(a, Vec2::new(16.0, 118.0));
        for p in [first, a] {
            assert!(p.x >= 0.0 && p.y >= 0.0);
        }
    }
}
