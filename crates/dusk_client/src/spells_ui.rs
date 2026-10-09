//! Spell UI: action bar, cast bar, cooldowns, tooltips, the Abilities window and aura icons.
//! Art: `content/ui/` (tools/artgen/ui.py).

use crate::{
    combat_ui::{FloatKind, FloatingText, UiFont},
    data::GameData,
    net::{Net, PlayerState, SpellNet},
    ui_input::CapturesPointer,
    unit::Unit,
    windows::{self, EscAction, Hint, HoverTint, UiWindow, WindowCommand, WindowId},
};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use dusk_formats::spell::{FormulaVars, SpellTemplate, aura, effect, eval_formula, target};
use dusk_protocol::{ClientMsg, EntityId, ServerMsg, SpellId};
use std::collections::HashMap;

pub struct SpellsUiPlugin;

impl Plugin for SpellsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Spellbook>()
            .init_resource::<ActionBar>()
            .init_resource::<Held>()
            .init_resource::<BookTab>()
            .add_systems(OnEnter(crate::state::AppState::InGame), spawn_ui)
            .add_systems(
                OnEnter(crate::state::AppState::Connecting),
                (
                    crate::state::reset::<Spellbook>,
                    crate::state::reset::<ActionBar>,
                    crate::state::reset::<Held>,
                    crate::state::reset::<BookTab>,
                ),
            )
            .add_systems(
                Update,
                (
                    handle_spell_net,
                    (input, slot_clicks, book_clicks).chain(),
                    (refresh_slots, update_cooldowns, update_cast_bar, update_errors, update_tooltip, follow_held),
                    (book_tabs, rebuild_book, update_aura_rows),
                )
                    .chain()
                    .run_if(crate::state::in_game),
            )
            .add_systems(
                Update,
                autocast.run_if(|| std::env::var_os("DUSK_AUTOPLAY").is_some()).run_if(crate::state::in_game),
            )
            .add_systems(
                Update,
                open_book_once.run_if(|| std::env::var_os("DUSK_OPEN_BOOK").is_some()).run_if(crate::state::in_game),
            );
    }
}

/// Slots on the toolbar art and their hotkeys.
const SLOTS: usize = 12;
const SLOT_KEYS: [KeyCode; SLOTS] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
    KeyCode::Minus,
    KeyCode::Equal,
];
const SLOT_LABELS: [&str; SLOTS] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "="];
/// Measured from `toolbar_base.png` (861x69): 37px wells starting at x=153, stride 46, y=13.
const SLOT_X0: f32 = 153.0;
const SLOT_STRIDE: f32 = 46.0;
const SLOT_Y: f32 = 13.0;
const SLOT_SIZE: f32 = 37.0;

/// Auto attacks are driven by clicking enemies, not by the bar.
pub const AUTO_SPELLS: [SpellId; 2] = [50100, 50101];

const GOLD: Color = Color::srgb(0.95, 0.82, 0.45);
const GREY: Color = Color::srgb(0.75, 0.72, 0.66);

#[derive(Resource, Default)]
pub struct Spellbook {
    pub known: Vec<SpellId>,
    /// spell -> (start, duration) in app seconds
    cooldowns: HashMap<SpellId, (f32, f32)>,
    gcd: (f32, f32),
    /// (spell, start, duration) of our own cast
    casting: Option<(SpellId, f32, f32)>,
}

impl Spellbook {
    fn remaining(&self, spell: SpellId, now: f32) -> (f32, f32) {
        let left = |(s, d): (f32, f32)| ((s + d - now).max(0.0), d);
        let cd = self.cooldowns.get(&spell).copied().map(left).unwrap_or((0.0, 0.0));
        let gcd = left(self.gcd);
        if cd.0 >= gcd.0 { cd } else { gcd }
    }

    /// Our own cast is in progress (`Esc` cancels it).
    pub fn is_casting(&self) -> bool {
        self.casting.is_some()
    }
}

#[derive(Resource, Default)]
pub struct ActionBar {
    pub slots: [Option<SpellId>; SLOTS],
}

/// Spell picked up from the Abilities window or a slot, following the cursor.
#[derive(Resource, Default)]
struct Held(Option<SpellId>);

#[derive(Resource, Default, PartialEq, Clone, Copy)]
enum BookTab {
    #[default]
    Spells,
    Actions,
}

/// Auras on a unit, client side: (spell, expires_at, positive).
#[derive(Component, Default)]
pub struct UnitAuras(pub Vec<(SpellId, f32, bool)>);

// ---------------------------------------------------------------- markers

