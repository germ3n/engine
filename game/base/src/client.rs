use std::sync::mpsc::{Receiver, Sender};
use crate::state::GameState;
use crate::network::{ServerToClient, ClientToServer, NetworkClient};
use crate::ui::{opengl::OpenGLWindow, window::Window};
use winit::event::{WindowEvent, Event};
use glutin::prelude::GlSurface;
use winit::event_loop::ControlFlow;
use glow::HasContext;
use crate::ui::menu::draw_menu;
use crate::script::engine::DrawCommand;
use std::sync::atomic::Ordering;
use core::net::SocketAddr;
use std::str::FromStr;
use crate::network::{PacketType, ReliableChannel, EnqueueStatus, NetSend};
use crate::network::packet::{FragmentAssembler, CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL};
use crate::network::usermessage::UserMsgReader;
use crate::entities::context::FrameInfo;

pub fn client_loop(mut game: GameState<ServerToClient, ClientToServer>) {
    let mut client_window = OpenGLWindow::create_window();
    client_window.set_window_title("Rust Engine - Rendering");
    client_window.set_size(800, 600);

    let event_loop = client_window.event_loop.take().expect("Event loop missing");

    let menu_script = include_bytes!("lua/menu/menu.lua");
    game.script_engine.lua.load(&menu_script[..])
        .exec()
        .expect("Failed to execute menu.lua");

    let mut last_frame = std::time::Instant::now();
    let mut accumulated_time = 0.0;

    event_loop.run(move |event, window_target| {
        window_target.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    window_target.exit();
                }
                WindowEvent::Resized(physical_size) => {
                    client_window.set_size(physical_size.width, physical_size.height);
                    
                    unsafe {
                        client_window.gl.viewport(0, 0, physical_size.width as i32, physical_size.height as i32);
                    }
                }
                WindowEvent::RedrawRequested => {
                    unsafe {
                        client_window.gl.clear_color(0.0, 0.0, 0.0, 1.0);
                        client_window.gl.clear(glow::COLOR_BUFFER_BIT);
                    }

                    draw_menu(&mut client_window, &mut game);

                    let draw_commands = {
                        let mut q = game.script_engine.render_queue.lock().unwrap();
                        std::mem::take(&mut *q)
                    };
                    
                    //todo: optimize
                    for cmd in draw_commands {
                        match cmd {
                            DrawCommand::Rect { x, y, w, h, color } => {
                                client_window.draw_rectangle(x, y, w, h, color);
                            },
                            DrawCommand::OutlinedRect { x, y, w, h, thickness, color } => {
                                client_window.draw_outlined_rectangle(x, y, w, h, thickness, color);
                            },
                            DrawCommand::Text { font, text, x, y, scale, color } => {
                                client_window.draw_text(
                                    &font.to_str().unwrap().to_owned(), 
                                    &text.to_str().unwrap().to_owned(), 
                                    x,
                                    y, 
                                    scale, 
                                    color
                                );
                            },
                        }
                    }

                    client_window.render_text();
    
                    client_window.surface.swap_buffers(&client_window.context).unwrap();
                }
                _ => (),
            },
            Event::AboutToWait => {
                let now = std::time::Instant::now();
                let dt = now.duration_since(last_frame).as_secs_f64();
                last_frame = now;

                {
                    let ct = game.cur_time(); //game.cur_time.lock().unwrap();
                    game.cur_time.store((ct + dt as f64).to_bits(), Ordering::Relaxed);
                    game.frame_time.store(dt.to_bits(), Ordering::Relaxed);

                    accumulated_time += dt;
                    //let mut ticked = false;
                    
                    // Use a while loop to catch up if a frame lags
                    while accumulated_time >= game.tick_interval {
                        accumulated_time -= game.tick_interval;

                        game.entities.set_frame(FrameInfo {
                            dt: game.tick_interval,
                            cur_time: game.cur_time(),
                            tick_count: game.tick_count(),
                        });
                        game.entities.tick_all();

                        let tc = game.tick_count.load(Ordering::Relaxed);
                        game.tick_count.store(tc + 1, Ordering::Relaxed);
                        //ticked = true;
                    }

                    /*if !ticked {
                        std::thread::sleep(Duration::from_millis(1));
                    }*/
                }

                while let Ok(net_event) = game.network_receiver.try_recv() {
                    match net_event {
                        ServerToClient::PlayerConnected { handle, name } => {
                            game.script_engine.run_hook("PlayerConnected", (handle, name));
                        },
                        ServerToClient::PlayerDisconnected { handle } => {
                            game.script_engine.run_hook("PlayerDisconnected", handle);
                        },
                        ServerToClient::PlayerSpawned { handle } => {
                            game.script_engine.run_hook("PlayerSpawned", handle);
                        },
                        ServerToClient::PlayerDamaged { handle, attacker, inflictor, damage, new_health } => {
                            game.script_engine.run_hook("PlayerDamaged", (handle, attacker, inflictor, damage, new_health));
                        },
                        ServerToClient::PlayerDied { handle, killer, inflictor } => {
                            game.script_engine.run_hook("PlayerDied", (handle, killer, inflictor));
                        },
                        ServerToClient::ModelChanged { handle, model } => {
                            game.script_engine.run_hook("ModelChanged", (handle, model));
                        },
                        ServerToClient::TransformUpdated { handle, position, angles, velocity } => {
                            if let Some(entity) = game.entities.get_mut(handle) {
                                if let Some(pos) = position { entity.base_mut().position = pos; }
                                if let Some(ang) = angles { entity.base_mut().angles = ang; }
                                if let Some(vel) = velocity { entity.base_mut().velocity = vel; }
                            }

                            game.script_engine.run_hook("TransformUpdated", (handle, position, angles, velocity));
                        },

                        ServerToClient::UserMessage { hash, data } => {
                            game.script_engine.run_usermessage(hash, UserMsgReader::new(data));
                        },

                        _ => { panic!() },
                    }
                }

                client_window.window.request_redraw();
            },
            _ => (),
        }
    }).unwrap();
}

