//! Duskhollow client (Bevy).
//!
//! ```text
//! dusk_client                                                 main menu (Play offline, Join, Options)
//! dusk_client [MAP] [--class N]                               offline: embedded server, start on MAP
//! dusk_client --connect [HOST:PORT] [--name NAME] [--class N] online, world from dusk_server
//! ```
//! Classes are `player_class_stats.Class` 1..=4. Any game argument, or a debug variable
//! (`DUSK_SCREENSHOT`, `DUSK_AUTOPLAY`, `DUSK_*_TEST`...), skips the menu and starts the game
//! directly, as before the menu existed; `DUSK_MENU` forces the menu (see `menu`).

mod audio;
mod chat;
mod combat_ui;
mod data;
mod dialogue;
mod director_ui;
mod env_light;
mod feel;
mod gaze;
mod hud;
mod iso;
mod items_ui;
mod journal;
mod lights;
mod map_render;
mod menu;
mod minimap;
mod nameplates;
mod net;
mod paper_doll;
mod particles;
mod player;
mod settings;
mod spell_fx;
mod spell_particles;
mod spells_ui;
mod state;
mod ui_input;
mod unit;
mod windows;

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, WindowMode};
use state::Launch;

enum Mode {
    /// Start map for the embedded server (`None` = the default map of `data/maps.txt`).
    Offline {
        map: Option<String>,
    },
    Online {
        addr: String,
    },
}

struct Args {
    mode: Mode,
    name: String,
    class: u8,
    /// A map, `--connect`, `--name` or `--class` was given.
    game_args: bool,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1).peekable();
    let (mut map, mut addr, mut name, mut class, mut game_args) = (None, None, None, 1, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--connect" => {
                let next_is_value = args.peek().is_some_and(|n| !n.starts_with("--"));
                addr = Some(if next_is_value {
                    args.next().unwrap()
                } else {
                    format!("127.0.0.1:{}", dusk_protocol::DEFAULT_PORT)
                });
                game_args = true;
            }
            "--name" => {
                name = args.next();
                game_args = true;
            }
            "--class" => {
                class = args.next().and_then(|c| c.parse().ok()).unwrap_or(1);
                game_args = true;
            }
            _ => {
                map = Some(a);
                game_args = true;
            }
        }
    }
    Args {
        mode: match addr {
            Some(addr) => Mode::Online { addr },
            None => Mode::Offline { map },
        },
        name: name.unwrap_or_else(|| format!("Player{}", std::process::id() % 1000)),
        class,
        game_args,
    }
}

/// Environment variables that do not mean "start the game directly".
const MENU_SAFE_VARS: [&str; 10] = [
    "DUSK_ASSETS",
    "DUSK_AUDIO_LOG",
    "DUSK_MUSIC_VOLUME",
    "DUSK_SFX_VOLUME",
    "DUSK_SETTINGS",
    "DUSK_MENU_AT",
    "DUSK_MENU_TAB",
    "DUSK_MENU_CYCLES",
    "DUSK_MENU_JOIN",
    "DUSK_MENU",
];

fn launch_mode(args: &Args) -> Launch {
    match std::env::var("DUSK_MENU").ok().as_deref() {
        Some("pause" | "pause_options") => return Launch::Direct,
        Some(_) => return Launch::Menu,
        None => {}
    }
    let debug = std::env::vars().any(|(k, _)| {
        k.starts_with("DUSK_") && !MENU_SAFE_VARS.contains(&k.as_str()) && !k.starts_with("DUSK_SCREENSHOT_")
    });
    if args.game_args || debug { Launch::Direct } else { Launch::Menu }
}