#[derive(Component)]
struct Slot(usize);
#[derive(Component)]
struct SlotIcon(usize);
#[derive(Component)]
struct SlotCooldown(usize);
#[derive(Component)]
struct SlotCdText(usize);
#[derive(Component)]
struct CastBar;
#[derive(Component)]
struct CastFill;
#[derive(Component)]
struct CastLabel;
#[derive(Component)]
struct ErrorText {
    shown_at: f32,
}
#[derive(Component)]
struct Tooltip;
#[derive(Component)]
struct HeldIcon;
#[derive(Component)]
struct BookList;
#[derive(Component)]
struct BookRow(SpellId);
#[derive(Component)]
struct BookTabButton(BookTab);
/// Darkens the inactive tab label / underlines the active one.
#[derive(Component)]
struct BookTabShade(BookTab);
#[derive(Component)]
struct AuraRow {
    player: bool,
}

fn img(data: &GameData, assets: &AssetServer, name: &str) -> Handle<Image> {
    data.asset_path(name).map(|p| assets.load(p)).unwrap_or_default()
}

fn icon(data: &GameData, assets: &AssetServer, spell: SpellId) -> Handle<Image> {
    data.spells.get(&(spell as i64)).map(|s| img(data, assets, &s.icon)).unwrap_or_default()
}

fn absolute(left: f32, top: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    }
}

fn spawn_ui(mut commands: Commands, data: Res<GameData>, assets: Res<AssetServer>, font: Res<UiFont>) {
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };

    // Action bar.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(0.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-430.0)),
                width: Val::Px(861.0),
                height: Val::Px(69.0),
                ..default()
            },
            ImageNode::new(img(&data, &assets, "toolbar_base.png")),
            CapturesPointer,
        ))
        .with_children(|bar| {
            for i in 0..SLOTS {
                bar.spawn((absolute(SLOT_X0 + SLOT_STRIDE * i as f32, SLOT_Y, SLOT_SIZE, SLOT_SIZE), Button, Slot(i)))
                    .with_children(|s| {
                        s.spawn((
                            absolute(0.0, 0.0, SLOT_SIZE, SLOT_SIZE),
                            ImageNode::default(),
                            Visibility::Hidden,
                            SlotIcon(i),
                        ));
                        s.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                bottom: Val::Px(0.0),
                                width: Val::Percent(100.0),
                                height: Val::Percent(0.0),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
                            SlotCooldown(i),
                        ));
                        s.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(8.0),
                                top: Val::Px(9.0),
                                ..default()
                            },
                            Text::new(""),
                            f(14.0),
                            TextColor(Color::WHITE),
                            SlotCdText(i),
                        ));
                        s.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(2.0),
                                top: Val::Px(0.0),
                                ..default()
                            },
                            Text::new(SLOT_LABELS[i]),
                            f(11.0),
                            TextColor(GREY),
                        ));
                    });
            }
        });

    // Cast bar (castbar.png 323x40, fill 276x4).
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(84.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-161.0)),
                width: Val::Px(323.0),
                height: Val::Px(40.0),
                ..default()
            },
            ImageNode::new(img(&data, &assets, "castbar.png")),
            Visibility::Hidden,
            CastBar,
        ))
        .with_children(|c| {
            c.spawn((
                absolute(36.0, 24.0, 276.0, 4.0),
                ImageNode::new(img(&data, &assets, "castbar_fill.png")),
                CastFill,
            ));
            c.spawn((absolute(6.0, 8.0, 22.0, 22.0), ImageNode::default(), CastBarIcon));
            c.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(40.0), top: Val::Px(4.0), ..default() },
                Text::new(""),
                f(14.0),
                TextColor(GOLD),
                CastLabel,
            ));
        });

    // Error line ("Not enough mana").
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(22.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::new(""),
        f(18.0),
        TextColor(Color::srgb(1.0, 0.25, 0.2)),
        TextLayout::justify(Justify::Center),
        ErrorText { shown_at: -10.0 },
    ));

    // Tooltip (filled on hover).
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(8.0)),
            width: Val::Px(290.0),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.06, 0.05, 0.04, 0.94)),
        BorderColor::all(Color::srgb(0.45, 0.38, 0.25)),
        GlobalZIndex(100),
        Visibility::Hidden,
        Tooltip,
    ));

    // Icon on the cursor while assigning a slot.
    commands.spawn((
        absolute(0.0, 0.0, 32.0, 32.0),
        ImageNode::default(),
        GlobalZIndex(110),
        Visibility::Hidden,
        HeldIcon,
    ));

    // Abilities window (abilities.png 474x592) with Spells / Actions tabs.
    commands
        .spawn((
            absolute(0.0, 0.0, 474.0, 592.0),
            ImageNode::new(img(&data, &assets, "abilities.png")),
            UiWindow(WindowId::Abilities),
        ))
        .with_children(|w| {
            windows::spawn_drag_handle(w, WindowId::Abilities, 12.0, 14.0, 404.0, 52.0);
            windows::spawn_close_button(w, &data, &assets, WindowId::Abilities, 424.0, 27.0, false);
            for (tab, cx, title) in [(BookTab::Spells, 183.0, "Spells"), (BookTab::Actions, 288.0, "Actions")] {
                w.spawn((absolute(cx - 50.0, 69.0, 100.0, 37.0), BackgroundColor(Color::NONE), BookTabShade(tab)));
                w.spawn((
                    absolute(cx - 50.0, 69.0, 100.0, 37.0),
                    HoverTint,
                    CapturesPointer,
                    BookTabButton(tab),
                    Hint::new(title),
                ));
            }
            w.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(36.0),
                    top: Val::Px(112.0),
                    width: Val::Px(401.0),
                    height: Val::Px(440.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BookList,
            ));
            w.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(36.0), top: Val::Px(560.0), ..default() },
                Text::new("Click a spell, then an action bar slot.  Right-click a slot to clear it."),
                f(11.0),
                TextColor(GREY.with_alpha(0.75)),
            ));
        });

    // Aura icon rows under the player and target frames.
    for player in [true, false] {
        let node = crate::hud::aura_row_node(player);
        commands.spawn((node, AuraRow { player }));
    }
}

