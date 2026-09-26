use std::sync::mpsc::{Receiver, Sender};
use std::os::unix::net::UnixStream;
use crate::network::wait_socket;
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
use crate::network::{PacketType, ReliableChannel, ReliableBody, EnqueueStatus, NetSend, FromServer, UnreliableInbox, UnreliableAssembly, take_unreliable, OUTBOUND_CAP, RECV_BUDGET};
use crate::network::packet::{bundle_part, owned_payload, pack_bundles, split_unreliable, BundlePart, CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL, STREAM_STATE};
use crate::network::events::{EntitySnapshot, NetTransform};
use crate::entities::Player;
use crate::network::usermessage::UserMsgReader;
use crate::entities::context::FrameInfo;

struct TickIngress {
    tick: u64,
    part_count: u16,
    filled: u16,
    started: bool,
    parts: Vec<Option<Vec<NetTransform>>>,
}

impl TickIngress {
    fn new() -> Self {
        Self {
            tick: 0,
            part_count: 0,
            filled: 0,
            started: false,
            parts: Vec::new(),
        }
    }

    fn push(&mut self, tick: u64, part: u16, part_count: u16, transforms: Vec<NetTransform>) -> Option<Vec<NetTransform>> {
        if part_count == 0 || part >= part_count || part_count > 1024 {
            return None;
        }

        if self.started && self.part_count == 0 && tick <= self.tick {
            return None;
        }

        if self.started && tick < self.tick {
            return None;
        }

        if !self.started || self.tick != tick || self.part_count != part_count || self.parts.len() != part_count as usize {
            self.started = true;
            self.tick = tick;
            self.part_count = part_count;
            self.filled = 0;
            self.parts = vec![None; part_count as usize];
        }

        if self.parts[part as usize].is_none() {
            self.parts[part as usize] = Some(transforms);
            self.filled = self.filled.saturating_add(1);
        }

        if self.filled != part_count {
            return None;
        }

        let mut all = Vec::new();
        for slot in self.parts.drain(..) {
            if let Some(batch) = slot {
                all.extend(batch);
            }
        }

        self.filled = 0;
        self.part_count = 0;

        Some(all)
    }
}

struct BuiltSnapshot {
    reset: bool,
    entities: Vec<EntitySnapshot>,
}

struct SnapshotIngress {
    generation: u32,
    reset: bool,
    part_count: u16,
    filled: u16,
    parts: Vec<Option<Vec<EntitySnapshot>>>,
}

impl SnapshotIngress {
    fn new() -> Self {
        Self {
            generation: 0,
            reset: false,
            part_count: 0,
            filled: 0,
            parts: Vec::new(),
        }
    }

