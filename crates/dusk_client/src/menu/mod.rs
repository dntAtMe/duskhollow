//! Front end: main menu, character / server panels, options, the connecting screen and the
//! in-game game menu (Esc). See `state.rs` for the app states.
//!
//! Screens are rebuilt from [`MenuModel`] whenever it changes screen; widgets read and write
//! the model and [`Settings`]. Keyboard: arrows / Tab move, Left / Right adjust, Enter picks,
//! Esc goes back. The game menu is an overlay: the world keeps running behind it.
//!
//! Debug: `DUSK_MENU=main|play|join|options|connecting` opens a menu screen directly (with
//! `DUSK_SCREENSHOT` for pictures); `DUSK_MENU=pause|pause_options` starts the game and opens
//! the game menu (or its options) after `DUSK_MENU_AT` seconds (default 3).

mod vista;
mod widgets;

use crate::{
    audio::PlaySfx,
    combat_ui::UiFont,
    data::GameData,
    net,
    settings::{self, RESOLUTIONS, Settings, UI_SCALES},
    state::{AppState, DirectConnect, Launch, MenuNotice, OfflineServer},
    ui_input::{CapturesPointer, UiInputCaptured},
};
use bevy::ecs::system::SystemParam;
use bevy::input::{
    ButtonState, InputSystems,
    keyboard::{Key, KeyboardInput},
};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use crossbeam_channel::{Receiver, TryRecvError};
use dusk_formats::sound::builtin;
use dusk_protocol::net::ClientConnection;
use std::time::Duration;
use widgets::*;