#[derive(Component)]
struct CastBarIcon;

// ---------------------------------------------------------------- server events

#[allow(clippy::too_many_arguments)]
fn handle_spell_net(
    mut commands: Commands,
    mut events: MessageReader<SpellNet>,
    time: Res<Time>,
    data: Res<GameData>,
    net: Res<Net>,
    mut book: ResMut<Spellbook>,
    mut bar: ResMut<ActionBar>,
    mut errors: Query<(&mut Text, &mut ErrorText)>,
    units: Query<&Unit>,
    mut auras: Query<&mut UnitAuras>,
) {
    let now = time.elapsed_secs();
    for SpellNet(msg) in events.read() {
        match msg {
            ServerMsg::KnownSpells { spells } => {
                book.known = spells.clone();
                if bar.slots.iter().all(Option::is_none) {
                    // Default layout: Spells tab first, then actions; auto attacks stay off the bar.
                    let mut order: Vec<_> = spells.iter().copied().filter(|s| !AUTO_SPELLS.contains(s)).collect();
                    order.sort_by_key(|s| {
                        data.spells.get(&(*s as i64)).map(|t| (t.abilities_tab != 1, t.entry)).unwrap_or((true, 0))
                    });
                    for (slot, s) in bar.slots.iter_mut().zip(order) {
                        *slot = Some(s);
                    }
                }
            }
            ServerMsg::Cooldown { spell, ms, gcd_ms } => {
                book.cooldowns.insert(*spell, (now, *ms as f32 / 1000.0));
                book.gcd = (now, *gcd_ms as f32 / 1000.0);
            }
            ServerMsg::CastStart { caster, spell, cast_ms, .. } if Some(*caster) == net.my_id => {
                book.casting = Some((*spell, now, *cast_ms as f32 / 1000.0));
            }
            ServerMsg::CastEnd { caster, interrupted, .. } if Some(*caster) == net.my_id => {
                book.casting = None;
                if *interrupted {
                    show_error(&mut errors, "Interrupted", now);
                }
            }
            ServerMsg::CastError { reason } => show_error(&mut errors, reason, now),
            ServerMsg::SpellHit { target: t, amount, heal, result, .. } => {
                let Some(&e) = net.entities.get(t) else { continue };
                let Ok(u) = units.get(e) else { continue };
                let (text, kind) = if *heal {
                    (format!("+{amount}"), FloatKind::Heal)
                } else if Some(*t) == net.my_id {
                    (amount.to_string(), FloatKind::Incoming)
                } else if *result == dusk_protocol::HitResult::Crit {
                    (amount.to_string(), FloatKind::Crit)
                } else {
                    (amount.to_string(), FloatKind::Outgoing)
                };
                commands.spawn(FloatingText::bundle(text, kind, u.pos, u.height * u.scale));
            }
            ServerMsg::AuraApply { target: t, spell, duration_ms, positive, .. } => {
                let Some(&e) = net.entities.get(t) else { continue };
                let entry = (*spell, now + *duration_ms as f32 / 1000.0, *positive);
                match auras.get_mut(e) {
                    Ok(mut a) => {
                        a.0.retain(|x| x.0 != *spell);
                        a.0.push(entry);
                    }
                    Err(_) => {
                        commands.entity(e).try_insert(UnitAuras(vec![entry]));
                    }
                }
            }
            ServerMsg::AuraRemove { target: t, spell } => {
                if let Some(Ok(mut a)) = net.entities.get(t).map(|e| auras.get_mut(*e)) {
                    a.0.retain(|x| x.0 != *spell);
                }
            }
            _ => {}
        }
    }
}

fn show_error(errors: &mut Query<(&mut Text, &mut ErrorText)>, msg: &str, now: f32) {
    if let Ok((mut t, mut e)) = errors.single_mut() {
        t.0 = msg.to_string();
        e.shown_at = now;
    }
}