fn main() -> AppExit {
    let root = dusk_formats::assets_root();
    let args = parse_args();
    let launch = launch_mode(&args);
    let settings = settings::Settings::load();
    let menu_launch = launch.is_menu();
    let game_data = data::GameData::load(&root)
        .unwrap_or_else(|e| panic!("failed to load game data from {} (set DUSK_ASSETS?): {e:#}", root.display()));

    // A menu launch opens the window as the player left it; the command line keeps 1280x720.
    let mut window = Window { title: "Duskhollow".into(), resolution: (1280, 720).into(), ..default() };
    if menu_launch {
        window.resolution = settings.resolution.into();
        window.present_mode = if settings.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
        if settings.fullscreen {
            window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
        }
    }

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin { file_path: root.to_string_lossy().into_owned(), ..default() })
            .set(WindowPlugin { primary_window: Some(window), ..default() })
            // Pixel art: no filtering.
            .set(ImagePlugin::default_nearest()),
    )
    .insert_resource(ClearColor(Color::BLACK))
    .insert_resource(game_data)
    .insert_resource(launch)
    .insert_resource(settings)
    .init_resource::<map_render::CurrentMap>()
    .add_plugins((state::StatePlugin, settings::SettingsPlugin, menu::MenuPlugin, net::NetPlugin))
    .add_plugins((map_render::MapRenderPlugin, unit::UnitPlugin, player::PlayerPlugin, combat_ui::CombatUiPlugin))
    .add_plugins((spells_ui::SpellsUiPlugin, spell_fx::SpellFxPlugin, audio::AudioPlugin))
    .add_plugins((ui_input::UiInputPlugin, hud::HudPlugin, chat::ChatPlugin))
    .add_plugins((nameplates::NameplatePlugin, minimap::MinimapPlugin))
    .add_plugins((particles::ParticlesPlugin, lights::LightsPlugin, spell_particles::SpellParticlesPlugin))
    .add_plugins((items_ui::ItemsUiPlugin, paper_doll::PaperDollPlugin, gaze::GazePlugin, env_light::EnvLightPlugin))
    .add_plugins((dialogue::DialoguePlugin, director_ui::DirectorUiPlugin, feel::FeelPlugin))
    .add_plugins((windows::WindowsPlugin, journal::JournalPlugin))
    .add_systems(Update, auto_screenshot.run_if(|| std::env::var_os("DUSK_SCREENSHOT").is_some()));

    if !menu_launch {
        // As before the menu: connect now (offline: embedded server first), fail loudly.
        let (addr, server) = match args.mode {
            Mode::Online { addr } => (addr, None),
            Mode::Offline { map } => {
                let config = dusk_server::ServerConfig { assets: root.clone(), start_map: map };
                let server = dusk_server::EmbeddedServer::start(config).expect("failed to start embedded server");
                (server.addr.to_string(), Some(server))
            }
        };
        let conn = dusk_protocol::net::connect(&addr)
            .unwrap_or_else(|e| panic!("cannot connect to {addr}: {e} (is dusk_server running?)"));
        app.insert_resource(state::DirectConnect(Some((conn, args.name, args.class))));
        if let Some(server) = server {
            app.insert_resource(state::OfflineServer(server));
        }
    }
    app.run()
}

/// Debug aid: `DUSK_SCREENSHOT=out.png` saves a screenshot after `DUSK_SCREENSHOT_AT` seconds
/// (default 4), then exits. `DUSK_SCREENSHOT_BURST=n` takes n shots `DUSK_SCREENSHOT_STEP` seconds
/// apart (default 0.1) as `<name>_<i>.png` instead (animation checks).
fn auto_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    mut taken: Local<Option<f32>>,
    mut shots: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d);
    let now = time.elapsed_secs();
    let burst = env("DUSK_SCREENSHOT_BURST", 1.0) as u32;
    let at = env("DUSK_SCREENSHOT_AT", 4.0) + *shots as f32 * env("DUSK_SCREENSHOT_STEP", 0.1);
    match *taken {
        None if now > at => {
            let mut path = std::env::var("DUSK_SCREENSHOT").unwrap();
            if burst > 1 {
                path = path.replace(".png", &format!("_{}.png", *shots));
            }
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            *shots += 1;
            if *shots >= burst {
                *taken = Some(now);
            }
        }
        Some(t) if now > t + 1.0 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