/// Kept when a session ends (menu-owned roots manage themselves).
#[derive(Component)]
pub struct KeepOnMenu;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    None,
    Main,
    Play,
    Join,
    Options,
    Connecting,
    GameMenu,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Play,
    Join,
    Options,
    Quit,
    Back,
    Start,
    Connect,
    Cancel,
    Resume,
    QuitToMenu,
    QuitGame,
    Tab(usize),
    Recent(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderKey {
    Master,
    Music,
    Effects,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleKey {
    Fullscreen,
    Vsync,
    ShowFps,
    Shake,
    HitStop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CycleKey {
    Resolution,
    UiScale,
    Map,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKey {
    Name,
    Address,
}

/// Display names and roles for `player_class_stats` 1..=4 (the data has numbers only).
const CLASSES: [(&str, &str, &str); 4] = [
    ("Vanguard", "Steel and stubbornness", "Heavy blows, a thick hide, a knife for the ones that run."),
    ("Emberwright", "Cairn fire, carried", "Throws cairn fire from range. The Eye cannot see through flame."),
    ("Cutthroat", "Knives and hooks", "Quick cuts and thrown knives; keeps the hollowed at the end of a chain."),
    ("Ashpriest", "Ember prayers", "Mends with ember prayers and settles the rest with a mace."),
];

const OPTION_TABS: [&str; 4] = ["Sound", "Display", "Game", "Controls"];

const BINDINGS: [(&str, &str); 19] = [
    ("W A S D", "Move"),
    ("Left click", "Select a target / talk / loot"),
    ("Right click", "Walk up and attack"),
    ("Double click", "Walk up and attack"),
    ("Tab / Shift+Tab", "Cycle nearby enemies"),
    ("1 - 0  -  =", "Action bar"),
    ("P", "Abilities"),
    ("I", "Inventory"),
    ("C", "Character"),
    ("J", "Journal"),
    ("Enter", "Chat (Enter sends)"),
    ("1 - 4", "Dialogue replies"),
    ("Esc", "Cancel cast / clear target / game menu"),
    ("M", "Music on / off"),
    ("N", "Sound effects on / off"),
    ("Mouse wheel", "Scroll chat / zoom minimap"),
    ("Numpad + / -", "Minimap zoom"),
    ("Shift + right click", "Destroy an item"),
    ("Arrows, Enter, Esc", "Menus"),
];

pub fn version_string() -> String {
    let build = if cfg!(debug_assertions) { "debug" } else { "release" };
    format!("Duskhollow {} ({build} build, protocol {})", env!("CARGO_PKG_VERSION"), dusk_protocol::PROTOCOL_VERSION)
}

#[derive(Clone)]
struct ClassInfo {
    name: String,
    role: String,
    line: String,
    stats: String,
}

#[derive(Resource)]
pub struct MenuModel {
    pub screen: Screen,
    /// The menu backdrop is up (menu, or connecting from it).
    pub vista: bool,
    /// Screen the options return to (`Main` or `GameMenu`).
    options_back: Screen,
    options_tab: usize,
    /// Play or Join: where a failed connection returns.
    connect_from: Screen,
    focus: usize,
    focus_count: usize,
    name: String,
    class: u8,
    addr: String,
    maps: Vec<(String, String)>,
    map: usize,
    error: Option<String>,
    status: String,
    classes: Vec<ClassInfo>,
    /// Screen the spawned widgets belong to (rebuild when it differs).
    built: Option<(Screen, usize, bool)>,
    rebuild: bool,
    sting_played: bool,
}

impl Default for MenuModel {
    fn default() -> Self {
        Self {
            screen: Screen::None,
            vista: false,
            options_back: Screen::Main,
            options_tab: 0,
            connect_from: Screen::Play,
            focus: 0,
            focus_count: 0,
            name: String::new(),
            class: 1,
            addr: format!("127.0.0.1:{}", dusk_protocol::DEFAULT_PORT),
            maps: Vec::new(),
            map: 0,
            error: None,
            status: "Walking in from the gorge".into(),
            classes: Vec::new(),
            built: None,
            rebuild: false,
            sting_played: false,
        }
    }
}

impl MenuModel {
    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.focus = 0;
        self.rebuild = true;
    }

    fn in_game_overlay(&self) -> bool {
        self.screen == Screen::GameMenu || (self.screen == Screen::Options && self.options_back == Screen::GameMenu)
    }
}

/// Keys gathered for the menu this frame (in `PreUpdate`, before gameplay sees them).
#[derive(Resource, Default)]
struct MenuInput {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    enter: bool,
    esc: bool,
    typed: String,
    backspace: usize,
}

/// A connection being opened on a background thread (embedded server start-up included).
#[derive(Resource)]
struct PendingConnect {
    rx: Receiver<Result<(ClientConnection, Option<dusk_server::EmbeddedServer>), String>>,
    name: String,
    class: u8,
    /// Server address (online), remembered once the connection opens.
    addr: Option<String>,
}

/// Seconds since `Hello` was sent, waiting for `Welcome`.
#[derive(Resource, Default)]
struct WelcomeWait(f32);
const WELCOME_TIMEOUT: f32 = 15.0;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuModel>()
            .init_resource::<MenuInput>()
            .init_resource::<vista::VistaClock>()
            .add_systems(Startup, setup.after(crate::combat_ui::load_font))
            .add_systems(OnEnter(AppState::Menu), (enter_menu, vista::request_map))
            .add_systems(OnEnter(AppState::Connecting), enter_connecting)
            .add_systems(OnEnter(AppState::InGame), enter_game)
            .add_systems(
                PreUpdate,
                (debug_esc, collect_input, block_game_input).chain().after(InputSystems).before(crate::chat::type_chat),
            )
            .add_systems(PreUpdate, hold_capture.after(crate::chat::type_chat))
            .add_systems(
                Update,
                (
                    (boot, debug_open, debug_cycle, poll_connect, welcome_timeout),
                    open_game_menu.run_if(crate::windows::pause_menu_requested.and_then(crate::state::in_game)),
                    (rebuild, navigate, pointer, edit_text, refresh).chain(),
                    (vista::manage, vista::animate).chain(),
                )
                    .chain(),
            );
    }
}

fn setup(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    bold: Res<crate::combat_ui::UiFontBold>,
    settings: Res<Settings>,
    mut model: ResMut<MenuModel>,
) {
    commands.insert_resource(Skin::load(&data, &assets, font.0.clone(), bold.0.clone()));
    let stats: Vec<_> = dusk_formats::content::rules::load(&data.root)
        .map(|r| r.class_stats.into_values().collect())
        .unwrap_or_default();
    model.classes = CLASSES
        .iter()
        .enumerate()
        .map(|(i, (name, role, line))| {
            let s = stats.iter().find(|s| s.class == i as i64 + 1 && s.level == 1);
            let stats = s.map_or(String::new(), |s| {
                format!(
                    "Health {}    Mana {}    Str {}  Agi {}  Wil {}  Int {}  Cou {}",
                    s.hp, s.mana, s.strength, s.agility, s.willpower, s.intelligence, s.courage
                )
            });
            ClassInfo { name: name.to_string(), role: role.to_string(), line: line.to_string(), stats }
        })
        .collect();
    model.name =
        if settings.name.is_empty() { format!("Wanderer{}", std::process::id() % 1000) } else { settings.name.clone() };
    model.class = settings.class.clamp(1, 4);
    if let Some(addr) = settings.servers.first() {
        model.addr = addr.clone();
    }
    model.maps = start_maps(&data);
    // A saved map that no longer exists (e.g. renamed) falls back to the default map.
    model.map = model.maps.iter().position(|(m, _)| *m == settings.map).unwrap_or(0);
}

/// Start maps from the map data (`content::maps`): the default map first, then the rest by
/// name. Labels are the map titles, else the name in words.
fn start_maps(data: &GameData) -> Vec<(String, String)> {
    let default = dusk_formats::content::maps::default_map(&data.maps).map(|m| m.name.clone());
    let mut maps: Vec<&dusk_formats::content::types::MapInfo> = data.maps.iter().collect();
    maps.sort_by_key(|m| (Some(&m.name) != default.as_ref(), m.name.clone()));
    let label = |n: &str| {
        n.split('_')
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    maps.into_iter().map(|m| (m.name.clone(), label(&m.name))).collect()
}

/// Leaves `Boot` for the menu, or straight for the game (command-line launch).
fn boot(
    mut commands: Commands,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    launch: Res<Launch>,
    skin: Option<Res<Skin>>,
    direct: Option<ResMut<DirectConnect>>,
) {
    if *state.get() != AppState::Boot || skin.is_none() {
        return;
    }
    match *launch {
        Launch::Menu => next.set(AppState::Menu),
        Launch::Direct => {
            if let Some((conn, name, class)) = direct.and_then(|mut d| d.0.take()) {
                net::begin_session(&mut commands, conn, &name, class);
                next.set(AppState::Connecting);
            }
        }
    }
}

fn enter_menu(mut model: ResMut<MenuModel>, mut notice: ResMut<MenuNotice>, mut sfx: MessageWriter<PlaySfx>) {
    info!("main menu");
    model.vista = true;
    // A failed or cancelled connection goes back to its panel; leaving a game, to the title.
    let returning = matches!(model.screen, Screen::Connecting | Screen::Play | Screen::Join);
    model.error = notice.0.take();
    let screen = if model.error.is_some() || returning { model.connect_from } else { Screen::Main };
    model.go(screen);
    let first = !model.sting_played;
    if let Some(screen) = debug_screen().filter(|_| first) {
        model.go(screen);
        model.options_tab = debug_tab();
    }
    if !model.sting_played {
        model.sting_played = true;
        sfx.write(PlaySfx::ui("title_sting.wav"));
    }
}

fn enter_connecting(mut model: ResMut<MenuModel>) {
    if model.vista {
        model.go(Screen::Connecting);
    }
}

fn enter_game(mut model: ResMut<MenuModel>) {
    info!("in game");
    model.vista = false;
    model.go(Screen::None);
}

/// `DUSK_MENU_TAB=0..3`: the options tab to open with `DUSK_MENU=options|pause_options`.
fn debug_tab() -> usize {
    std::env::var("DUSK_MENU_TAB").ok().and_then(|s| s.parse().ok()).unwrap_or(0).min(OPTION_TABS.len() - 1)
}

fn debug_screen() -> Option<Screen> {
    match std::env::var("DUSK_MENU").ok()?.as_str() {
        "main" => Some(Screen::Main),
        "play" => Some(Screen::Play),
        "join" => Some(Screen::Join),
        "options" => Some(Screen::Options),
        "connecting" => Some(Screen::Connecting),
        _ => None,
    }
}

/// `DUSK_MENU=pause|pause_options`: open the game menu a few seconds into the game.
fn debug_open(
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut model: ResMut<MenuModel>,
    mut since: Local<Option<f32>>,
    mut done: Local<bool>,
) {
    if *done || *state.get() != AppState::InGame {
        return;
    }
    let Ok(which) = std::env::var("DUSK_MENU") else {
        *done = true;
        return;
    };
    let at = std::env::var("DUSK_MENU_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let t = since.get_or_insert(time.elapsed_secs());
    if time.elapsed_secs() - *t < at {
        return;
    }
    *done = true;
    match which.as_str() {
        "pause" => model.go(Screen::GameMenu),
        "pause_options" => {
            model.options_back = Screen::GameMenu;
            model.options_tab = debug_tab();
            model.go(Screen::Options);
        }
        _ => {}
    }
}

/// `DUSK_MENU=cycle`: Play, play a few seconds, Quit to Menu, Play again... `DUSK_MENU_CYCLES`
/// times (default 2), then quit. Checks that sessions start and end cleanly.
fn debug_cycle(mut commands: Commands, time: Res<Time>, mut ctx: Ctx, mut phase: Local<(u32, f32)>) {
    if std::env::var("DUSK_MENU").ok().as_deref() != Some("cycle") {
        return;
    }
    let now = time.elapsed_secs();
    let max: u32 = std::env::var("DUSK_MENU_CYCLES").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
    let (cycles, since) = &mut *phase;
    let state = *ctx.state.get();
    let ready = state == AppState::InGame || (state == AppState::Menu && ctx.model.screen == Screen::Main);
    if !ready {
        *since = 0.0;
        return;
    }
    if *since == 0.0 {
        *since = now;
        return;
    }
    let wait = if state == AppState::Menu { 2.5 } else { 6.0 };
    if now - *since < wait {
        return;
    }
    *since = 0.0;
    if state == AppState::Menu {
        if *cycles >= max {
            info!("menu cycle test: {cycles} sessions, done");
            ctx.exit.write(AppExit::Success);
            return;
        }
        // `DUSK_MENU_JOIN=HOST:PORT`: join that server instead of playing offline.
        let join = std::env::var("DUSK_MENU_JOIN").ok();
        info!("menu cycle test: {} #{}", if join.is_some() { "Join" } else { "Play" }, *cycles + 1);
        ctx.model.connect_from = if join.is_some() { Screen::Join } else { Screen::Play };
        if let Some(addr) = &join {
            ctx.model.addr = addr.clone();
        }
        ctx.start(&mut commands, join.is_some());
    } else {
        *cycles += 1;
        info!("menu cycle test: Quit to Menu");
        ctx.activate(&mut commands, &Action::QuitToMenu);
    }
}

// ---------------------------------------------------------------- input

/// Esc that nothing in game wanted, or the micro-menu's Menu button (`windows::EscAction`):
/// the game menu. While it is open the keyboard is ours, so the next Esc closes it.
fn open_game_menu(mut model: ResMut<MenuModel>, mut sfx: MessageWriter<PlaySfx>) {
    if model.screen == Screen::None {
        info!("game menu");
        model.go(Screen::GameMenu);
        sfx.write(PlaySfx::ui(builtin::WINDOW_OPEN));
    }
}

/// `DUSK_MENU_ESC=6,8.5`: presses Esc at those seconds (tests the Esc priority chain).
fn debug_esc(time: Res<Time>, mut keys: ResMut<ButtonInput<KeyCode>>, mut pressed: Local<usize>) {
    let Ok(list) = std::env::var("DUSK_MENU_ESC") else { return };
    keys.release(KeyCode::Escape);
    let times: Vec<f32> = list.split(',').filter_map(|t| t.trim().parse().ok()).collect();
    if times.get(*pressed).is_some_and(|t| time.elapsed_secs() >= *t) {
        *pressed += 1;
        info!("debug: Esc");
        keys.press(KeyCode::Escape);
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<KeyboardInput>,
    mut input: ResMut<MenuInput>,
    model: Res<MenuModel>,
) {
    *input = MenuInput::default();
    let events: Vec<KeyboardInput> = typed.read().cloned().collect();
    if model.screen == Screen::None {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    input.up = keys.just_pressed(KeyCode::ArrowUp) || (keys.just_pressed(KeyCode::Tab) && shift);
    input.down = keys.just_pressed(KeyCode::ArrowDown) || (keys.just_pressed(KeyCode::Tab) && !shift);
    input.left = keys.just_pressed(KeyCode::ArrowLeft);
    input.right = keys.just_pressed(KeyCode::ArrowRight);
    input.enter = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]);
    input.esc = keys.just_pressed(KeyCode::Escape);
    for k in events.iter().filter(|k| k.state == ButtonState::Pressed) {
        match &k.logical_key {
            Key::Backspace => input.backspace += 1,
            Key::Enter | Key::Tab | Key::Escape => {}
            _ => {
                if let Some(t) = &k.text {
                    input.typed.extend(t.chars().filter(|c| !c.is_control()));
                }
            }
        }
    }
}

/// While the game menu (or its options) is open, the game does not see the keyboard.
fn block_game_input(
    model: Res<MenuModel>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut messages: ResMut<Messages<KeyboardInput>>,
) {
    if model.in_game_overlay() {
        keys.reset_all();
        messages.clear();
    }
}

fn hold_capture(model: Res<MenuModel>, mut captured: ResMut<UiInputCaptured>) {
    if model.in_game_overlay() && !captured.keyboard {
        captured.keyboard = true;
    }
}

// ---------------------------------------------------------------- screens

#[derive(Component)]
struct ScreenRoot;
#[derive(Component)]
struct ErrorLine;
#[derive(Component)]
struct StatusLine;
/// A line of the class description box.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum ClassPart {
    Title,
    Role,
    Line,
    Stats,
}

fn rebuild(
    mut commands: Commands,
    mut model: ResMut<MenuModel>,
    skin: Option<Res<Skin>>,
    settings: Res<Settings>,
    roots: Query<Entity, With<ScreenRoot>>,
) {
    let key = (model.screen, model.options_tab, model.vista);
    if !model.rebuild && model.built == Some(key) {
        return;
    }
    let Some(skin) = skin else { return };
    model.rebuild = false;
    model.built = Some(key);
    for e in &roots {
        commands.entity(e).despawn();
    }
    if model.screen == Screen::None {
        return;
    }
    let overlay = !model.vista;
    let mut root = commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        GlobalZIndex(150),
        ScreenRoot,
        KeepOnMenu,
    ));
    if overlay {
        root.insert((BackgroundColor(Color::srgba(0.02, 0.0, 0.0, 0.45)), CapturesPointer));
    }
    let mut focus = 0usize;
    let mut next = || {
        focus += 1;
        focus - 1
    };
    let model_ref = &*model;
    let mut primary = 0;
    root.with_children(|r| match model_ref.screen {
        Screen::None => {}
        Screen::Main => {
            r.spawn(Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(47.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|c| {
                error_line(c, &skin);
                for (label, action) in [
                    ("Play", Action::Play),
                    ("Join Server", Action::Join),
                    ("Options", Action::Options),
                    ("Quit", Action::Quit),
                ] {
                    button(c, &skin, label, action, next(), 264.0, 42.0);
                }
            });
            r.spawn((
                Node { position_type: PositionType::Absolute, bottom: Val::Px(10.0), ..default() },
                Text::new("Arrows choose  -  Enter selects  -  Esc goes back"),
                skin.text(12.0),
                TextColor(FAINT),
                shadow(),
            ));
        }
        Screen::Play | Screen::Join => {
            let join = model_ref.screen == Screen::Join;
            panel(r, &skin, 720.0, false, |p| {
                header(
                    p,
                    &skin,
                    if join { "Join a Server" } else { "Take Up the Sword" },
                    if join {
                        "Walk into someone else's vale."
                    } else {
                        "Offline: the vale runs on this machine, the Eye included."
                    },
                );
                row(p, &skin, "Name", |r| {
                    field(r, &skin, FieldKey::Name, next(), 260.0);
                    r.spawn((Text::new("letters and digits, up to 15"), skin.text(12.0), TextColor(FAINT), shadow()));
                });
                let names: [String; 4] = std::array::from_fn(|i| CLASSES[i].0.to_string());
                p.spawn(Node {
                    margin: UiRect::vertical(Val::Px(4.0)),
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_children(|c| class_row(c, &skin, &names, next()));
                class_box(p, &skin);
                if join {
                    row(p, &skin, "Server", |r| field(r, &skin, FieldKey::Address, next(), 300.0));
                    if !settings.servers.is_empty() {
                        row(p, &skin, "Recent", |r| {
                            r.spawn(Node {
                                column_gap: Val::Px(6.0),
                                flex_wrap: FlexWrap::Wrap,
                                max_width: Val::Px(470.0),
                                ..default()
                            })
                            .with_children(|c| {
                                for (i, a) in settings.servers.iter().enumerate() {
                                    button(c, &skin, a, Action::Recent(i), next(), 150.0, 28.0);
                                }
                            });
                        });
                    }
                } else {
                    row(p, &skin, "Start in", |r| cycler(r, &skin, CycleKey::Map, next(), 320.0));
                }
                error_line(p, &skin);
                footer(p, |f| {
                    button(f, &skin, "Back", Action::Back, next(), 160.0, 38.0);
                    primary = next();
                    if join {
                        button(f, &skin, "Connect", Action::Connect, primary, 230.0, 38.0);
                    } else {
                        button(f, &skin, "Enter the Vale", Action::Start, primary, 230.0, 38.0);
                    }
                });
            });
        }
        Screen::Options => {
            panel(r, &skin, 640.0, overlay, |p| {
                header(p, &skin, "Options", "Changes apply at once and are kept for next time.");
                p.spawn(Node { column_gap: Val::Px(6.0), margin: UiRect::bottom(Val::Px(6.0)), ..default() })
                    .with_children(|t| {
                        for (i, name) in OPTION_TABS.iter().enumerate() {
                            let e = button(t, &skin, name, Action::Tab(i), next(), 132.0, 30.0);
                            if i == model_ref.options_tab {
                                t.commands().entity(e).insert(SelectedTab);
                            }
                        }
                    });
                p.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    min_height: Val::Px(250.0),
                    ..default()
                })
                .with_children(|c| match model_ref.options_tab {
                    0 => {
                        for (label, key) in [
                            ("Master volume", SliderKey::Master),
                            ("Music", SliderKey::Music),
                            ("Effects", SliderKey::Effects),
                        ] {
                            row(c, &skin, label, |r| slider(r, &skin, key, next()));
                        }
                        c.spawn((
                            Text::new("M and N switch music and effects off in game."),
                            skin.text(12.0),
                            TextColor(FAINT),
                            shadow(),
                            Node { margin: UiRect::top(Val::Px(10.0)), ..default() },
                        ));
                    }
                    1 => {
                        row(c, &skin, "Fullscreen", |r| {
                            toggle(r, &skin, ToggleKey::Fullscreen, next(), Some("borderless, on this monitor"))
                        });
                        row(c, &skin, "Window size", |r| cycler(r, &skin, CycleKey::Resolution, next(), 220.0));
                        row(c, &skin, "VSync", |r| toggle(r, &skin, ToggleKey::Vsync, next(), None));
                        row(c, &skin, "Interface scale", |r| cycler(r, &skin, CycleKey::UiScale, next(), 220.0));
                        row(c, &skin, "Show FPS", |r| toggle(r, &skin, ToggleKey::ShowFps, next(), None));
                    }
                    2 => {
                        row(c, &skin, "Screen shake", |r| toggle(r, &skin, ToggleKey::Shake, next(), None));
                        row(c, &skin, "Hit-stop", |r| {
                            toggle(r, &skin, ToggleKey::HitStop, next(), Some("the brief freeze on heavy blows"))
                        });
                    }
                    _ => {
                        // Two columns of fixed-height cells: keys right-aligned, actions left.
                        c.spawn(Node {
                            column_gap: Val::Px(16.0),
                            justify_content: JustifyContent::Center,
                            ..default()
                        })
                        .with_children(|l| {
                            for (col, right) in [(0, true), (1, false)] {
                                l.spawn(Node {
                                    flex_direction: FlexDirection::Column,
                                    align_items: if right { AlignItems::FlexEnd } else { AlignItems::FlexStart },
                                    ..default()
                                })
                                .with_children(|k| {
                                    for pair in BINDINGS {
                                        let (text, color) = if col == 0 { (pair.0, GOLD) } else { (pair.1, BONE) };
                                        k.spawn(Node {
                                            height: Val::Px(19.0),
                                            align_items: AlignItems::Center,
                                            ..default()
                                        })
                                        .with_child((
                                            Text::new(text),
                                            skin.text(13.0),
                                            TextColor(color),
                                            shadow(),
                                        ));
                                    }
                                });
                            }
                        });
                    }
                });
                footer(p, |f| {
                    primary = next();
                    button(f, &skin, "Back", Action::Back, primary, 180.0, 38.0);
                });
            });
        }
        Screen::Connecting => {
            panel(r, &skin, 440.0, true, |p| {
                p.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|c| {
                    c.spawn((Text::new(""), skin.title(22.0), TextColor(GOLD), shadow(), StatusLine));
                    c.spawn((
                        Node { height: Val::Px(14.0), width: Val::Px(300.0), ..default() },
                        ImageNode::new(skin.rule.clone()),
                    ));
                    c.spawn((Text::new(""), skin.text(14.0), TextColor(DIM), shadow(), ConnectDots));
                });
                footer(p, |f| {
                    primary = next();
                    button(f, &skin, "Cancel", Action::Cancel, primary, 180.0, 38.0);
                });
            });
        }
        Screen::GameMenu => {
            panel(r, &skin, 330.0, true, |p| {
                header(p, &skin, "Game Menu", "The world does not wait.");
                p.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|c| {
                    for (label, action) in [
                        ("Resume", Action::Resume),
                        ("Options", Action::Options),
                        ("Quit to Menu", Action::QuitToMenu),
                        ("Quit Game", Action::QuitGame),
                    ] {
                        button(c, &skin, label, action, next(), 240.0, 38.0);
                    }
                });
            });
        }
    });
    model.focus_count = focus;
    if model.focus == 0 && matches!(model.screen, Screen::Play | Screen::Join | Screen::Connecting | Screen::Options) {
        model.focus = primary;
    }
    model.focus = model.focus.min(focus.saturating_sub(1));
}

