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
use std::collections::VecDeque;
use std::time::Instant;
use crate::network::{PacketType, ReliableChannel, ReliableBody, EnqueueStatus, NetSend, FromServer, accept_unreliable};
use crate::network::packet::{CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL, unreliable_payload_limit};
use crate::network::reliable::MAX_UNSENT;
use crate::network::usermessage::UserMsgReader;
use crate::entities::context::FrameInfo;

pub fn client_loop(mut game: GameState<FromServer, ClientToServer>) {
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
                    accumulated_time += dt;
                    //let mut ticked = false;

                    // Use a while loop to catch up if a frame lags
                    while accumulated_time >= game.tick_interval {
                        accumulated_time -= game.tick_interval;

                        let ct = game.cur_time() + game.tick_interval; //game.cur_time.lock().unwrap();
                        game.cur_time.store(ct.to_bits(), Ordering::Relaxed);
                        game.frame_time.store(game.tick_interval.to_bits(), Ordering::Relaxed);

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

                while let Some((hash, data)) = game.script_engine.poll_usermessage() {
                    game.send_reliable(ClientToServer::UserMessage { hash, data });
                }

                while let Ok(net_event) = game.network_receiver.try_recv() {
                    let message = match net_event {
                        FromServer::Connected => {
                            println!("[cl] link up");

                            continue;
                        }
                        FromServer::Disconnected => {
                            println!("[cl] link lost");

                            continue;
                        }
                        FromServer::Message(message) => message,
                    };

                    match message {
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

                        other => {
                            println!("[cl] unhandled {other:?}");
                        },
                    }
                }

                while let Some((hash, data)) = game.script_engine.poll_usermessage() {
                    game.send_reliable(ClientToServer::UserMessage { hash, data });
                }

                client_window.window.request_redraw();
            },
            _ => (),
        }
    }).unwrap();
}

#[cfg(feature = "client")]
pub fn client_network_loop(server_addr: SocketAddr, tx: Sender<FromServer>, rx: Receiver<NetSend<ClientToServer>>) {
    let local_addr = if server_addr.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let mut client = NetworkClient::new(SocketAddr::from_str(local_addr).unwrap());
    client.connect(server_addr).expect("Failed to connect to server");
    let mut reliable_chan = ReliableChannel::new();
    let mut connected = false;
    let mut local_reliable: VecDeque<Vec<u8>> = VecDeque::new();
    let mut unreliable_out: u32 = 0;
    let mut unreliable_in: Option<u32> = None;

    let connect_bytes = wincode::serialize(&PacketType::Connect).unwrap();
    let _ = client.send_message(&connect_bytes);
    let mut challenge_response_bytes: Option<Vec<u8>> = None;
    let mut session: Option<u64> = None;
    let mut last_sent = Instant::now();
    let mut last_server_seen = Instant::now();

    loop {
        flush_local_reliable(&mut reliable_chan, &mut local_reliable);

        while let Ok(outgoing) = rx.try_recv() {
            match outgoing {
                NetSend::Reliable(event) | NetSend::ReliableTo(_, event) => {
                    let payload = wincode::serialize(&event).unwrap();
                    if local_reliable.len() >= MAX_UNSENT {
                        println!("[cl] reliable outbound full");
                    } else {
                        local_reliable.push_back(payload);
                    }
                }
                NetSend::Unreliable(event) | NetSend::UnreliableTo(_, event) => {
                    send_client_unreliable(&client, &mut unreliable_out, session, &event, &mut last_sent);
                }
            }
        }

        flush_local_reliable(&mut reliable_chan, &mut local_reliable);

        reliable_chan.evict_stale_fragments();
        reliable_chan.pump(|packet_bytes| {
            let _ = client.send_message(packet_bytes);
            last_sent = Instant::now();
        });

        let recv_started = Instant::now();
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
                                last_sent = Instant::now();
                                println!("[cl] challenge {token}");
                            }
                        }
                        PacketType::ChallengeResponse { .. } => {
                            println!("[cl] challenge response");
                        }
                        PacketType::Connected { session: new_session } => {
                            if !connected {
                                if session.is_some() && session != Some(new_session) {
                                    restart_channel(&mut reliable_chan, &mut local_reliable);
                                    unreliable_out = 0;
                                    unreliable_in = None;
                                }

                                connected = true;
                                session = Some(new_session);
                                reliable_chan.set_session(new_session);
                                challenge_response_bytes = None;
                                last_server_seen = Instant::now();
                                println!("[cl] connected");
                                let _ = tx.send(FromServer::Connected);
                            } else if session == Some(new_session) {
                                last_server_seen = Instant::now();
                            }
                        }
                        PacketType::Disconnect { session: incoming } => {
                            if connected && session == Some(incoming) {
                                connected = false;
                                session = None;
                                challenge_response_bytes = None;
                                unreliable_out = 0;
                                unreliable_in = None;
                                restart_channel(&mut reliable_chan, &mut local_reliable);
                                println!("[cl] disconnect");
                                let _ = tx.send(FromServer::Disconnected);
                                let _ = client.send_message(&connect_bytes);
                                last_sent = Instant::now();
                            }
                        }
                        PacketType::KeepAlive { session: incoming } => {
                            if connected && session == Some(incoming) {
                                last_server_seen = Instant::now();
                            }
                        }
                        PacketType::Reliable { session: incoming, sequence, payload } => {
                            push_client_reliable(
                                &mut reliable_chan,
                                &client,
                                &tx,
                                connected,
                                session,
                                incoming,
                                sequence,
                                ReliableBody::Complete(payload.to_vec()),
                                &mut last_sent,
                                &mut last_server_seen,
                            );
                        }
                        PacketType::Ack { session: incoming, sequence } => {
                            if connected && session == Some(incoming) {
                                last_server_seen = Instant::now();
                                reliable_chan.handle_ack(sequence);
                                println!("[cl] ack {}", sequence);
                            }
                        }
                        PacketType::Unreliable { session: incoming, sequence, payload } => {
                            if connected && session == Some(incoming) && accept_unreliable(&mut unreliable_in, sequence) {
                                last_server_seen = Instant::now();
                                if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
                                    let _ = tx.send(FromServer::Message(event));
                                    println!("[cl] unreliable");
                                }
                            }
                        }
                        PacketType::Fragment { session: incoming, sequence, packet_id, fragment_idx, total_fragments, data } => {
                            push_client_reliable(
                                &mut reliable_chan,
                                &client,
                                &tx,
                                connected,
                                session,
                                incoming,
                                sequence,
                                ReliableBody::Fragment {
                                    packet_id,
                                    fragment_idx,
                                    total_fragments,
                                    data: data.to_vec(),
                                },
                                &mut last_sent,
                                &mut last_server_seen,
                            );
                        }
                    }
                }
            }
            Err(_) => {
                if !connected {
                    let _ = client.send_message(&connect_bytes);
                    last_sent = Instant::now();
                    if let Some(ref bytes) = challenge_response_bytes {
                        let _ = client.send_message(bytes);
                        last_sent = Instant::now();
                    }
                }

                let poll = std::time::Duration::from_millis(50);
                let rest = poll.saturating_sub(recv_started.elapsed());
                if !rest.is_zero() {
                    std::thread::sleep(rest);
                }
            }
        }

        if connected && last_server_seen.elapsed() >= CONNECTION_TIMEOUT {
            connected = false;
            challenge_response_bytes = None;
            println!("[cl] server timeout");
            let _ = tx.send(FromServer::Disconnected);
            let _ = client.send_message(&connect_bytes);
            last_sent = Instant::now();
        } else if connected && last_sent.elapsed() >= KEEPALIVE_INTERVAL {
            if let Some(current) = session {
                let bytes = wincode::serialize(&PacketType::KeepAlive { session: current }).unwrap();
                let _ = client.send_message(&bytes);
                last_sent = Instant::now();
            }
        }

        reliable_chan.pump(|packet_bytes| {
            let _ = client.send_message(packet_bytes);
            last_sent = Instant::now();
        });
    }
}

