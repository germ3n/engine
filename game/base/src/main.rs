pub mod entities;
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
use crate::network::OUTBOUND_CAP;
use std::net::SocketAddr;
use std::str::FromStr;

fn main() {
    let cmdargs = console::get_cmdline_args();
    println!("Game startup");
    println!("Map: {:?}", cmdargs.map);
    println!("Tick Rate: {}", cmdargs.tickrate);
    let tick_interval = 1.0 / cmdargs.tickrate as f64;
    println!("Tick Interval: {}", tick_interval);
    
    #[cfg(feature = "server")]
    {
        println!("Starting server network loop");
        let (server_tx, server_rx) = std::sync::mpsc::channel();
        let (server_out_tx, server_out_rx) = std::sync::mpsc::sync_channel(OUTBOUND_CAP);
        std::thread::spawn(move || {
            server::server_network_loop(server_tx, server_out_rx);
        });

        let server_game = GameState::new(
            Realm::Server,
            server_rx,
            server_out_tx,
            tick_interval
        );

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
        println!("Starting Client network loop");
        std::thread::spawn(move || {
            client::client_network_loop(
                SocketAddr::from_str("127.0.0.1:25400").expect("Failed to create SocketAddr"), 
                client_tx, 
                client_out_rx
            );
        });

        let client_game = GameState::new(
            Realm::Client,
            client_rx,
            client_out_tx,
            tick_interval
        );
        println!("Entering Client loop");
        client::client_loop(client_game);
    }
}