#[derive(Component)]
struct SelectedTab;
#[derive(Component)]
struct ConnectDots;

fn panel(r: &mut ChildSpawnerCommands, skin: &Skin, w: f32, dark: bool, body: impl FnOnce(&mut ChildSpawnerCommands)) {
    // The frame image fills the outer node; the padding lives on an inner node (images are
    // drawn in the content box).
    r.spawn((
        Node { width: Val::Px(w), flex_direction: FlexDirection::Column, ..default() },
        panel_image(skin, dark),
        BoxShadow::new(Color::BLACK.with_alpha(0.75), Val::Px(0.0), Val::Px(8.0), Val::Px(4.0), Val::Px(18.0)),
        CapturesPointer,
    ))
    .with_children(|o| {
        o.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(10.0),
            padding: UiRect::new(Val::Px(36.0), Val::Px(36.0), Val::Px(24.0), Val::Px(26.0)),
            ..default()
        })
        .with_children(body);
    });
}

fn header(p: &mut ChildSpawnerCommands, skin: &Skin, title: &str, sub: &str) {
    p.spawn(Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: Val::Px(3.0),
        margin: UiRect::bottom(Val::Px(4.0)),
        ..default()
    })
    .with_children(|h| {
        h.spawn((
            Text::new(title),
            skin.title(26.0),
            TextColor(GOLD),
            TextShadow { offset: Vec2::new(0.0, 2.0), color: Color::srgba(0.30, 0.03, 0.02, 0.95) },
        ));
        h.spawn((
            Node { width: Val::Px(300.0), height: Val::Px(14.0), ..default() },
            ImageNode::new(skin.rule.clone()),
        ));
        if !sub.is_empty() {
            h.spawn((Text::new(sub), skin.text(13.0), TextColor(DIM), shadow()));
        }
    });
}