#[cfg(feature = "client")]
fn flush_local_reliable(reliable_chan: &mut ReliableChannel, local_reliable: &mut VecDeque<Vec<u8>>) {
    loop {
        let status = match local_reliable.front() {
            Some(payload) => reliable_chan.enqueue(payload),
            None => {
                break;
            }
        };

        match status {
            EnqueueStatus::Queued => {
                local_reliable.pop_front();
            }
            EnqueueStatus::Full => {
                break;
            }
            EnqueueStatus::TooLarge => {
                println!("[cl] reliable payload too large");
                local_reliable.pop_front();
            }
        }
    }
}

#[cfg(feature = "client")]
fn restart_channel(reliable_chan: &mut ReliableChannel, local_reliable: &mut VecDeque<Vec<u8>>) {
    let queued = reliable_chan.take_unsent();
    *reliable_chan = ReliableChannel::new();

    for payload in queued.into_iter().rev() {
        local_reliable.push_front(payload);
    }
}

#[cfg(feature = "client")]
fn send_client_unreliable(
    client: &NetworkClient,
    unreliable_out: &mut u32,
    session: Option<u64>,
    event: &ClientToServer,
    last_sent: &mut Instant,
) {
    let Some(session) = session else {
        return;
    };

    let payload = wincode::serialize(event).unwrap();
    if payload.len() > unreliable_payload_limit() {
        println!("[cl] unreliable payload too large");

        return;
    }

    let sequence = *unreliable_out;
    *unreliable_out = unreliable_out.wrapping_add(1);
    let packet = PacketType::Unreliable {
        session,
        sequence,
        payload: payload.into(),
    };
    let bytes = wincode::serialize(&packet).unwrap();
    let _ = client.send_message(&bytes);
    *last_sent = Instant::now();
}

#[cfg(feature = "client")]
fn push_client_reliable(
    reliable_chan: &mut ReliableChannel,
    client: &NetworkClient,
    tx: &Sender<FromServer>,
    connected: bool,
    session: Option<u64>,
    packet_session: u64,
    sequence: u32,
    body: ReliableBody,
    last_sent: &mut Instant,
    last_server_seen: &mut Instant,
) {
    if !connected || session != Some(packet_session) {
        return;
    }

    *last_server_seen = Instant::now();

    let result = reliable_chan.receive(sequence, body);
    if !result.ack {
        return;
    }

    let ack_packet = PacketType::Ack { session: packet_session, sequence };
    let ack_bytes = wincode::serialize(&ack_packet).unwrap();
    let _ = client.send_message(&ack_bytes);
    *last_sent = Instant::now();

    for payload in result.messages {
        // Forward event
        if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
            let _ = tx.send(FromServer::Message(event));
            println!("[cl] deserialized and forwarded {}", sequence);
        }
    }
}