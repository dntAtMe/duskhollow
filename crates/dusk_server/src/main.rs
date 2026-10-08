//! Standalone server. Usage: `cargo run -p dusk_server -- [PORT] [--start-map NAME]`
//! (default port 16383; default start is the original `start` teleport in fanadin).

use bevy::{log::LogPlugin, prelude::*};
use dusk_server::{ServerConfig, build_app, net::Acceptor};

fn main() -> AppExit {
    let (mut port, mut start_map) = (dusk_protocol::DEFAULT_PORT, None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--start-map" => start_map = args.next(),
            p => port = p.parse().expect("usage: dusk_server [PORT] [--start-map NAME]"),
        }
    }
    let (rx, addr) = dusk_protocol::net::listen(("0.0.0.0", port)).expect("bind server port");
    let config = ServerConfig { assets: dusk_formats::assets_root(), start_map };
    let mut app = build_app(&config, Acceptor(rx)).expect("load world data (run dusk_extract first)");
    app.add_plugins(LogPlugin::default());
    app.add_systems(Startup, move || info!("listening on {addr}"));
    app.run()
}