fn footer(p: &mut ChildSpawnerCommands, body: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn(Node {
        justify_content: JustifyContent::Center,
        column_gap: Val::Px(18.0),
        margin: UiRect::top(Val::Px(6.0)),
        ..default()
    })
    .with_children(body);
}

fn error_line(p: &mut ChildSpawnerCommands, skin: &Skin) {
    p.spawn((
        Node { justify_content: JustifyContent::Center, min_height: Val::Px(18.0), ..default() },
        Text::new(""),
        skin.text(14.0),
        TextColor(ERROR),
        shadow(),
        TextLayout::justify(Justify::Center),
        ErrorLine,
    ));
}

fn class_box(p: &mut ChildSpawnerCommands, skin: &Skin) {
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
            border: UiRect::all(Val::Px(1.0)),
            margin: UiRect::bottom(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.02, 0.02, 0.55)),
        BorderColor::all(Color::srgba(0.55, 0.42, 0.24, 0.6)),
    ))
    .with_children(|b| {
        b.spawn(Node { column_gap: Val::Px(10.0), align_items: AlignItems::Baseline, ..default() }).with_children(
            |t| {
                t.spawn((Text::new(""), skin.title(18.0), TextColor(GOLD), shadow(), ClassPart::Title));
                t.spawn((Text::new(""), skin.text(13.0), TextColor(DIM), shadow(), ClassPart::Role));
            },
        );
        b.spawn((Text::new(""), skin.text(14.0), TextColor(BONE), shadow(), ClassPart::Line));
        b.spawn((Text::new(""), skin.text(13.0), TextColor(PARCH), shadow(), ClassPart::Stats));
    });
}