    fn push(&mut self, generation: u32, reset: bool, part: u16, part_count: u16, entities: Vec<EntitySnapshot>) -> Option<BuiltSnapshot> {
        if part_count == 0 || part >= part_count || part_count > 1024 {
            return None;
        }

        if self.part_count != part_count || self.generation != generation || self.parts.len() != part_count as usize {
            self.generation = generation;
            self.reset = false;
            self.part_count = part_count;
            self.filled = 0;
            self.parts = vec![None; part_count as usize];
        }

        if part == 0 {
            self.reset = reset;
        }

        if self.parts[part as usize].is_none() {
            self.parts[part as usize] = Some(entities);
            self.filled = self.filled.saturating_add(1);
        }

        if self.filled != part_count {
            return None;
        }

        let mut built_entities = Vec::new();
        for slot in self.parts.drain(..) {
            if let Some(batch) = slot {
                built_entities.extend(batch);
            }
        }

        let built = BuiltSnapshot {
            reset: self.reset,
            entities: built_entities,
        };
        self.filled = 0;
        self.part_count = 0;

        Some(built)
    }
}

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
    let mut world_generation = 0u32;
    let mut hold_events = false;
    let mut held: VecDeque<ServerToClient> = VecDeque::new();
    let mut snapshot_ingress = SnapshotIngress::new();
    let mut tick_ingress = TickIngress::new();

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

                        game.cur_time += game.tick_interval; //game.cur_time.lock().unwrap();
                        game.frame_time = game.tick_interval;
                        game.tick_count += 1;

                        game.entities.set_frame(FrameInfo {
                            dt: game.tick_interval,
                            cur_time: game.cur_time,
                            tick_count: game.tick_count,
                        });
                        game.entities.tick_all();
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
                    match net_event {
                        FromServer::Connected { generation } => {
                            println!("[cl] link up");
                            if world_generation != generation {
                                world_generation = generation;
                                game.entities.clear();
                                hold_events = true;
                                held.clear();
                                snapshot_ingress = SnapshotIngress::new();
                            }

                            continue;
                        }
                        FromServer::Disconnected => {
                            println!("[cl] link lost");

                            continue;
                        }
                        FromServer::Message(message) => {
                            if let ServerToClient::WorldSnapshot { generation, reset, part, parts, entities } = message {
                                if generation == world_generation {
                                    if let Some(built) = snapshot_ingress.push(generation, reset, part, parts, entities) {
                                        if built.reset {
                                            game.entities.clear();
                                        }

                                        for entity in built.entities {
                                            apply_spawn(&mut game, entity);
                                        }

                                        hold_events = false;
                                        while let Some(waiting) = held.pop_front() {
                                            apply_server_event(&mut game, &mut tick_ingress, waiting);
                                        }
                                    } else {
                                        hold_events = true;
                                    }
                                }

                                continue;
                            }

                            if hold_events {
                                held.push_back(message);

                                continue;
                            }

                            apply_server_event(&mut game, &mut tick_ingress, message);

                            continue;
                        }
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

fn apply_server_event(game: &mut GameState<FromServer, ClientToServer>, tick_ingress: &mut TickIngress, message: ServerToClient) {
    match message {
                        ServerToClient::PlayerConnected { handle, name } => {
                            game.run_hook("PlayerConnected", (handle, name));
                        },
                        ServerToClient::PlayerDisconnected { handle } => {
                            game.run_hook("PlayerDisconnected", handle);
                        },
                        ServerToClient::PlayerSpawned { handle } => {
                            game.run_hook("PlayerSpawned", handle);
                        },
                        ServerToClient::PlayerDamaged { handle, attacker, inflictor, damage, new_health } => {
                            game.run_hook("PlayerDamaged", (handle, attacker, inflictor, damage, new_health));
                        },
                        ServerToClient::PlayerDied { handle, killer, inflictor } => {
                            game.run_hook("PlayerDied", (handle, killer, inflictor));
                        },
                        ServerToClient::ModelChanged { handle, model } => {
                            game.run_hook("ModelChanged", (handle, model));
                        },
                        ServerToClient::TransformUpdated { handle, position, angles, velocity } => {
                            if let Some(entity) = game.entities.get_mut(handle) {
                                if let Some(pos) = position { entity.base_mut().position = pos; }
                                if let Some(ang) = angles { entity.base_mut().angles = ang; }
                                if let Some(vel) = velocity { entity.base_mut().velocity = vel; }
                            }

                            game.run_hook("TransformUpdated", (handle, position, angles, velocity));
                        },

                        ServerToClient::UserMessage { hash, data } => {
                            game.run_usermessage(hash, UserMsgReader::new(data));
                        },
                        ServerToClient::WorldSnapshot { .. } => {
                        },
                        ServerToClient::TickState { tick, part, parts, transforms } => {
                            if let Some(transforms) = tick_ingress.push(tick, part, parts, transforms) {
                                for transform in transforms {
                                    if let Some(entity) = game.entities.get_mut(transform.handle) {
                                        let base = entity.base_mut();
                                        base.position = transform.position;
                                        base.angles = transform.angles;
                                        base.velocity = transform.velocity;
                                    }

                                    game.run_hook(
                                        "TransformUpdated",
                                        (transform.handle, Some(transform.position), Some(transform.angles), Some(transform.velocity)),
                                    );
                                }
                            }
                        },

                        other => {
                            println!("[cl] unhandled {other:?}");
                        },
    }
}

#[cfg(feature = "client")]
const HANDSHAKE_INTERVAL: Duration = Duration::from_millis(200);

#[cfg(feature = "client")]
pub fn client_network_loop(
    server_addr: SocketAddr,
    tx: Sender<FromServer>,
    rx: Receiver<NetSend<ClientToServer>>,
    shutdown: Arc<AtomicBool>,
    mut wake: UnixStream,
) {
    let local_addr = if server_addr.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let mut client = NetworkClient::new(SocketAddr::from_str(local_addr).unwrap());
    client.connect(server_addr).expect("Failed to connect to server");
    let mut reliable_chan = ReliableChannel::new();
    let mut state_chan = ReliableChannel::with_stream(STREAM_STATE);
    let mut connected = false;
    let mut local_reliable: VecDeque<Vec<u8>> = VecDeque::new();
    let mut unreliable_out: u32 = 0;
    let mut unreliable_in = UnreliableInbox::new();
    let mut unreliable_assembly = UnreliableAssembly::new();
    let mut replace_session: Option<u64> = None;
    let mut generation: Option<u32> = None;

    let _ = client.send_message(&connect_packet(None));
    let mut challenge_response_bytes: Option<Vec<u8>> = None;
    let mut session: Option<u64> = None;
    let mut last_sent = Instant::now();
    let mut last_server_seen = Instant::now();
    let mut unreliable_parts: Vec<BundlePart> = Vec::new();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            if let Some(current) = session {
                let bytes = wincode::serialize(&PacketType::Disconnect { session: current }).unwrap();
                let _ = client.send_message(&bytes);
            }

            let _ = tx.send(FromServer::Disconnected);

            return;
        }

        while let Ok(outgoing) = rx.try_recv() {
            queue_client_send(outgoing, &reliable_chan, &mut local_reliable, &mut unreliable_out, connected, session, &mut unreliable_parts);
        }

        let mut got_packet = false;
        let mut need_ack = false;
        for _idx in 0..RECV_BUDGET {
            let Some(parsed) = client.poll_packet() else {
                break;
            };

            got_packet = true;
            let Ok(packet) = parsed else {
                continue;
            };

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
                    PacketType::Connected { session: new_session, generation: new_generation } => {
                        if !connected && replace_session != Some(new_session) {
                            let session_changed = session.is_some() && session != Some(new_session);
                            let generation_changed = generation != Some(new_generation);
                            if session_changed || generation_changed {
                                reclaim_reliable(&mut reliable_chan, generation, &mut local_reliable);
                                reliable_chan = ReliableChannel::new();
                                state_chan = ReliableChannel::with_stream(STREAM_STATE);
                                unreliable_out = 0;
                                unreliable_in = UnreliableInbox::new();
                                unreliable_assembly = UnreliableAssembly::new();
                                unreliable_parts.clear();
                            }

                            connected = true;
                            session = Some(new_session);
                            generation = Some(new_generation);
                            replace_session = None;
                            reliable_chan.set_session(new_session);
                            state_chan.set_session(new_session);
                            challenge_response_bytes = None;
                            last_server_seen = Instant::now();
                            println!("[cl] connected");
                            let _ = tx.send(FromServer::Connected { generation: new_generation });
                        } else if session == Some(new_session) {
                            last_server_seen = Instant::now();
                        }
                    }
                    PacketType::Disconnect { session: incoming } => {
                        if connected && session == Some(incoming) {
                            unreliable_parts.clear();
                            begin_reconnect(
                                &client,
                                &mut reliable_chan,
                                &mut state_chan,
                                &mut local_reliable,
                                &mut connected,
                                &mut session,
                                &mut generation,
                                &mut replace_session,
                                &mut challenge_response_bytes,
                                &mut unreliable_out,
                                &mut unreliable_in,
                                &mut unreliable_assembly,
                                &mut last_sent,
                            );
                            println!("[cl] disconnect");
                            let _ = tx.send(FromServer::Disconnected);
                        }
                    }
                    PacketType::Bundle { session: incoming, ack, cumulative, selective, state_cumulative, state_selective, parts } => {
                        if connected && session == Some(incoming) {
                            last_server_seen = Instant::now();
                            if ack {
                                reliable_chan.handle_ack(cumulative, selective);
                                state_chan.handle_ack(state_cumulative, state_selective);
                            }

                            for part in parts {
                                if apply_client_part(
                                    &mut reliable_chan,
                                    &mut state_chan,
                                    &tx,
                                    &mut unreliable_in,
                                    &mut unreliable_assembly,
                                    connected,
                                    session,
                                    generation,
                                    incoming,
                                    part,
                                    &mut last_server_seen,
                                ) {
                                    need_ack = true;
                                }
                            }
                        }
                    }
                    PacketType::KeepAlive { session: incoming } => {
                        if connected && session == Some(incoming) {
                            last_server_seen = Instant::now();
                        }
                    }
                    PacketType::Reliable { session: incoming, stream, sequence, generation: packet_generation, payload } => {
                        let channel = if stream == STREAM_STATE {
                            &mut state_chan
                        } else {
                            &mut reliable_chan
                        };

                        if push_client_reliable(
                            channel,
                            &tx,
                            connected,
                            session,
                            generation,
                            incoming,
                            packet_generation,
                            sequence,
                            ReliableBody::Complete(owned_payload(payload)),
                            &mut last_server_seen,
                        ) {
                            need_ack = true;
                        }
                    }
                    PacketType::Ack { session: incoming, cumulative, selective, state_cumulative, state_selective } => {
                        if connected && session == Some(incoming) {
                            last_server_seen = Instant::now();
                            reliable_chan.handle_ack(cumulative, selective);
                            state_chan.handle_ack(state_cumulative, state_selective);
                        }
                    }
                    PacketType::Unreliable { session: incoming, sequence, payload } => {
                        if connected && session == Some(incoming) {
                            if let Some(payload) = take_unreliable(&mut unreliable_in, &mut unreliable_assembly, sequence, owned_payload(payload)) {
                                last_server_seen = Instant::now();
                                if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
                                    let _ = tx.send(FromServer::Message(event));
                                }
                            }
                        }
                    }
                    PacketType::Fragment { session: incoming, stream, sequence, generation: packet_generation, packet_id, fragment_idx, total_fragments, data } => {
                        let channel = if stream == STREAM_STATE {
                            &mut state_chan
                        } else {
                            &mut reliable_chan
                        };

                        if push_client_reliable(
                            channel,
                            &tx,
                            connected,
                            session,
                            generation,
                            incoming,
                            packet_generation,
                            sequence,
                            ReliableBody::Fragment {
                                packet_id,
                                fragment_idx,
                                total_fragments,
                                data: owned_payload(data),
                            },
                            &mut last_server_seen,
                        ) {
                            need_ack = true;
                        }
                    }
            }
        }

        if connected {
            if let Some(current_generation) = generation {
                reliable_chan.set_generation(current_generation);
                state_chan.set_generation(current_generation);
            }
            flush_local_reliable(&mut reliable_chan, &mut local_reliable);
            let mut parts = Vec::new();
            reliable_chan.pump(|packet| {
                if let Some(part) = bundle_part(&packet) {
                    parts.push(part);
                }
            });
            state_chan.pump(|packet| {
                if let Some(part) = bundle_part(&packet) {
                    parts.push(part);
                }
            });
            parts.extend(std::mem::take(&mut unreliable_parts));

            if let Some(current) = session {
                if !parts.is_empty() || need_ack {
                    let ack = reliable_chan.selective_ack();
                    let state_ack = state_chan.selective_ack();
                    let datagrams = pack_bundles(
                        current,
                        ack.cumulative,
                        ack.selective,
                        state_ack.cumulative,
                        state_ack.selective,
                        true,
                        parts,
                    );
                    for datagram in datagrams {
                        let _ = client.send_message(&datagram);
                        last_sent = Instant::now();
                    }

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

        if !got_packet {
            wait_socket(client.socket(), &mut wake);
        }
    }
}

#[cfg(feature = "client")]
fn queue_client_send(
    outgoing: NetSend<ClientToServer>,
    reliable_chan: &ReliableChannel,
    local_reliable: &mut VecDeque<Vec<u8>>,
    unreliable_out: &mut u32,
    connected: bool,
    session: Option<u64>,
    unreliable_parts: &mut Vec<BundlePart>,
) {
    match outgoing {
        NetSend::Reliable(event) | NetSend::ReliableTo(_, event) | NetSend::StateTo(_, event) => {
            let payload = wincode::serialize(&event).unwrap();
            let backlog = reliable_backlog(local_reliable, reliable_chan);
            if backlog >= OUTBOUND_CAP {
                println!("[cl] reliable outbound full");
            } else {
                local_reliable.push_back(payload);
            }
        }
        NetSend::Unreliable(event) | NetSend::UnreliableTo(_, event) => {
            queue_client_unreliable(unreliable_out, connected, session, &event, unreliable_parts);
        }
    }
}

#[cfg(feature = "client")]
fn flush_local_reliable(reliable_chan: &mut ReliableChannel, local_reliable: &mut VecDeque<Vec<u8>>) {
    loop {
        let status = match local_reliable.front() {
            Some(payload) => {
                reliable_chan.enqueue_bytes(payload.clone())
            }
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
    state_chan: &mut ReliableChannel,
    local_reliable: &mut VecDeque<Vec<u8>>,
    connected: &mut bool,
    session: &mut Option<u64>,
    generation: &mut Option<u32>,
    replace_session: &mut Option<u64>,
    challenge_response_bytes: &mut Option<Vec<u8>>,
    unreliable_out: &mut u32,
    unreliable_in: &mut UnreliableInbox,
    unreliable_assembly: &mut UnreliableAssembly,
    last_sent: &mut Instant,
) {
    reclaim_reliable(reliable_chan, *generation, local_reliable);
    *replace_session = *session;
    *connected = false;
    *session = None;
    *generation = None;
    *challenge_response_bytes = None;
    *unreliable_out = 0;
    *unreliable_in = UnreliableInbox::new();
    *unreliable_assembly = UnreliableAssembly::new();
    *reliable_chan = ReliableChannel::new();
    *state_chan = ReliableChannel::with_stream(STREAM_STATE);
    let _ = client.send_message(&connect_packet(*replace_session));
    *last_sent = Instant::now();
}

#[cfg(feature = "client")]
fn reclaim_reliable(channel: &mut ReliableChannel, generation: Option<u32>, local_reliable: &mut VecDeque<Vec<u8>>) {
    let Some(_generation) = generation else {
        return;
    };

    let pending = channel.take_unacked();
    let mut restored = VecDeque::new();
    for payload in pending {
        restored.push_back(payload);
    }

    while restored.len() + local_reliable.len() > OUTBOUND_CAP && !local_reliable.is_empty() {
        local_reliable.pop_back();
    }

    while restored.len() > OUTBOUND_CAP {
        restored.pop_back();
    }

    restored.append(local_reliable);
    *local_reliable = restored;
}

#[cfg(feature = "client")]
fn queue_client_unreliable(
    unreliable_out: &mut u32,
    connected: bool,
    session: Option<u64>,
    event: &ClientToServer,
    parts: &mut Vec<BundlePart>,
) {
    if !connected || session.is_none() {
        return;
    }

    let payload = Arc::new(wincode::serialize(event).unwrap());
    let sequence = *unreliable_out;
    let split = split_unreliable(sequence, payload);
    if split.is_empty() {
        return;
    }

    *unreliable_out = unreliable_out.wrapping_add(1);
    parts.extend(split);
}

#[cfg(feature = "client")]
fn push_client_reliable(
    reliable_chan: &mut ReliableChannel,
    tx: &Sender<FromServer>,
    connected: bool,
    session: Option<u64>,
    generation: Option<u32>,
    packet_session: u64,
    packet_generation: u32,
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

    let Some(generation) = generation else {
        return true;
    };

    if packet_generation != generation {
        return true;
    }

    for payload in result.messages {
        // Forward event
        if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
            let _ = tx.send(FromServer::Message(event));
        }
    }

    true
}

#[cfg(feature = "client")]
fn apply_client_part(
    reliable_chan: &mut ReliableChannel,
    state_chan: &mut ReliableChannel,
    tx: &Sender<FromServer>,
    unreliable_in: &mut UnreliableInbox,
    unreliable_assembly: &mut UnreliableAssembly,
    connected: bool,
    session: Option<u64>,
    generation: Option<u32>,
    incoming: u64,
    part: BundlePart,
    last_server_seen: &mut Instant,
) -> bool {
    match part {
        BundlePart::Reliable { stream, sequence, generation: packet_generation, payload } => {
            let channel = if stream == STREAM_STATE {
                state_chan
            } else {
                reliable_chan
            };

            push_client_reliable(
                channel,
                tx,
                connected,
                session,
                generation,
                incoming,
                packet_generation,
                sequence,
                ReliableBody::Complete(owned_payload(payload)),
                last_server_seen,
            )
        }
        BundlePart::Fragment { stream, sequence, generation: packet_generation, packet_id, fragment_idx, total_fragments, data } => {
            let channel = if stream == STREAM_STATE {
                state_chan
            } else {
                reliable_chan
            };

            push_client_reliable(
                channel,
                tx,
                connected,
                session,
                generation,
                incoming,
                packet_generation,
                sequence,
                ReliableBody::Fragment {
                    packet_id,
                    fragment_idx,
                    total_fragments,
                    data: owned_payload(data),
                },
                last_server_seen,
            )
        }
        BundlePart::Unreliable { sequence, payload } => {
            if connected && session == Some(incoming) {
                if let Some(payload) = take_unreliable(unreliable_in, unreliable_assembly, sequence, owned_payload(payload)) {
                    *last_server_seen = Instant::now();
                    if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
                        let _ = tx.send(FromServer::Message(event));
                    }
                }
            }

            false
        }
        BundlePart::UnreliableFragment { sequence, fragment_idx, total_fragments, data } => {
            if connected && session == Some(incoming) {
                if let Some(payload) = unreliable_assembly.push(unreliable_in, sequence, fragment_idx, total_fragments, owned_payload(data)) {
                    *last_server_seen = Instant::now();
                    if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
                        let _ = tx.send(FromServer::Message(event));
                    }
                }
            }

            false
        }
    }
}

fn apply_spawn(game: &mut GameState<FromServer, ClientToServer>, entity: EntitySnapshot) {
    if entity.class_hash != Player::CLASS_HASH {
        println!("[cl] unknown class {}", entity.class_hash);

        return;
    }

    let mut player = Player::new();
    player.health = entity.health;
    player.base.position = entity.position;
    player.base.angles = entity.angles;
    player.base.velocity = entity.velocity;
    game.entities.insert_at(entity.handle, Box::new(player));
}