#[cfg(feature = "client")]
pub fn client_network_loop(server_addr: SocketAddr, tx: Sender<ServerToClient>, rx: Receiver<NetSend<ClientToServer>>) {
    let local_addr = if server_addr.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let mut client = NetworkClient::new(SocketAddr::from_str(local_addr).unwrap());
    client.connect(server_addr).expect("Failed to connect to server");
    let mut reliable_chan = ReliableChannel::new();
    let mut connected = false;
    let mut assembler = FragmentAssembler::new(); 

    let connect_bytes = wincode::serialize(&PacketType::Connect).unwrap();
    let _ = client.send_message(&connect_bytes);
    let mut challenge_response_bytes: Option<Vec<u8>> = None;
    let mut session: Option<u64> = None;
    let mut held_reliable: Option<Vec<u8>> = None;
    let mut last_sent = std::time::Instant::now();
    let mut last_server_seen = std::time::Instant::now();

    loop {
        if let Some(payload) = held_reliable.take() {
            match reliable_chan.enqueue(&payload) {
                EnqueueStatus::Queued => {}
                EnqueueStatus::Full => {
                    held_reliable = Some(payload);
                }
                EnqueueStatus::TooLarge => {
                    println!("[cl] reliable payload too large");
                }
            }
        }

        if held_reliable.is_none() {
            while let Ok(outgoing) = rx.try_recv() {
                match outgoing {
                    NetSend::Reliable(event) => {
                        let payload = wincode::serialize(&event).unwrap();
                        match reliable_chan.enqueue(&payload) {
                            EnqueueStatus::Queued => {}
                            EnqueueStatus::Full => {
                                held_reliable = Some(payload);

                                break;
                            }
                            EnqueueStatus::TooLarge => {
                                println!("[cl] reliable payload too large");
                            }
                        }
                    }
                    NetSend::Unreliable(event) => {
                        let payload = wincode::serialize(&event).unwrap();
                        let packet = PacketType::Unreliable(payload.into());
                        let bytes = wincode::serialize(&packet).unwrap();
                        let _ = client.send_message(&bytes);
                        last_sent = std::time::Instant::now();
                    }
                }
            }
        }

        reliable_chan.pump(|packet_bytes| {
            let _ = client.send_message(packet_bytes);
            last_sent = std::time::Instant::now();
        });

        match client.receive_message() {
            Ok((data, _from)) => {
                if let Ok(packet) = wincode::deserialize::<PacketType>(&data) {
                    match packet {
                        PacketType::Connect => {
                        }
                        PacketType::Challenge { token } => {
                            if !connected {
                                let bytes = wincode::serialize(&PacketType::ChallengeResponse { token }).unwrap();
                                challenge_response_bytes = Some(bytes.clone());
                                let _ = client.send_message(&bytes);
                                last_sent = std::time::Instant::now();
                                println!("[cl] challenge {token}");
                            }
                        }
                        PacketType::ChallengeResponse { .. } => {
                            println!("[cl] challenge response");
                        }
                        PacketType::Connected { session: new_session } => {
                            if !connected {
                                if let Some(previous) = session {
                                    if previous != new_session {
                                        reliable_chan = ReliableChannel::new();
                                        assembler = FragmentAssembler::new();
                                    }
                                }

                                connected = true;
                                session = Some(new_session);
                                challenge_response_bytes = None;
                                last_server_seen = std::time::Instant::now();
                                println!("[cl] connected");
                            } else if session == Some(new_session) {
                                last_server_seen = std::time::Instant::now();
                            }
                        }
                        PacketType::KeepAlive { session: incoming } => {
                            if connected && session == Some(incoming) {
                                last_server_seen = std::time::Instant::now();
                            }
                        }
                        PacketType::Reliable { sequence, payload } => {
                            if connected {
                                last_server_seen = std::time::Instant::now();
                                let ack_packet = PacketType::Ack { sequence };
                                let ack_bytes = wincode::serialize(&ack_packet).unwrap();
                                let _ = client.send_message(&ack_bytes);
                                last_sent = std::time::Instant::now();

                                // Forward event
                                if !reliable_chan.is_duplicate_and_track(sequence) {
                                    if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
                                        let _ = tx.send(event);
                                        println!("[cl] deserialized and forwarded {}", sequence);
                                    }
                                }
                            }
                        }
                        PacketType::Ack { sequence } => {
                            if connected {
                                last_server_seen = std::time::Instant::now();
                                reliable_chan.handle_ack(sequence);
                                println!("[cl] ack {}", sequence);
                            }
                        }
                        PacketType::Unreliable(payload) => {
                            if connected {
                                last_server_seen = std::time::Instant::now();
                                if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
                                    let _ = tx.send(event);
                                    println!("[cl] unreliable");
                                }
                            }
                        }
                        PacketType::Fragment { sequence, packet_id, fragment_idx, total_fragments, data } => {
                            if connected {
                                last_server_seen = std::time::Instant::now();
                                let ack_packet = PacketType::Ack { sequence };
                                let ack_bytes = wincode::serialize(&ack_packet).unwrap();
                                let _ = client.send_message(&ack_bytes);
                                last_sent = std::time::Instant::now();

                                if !reliable_chan.is_duplicate_and_track(sequence) {
                                    if let Some(full_payload) = assembler.insert(packet_id, fragment_idx, total_fragments, data.to_vec()) {
                                        if let Ok(event) = wincode::deserialize::<ServerToClient>(&full_payload) {
                                            let _ = tx.send(event);
                                            println!("[cl] reassembled and forwarded fragment packet {}", packet_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(_) => {
                if !connected {
                    let _ = client.send_message(&connect_bytes);
                    last_sent = std::time::Instant::now();
                    if let Some(ref bytes) = challenge_response_bytes {
                        let _ = client.send_message(bytes);
                        last_sent = std::time::Instant::now();
                    }
                }
            }
        }

        if connected && last_server_seen.elapsed() >= CONNECTION_TIMEOUT {
            connected = false;
            challenge_response_bytes = None;
            println!("[cl] server timeout");
            let _ = client.send_message(&connect_bytes);
            last_sent = std::time::Instant::now();
        } else if connected && last_sent.elapsed() >= KEEPALIVE_INTERVAL {
            if let Some(current) = session {
                let bytes = wincode::serialize(&PacketType::KeepAlive { session: current }).unwrap();
                let _ = client.send_message(&bytes);
                last_sent = std::time::Instant::now();
            }
        }

        reliable_chan.pump(|packet_bytes| {
            let _ = client.send_message(packet_bytes);
            last_sent = std::time::Instant::now();
        });
    }
}