// ---------------------------------------------------------------- behaviour

#[derive(SystemParam)]
struct Ctx<'w> {
    model: ResMut<'w, MenuModel>,
    settings: ResMut<'w, Settings>,
    next: ResMut<'w, NextState<AppState>>,
    state: Res<'w, State<AppState>>,
    exit: MessageWriter<'w, AppExit>,
    sfx: MessageWriter<'w, PlaySfx>,
    data: Res<'w, GameData>,
}

impl Ctx<'_> {
    fn activate(&mut self, commands: &mut Commands, action: &Action) {
        let m = &mut self.model;
        match action {
            Action::Play | Action::Join => {
                m.error = None;
                m.connect_from = if *action == Action::Play { Screen::Play } else { Screen::Join };
                let to = m.connect_from;
                m.go(to);
                self.sfx.write(PlaySfx::ui("dialogue_open.wav"));
            }
            Action::Options => {
                m.options_back = if *self.state.get() == AppState::InGame { Screen::GameMenu } else { Screen::Main };
                m.go(Screen::Options);
                self.sfx.write(PlaySfx::ui(builtin::WINDOW_OPEN));
            }
            Action::Quit | Action::QuitGame => {
                self.settings.save();
                self.exit.write(AppExit::Success);
            }
            Action::Back => self.back(commands),
            Action::Tab(i) => {
                if m.options_tab != *i {
                    m.options_tab = *i;
                    m.rebuild = true;
                    m.focus = *i;
                }
            }
            Action::Recent(i) => {
                if let Some(a) = self.settings.servers.get(*i) {
                    m.addr = a.clone();
                }
            }
            Action::Start | Action::Connect => self.start(commands, *action == Action::Connect),
            Action::Cancel => self.back(commands),
            Action::Resume => {
                m.go(Screen::None);
                self.sfx.write(PlaySfx::ui(builtin::WINDOW_CLOSE));
            }
            Action::QuitToMenu => {
                m.go(Screen::None);
                self.next.set(AppState::Menu);
            }
        }
    }

    fn back(&mut self, commands: &mut Commands) {
        let m = &mut self.model;
        match m.screen {
            Screen::Main | Screen::None => {}
            Screen::Play | Screen::Join => {
                m.error = None;
                m.go(Screen::Main);
            }
            Screen::Options => {
                self.settings.save();
                let to = m.options_back;
                m.go(to);
                m.focus = if to == Screen::GameMenu { 1 } else { 2 };
                self.sfx.write(PlaySfx::ui(builtin::WINDOW_CLOSE));
            }
            Screen::Connecting => {
                commands.remove_resource::<PendingConnect>();
                let to = m.connect_from;
                m.go(to);
                if *self.state.get() == AppState::Connecting {
                    self.next.set(AppState::Menu);
                }
            }
            Screen::GameMenu => {
                m.go(Screen::None);
                self.sfx.write(PlaySfx::ui(builtin::WINDOW_CLOSE));
            }
        }
    }

    fn start(&mut self, commands: &mut Commands, online: bool) {
        let m = &mut self.model;
        let name = settings::clean_name(&m.name);
        if name.is_empty() {
            m.error = Some("Every sword needs a name.".into());
            return;
        }
        m.error = None;
        let class = m.class;
        self.settings.name = name.clone();
        self.settings.class = class;
        let target = if online {
            let mut addr = m.addr.trim().to_string();
            if addr.is_empty() {
                m.error = Some("Where to? Enter a server address.".into());
                return;
            }
            if !addr.contains(':') {
                addr = format!("{addr}:{}", dusk_protocol::DEFAULT_PORT);
            }
            m.addr = addr.clone();
            m.status = format!("Knocking at {addr}");
            Target::Online(addr)
        } else {
            let map = m.maps.get(m.map).map(|(n, _)| n.clone()).unwrap_or_else(|| settings::DEFAULT_MAP.into());
            self.settings.map = map.clone();
            m.status = "Kindling the vale".into();
            Target::Offline(map)
        };
        self.settings.save();
        let rx = connect_in_background(self.data.root.clone(), target);
        let addr = online.then(|| m.addr.clone());
        commands.insert_resource(PendingConnect { rx, name, class, addr });
        m.go(Screen::Connecting);
        self.next.set(AppState::Connecting);
        self.sfx.write(PlaySfx::ui("quest_accept.wav"));
    }

    fn adjust(&mut self, w: &Widget, dir: i32) {
        let s = &mut self.settings;
        let m = &mut self.model;
        match w {
            Widget::Slider(k) => {
                let v = slider_value(s, *k);
                set_slider(s, *k, (v as i32 + dir * 5).clamp(0, 100) as u32);
            }
            Widget::Toggle(k) => toggle_value(s, *k),
            Widget::Cycle(CycleKey::Resolution) => {
                let i = RESOLUTIONS.iter().position(|r| *r == s.resolution).unwrap_or(0) as i32;
                s.resolution = RESOLUTIONS[(i + dir).rem_euclid(RESOLUTIONS.len() as i32) as usize];
            }
            Widget::Cycle(CycleKey::UiScale) => {
                let i = UI_SCALES.iter().position(|r| (*r - s.ui_scale).abs() < 0.01).unwrap_or(1) as i32;
                s.ui_scale = UI_SCALES[(i + dir).clamp(0, UI_SCALES.len() as i32 - 1) as usize];
            }
            Widget::Cycle(CycleKey::Map) => {
                if !m.maps.is_empty() {
                    m.map = (m.map as i32 + dir).rem_euclid(m.maps.len() as i32) as usize;
                }
            }
            Widget::Classes => {
                m.class = ((m.class as i32 - 1 + dir).rem_euclid(4) + 1) as u8;
            }
            Widget::Button(Action::Tab(_)) => {
                let t = (m.options_tab as i32 + dir).rem_euclid(OPTION_TABS.len() as i32) as usize;
                m.options_tab = t;
                m.rebuild = true;
                m.focus = t;
            }
            _ => {}
        }
    }
}

