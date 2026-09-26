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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use core::net::SocketAddr;
use std::str::FromStr;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use crate::network::{PacketType, ReliableChannel, ReliableBody, EnqueueStatus, NetSend, FromServer, UnreliableInbox, accept_unreliable, OUTBOUND_CAP};
use crate::network::packet::{CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL, unreliable_payload_limit};
use crate::network::reliable::MAX_UNSENT;
use crate::network::usermessage::UserMsgReader;
use crate::entities::context::FrameInfo;

pub fn client_loop(mut game: GameState<FromServer, ClientToServer>, shutdown: Arc<AtomicBool>) {
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
                    shutdown.store(true, Ordering::Relaxed);
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
const HANDSHAKE_INTERVAL: Duration = Duration::from_millis(200);

#[cfg(feature = "client")]
pub fn client_network_loop(
    server_addr: SocketAddr,
    tx: Sender<FromServer>,
    rx: Receiver<NetSend<ClientToServer>>,
    shutdown: Arc<AtomicBool>,
) {
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
    let mut unreliable_in = UnreliableInbox::new();
    let mut replace_session: Option<u64> = None;
    let mut reset_latched = false;

    let _ = client.send_message(&connect_packet(None));
    let mut challenge_response_bytes: Option<Vec<u8>> = None;
    let mut session: Option<u64> = None;
    let mut last_sent = Instant::now();
    let mut last_server_seen = Instant::now();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            if let Some(current) = session {
                let bytes = wincode::serialize(&PacketType::Disconnect { session: current }).unwrap();
                let _ = client.send_message(&bytes);
            }

            let _ = tx.send(FromServer::Disconnected);

            return;
        }

        if reliable_backlog(&local_reliable, &reliable_chan) < MAX_UNSENT {
            reset_latched = false;
        }

        flush_local_reliable(&mut reliable_chan, &mut local_reliable);

        while let Ok(outgoing) = rx.try_recv() {
            match outgoing {
                NetSend::Reliable(event) | NetSend::ReliableTo(_, event) => {
                    let payload = wincode::serialize(&event).unwrap();
                    let backlog = reliable_backlog(&local_reliable, &reliable_chan);
                    if connected && !reset_latched && backlog >= MAX_UNSENT {
                        reset_latched = true;
                        local_reliable.push_back(payload);
                        begin_reconnect(
                            &client,
                            &mut reliable_chan,
                            &mut local_reliable,
                            &mut connected,
                            &mut session,
                            &mut replace_session,
                            &mut challenge_response_bytes,
                            &mut unreliable_out,
                            &mut unreliable_in,
                            &mut last_sent,
                        );
                        println!("[cl] reliable window full");
                        let _ = tx.send(FromServer::Disconnected);
                    } else if backlog >= OUTBOUND_CAP {
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

        let mut got_packet = false;
        let mut need_ack = false;
        loop {
            let Some((data, _from)) = client.poll_message() else {
                break;
            };

            got_packet = true;
            if let Ok(packet) = wincode::deserialize::<PacketType>(&data) {
                match packet {
                    PacketType::Connect { .. } => {
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
                        if !connected && replace_session != Some(new_session) {
                            if session.is_some() && session != Some(new_session) {
                                restart_channel(&mut reliable_chan, &mut local_reliable);
                                unreliable_out = 0;
                                unreliable_in = UnreliableInbox::new();
                            }

                            connected = true;
                            session = Some(new_session);
                            replace_session = None;
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
                            begin_reconnect(
                                &client,
                                &mut reliable_chan,
                                &mut local_reliable,
                                &mut connected,
                                &mut session,
                                &mut replace_session,
                                &mut challenge_response_bytes,
                                &mut unreliable_out,
                                &mut unreliable_in,
                                &mut last_sent,
                            );
                            println!("[cl] disconnect");
                            let _ = tx.send(FromServer::Disconnected);
                        }
                    }
                    PacketType::KeepAlive { session: incoming } => {
                        if connected && session == Some(incoming) {
                            last_server_seen = Instant::now();
                        }
                    }
                    PacketType::Reliable { session: incoming, sequence, payload } => {
                        if push_client_reliable(
                            &mut reliable_chan,
                            &tx,
                            connected,
                            session,
                            incoming,
                            sequence,
                            ReliableBody::Complete(payload.to_vec()),
                            &mut last_server_seen,
                        ) {
                            need_ack = true;
                        }
                    }
                    PacketType::Ack { session: incoming, cumulative, selective } => {
                        if connected && session == Some(incoming) {
                            last_server_seen = Instant::now();
                            reliable_chan.handle_ack(cumulative, selective);
                            println!("[cl] ack {}", cumulative);
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
                        if push_client_reliable(
                            &mut reliable_chan,
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
                            &mut last_server_seen,
                        ) {
                            need_ack = true;
                        }
                    }
                }
            }
        }

        if need_ack {
            if let Some(current) = session {
                if connected {
                    let ack = reliable_chan.selective_ack();
                    let packet = PacketType::Ack {
                        session: current,
                        cumulative: ack.cumulative,
                        selective: ack.selective,
                    };
                    let bytes = wincode::serialize(&packet).unwrap();
                    let _ = client.send_message(&bytes);
                    last_sent = Instant::now();
                    println!("[cl] ack {}", ack.cumulative);
                }
            }
        }

        if connected && last_server_seen.elapsed() >= CONNECTION_TIMEOUT {
            connected = false;
            replace_session = None;
            challenge_response_bytes = None;
            println!("[cl] server timeout");
            let _ = tx.send(FromServer::Disconnected);
            let _ = client.send_message(&connect_packet(None));
            last_sent = Instant::now();
        } else if connected && last_sent.elapsed() >= KEEPALIVE_INTERVAL {
            if let Some(current) = session {
                let bytes = wincode::serialize(&PacketType::KeepAlive { session: current }).unwrap();
                let _ = client.send_message(&bytes);
                last_sent = Instant::now();
            }
        } else if !connected && last_sent.elapsed() >= HANDSHAKE_INTERVAL {
            let _ = client.send_message(&connect_packet(replace_session));
            last_sent = Instant::now();
            if let Some(ref bytes) = challenge_response_bytes {
                let _ = client.send_message(bytes);
                last_sent = Instant::now();
            }
        }

        flush_local_reliable(&mut reliable_chan, &mut local_reliable);
        reliable_chan.pump(|packet_bytes| {
            let _ = client.send_message(packet_bytes);
            last_sent = Instant::now();
        });

        if !got_packet {
            std::thread::sleep(Duration::from_millis(2));
        }
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
    let queued = reliable_chan.take_unacked();
    *reliable_chan = ReliableChannel::new();

    for payload in queued.into_iter().rev() {
        local_reliable.push_front(payload);
    }
}

#[cfg(feature = "client")]
fn reliable_backlog(local_reliable: &VecDeque<Vec<u8>>, reliable_chan: &ReliableChannel) -> usize {
    local_reliable.len() + reliable_chan.queued_messages()
}

#[cfg(feature = "client")]
fn connect_packet(replace: Option<u64>) -> Vec<u8> {
    wincode::serialize(&PacketType::Connect { replace }).unwrap()
}

#[cfg(feature = "client")]
fn begin_reconnect(
    client: &NetworkClient,
    reliable_chan: &mut ReliableChannel,
    local_reliable: &mut VecDeque<Vec<u8>>,
    connected: &mut bool,
    session: &mut Option<u64>,
    replace_session: &mut Option<u64>,
    challenge_response_bytes: &mut Option<Vec<u8>>,
    unreliable_out: &mut u32,
    unreliable_in: &mut UnreliableInbox,
    last_sent: &mut Instant,
) {
    *replace_session = *session;
    *connected = false;
    *session = None;
    *challenge_response_bytes = None;
    *unreliable_out = 0;
    *unreliable_in = UnreliableInbox::new();
    restart_channel(reliable_chan, local_reliable);
    let _ = client.send_message(&connect_packet(*replace_session));
    *last_sent = Instant::now();
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
    tx: &Sender<FromServer>,
    connected: bool,
    session: Option<u64>,
    packet_session: u64,
    sequence: u32,
    body: ReliableBody,
    last_server_seen: &mut Instant,
) -> bool {
    if !connected || session != Some(packet_session) {
        return false;
    }

    *last_server_seen = Instant::now();

    let result = reliable_chan.receive(sequence, body);
    if !result.ack {
        return false;
    }

    for payload in result.messages {
        // Forward event
        if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
            let _ = tx.send(FromServer::Message(event));
            println!("[cl] deserialized and forwarded {}", sequence);
        }
    }

    true
}