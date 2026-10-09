//! Authoritative game server (headless Bevy), usable standalone (`main.rs`) or
//! embedded in the client for offline play ([`spawn_embedded`], or [`EmbeddedServer`] when it
//! has to be stopped again).

pub mod ai;
pub mod combat;
pub mod director;
pub mod eye;
pub mod gaze;
pub mod items;
pub mod net;
pub mod spells;
pub mod stats;
pub mod world;

use bevy::{app::ScheduleRunnerPlugin, prelude::*};
use std::{net::SocketAddr, path::PathBuf, time::Duration};

/// Server simulation rate.
pub const TICK_HZ: f64 = 20.0;

pub struct ServerConfig {
    /// The asset root ([`dusk_formats::assets_root`]).
    pub assets: PathBuf,
    /// Spawn players on this map instead of the default map (`data/maps.txt`).
    pub start_map: Option<String>,
}

/// Builds the server app around an already-bound acceptor. Logging is left to the caller
/// (the embedded server shares the client's logger).
pub fn build_app(config: &ServerConfig, acceptor: net::Acceptor) -> anyhow::Result<App> {
    let world = world::GameWorld::load(&config.assets, config.start_map.as_deref())?;
    let gaze = gaze::Gaze::load(&config.assets, &world);
    info!("loaded {} maps; players start on map {} at {:?}", world.maps.len(), world.start.0, world.start.1);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / TICK_HZ))))
        .insert_resource(world)
        .insert_resource(gaze)
        .insert_resource(acceptor)
        .init_resource::<world::NetIndex>()
        .init_resource::<world::Outbox>()
        .init_resource::<spells::CastRequests>()
        .init_resource::<spells::PendingEffects>()
        .insert_resource(items::ItemData::load(&config.assets)?)
        .init_resource::<items::ItemRequests>()
        .init_resource::<items::Kills>()
        .add_systems(Startup, world::spawn_npcs)
        .add_systems(
            Update,
            (
                (net::accept, net::handle_players, items::init_inventories, items::handle_item_requests).chain(),
                (ai::aggro, ai::wander, ai::chase, ai::evade).chain(),
                (
                    spells::npc_cast,
                    spells::start_casts,
                    spells::update_casts,
                    spells::apply_effects,
                    spells::tick_auras,
                    spells::tick_cooldowns,
                )
                    .chain(),
                (
                    combat::melee,
                    combat::npc_deaths,
                    combat::player_deaths,
                    combat::corpses,
                    combat::tick_combat_clocks,
                    combat::regen,
                    spells::combat_mana,
                    spells::sync_control,
                )
                    .chain(),
                (items::roll_loot, items::expire_loot).chain(),
                (gaze::apply_eye_commands, gaze::tick_eyes, gaze::update_strain, gaze::npc_mods).chain(),
                (net::broadcast_motion, net::flush_outbox).chain(),
            )
                .chain(),
        );
    director::plugin(&mut app, &config.assets);
    Ok(app)
}

/// Runs a server on a background thread bound to `127.0.0.1:0`; returns its address.
pub fn spawn_embedded(config: ServerConfig) -> anyhow::Result<SocketAddr> {
    let (rx, addr) = dusk_protocol::net::listen("127.0.0.1:0")?;
    // `App` is not `Send`, so it is built on the thread that runs it.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("embedded-server".into()).spawn(move || {
        match build_app(&config, net::Acceptor(rx)) {
            Ok(mut app) => {
                let _ = ready_tx.send(Ok(()));
                app.run();
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        }
    })?;
    ready_rx.recv()??;
    Ok(addr)
}

/// An embedded server that can be shut down: [`EmbeddedServer::stop`] (or dropping it) ends the
/// server loop, closes every client connection and frees the port.
pub struct EmbeddedServer {
    pub addr: SocketAddr,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl EmbeddedServer {
    /// Like [`spawn_embedded`], but keeps a handle to stop the server again (the client's
    /// "Quit to menu").
    pub fn start(config: ServerConfig) -> anyhow::Result<Self> {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (rx, addr) = dusk_protocol::net::listen("127.0.0.1:0")?;
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new().name("embedded-server".into()).spawn(move || {
            match build_app(&config, net::Acceptor(rx)) {
                Ok(mut app) => {
                    app.add_systems(Last, move |mut exit: MessageWriter<AppExit>| {
                        if flag.load(Ordering::Relaxed) {
                            exit.write(AppExit::Success);
                        }
                    });
                    let _ = ready_tx.send(Ok(()));
                    app.run();
                    info!("embedded server stopped");
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })?;
        ready_rx.recv()??;
        Ok(Self { addr, stop, thread: Some(thread) })
    }

    /// Stops the server loop and waits for it to wind down.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
            // The accept thread blocks in `accept`; one last connection lets it notice that the
            // server is gone (its channel is closed) and release the port.
            let _ = std::net::TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        }
    }
}

impl Drop for EmbeddedServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}
