//! Duskhollow client (Bevy).
//!
//! ```text
//! dusk_client [MAP] [--class N] [--art custom]                offline: embedded server, start on MAP
//! dusk_client --connect [HOST:PORT] [--name NAME] [--class N] online, world from dusk_server
//! ```
//! Classes are `player_class_stats.Class` 1..=4.

mod audio;
mod chat;
mod combat_ui;
mod data;
mod dialogue;
mod director_ui;
mod feel;
mod gaze;
mod hud;
mod iso;
mod items_ui;
mod lights;
mod map_render;
mod minimap;
mod nameplates;
mod net;
mod paper_doll;
mod particles;
mod player;
mod spell_fx;
mod spell_particles;
mod spells_ui;
mod ui_input;
mod unit;

use bevy::prelude::*;

enum Mode {
    /// Start map for the embedded server (`None` = original start point).
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
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1).peekable();
    let (mut map, mut addr, mut name, mut class) = (None, None, None, 1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--connect" => {
                let next_is_value = args.peek().is_some_and(|n| !n.starts_with("--"));
                addr = Some(if next_is_value {
                    args.next().unwrap()
                } else {
                    format!("127.0.0.1:{}", dusk_protocol::DEFAULT_PORT)
                });
            }
            "--name" => name = args.next(),
            "--class" => class = args.next().and_then(|c| c.parse().ok()).unwrap_or(1),
            // Read by `GameData::load` (custom player art); consume its value here.
            "--art" => {
                args.next();
            }
            _ => map = Some(a),
        }
    }
    Args {
        mode: match addr {
            Some(addr) => Mode::Online { addr },
            None => Mode::Offline { map },
        },
        name: name.unwrap_or_else(|| format!("Player{}", std::process::id() % 1000)),
        class,
    }
}

fn main() -> AppExit {
    let root = dusk_formats::assets_root();
    let game_data =
        data::GameData::load(&root).expect("failed to load game data (run `cargo run -p dusk_extract` first)");

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin { file_path: root.to_string_lossy().into_owned(), ..default() })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Duskhollow".into(),
                    resolution: (1280, 720).into(),
                    ..default()
                }),
                ..default()
            })
            // Pixel art: no filtering.
            .set(ImagePlugin::default_nearest()),
    )
    .insert_resource(ClearColor(Color::BLACK))
    .insert_resource(game_data)
    .init_resource::<map_render::CurrentMap>()
    .add_plugins((map_render::MapRenderPlugin, unit::UnitPlugin, player::PlayerPlugin, combat_ui::CombatUiPlugin))
    .add_plugins((spells_ui::SpellsUiPlugin, spell_fx::SpellFxPlugin, audio::AudioPlugin))
    .add_plugins((ui_input::UiInputPlugin, hud::HudPlugin, chat::ChatPlugin))
    .add_plugins((nameplates::NameplatePlugin, minimap::MinimapPlugin))
    .add_plugins((particles::ParticlesPlugin, lights::LightsPlugin, spell_particles::SpellParticlesPlugin))
    .add_plugins((items_ui::ItemsUiPlugin, paper_doll::PaperDollPlugin, gaze::GazePlugin))
    .add_plugins((dialogue::DialoguePlugin, director_ui::DirectorUiPlugin, feel::FeelPlugin))
    .add_systems(Update, auto_screenshot.run_if(|| std::env::var_os("DUSK_SCREENSHOT").is_some()));

    let args = parse_args();
    let addr = match args.mode {
        Mode::Online { addr } => addr,
        Mode::Offline { map } => {
            let config = dusk_server::ServerConfig { assets: root.clone(), start_map: map };
            dusk_server::spawn_embedded(config).expect("failed to start embedded server").to_string()
        }
    };
    app.add_plugins(net::NetPlugin { addr, name: args.name, class: args.class });
    app.run()
}

/// Debug aid: `DUSK_SCREENSHOT=out.png` saves a screenshot after `DUSK_SCREENSHOT_AT` seconds
/// (default 4), then exits.
fn auto_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    mut taken: Local<Option<f32>>,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let now = time.elapsed_secs();
    match *taken {
        None if now > std::env::var("DUSK_SCREENSHOT_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(4.0) => {
            let path = std::env::var("DUSK_SCREENSHOT").unwrap();
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            *taken = Some(now);
        }
        Some(t) if now > t + 1.0 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
