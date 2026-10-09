//! App states and the play session.
//!
//! ```text
//! Boot ─┬─> Menu ──Play/Join──> Connecting ──Welcome──> InGame
//!       │    ^                      │                     │
//!       │    └── failure / cancel ──┘                     │
//!       │    └──────── Quit to menu / disconnect ─────────┘
//!       └─> Connecting (command line / debug launch: no menu, exactly as before)
//! ```
//!
//! Gameplay systems run only in [`AppState::InGame`] (`in_game`); the HUD is spawned on
//! entering it. Everything the session spawns (units, HUD, effects, the minimap camera...) is
//! removed when it ends: entities with a `Transform` or a `Node` created after the session
//! started are despawned, except map tiles (the menu keeps showing the world) and audio.
//! Gameplay resources are reset with [`reset`] when a new session starts.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use std::collections::HashSet;

#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    /// First frames: fonts and data load; then `Menu` or (command-line launch) `Connecting`.
    #[default]
    Boot,
    Menu,
    /// Connected (or connecting), waiting for the server's `Welcome`.
    Connecting,
    InGame,
}

/// Run condition: the game session is live.
pub fn in_game(state: Option<Res<State<AppState>>>) -> bool {
    state.is_some_and(|s| *s.get() == AppState::InGame)
}

/// Run condition: connected or connecting (the net receive loop).
pub fn in_session(state: Option<Res<State<AppState>>>) -> bool {
    state.is_some_and(|s| matches!(s.get(), AppState::Connecting | AppState::InGame))
}

/// How the client was launched.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// No map / `--connect` / `--name` / `--class` argument and no debug variable: main menu.
    Menu,
    /// Command line or debug launch: straight into the game; leaving it quits the app.
    Direct,
}

impl Launch {
    pub fn is_menu(&self) -> bool {
        *self == Launch::Menu
    }
}

/// Why the last session ended, shown by the main menu (`None`: the player chose to leave).
#[derive(Resource, Default, Debug, Clone)]
pub struct MenuNotice(pub Option<String>);

/// Session control for the net loop: `Welcome` starts the game, a lost or refused connection
/// returns to the menu (or quits a command-line launch, as before the menu existed).
#[derive(SystemParam)]
pub struct Session<'w> {
    state: Res<'w, State<AppState>>,
    next: ResMut<'w, NextState<AppState>>,
    launch: Res<'w, Launch>,
    notice: ResMut<'w, MenuNotice>,
    exit: MessageWriter<'w, AppExit>,
}

impl Session<'_> {
    /// Called on `Welcome`; true if this started the game (stop reading until it has).
    pub fn welcomed(&mut self) -> bool {
        if *self.state.get() == AppState::Connecting {
            self.next.set(AppState::InGame);
            true
        } else {
            false
        }
    }

    pub fn lost(&mut self, why: &str) {
        if self.launch.is_menu() {
            self.notice.0 = Some(why.to_string());
            self.next.set(AppState::Menu);
        } else {
            self.exit.write(AppExit::error());
        }
    }
}

/// Command-line launch: the connection `main` opened, used once on boot (name, class).
#[derive(Resource)]
pub struct DirectConnect(pub Option<(dusk_protocol::net::ClientConnection, String, u8)>);

/// The embedded server of an offline session; dropping it stops the server.
#[derive(Resource)]
pub struct OfflineServer(#[allow(dead_code)] pub dusk_server::EmbeddedServer);

/// Root entities (with a `Transform` or a `Node`) that existed before the session started.
#[derive(Resource, Default)]
struct BeforeSession(HashSet<Entity>);

/// Replaces a resource with a fresh one (`OnEnter(Connecting)`: a new session starts clean).
pub fn reset<R: Resource + FromWorld>(world: &mut World) {
    let fresh = R::from_world(world);
    world.insert_resource(fresh);
}

pub struct StatePlugin;

impl Plugin for StatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .init_resource::<MenuNotice>()
            .init_resource::<BeforeSession>()
            .add_systems(OnEnter(AppState::Connecting), remember_world)
            .add_systems(OnExit(AppState::InGame), end_session)
            .add_systems(OnTransition { exited: AppState::Connecting, entered: AppState::Menu }, end_session);
    }
}

type Roots<'w, 's> = Query<'w, 's, Entity, (Or<(With<Transform>, With<Node>)>, Without<ChildOf>)>;

fn remember_world(roots: Roots, mut before: ResMut<BeforeSession>) {
    before.0 = roots.iter().collect();
}

/// Despawns everything the session created and drops the connection / embedded server.
#[allow(clippy::type_complexity)]
fn end_session(
    mut commands: Commands,
    roots: Query<
        Entity,
        (
            Or<(With<Transform>, With<Node>)>,
            Without<ChildOf>,
            Without<crate::map_render::MapTile>,
            Without<AudioPlayer>,
            Without<crate::menu::KeepOnMenu>,
        ),
    >,
    mut before: ResMut<BeforeSession>,
) {
    let mut n = 0;
    for e in &roots {
        if !before.0.contains(&e) {
            commands.entity(e).despawn();
            n += 1;
        }
    }
    before.0.clear();
    commands.remove_resource::<crate::net::Net>();
    commands.remove_resource::<crate::net::PlayerState>();
    commands.remove_resource::<OfflineServer>();
    info!("session ended: {n} entities removed");
}
