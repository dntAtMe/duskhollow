//! Standalone server. Usage: `cargo run -p dusk_server -- [PORT] [--start-map NAME]`
//! (default port 16383; players start at the `arrival` marker of the default map of
//! `data/maps.txt`).

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
    let config = ServerConfig { assets: dusk_formats::content_root(), start_map };
    let mut app = build_app(&config, Acceptor(rx)).expect("load world data");
    app.add_plugins(LogPlugin::default());
    app.add_systems(Startup, move || info!("listening on {addr}"));
    app.run()
}