fn slider_value(s: &Settings, k: SliderKey) -> u32 {
    match k {
        SliderKey::Master => s.master,
        SliderKey::Music => s.music,
        SliderKey::Effects => s.effects,
    }
}

fn set_slider(s: &mut Settings, k: SliderKey, v: u32) {
    let slot = match k {
        SliderKey::Master => &mut s.master,
        SliderKey::Music => &mut s.music,
        SliderKey::Effects => &mut s.effects,
    };
    if *slot != v {
        *slot = v;
    }
}

fn toggle_state(s: &Settings, k: ToggleKey) -> bool {
    match k {
        ToggleKey::Fullscreen => s.fullscreen,
        ToggleKey::Vsync => s.vsync,
        ToggleKey::ShowFps => s.show_fps,
        ToggleKey::Shake => s.screen_shake,
        ToggleKey::HitStop => s.hit_stop,
    }
}

fn toggle_value(s: &mut Settings, k: ToggleKey) {
    match k {
        ToggleKey::Fullscreen => s.fullscreen = !s.fullscreen,
        ToggleKey::Vsync => s.vsync = !s.vsync,
        ToggleKey::ShowFps => s.show_fps = !s.show_fps,
        ToggleKey::Shake => s.screen_shake = !s.screen_shake,
        ToggleKey::HitStop => s.hit_stop = !s.hit_stop,
    }
}

enum Target {
    Offline(String),
    Online(String),
}

type ConnectResult = Result<(ClientConnection, Option<dusk_server::EmbeddedServer>), String>;

/// Starts the embedded server (offline) and opens the connection off the main thread.
fn connect_in_background(root: std::path::PathBuf, target: Target) -> Receiver<ConnectResult> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("menu-connect".into())
        .spawn(move || {
            let result = (|| -> ConnectResult {
                match target {
                    Target::Offline(map) => {
                        let config = dusk_server::ServerConfig { assets: root, start_map: Some(map) };
                        let server = dusk_server::EmbeddedServer::start(config)
                            .map_err(|e| format!("The vale would not wake: {e}"))?;
                        let conn = dusk_protocol::net::connect(server.addr)
                            .map_err(|e| format!("Cannot reach the embedded server: {e}"))?;
                        Ok((conn, Some(server)))
                    }
                    Target::Online(addr) => {
                        use std::net::ToSocketAddrs;
                        let addrs: Vec<_> =
                            addr.to_socket_addrs().map_err(|e| format!("No such place: {addr} ({e})"))?.collect();
                        let mut last = format!("No such place: {addr}");
                        for a in addrs {
                            match std::net::TcpStream::connect_timeout(&a, CONNECT_TIMEOUT) {
                                Ok(stream) => {
                                    return dusk_protocol::net::wrap(stream)
                                        .map(|c| (c, None))
                                        .map_err(|e| e.to_string());
                                }
                                Err(e) => {
                                    let why = match e.kind() {
                                        std::io::ErrorKind::ConnectionRefused => "refused".to_string(),
                                        std::io::ErrorKind::TimedOut => "no answer".to_string(),
                                        _ => e.to_string(),
                                    };
                                    last = format!("Nobody answers at {addr} ({why}).");
                                }
                            }
                        }
                        Err(last)
                    }
                }
            })();
            // A cancelled attempt has dropped the receiver; the server (if any) stops on drop.
            let _ = tx.send(result);
        })
        .expect("spawn connect thread");
    rx
}