// ---------------------------------------------------------------- input

fn cast(
    spell: SpellId,
    data: &GameData,
    net: &Net,
    state: &PlayerState,
    errors: &mut Query<(&mut Text, &mut ErrorText)>,
    now: f32,
) {
    let Some(t) = data.spells.get(&(spell as i64)) else { return };
    let main = t.effects.iter().map(|e| e.target).find(|t| *t != 0).unwrap_or(target::CASTER);
    let target = match main {
        target::HOSTILE => match state.target {
            Some(id) => Some(id),
            None => {
                show_error(errors, "You have no target.", now);
                return;
            }
        },
        // The server lands friendly spells on the target if it is a friend, else on us.
        target::FRIENDLY | target::ANY => state.target,
        _ => None,
    };
    net.send(ClientMsg::CastSpell { spell, target });
}

#[allow(clippy::too_many_arguments)]
fn input(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    data: Res<GameData>,
    net: Res<Net>,
    state: Res<PlayerState>,
    bar: Res<ActionBar>,
    book: Res<Spellbook>,
    mut errors: Query<(&mut Text, &mut ErrorText)>,
    captured: Res<crate::ui_input::UiInputCaptured>,
    esc: Res<EscAction>,
) {
    if *esc == EscAction::CancelCast && book.casting.is_some() {
        net.send(ClientMsg::CancelCast);
    }
    if state.dead || captured.keyboard {
        return;
    }
    for (i, key) in SLOT_KEYS.iter().enumerate() {
        if keys.just_pressed(*key) {
            if let Some(spell) = bar.slots[i] {
                cast(spell, &data, &net, &state, &mut errors, time.elapsed_secs());
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn slot_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    data: Res<GameData>,
    net: Res<Net>,
    state: Res<PlayerState>,
    mut bar: ResMut<ActionBar>,
    mut held: ResMut<Held>,
    slots: Query<(&Interaction, &Slot)>,
    book_rows: Query<&Interaction, With<BookRow>>,
    mut errors: Query<(&mut Text, &mut ErrorText)>,
) {
    let hovered = slots.iter().find(|(i, _)| **i != Interaction::None).map(|(_, s)| s.0);
    if mouse.just_pressed(MouseButton::Right) {
        if let Some(i) = hovered {
            bar.slots[i] = None;
        }
        held.0 = None;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    match (hovered, held.0) {
        (Some(i), Some(spell)) => {
            // Drop onto a slot; whatever was there is picked up (swap).
            held.0 = bar.slots[i].replace(spell);
        }
        (Some(i), None) => {
            if let Some(spell) = bar.slots[i] {
                cast(spell, &data, &net, &state, &mut errors, time.elapsed_secs());
            }
        }
        (None, Some(_)) if book_rows.iter().all(|i| *i == Interaction::None) => held.0 = None,
        _ => {}
    }
}

fn book_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    mut held: ResMut<Held>,
    mut tab: ResMut<BookTab>,
    rows: Query<(&Interaction, &BookRow), Changed<Interaction>>,
    tabs: Query<(&Interaction, &BookTabButton), Changed<Interaction>>,
) {
    for (i, row) in &rows {
        if *i == Interaction::Pressed && mouse.just_pressed(MouseButton::Left) {
            held.0 = Some(row.0);
        }
    }
    for (i, t) in &tabs {
        if *i == Interaction::Pressed && *tab != t.0 {
            *tab = t.0;
        }
    }
}

/// Debug aid (`DUSK_AUTOPLAY=1`): cycle through ready action bar spells while fighting.
#[allow(clippy::too_many_arguments)]
fn autocast(
    time: Res<Time>,
    data: Res<GameData>,
    net: Res<Net>,
    state: Res<PlayerState>,
    bar: Res<ActionBar>,
    book: Res<Spellbook>,
    mut next: Local<(usize, f32)>,
    mut errors: Query<(&mut Text, &mut ErrorText)>,
) {
    let now = time.elapsed_secs();
    if state.target.is_none() || book.casting.is_some() || now < next.1 {
        return;
    }
    for k in 0..SLOTS {
        let i = (next.0 + k) % SLOTS;
        let Some(spell) = bar.slots[i] else { continue };
        let Some(t) = data.spells.get(&(spell as i64)) else { continue };
        if t.abilities_tab != 1 || book.remaining(spell, now).0 > 0.0 || mana_cost(t, &state) > state.mana {
            continue;
        }
        cast(spell, &data, &net, &state, &mut errors, now);
        *next = (i + 1, now + 1.6);
        return;
    }
}

fn open_book_once(mut done: Local<bool>, mut out: MessageWriter<WindowCommand>) {
    if !*done {
        out.write(WindowCommand::Open(WindowId::Abilities));
        *done = true;
    }
}

// ---------------------------------------------------------------- display

fn refresh_slots(
    bar: Res<ActionBar>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    mut icons: Query<(&SlotIcon, &mut ImageNode, &mut Visibility)>,
) {
    if !bar.is_changed() {
        return;
    }
    for (slot, mut image, mut vis) in &mut icons {
        match bar.slots[slot.0] {
            Some(spell) => {
                image.image = icon(&data, &assets, spell);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

fn update_cooldowns(
    time: Res<Time>,
    bar: Res<ActionBar>,
    book: Res<Spellbook>,
    state: Res<PlayerState>,
    data: Res<GameData>,
    mut overlays: Query<(&SlotCooldown, &mut Node)>,
    mut texts: Query<(&SlotCdText, &mut Text)>,
    mut icons: Query<(&SlotIcon, &mut ImageNode)>,
) {
    let now = time.elapsed_secs();
    for (slot, mut node) in &mut overlays {
        let (left, total) = bar.slots[slot.0].map(|s| book.remaining(s, now)).unwrap_or((0.0, 0.0));
        node.height = Val::Percent(if total > 0.0 { left / total * 100.0 } else { 0.0 });
    }
    for (slot, mut text) in &mut texts {
        let (left, _) = bar.slots[slot.0].map(|s| book.remaining(s, now)).unwrap_or((0.0, 0.0));
        text.0 = if left >= 1.5 { format!("{}", left.ceil() as i32) } else { String::new() };
    }
    // Dim spells we can't afford.
    for (slot, mut image) in &mut icons {
        let cost =
            bar.slots[slot.0].and_then(|s| data.spells.get(&(s as i64))).map(|t| mana_cost(t, &state)).unwrap_or(0);
        image.color = if cost > state.mana { Color::srgb(0.4, 0.4, 0.8) } else { Color::WHITE };
    }
}

#[allow(clippy::type_complexity)]
fn update_cast_bar(
    time: Res<Time>,
    book: Res<Spellbook>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    mut bar: Query<&mut Visibility, With<CastBar>>,
    mut parts: ParamSet<(
        Query<&mut Node, With<CastFill>>,
        Query<&mut Text, With<CastLabel>>,
        Query<&mut ImageNode, With<CastBarIcon>>,
    )>,
) {
    let Ok(mut vis) = bar.single_mut() else { return };
    let Some((spell, start, dur)) = book.casting else {
        *vis = Visibility::Hidden;
        return;
    };
    *vis = Visibility::Visible;
    let pct = ((time.elapsed_secs() - start) / dur.max(0.01)).clamp(0.0, 1.0);
    if let Ok(mut n) = parts.p0().single_mut() {
        n.width = Val::Px(276.0 * pct);
    }
    if book.is_changed() {
        let name = data.spells.get(&(spell as i64)).map(|s| s.name.clone()).unwrap_or_default();
        if let Ok(mut t) = parts.p1().single_mut() {
            t.0 = name;
        }
        if let Ok(mut i) = parts.p2().single_mut() {
            i.image = icon(&data, &assets, spell);
        }
    }
}

fn update_errors(time: Res<Time>, mut errors: Query<(&ErrorText, &mut TextColor)>) {
    for (e, mut c) in &mut errors {
        let age = time.elapsed_secs() - e.shown_at;
        c.0 = c.0.with_alpha((1.0 - (age - 1.5).max(0.0)).clamp(0.0, 1.0));
    }
}

fn follow_held(
    held: Res<Held>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut icon_q: Query<(&mut Node, &mut ImageNode, &mut Visibility), With<HeldIcon>>,
) {
    let Ok((mut node, mut image, mut vis)) = icon_q.single_mut() else { return };
    let (Some(spell), Some(cursor)) = (held.0, window.single().ok().and_then(|w| w.cursor_position())) else {
        *vis = Visibility::Hidden;
        return;
    };
    if held.is_changed() {
        image.image = icon(&data, &assets, spell);
    }
    node.left = Val::Px(cursor.x - 16.0);
    node.top = Val::Px(cursor.y - 16.0);
    *vis = Visibility::Visible;
}

// ---------------------------------------------------------------- tooltip

fn mana_cost(t: &SpellTemplate, state: &PlayerState) -> i32 {
    let flat =
        if t.mana_formula.is_empty() { 0.0 } else { eval_formula(&t.mana_formula, &vars(state, 0.0)).unwrap_or(0.0) };
    (flat as i32).max(0) + state.max_mana * t.mana_pct.max(0) as i32 / 100
}

fn vars(state: &PlayerState, value: f64) -> FormulaVars {
    let a = state.attributes;
    FormulaVars {
        clvl: state.level.max(1) as f64,
        splvl: 1.0,
        value,
        str: a.strength as f64,
        agi: a.agility as f64,
        wil: a.willpower as f64,
        int: a.intelligence as f64,
        cur: a.courage as f64,
    }
}

fn secs(ms: f64) -> String {
    let s = ms / 1000.0;
    if s >= 60.0 {
        format!("{} min", (s / 60.0 * 10.0).round() / 10.0)
    } else {
        format!("{} sec", (s * 10.0).round() / 10.0)
    }
}

/// Replaces `$E1min`, `$E1max`, `$E1D3`, `$DUR`, `$INVL` in tooltips
/// (whose numbers came from the server; we compute them with the same formulas).
pub fn describe(t: &SpellTemplate, state: &PlayerState) -> String {
    let mut text = t.description.clone();
    let duration = if t.duration_formula.is_empty() {
        t.duration_ms as f64
    } else {
        eval_formula(&t.duration_formula, &vars(state, t.duration_ms as f64)).unwrap_or(t.duration_ms as f64)
    };
    text = text.replace("$DUR", &secs(duration)).replace("$INVL", &secs(t.interval_ms.max(1000) as f64));
    for (i, e) in t.effects.iter().enumerate() {
        let n = i + 1;
        let base_input = if e.kind == effect::APPLY_AURA { e.data[2] } else { e.data[1] };
        let v = eval_formula(&e.formula, &vars(state, base_input as f64)).unwrap_or(base_input as f64);
        let rolls = matches!(e.kind, effect::SCHOOL_DAMAGE | effect::HEAL);
        let (min, max) = if rolls { (v * 0.9, v * 1.1) } else { (v, v) };
        let periodic_ticks = (duration / t.interval_ms.max(1000) as f64).floor().max(1.0);
        let shown = |x: f64| {
            if e.kind == effect::APPLY_AURA
                && matches!(e.data[0], aura::PERIODIC_DAMAGE | aura::PERIODIC_HEAL)
                && t.description.contains("every")
            {
                x.round()
            } else if e.kind == effect::APPLY_AURA && matches!(e.data[0], aura::PERIODIC_DAMAGE | aura::PERIODIC_HEAL) {
                (x / periodic_ticks).ceil() * periodic_ticks
            } else {
                x.round()
            }
        };
        text = text
            .replace(&format!("$E{n}min"), &format!("{}", shown(min).abs()))
            .replace(&format!("$E{n}max"), &format!("{}", shown(max).abs()));
        for d in 1..=3 {
            text = text.replace(&format!("$E{n}D{d}"), &e.data[d - 1].abs().to_string());
        }
    }
    text
}

/// What the spell tooltip shows.
#[derive(Clone, Copy, PartialEq, Debug)]
enum TipFor {
    Spell(SpellId),
    /// An empty action bar slot.
    EmptySlot(usize),
    /// An aura icon: spell, expiry (app seconds), positive.
    Aura(SpellId, f32, bool),
}

#[allow(clippy::too_many_arguments)]
fn update_tooltip(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    state: Res<PlayerState>,
    bar: Res<ActionBar>,
    font: Res<UiFont>,
    held: Res<Held>,
    window: Query<&Window, With<PrimaryWindow>>,
    slots: Query<(&Interaction, &Slot)>,
    rows: Query<(&Interaction, &BookRow)>,
    aura_icons: Query<(&Interaction, &AuraIcon, &InheritedVisibility)>,
    mut tooltip: Query<(Entity, &mut Node, &mut Visibility), With<Tooltip>>,
    mut shown: Local<Option<(TipFor, i32)>>,
) {
    let Ok((entity, mut node, mut vis)) = tooltip.single_mut() else { return };
    // Debug aid: `DUSK_TOOLTIP_SLOT=<n>` shows slot n's tooltip as if hovered.
    let forced = std::env::var("DUSK_TOOLTIP_SLOT").ok().and_then(|s| s.parse::<usize>().ok()).filter(|i| *i < SLOTS);
    let now = time.elapsed_secs();
    let hovered = slots
        .iter()
        .find(|(i, _)| **i == Interaction::Hovered)
        .map(|(_, s)| bar.slots[s.0].map_or(TipFor::EmptySlot(s.0), TipFor::Spell))
        .or_else(|| rows.iter().find(|(i, _)| **i == Interaction::Hovered).map(|(_, r)| TipFor::Spell(r.0)))
        .or_else(|| {
            aura_icons
                .iter()
                .find(|(i, _, v)| **i == Interaction::Hovered && v.get())
                .map(|(_, a, _)| TipFor::Aura(a.spell, a.expires, a.positive))
        })
        .or_else(|| forced.and_then(|i| bar.slots[i]).map(TipFor::Spell))
        .filter(|_| held.0.is_none());
    let cursor = window
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .or_else(|| forced.map(|i| Vec2::new(210.0 + SLOT_X0 + SLOT_STRIDE * i as f32, 680.0)));
    let (Some(tip), Some(cursor)) = (hovered, cursor) else {
        *vis = Visibility::Hidden;
        *shown = None;
        return;
    };
    let spell = match tip {
        TipFor::Spell(s) | TipFor::Aura(s, ..) => data.spells.get(&(s as i64)),
        TipFor::EmptySlot(_) => None,
    };
    if spell.is_none() && !matches!(tip, TipFor::EmptySlot(_)) {
        return;
    }
    *vis = Visibility::Visible;
    // Keep it on screen: above the action bar, right of the book.
    let (w, h) = window.single().map(|w| (w.width(), w.height())).unwrap_or((1280.0, 720.0));
    node.left = Val::Px((cursor.x + 16.0).min(w - 300.0));
    node.top = Val::Px(if cursor.y < h / 2.0 { cursor.y + 24.0 } else { (cursor.y - 180.0).max(8.0) });
    // Aura timers tick: rebuild when the shown second changes.
    let tick = match tip {
        TipFor::Aura(_, expires, _) => (expires - now).ceil() as i32,
        _ => 0,
    };
    if *shown == Some((tip, tick)) && !state.is_changed() {
        return;
    }
    *shown = Some((tip, tick));
    commands.entity(entity).despawn_related::<Children>();
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let mut lines: Vec<(String, f32, Color)> = Vec::new();
    match (tip, spell) {
        (TipFor::EmptySlot(i), _) => {
            lines.push((format!("Empty slot ({})", SLOT_LABELS[i]), 15.0, GOLD));
            lines.push(("Open Abilities (P), click a spell, then click this slot.".into(), 12.0, GREY));
        }
        (TipFor::Aura(_, expires, positive), Some(t)) => {
            lines.push((t.name.clone(), 16.0, if positive { GOLD } else { Color::srgb(1.0, 0.45, 0.35) }));
            let text =
                if t.aura_description.trim().is_empty() { describe(t, &state) } else { t.aura_description.clone() };
            lines.push((text.trim().to_string(), 13.0, Color::srgb(1.0, 0.82, 0.0)));
            lines.push((format!("{} remaining", secs_left(expires - now)), 12.0, GREY));
        }
        (_, Some(t)) => {
            let cost = mana_cost(t, &state);
            lines.push((t.name.clone(), 17.0, GOLD));
            let mut meta = Vec::new();
            if cost > 0 {
                meta.push(format!("{cost} Mana"));
            }
            if t.range > 0 {
                meta.push(format!("{} yd range", (t.range_cells() * 3.0).round()));
            }
            lines.push((meta.join("    "), 13.0, Color::WHITE));
            let cast =
                if t.cast_time_ms > 0 { format!("{} cast", secs(t.cast_time_ms as f64)) } else { "Instant".into() };
            let cd =
                if t.cooldown_ms > 0 { format!("    {} cooldown", secs(t.cooldown_ms as f64)) } else { String::new() };
            lines.push((format!("{cast}{cd}"), 13.0, Color::WHITE));
            lines.push((describe(t, &state), 13.0, Color::srgb(1.0, 0.82, 0.0)));
            if let Some(i) = bar.slots.iter().position(|s| *s == Some(t.entry as SpellId)) {
                lines.push((format!("Key: {}", SLOT_LABELS[i]), 11.0, GREY));
            }
        }
        _ => {}
    }
    commands.entity(entity).with_children(|p| {
        for (text, size, color) in lines.into_iter().filter(|l| !l.0.is_empty()) {
            p.spawn((
                Text::new(text),
                f(size),
                TextColor(color),
                Node { margin: UiRect::bottom(Val::Px(3.0)), ..default() },
            ));
        }
    });
}

fn secs_left(s: f32) -> String {
    let s = s.max(0.0).ceil() as i32;
    if s >= 60 { format!("{}:{:02}", s / 60, s % 60) } else { format!("{s} sec") }
}

// ---------------------------------------------------------------- abilities window

/// Active tab: lit and underlined; the other one dimmed.
fn book_tabs(tab: Res<BookTab>, mut shades: Query<(&BookTabShade, &mut BackgroundColor, &mut Node, &mut BorderColor)>) {
    if !tab.is_changed() {
        return;
    }
    for (s, mut bg, mut node, mut border) in &mut shades {
        let active = s.0 == *tab;
        bg.0 = if active { Color::srgba(0.55, 0.12, 0.08, 0.22) } else { Color::srgba(0.0, 0.0, 0.0, 0.45) };
        node.border = UiRect::bottom(Val::Px(2.0));
        *border = BorderColor::all(if active { GOLD.with_alpha(0.85) } else { Color::NONE });
    }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_book(
    mut commands: Commands,
    book: Res<Spellbook>,
    tab: Res<BookTab>,
    state: Res<PlayerState>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    list: Query<Entity, With<BookList>>,
) {
    if !(book.is_changed() || tab.is_changed() || state.is_changed() && state.level != 0) {
        return;
    }
    let Ok(list) = list.single() else { return };
    commands.entity(list).despawn_related::<Children>();
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let slot_img = img(&data, &assets, "abilities_slot_idle.png");
    let mut spells: Vec<&SpellTemplate> = book.known.iter().filter_map(|s| data.spells.get(&(*s as i64))).collect();
    spells.retain(|s| (s.abilities_tab == 1) == (*tab == BookTab::Spells));
    spells.sort_by_key(|s| s.entry);
    commands.entity(list).with_children(|l| {
        for s in spells {
            let mut summary = describe(s, &state);
            if summary.len() > 70 {
                summary.truncate(summary[..70].rfind(' ').unwrap_or(70));
                summary.push_str("...");
            }
            l.spawn((
                Node { width: Val::Px(401.0), height: Val::Px(70.0), flex_shrink: 0.0, ..default() },
                ImageNode::new(slot_img.clone()),
                Button,
                CapturesPointer,
                BookRow(s.entry as SpellId),
            ))
            .with_children(|r| {
                r.spawn((absolute(18.0, 14.0, 40.0, 40.0), ImageNode::new(img(&data, &assets, &s.icon))));
                r.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(72.0),
                        top: Val::Px(12.0),
                        ..default()
                    },
                    Text::new(s.name.clone()),
                    f(16.0),
                    TextColor(GOLD),
                ));
                r.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(72.0),
                        top: Val::Px(34.0),
                        width: Val::Px(315.0),
                        ..default()
                    },
                    Text::new(summary),
                    f(11.0),
                    TextColor(GREY),
                ));
            });
        }
    });
}

// ---------------------------------------------------------------- aura icons

/// A hoverable aura icon (the spell tooltip shows it).
#[derive(Component)]
struct AuraIcon {
    spell: SpellId,
    expires: f32,
    positive: bool,
}

#[derive(Component)]
struct AuraTimer;

/// Rebuilds a row only when its set of auras changes (so hovering keeps working); timers tick
/// in place.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_aura_rows(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    net: Res<Net>,
    state: Res<PlayerState>,
    rows: Query<(Entity, &AuraRow)>,
    children: Query<&Children, With<AuraRow>>,
    auras: Query<&UnitAuras>,
    mut icons: Query<(&mut AuraIcon, &Children)>,
    mut timers: Query<&mut Text, With<AuraTimer>>,
    mut built: Local<HashMap<Entity, Vec<(SpellId, bool)>>>,
) {
    let now = time.elapsed_secs();
    let f = TextFont { font: font.0.clone().into(), font_size: 11.0.into(), ..default() };
    let unit_of = |id: Option<EntityId>| id.and_then(|i| net.entities.get(&i)).and_then(|e| auras.get(*e).ok());
    for (row, which) in &rows {
        let list: Vec<(SpellId, f32, bool)> = (if which.player { unit_of(net.my_id) } else { unit_of(state.target) })
            .map(|a| a.0.iter().filter(|a| a.1 > now).copied().collect())
            .unwrap_or_default();
        let key: Vec<(SpellId, bool)> = list.iter().map(|a| (a.0, a.2)).collect();
        if built.get(&row) != Some(&key) {
            built.insert(row, key);
            commands.entity(row).despawn_related::<Children>();
            commands.entity(row).with_children(|r| {
                for (spell, expires, positive) in &list {
                    let border = if *positive { Color::srgb(0.2, 0.7, 0.2) } else { Color::srgb(0.8, 0.15, 0.1) };
                    r.spawn((
                        Node {
                            width: Val::Px(26.0),
                            height: Val::Px(26.0),
                            border: UiRect::all(Val::Px(1.0)),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        BorderColor::all(border),
                        ImageNode::new(icon(&data, &assets, *spell)),
                        Interaction::default(),
                        CapturesPointer,
                        AuraIcon { spell: *spell, expires: *expires, positive: *positive },
                    ))
                    .with_children(|i| {
                        i.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                top: Val::Px(26.0),
                                left: Val::Px(2.0),
                                ..default()
                            },
                            Text::new(format!("{}", (expires - now).ceil() as i32)),
                            f.clone(),
                            TextColor(Color::WHITE),
                            TextShadow::default(),
                            AuraTimer,
                        ));
                    });
                }
            });
            continue;
        }
        // Same auras (in the same order): refresh expiry (re-applied auras) and the countdown.
        let Ok(kids) = children.get(row) else { continue };
        for ((_, expires, _), k) in list.iter().zip(kids.iter()) {
            let Ok((mut icon, icon_kids)) = icons.get_mut(k) else { continue };
            icon.expires = *expires;
            for t in icon_kids.iter() {
                if let Ok(mut t) = timers.get_mut(t) {
                    let s = format!("{}", (expires - now).ceil() as i32);
                    if t.0 != s {
                        t.0 = s;
                    }
                }
            }
        }
    }
}
