pub mod entities;
pub mod world;
pub mod network;
pub mod state;
pub mod client;
pub mod server;
pub mod console;
pub mod ui;
pub mod script;
pub mod r#enum;

use crate::state::GameState;
use crate::script::Realm;
use crate::network::{wake_pair, NetWake, OUTBOUND_CAP};
use std::net::SocketAddr;
use std::str::FromStr;
#[cfg(feature = "client")]
use std::sync::atomic::AtomicBool;
#[cfg(feature = "client")]
use std::sync::Arc;

fn main() {
    let cmdargs = console::get_cmdline_args();
    println!("Game startup");
    println!("Map: {:?}", cmdargs.map);
    println!("Tick Rate: {}", cmdargs.tickrate);
    let tick_interval = 1.0 / cmdargs.tickrate as f64;
    println!("Tick Interval: {}", tick_interval);

    if cmdargs.compile_map {
        let map_name = cmdargs.map.clone().unwrap_or_else(|| "hall".to_string());

        if let Err(err) = crate::world::compile_map(&map_name) {
            println!("[map] {err}");
            std::process::exit(1);
        }

        return;
    }

    #[cfg(feature = "server")]
    {
        println!("Starting server network loop");
        let (server_tx, server_rx) = std::sync::mpsc::channel();
        let (server_out_tx, server_out_rx) = std::sync::mpsc::sync_channel(OUTBOUND_CAP);
        let (server_wake_read, server_wake_write) = wake_pair();
        std::thread::spawn(move || {
            server::server_network_loop(server_tx, server_out_rx, server_wake_read);
        });

        let mut server_game = GameState::new(
            Realm::Server,
            server_rx,
            server_out_tx,
            tick_interval,
            NetWake::new(server_wake_write),
        );
        server_game.voxel_world.fill(
            crate::world::BlockPos::new(-12, -12, 0),
            crate::world::BlockPos::new(12, 12, 1),
            crate::world::Block(1),
        );
        server_game.voxel_world.fill(
            crate::world::BlockPos::new(-2, -2, 1),
            crate::world::BlockPos::new(3, 3, 4),
            crate::world::Block(2),
        );
        let map_name = cmdargs.map.clone().unwrap_or_else(|| "hall".to_string());
        server_game.map_name = map_name.clone();

        if let Err(err) = server_game.brush_world.load_file(&map_name) {
            println!("[map] {err}");
        } else {
            println!("[map] {map_name}");
        }

        #[cfg(feature = "client")]
        std::thread::spawn(move || {
            println!("Starting Server loop");
            server::server_loop(server_game);
        });

        #[cfg(not(feature = "client"))]
        {
            println!("Entering Server loop");
            server::server_loop(server_game);
        }
    }

    #[cfg(feature = "client")]
    {
        let (client_tx, client_rx) = std::sync::mpsc::channel();
        let (client_out_tx, client_out_rx) = std::sync::mpsc::sync_channel(OUTBOUND_CAP);
        let (client_wake_read, client_wake_write) = wake_pair();
        let shutdown = Arc::new(AtomicBool::new(false));
        let net_shutdown = Arc::clone(&shutdown);
        println!("Starting Client network loop");
        let net = std::thread::spawn(move || {
            client::client_network_loop(
                SocketAddr::from_str("127.0.0.1:25400").expect("Failed to create SocketAddr"), 
                client_tx, 
                client_out_rx,
                net_shutdown,
                client_wake_read,
            );
        });

        let mut client_game = GameState::new(
            Realm::Client,
            client_rx,
            client_out_tx,
            tick_interval,
            NetWake::new(client_wake_write),
        );
        let map_name = cmdargs.map.clone().unwrap_or_else(|| "hall".to_string());
        client_game.map_name = map_name.clone();

        if let Err(err) = client_game.brush_world.load_file(&map_name) {
            println!("[map] {err}");
        } else {
            println!("[map] {map_name}");
        }
        println!("Entering Client loop");
        client::client_loop(client_game, shutdown);
        let _ = net.join();
    }
}