fn poll_connect(
    mut commands: Commands,
    pending: Option<Res<PendingConnect>>,
    mut model: ResMut<MenuModel>,
    mut notice: ResMut<MenuNotice>,
    mut settings: ResMut<Settings>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(p) = pending else { return };
    match p.rx.try_recv() {
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            commands.remove_resource::<PendingConnect>();
        }
        Ok(Ok((conn, server))) => {
            net::begin_session(&mut commands, conn, &p.name, p.class);
            if let Some(addr) = &p.addr {
                settings.remember_server(addr);
                settings.save();
            }
            if let Some(server) = server {
                commands.insert_resource(OfflineServer(server));
            }
            commands.insert_resource(WelcomeWait::default());
            model.status = "Walking in from the gorge".into();
            commands.remove_resource::<PendingConnect>();
        }
        Ok(Err(why)) => {
            warn!("connection failed: {why}");
            commands.remove_resource::<PendingConnect>();
            notice.0 = Some(why);
            next.set(AppState::Menu);
        }
    }
}

fn welcome_timeout(
    time: Res<Time>,
    state: Res<State<AppState>>,
    wait: Option<ResMut<WelcomeWait>>,
    net: Option<Res<net::Net>>,
    mut session: crate::state::Session,
) {
    let (Some(mut wait), Some(_)) = (wait, net) else { return };
    if *state.get() != AppState::Connecting {
        return;
    }
    wait.0 += time.delta_secs();
    if wait.0 > WELCOME_TIMEOUT {
        wait.0 = f32::NEG_INFINITY;
        session.lost("The server did not answer.");
    }
}

/// Keyboard: move focus, adjust, activate, go back.
fn navigate(mut commands: Commands, input: Res<MenuInput>, mut ctx: Ctx, widgets: Query<(&Widget, &Focus)>) {
    if ctx.model.screen == Screen::None || ctx.model.built.map(|b| b.0) != Some(ctx.model.screen) {
        return;
    }
    let n = ctx.model.focus_count.max(1);
    let focused = widgets.iter().find(|(_, f)| f.0 == ctx.model.focus).map(|(w, _)| w.clone());
    let is_field = matches!(focused, Some(Widget::Field(_)));
    if input.esc {
        ctx.back(&mut commands);
        return;
    }
    if input.up || input.down {
        let d = if input.up { n - 1 } else { 1 };
        ctx.model.focus = (ctx.model.focus + d) % n;
    }
    let Some(w) = focused else { return };
    if input.left || input.right {
        let dir = if input.left { -1 } else { 1 };
        if !is_field {
            ctx.adjust(&w, dir);
        }
    }
    if input.enter {
        match &w {
            Widget::Button(a) => {
                let a = a.clone();
                ctx.sfx.write(PlaySfx::ui(builtin::BUTTON_CLICK));
                ctx.activate(&mut commands, &a);
            }
            Widget::Toggle(_) => ctx.adjust(&w, 1),
            Widget::Cycle(_) | Widget::Classes | Widget::Field(_) => {
                // Enter on the character panel starts; elsewhere it steps to the next control.
                if matches!(ctx.model.screen, Screen::Play | Screen::Join) {
                    let online = ctx.model.screen == Screen::Join;
                    ctx.start(&mut commands, online);
                } else {
                    ctx.model.focus = (ctx.model.focus + 1) % n;
                }
            }
            Widget::Slider(_) => ctx.model.focus = (ctx.model.focus + 1) % n,
        }
    }
}

