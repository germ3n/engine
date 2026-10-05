mod ai;
mod anchor;
mod anim;
mod platform;

mod client;
mod console;
mod demo;
mod entities;
mod r#enum;
pub mod fs;
mod input;
mod lagcomp;
mod localize;
mod movement;
mod network;
mod physics;
mod scale;
mod script;
mod server;
mod sound;
mod state;
mod third_party;
mod ui;
mod world;

use crate::network::{wake_pair, NetWake, OUTBOUND_CAP};
use crate::script::Realm;
use crate::state::GameState;
use std::net::SocketAddr;
use std::str::FromStr;
#[cfg(feature = "client")]
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    crate::platform::remember(app);
    crate::platform::redirect_stdio();
    run();
}

#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn engine_main() {
    run();
}

pub fn editor(map_name: &str) {
    init_logging();
    ui::editor::run(map_name);
}

fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .try_init();
}

pub fn run() {
    init_logging();
    let cmdargs = console::get_cmdline_args();
    log::info!("Game startup");
    log::info!("Map: {:?}", cmdargs.map);
    log::info!("Tick Rate: {}", cmdargs.tickrate);
    let tick_interval = 1.0 / cmdargs.tickrate as f64;
    log::info!("Tick Interval: {}", tick_interval);

    if cmdargs.compile_map {
        let map_name = cmdargs.map.clone().unwrap_or_else(|| "hall".to_string());

        if let Err(err) = crate::world::compile_map(&map_name) {
            log::warn!("[map] {err}");
            std::process::exit(1);
        }

        return;
    }

    if cmdargs.editor {
        let map_name = cmdargs.map.clone().unwrap_or_else(|| "hall".to_string());
        editor(&map_name);

        return;
    }

    crate::network::steam::startup(crate::network::steam::Start {
        lobby: cmdargs.connect_lobby,
        connect: cmdargs.connect.clone(),
        dedicated: cmdargs.dedicated,
        insecure: cmdargs.insecure,
        map: cmdargs.map.clone().unwrap_or_else(|| "hall".to_string()),
    });
    crate::network::sim::configure(crate::network::sim::Settings {
        lag_ms: cmdargs.fakelag,
        jitter_ms: cmdargs.fakejitter,
        loss_pct: cmdargs.fakeloss,
    });

    let fs = match crate::fs::Fs::boot() {
        Ok(fs) => Arc::new(fs),
        Err(err) => {
            log::error!("[fs] {err}");
            std::process::exit(1);
        }
    };
    crate::fs::set_global(Arc::clone(&fs));
    log::info!("[fs] global filesystem installed");

    #[cfg(feature = "server")]
    let terminal_server = {
        log::info!("Starting server network loop");
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
            Arc::clone(&fs),
        );
        let map_name = cmdargs.map.clone().unwrap_or_else(|| "hall".to_string());
        server_game.map_name = map_name.clone();

        if let Err(err) = server_game.brush_world.load_file(&map_name) {
            log::warn!("[map] {err}");
        } else {
            let _ = server_game.brush_world.set_scale(cmdargs.map_scale);
            log::info!("[map] {map_name} scale {}", server_game.brush_world.scale());
        }

        server_game.nav.load_saved(&map_name);

        let side = console::ConsoleSide {
            cvars: Arc::clone(&server_game.cvars),
            binds: Arc::clone(&server_game.binds),
        };

        let listen = cfg!(feature = "client") && !cmdargs.dedicated;

        std::thread::spawn(move || {
            log::info!("Starting Server loop");
            server::server_loop(server_game, listen);
        });

        Some(side)
    };

    #[cfg(not(feature = "server"))]
    let terminal_server = None;

    #[cfg(feature = "client")]
    if cmdargs.dedicated {
        console::bind_sides(terminal_server.clone(), None);
        console::spawn_terminal(terminal_server, None);
        loop {
            std::thread::park();
        }
    }

    #[cfg(feature = "client")]
    {
        let server_addr = client_server_addr(cmdargs.connect.as_deref());
        let (client_tx, client_rx) = std::sync::mpsc::channel();
        let (client_out_tx, client_out_rx) = std::sync::mpsc::sync_channel(OUTBOUND_CAP);
        let (client_wake_read, client_wake_write) = wake_pair();
        let shutdown = Arc::new(AtomicBool::new(false));
        let net_shutdown = Arc::clone(&shutdown);
        let resync = Arc::new(AtomicBool::new(false));
        let net_resync = Arc::clone(&resync);
        log::info!("Starting Client network loop");
        let net = std::thread::spawn(move || {
            client::client_network_loop(
                server_addr,
                client_tx,
                client_out_rx,
                net_shutdown,
                client_wake_read,
                net_resync,
            );
        });

        crate::ui::webview::prefer_platform();
        let mut client_game = GameState::new(
            Realm::Client,
            client_rx,
            client_out_tx,
            tick_interval,
            NetWake::new(client_wake_write),
            Arc::clone(&fs),
        );
        let map_name = cmdargs.map.clone().unwrap_or_else(|| "hall".to_string());
        client_game.map_name = map_name.clone();

        if let Err(err) = client_game.brush_world.load_file(&map_name) {
            log::warn!("[map] {err}");
        } else {
            let _ = client_game.brush_world.set_scale(cmdargs.map_scale);
            log::info!("[map] {map_name} scale {}", client_game.brush_world.scale());
        }
        let terminal_client = console::ConsoleSide {
            cvars: Arc::clone(&client_game.cvars),
            binds: Arc::clone(&client_game.binds),
        };
        console::bind_sides(terminal_server.clone(), Some(terminal_client.clone()));
        console::spawn_terminal(terminal_server, Some(terminal_client));
        log::info!("Entering Client loop");
        client::client_loop(client_game, shutdown, resync);
        let _ = net.join();
    }

    #[cfg(not(feature = "client"))]
    drop(terminal_server);
}

#[cfg(feature = "client")]
fn client_server_addr(connect: Option<&str>) -> SocketAddr {
    if let Some(text) = connect {
        if let Ok(addr) = text.parse() {
            return addr;
        }
    }

    SocketAddr::from_str("127.0.0.1:25400").expect("Failed to create SocketAddr")
}