/// Mouse: hover focuses, clicks activate, sliders drag.
#[allow(clippy::type_complexity)]
fn pointer(
    mut commands: Commands,
    mut ctx: Ctx,
    changed: Query<
        (&Interaction, &Widget, &Focus, Option<&RelativeCursorPosition>, Has<Disabled>),
        Changed<Interaction>,
    >,
    held: Query<(&Interaction, &Widget, &RelativeCursorPosition)>,
    cards: Query<(&Interaction, &ClassCard), Changed<Interaction>>,
) {
    if ctx.model.screen == Screen::None {
        return;
    }
    let mut todo: Vec<(Widget, i32)> = Vec::new();
    let mut act: Option<Action> = None;
    for (i, w, f, rel, disabled) in &changed {
        match i {
            Interaction::Hovered => ctx.model.focus = f.0,
            Interaction::Pressed if !disabled => {
                ctx.model.focus = f.0;
                match w {
                    Widget::Button(a) => act = Some(a.clone()),
                    Widget::Toggle(_) => todo.push((w.clone(), 1)),
                    Widget::Cycle(_) => {
                        let x = rel.and_then(|r| r.normalized).map_or(0.5, |n| n.x);
                        todo.push((w.clone(), if x < 0.0 { -1 } else { 1 }));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    for (i, w, rel) in &held {
        if *i == Interaction::Pressed
            && let (Widget::Slider(k), Some(n)) = (w, rel.normalized)
        {
            let v = ((n.x + 0.5).clamp(0.0, 1.0) * 100.0).round() as u32;
            set_slider(&mut ctx.settings, *k, v);
        }
    }
    for (i, card) in &cards {
        if *i == Interaction::Pressed {
            ctx.model.class = card.0;
        }
    }
    for (w, d) in todo {
        ctx.adjust(&w, d);
    }
    if let Some(a) = act {
        ctx.activate(&mut commands, &a);
    }
}

fn edit_text(input: Res<MenuInput>, mut model: ResMut<MenuModel>, widgets: Query<(&Widget, &Focus)>) {
    if input.typed.is_empty() && input.backspace == 0 {
        return;
    }
    let focus = model.focus;
    let Some(key) = widgets.iter().find_map(|(w, f)| match w {
        Widget::Field(k) if f.0 == focus => Some(*k),
        _ => None,
    }) else {
        return;
    };
    let (text, max, ok): (&mut String, usize, fn(char) -> bool) = match key {
        FieldKey::Name => (&mut model.name, settings::MAX_NAME, |c: char| c.is_alphanumeric()),
        FieldKey::Address => {
            (&mut model.addr, 64, |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']'))
        }
    };
    for _ in 0..input.backspace {
        text.pop();
    }
    for c in input.typed.chars().filter(|c| ok(*c)) {
        if text.chars().count() < max {
            text.push(c);
        }
    }
}

/// Draws the widgets from the model and settings.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn refresh(
    time: Res<Time>,
    model: Res<MenuModel>,
    settings: Res<Settings>,
    skin: Option<Res<Skin>>,
    mut widgets: Query<(Entity, &Widget, &Focus, &Interaction, Option<&mut ImageNode>, Has<SelectedTab>)>,
    children: Query<&Children>,
    mut texts: Query<(&mut Text, &mut TextColor), With<ValueText>>,
    mut fills: Query<&mut Node, (With<SliderFill>, Without<SliderKnob>)>,
    mut knobs: Query<(&mut Node, &mut ImageNode), (With<SliderKnob>, Without<SliderFill>, Without<Widget>)>,
    cards: Query<(&ClassCard, &Children)>,
    mut rings: Query<&mut Visibility, With<ClassRing>>,
    mut lines: ParamSet<(
        Query<&mut Text, (With<ErrorLine>, Without<ValueText>)>,
        Query<&mut Text, (With<StatusLine>, Without<ValueText>)>,
        Query<&mut Text, (With<ConnectDots>, Without<ValueText>)>,
        Query<(&mut Text, &ClassPart), Without<ValueText>>,
        Query<(&mut Text, &SliderValue), Without<ValueText>>,
        Query<&mut Text, (With<FieldText>, Without<ValueText>)>,
    )>,
) {
    let Some(skin) = skin else { return };
    if model.screen == Screen::None {
        return;
    }
    let caret = ((time.elapsed_secs() * 2.0) as u32).is_multiple_of(2);
    for (e, w, f, i, image, tab) in &mut widgets {
        let focused = f.0 == model.focus;
        let pressed = *i == Interaction::Pressed;
        match w {
            Widget::Button(_) => {
                if let Some(mut img) = image {
                    let art = button_art(&skin, focused || tab, pressed, false);
                    if img.image != art {
                        img.image = art;
                    }
                }
                for c in children.iter_descendants(e) {
                    if let Ok((_, mut col)) = texts.get_mut(c) {
                        col.0 = if focused {
                            GOLD
                        } else if tab {
                            BONE
                        } else {
                            PARCH
                        };
                    }
                }
            }
            Widget::Field(k) => {
                if let Some(mut img) = image {
                    let art = field_art(&skin, focused);
                    if img.image != art {
                        img.image = art;
                    }
                }
                let v = match k {
                    FieldKey::Name => &model.name,
                    FieldKey::Address => &model.addr,
                };
                for c in children.iter_descendants(e) {
                    if let Ok(mut t) = lines.p5().get_mut(c) {
                        let s = format!("{v}{}", if focused && caret { "|" } else { " " });
                        if t.0 != s {
                            t.0 = s;
                        }
                    }
                }
            }
            Widget::Toggle(k) => {
                if let Some(mut img) = image {
                    let art = skin.check[toggle_state(&settings, *k) as usize].clone();
                    if img.image != art {
                        img.image = art;
                    }
                    img.color = if focused { Color::srgb(1.25, 1.1, 0.9) } else { Color::WHITE };
                }
            }
            Widget::Cycle(k) => {
                if let Some(mut img) = image {
                    let art = field_art(&skin, focused);
                    if img.image != art {
                        img.image = art;
                    }
                }
                let v = match k {
                    CycleKey::Resolution => format!("{} x {}", settings.resolution.0, settings.resolution.1),
                    CycleKey::UiScale => format!("{:.0} %", settings.ui_scale * 100.0),
                    CycleKey::Map => model.maps.get(model.map).map_or("-".into(), |(_, l)| l.clone()),
                };
                for c in children.iter_descendants(e) {
                    if let Ok((mut t, mut col)) = texts.get_mut(c) {
                        if t.0 != v {
                            t.0 = v.clone();
                        }
                        col.0 = if focused { GOLD } else { BONE };
                    }
                }
            }
            Widget::Slider(k) => {
                let v = slider_value(&settings, *k) as f32 / 100.0;
                let inner = SLIDER_W - 6.0;
                for c in children.iter_descendants(e) {
                    if let Ok(mut n) = fills.get_mut(c) {
                        n.width = Val::Px((inner * v).round());
                    }
                    if let Ok((mut n, mut img)) = knobs.get_mut(c) {
                        n.left = Val::Px(((SLIDER_W - 14.0) * v).round());
                        let art = skin.knob[(focused || pressed) as usize].clone();
                        if img.image != art {
                            img.image = art;
                        }
                    }
                }
            }
            Widget::Classes => {}
        }
    }
    for (card, kids) in &cards {
        let selected = card.0 == model.class;
        for c in kids.iter() {
            for d in children.iter_descendants(c).chain(std::iter::once(c)) {
                if let Ok(mut v) = rings.get_mut(d) {
                    v.set_if_neq(if selected { Visibility::Inherited } else { Visibility::Hidden });
                }
                if let Ok((_, mut col)) = texts.get_mut(d) {
                    col.0 = if selected { GOLD } else { DIM };
                }
            }
        }
    }
    let err = model.error.clone().unwrap_or_default();
    for mut t in &mut lines.p0() {
        if t.0 != err {
            t.0 = err.clone();
        }
    }
    for mut t in &mut lines.p1() {
        if t.0 != model.status {
            t.0 = model.status.clone();
        }
    }
    let dots = ".".repeat(1 + (time.elapsed_secs() * 2.0) as usize % 3);
    for mut t in &mut lines.p2() {
        t.0 = format!("{dots}   the Eye is watching   {dots}");
    }
    if let Some(info) = model.classes.get(model.class as usize - 1) {
        for (mut t, part) in &mut lines.p3() {
            let v = match part {
                ClassPart::Title => &info.name,
                ClassPart::Role => &info.role,
                ClassPart::Line => &info.line,
                ClassPart::Stats => &info.stats,
            };
            if t.0 != *v {
                t.0 = v.clone();
            }
        }
    }
    for (mut t, sv) in &mut lines.p4() {
        let v = format!("{}", slider_value(&settings, sv.0));
        if t.0 != v {
            t.0 = v;
        }
    }
}
