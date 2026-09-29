use crate::entities::context::FrameInfo;
use crate::entities::{EntityHandle, Player};
use crate::movement::{self, UserCommand};
use crate::network::events::{EntitySnapshot, NetTransform};
use crate::network::packet::STREAM_STATE;
use crate::network::packet::{
    encoded_packet_count, owned_payload, unreliable_message_limit, BundlePart,
};
use crate::network::server::NetworkServer;
use crate::network::server::ReliableSendError;
use crate::network::usermessage::hash_usermessage_name;
use crate::network::usermessage::UserMsgReader;
use crate::network::wait_socket;
use crate::network::{ClientToServer, ServerToClient};
use crate::network::{FromClient, NetSend, PacketType, ReliableBody, RECV_BUDGET};
use crate::r#enum::InputButtons;
use crate::script::libs::vector3::Vector3;
use crate::state::GameState;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

struct RemotePlayer {
    addr: SocketAddr,
    player: EntityHandle,
    pending: VecDeque<UserCommand>,
    last_buttons: InputButtons,
    ack: u64,
}

impl RemotePlayer {
    fn push_cmd(&mut self, mut cmd: UserCommand) {
        movement::sanitize(&mut cmd);

        if cmd.tick <= self.ack {
            return;
        }

        let mut idx = 0;

        while idx < self.pending.len() {
            if self.pending[idx].tick == cmd.tick {
                return;
            }

            if self.pending[idx].tick > cmd.tick {
                break;
            }

            idx += 1;
        }

        self.pending.insert(idx, cmd);

        while self.pending.len() > 64 {
            self.pending.pop_front();
        }
    }

    fn take_cmd(&mut self) -> Option<UserCommand> {
        loop {
            let Some(tick) = self.pending.front().map(|cmd| cmd.tick) else {
                break;
            };

            if tick > self.ack {
                break;
            }

            self.pending.pop_front();
        }

        self.pending.pop_front()
    }
}

#[cfg(feature = "server")]
pub fn server_loop(mut game: GameState<FromClient, ServerToClient>) {
    let mut last_time = Instant::now();
    let mut accumulated_time = 0.0;
    let mut tick_idx = 0;
    let mut peers = Vec::new();
    let mut players = Vec::new();

    loop {
        let now = Instant::now();
        let dt = now.duration_since(last_time).as_secs_f64();
        last_time = now;

        accumulated_time += dt;

        let mut ticked = false;
        while accumulated_time >= game.tick_interval {
            accumulated_time -= game.tick_interval;

            game.cur_time += game.tick_interval;
            game.frame_time = game.tick_interval;
            game.tick_count += 1;

            game.entities.set_frame(FrameInfo {
                dt: game.tick_interval,
                cur_time: game.cur_time,
                tick_count: game.tick_count,
            });
            game.entities.tick_all();
            simulate_players(&mut game, &mut players);

            if tick_idx % 100 == 0 {
                let hash = hash_usermessage_name("Test");
                game.send_reliable(ServerToClient::UserMessage {
                    hash,
                    data: [128; 256].to_vec(),
                });
            }
            tick_idx += 1;

            ticked = true;
        }

        while let Some((hash, data)) = game.script_engine.poll_usermessage() {
            game.send_reliable(ServerToClient::UserMessage { hash, data });
        }

        while let Ok(net_event) = game.network_receiver.try_recv() {
            match net_event {
                FromClient::Connected { addr, generation } => {
                    println!("[sv] peer {}", addr);
                    if !peers.contains(&addr) {
                        peers.push(addr);
                    }

                    if !players.iter().any(|player| player.addr == addr) {
                        let handle = spawn_player(&mut game, players.len());

                        if !handle.is_null() {
                            players.push(RemotePlayer {
                                addr,
                                player: handle,
                                pending: VecDeque::new(),
                                last_buttons: InputButtons::NONE,
                                ack: 0,
                            });
                            println!("[sv] spawn {handle:?}");
                            let mut idx = 0;

                            while idx < players.len() {
                                if players[idx].addr != addr {
                                    let origin = game
                                        .entities
                                        .get(handle)
                                        .map(|entity| entity.base().position)
                                        .unwrap_or(Vector3::new(0.0, 28.0, 2.0));
                                    game.send_reliable_to(
                                        players[idx].addr,
                                        ServerToClient::EntitySpawned {
                                            handle,
                                            class_hash: Player::CLASS_HASH,
                                            position: origin,
                                        },
                                    );
                                }

                                idx += 1;
                            }
                        }
                    }

                    emit_snapshot(&game, addr, generation);
                    emit_voxel_baseline(&game, addr);
                    game.send_state_to(
                        addr,
                        ServerToClient::MapChange {
                            map_name: game.map_name.clone(),
                        },
                    );

                    if let Some(player) = players.iter().find(|player| player.addr == addr) {
                        game.send_state_to(
                            addr,
                            ServerToClient::PlayerSpawned {
                                handle: player.player,
                            },
                        );
                    }
                }
                FromClient::Disconnected { addr } => {
                    println!("[sv] peer left {}", addr);
                    peers.retain(|peer| *peer != addr);
                    drop_player(&mut game, addr, &mut players);
                }
                FromClient::Message { addr, event } => match event {
                    ClientToServer::UserMessage { hash, data } => {
                        println!("[sv] usermessage {hash} from {addr}");
                        game.run_usermessage(hash, UserMsgReader::new(data));
                    }
                    ClientToServer::PlayerInput {
                        tick,
                        buttons,
                        movement,
                        viewangles,
                    } => {
                        let mut idx = 0;

                        while idx < players.len() {
                            if players[idx].addr == addr {
                                players[idx].push_cmd(UserCommand {
                                    tick,
                                    buttons,
                                    wish: movement,
                                    view: viewangles,
                                });

                                break;
                            }

                            idx += 1;
                        }
                    }
                    _ => {}
                },
            }
        }

        while let Some((hash, data)) = game.script_engine.poll_usermessage() {
            game.send_reliable(ServerToClient::UserMessage { hash, data });
        }

        if ticked {
            emit_tick_state(&game, &players);
            emit_voxel_dirty(&mut game, &peers);
        }

        if !ticked {
            let remaining = game.tick_interval - accumulated_time;
            if remaining > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(remaining));
            }
        }
    }
}

#[cfg(feature = "server")]
pub fn server_network_loop(
    tx: Sender<FromClient>,
    rx: Receiver<NetSend<ServerToClient>>,
    mut wake: TcpStream,
) {
    let mut server = match NetworkServer::new(25400, 128) {
        Ok(server) => server,
        Err(err) => {
            println!("[sv] {err}");
            std::process::exit(1);
        }
    };

    loop {
        while let Ok(outgoing) = rx.try_recv() {
            handle_server_send(&mut server, outgoing);
        }

        let mut ack_addrs = Vec::new();
        let mut got_packet = false;

        // Use match instead of unwrap to handle the timeout gracefully
        for _idx in 0..RECV_BUDGET {
            let Some((parsed, from)) = server.poll_packet() else {
                break;
            };

            got_packet = true;
            let Ok(packet) = parsed else {
                continue;
            };

            match packet {
                PacketType::Connect { replace } => {
                    println!("[sv] connect");
                    let restart = replace
                        .map(|session| server.session_matches(from, session))
                        .unwrap_or(false);
                    if restart && server.disconnect_client(from) {
                        let _ = tx.send(FromClient::Disconnected { addr: from });
                    }

                    if server.is_connected(from) {
                        server.send_connected(from);
                    } else {
                        let token = server.challenge_for(from);
                        let challenge_bytes =
                            wincode::serialize(&PacketType::Challenge { token }).unwrap();
                        let _ = server.send_to(from, &challenge_bytes);
                    }
                }
                PacketType::ChallengeResponse { token } => {
                    println!("[sv] challenge response");
                    if server.is_connected(from) {
                        server.send_connected(from);
                    } else if server.verify_challenge(from, token) {
                        if let Some((_session, generation)) = server.add_client(from) {
                            server.send_connected(from);
                            println!("[sv] connect {}", from);
                            let _ = tx.send(FromClient::Connected {
                                addr: from,
                                generation,
                            });
                        }
                    } else {
                        let token = server.challenge_for(from);
                        let challenge_bytes =
                            wincode::serialize(&PacketType::Challenge { token }).unwrap();
                        let _ = server.send_to(from, &challenge_bytes);
                    }
                }
                PacketType::Challenge { .. } => {
                    println!("[sv] challenge");
                }
                PacketType::Connected { .. } => {}
                PacketType::Disconnect { session } => {
                    if server.retire_client(from, session) {
                        let _ = tx.send(FromClient::Disconnected { addr: from });
                    }
                }
                PacketType::Bundle {
                    session,
                    ack,
                    cumulative,
                    selective,
                    state_cumulative,
                    state_selective,
                    parts,
                } => {
                    if server.session_matches(from, session) {
                        server.touch_client(from);
                        if ack {
                            if let Some(client) = server.clients.get_mut(&from) {
                                client.reliable.handle_ack(cumulative, selective);
                                client.state.handle_ack(state_cumulative, state_selective);
                            }
                        }

                        for part in parts {
                            if apply_server_part(&mut server, from, session, part, &tx) {
                                remember_ack(&mut ack_addrs, from);
                            }
                        }
                    }
                }
                PacketType::KeepAlive { session } => {
                    if server.touch_if_session(from, session) {
                        let bytes = wincode::serialize(&PacketType::KeepAlive { session }).unwrap();
                        let _ = server.send_to(from, &bytes);
                    }
                }
                PacketType::Reliable {
                    session,
                    stream,
                    sequence,
                    generation,
                    payload,
                } => {
                    if accept_from_client(
                        &mut server,
                        from,
                        stream,
                        session,
                        generation,
                        sequence,
                        ReliableBody::Complete(owned_payload(payload)),
                        &tx,
                    ) {
                        remember_ack(&mut ack_addrs, from);
                    }
                }
                PacketType::Ack {
                    session,
                    cumulative,
                    selective,
                    state_cumulative,
                    state_selective,
                } => {
                    if server.session_matches(from, session) {
                        server.touch_client(from);

                        if let Some(client) = server.clients.get_mut(&from) {
                            client.reliable.handle_ack(cumulative, selective);
                            client.state.handle_ack(state_cumulative, state_selective);
                        }
                    }
                }
                PacketType::Unreliable {
                    session,
                    sequence,
                    payload,
                } => {
                    if server.session_matches(from, session) {
                        if let Some(payload) = take_client_unreliable(
                            &mut server,
                            from,
                            sequence,
                            owned_payload(payload),
                        ) {
                            server.touch_client(from);

                            if let Ok(event) = wincode::deserialize::<ClientToServer>(&payload) {
                                let _ = tx.send(FromClient::Message { addr: from, event });
                            }
                        }
                    }
                }
                PacketType::Fragment {
                    session,
                    stream,
                    sequence,
                    generation,
                    packet_id,
                    fragment_idx,
                    total_fragments,
                    data,
                } => {
                    if accept_from_client(
                        &mut server,
                        from,
                        stream,
                        session,
                        generation,
                        sequence,
                        ReliableBody::Fragment {
                            packet_id,
                            fragment_idx,
                            total_fragments,
                            data: owned_payload(data),
                        },
                        &tx,
                    ) {
                        remember_ack(&mut ack_addrs, from);
                    }
                }
            }
        }

        // This catches the WouldBlock timeout.
        // Leaving this empty allows the loop to continue to pump_reliable().
        let flush_addrs: Vec<SocketAddr> = server.clients.keys().copied().collect();
        for addr in flush_addrs {
            let force_ack = ack_addrs.contains(&addr);
            for datagram in server.flush_client(addr, force_ack) {
                let _ = server.send_to(addr, &datagram);
            }
        }

        for addr in server.drop_idle_clients() {
            let _ = tx.send(FromClient::Disconnected { addr });
        }

        if !got_packet {
            wait_socket(&server.socket, &mut wake);
        }
    }
}

#[cfg(feature = "server")]
fn handle_server_send(server: &mut NetworkServer, outgoing: NetSend<ServerToClient>) {
    match outgoing {
        NetSend::Reliable(event) => {
            let payload = wincode::serialize(&event).unwrap();
            for addr in server.broadcast_reliable(&payload) {
                println!("[sv] reliable outbound full {}", addr);
            }
        }
        NetSend::Unreliable(event) => {
            let payload = wincode::serialize(&event).unwrap();
            server.broadcast_unreliable(payload);
        }
        NetSend::ReliableTo(addr, event) => {
            let payload = wincode::serialize(&event).unwrap();
            match server.enqueue_reliable(addr, &payload) {
                Ok(()) => {}
                Err(ReliableSendError::Full) => {
                    println!("[sv] reliable outbound full {}", addr);
                }
                Err(ReliableSendError::TooLarge) => {
                    println!("[sv] reliable payload too large");
                }
                Err(ReliableSendError::Missing) => {}
            }
        }
        NetSend::StateTo(addr, event) => {
            let payload = wincode::serialize(&event).unwrap();
            match server.enqueue_state(addr, &payload) {
                Ok(()) => {}
                Err(ReliableSendError::Full) => {
                    println!("[sv] reliable outbound full {}", addr);
                }
                Err(ReliableSendError::TooLarge) => {
                    println!("[sv] reliable payload too large");
                }
                Err(ReliableSendError::Missing) => {}
            }
        }
        NetSend::UnreliableTo(addr, event) => {
            let payload = wincode::serialize(&event).unwrap();
            server.send_unreliable_to(addr, payload);
        }
    }
}

#[cfg(feature = "server")]
fn remember_ack(addrs: &mut Vec<SocketAddr>, addr: SocketAddr) {
    if !addrs.contains(&addr) {
        addrs.push(addr);
    }
}

#[cfg(feature = "server")]
fn accept_from_client(
    server: &mut NetworkServer,
    from: SocketAddr,
    stream: u8,
    session: u64,
    packet_generation: u32,
    sequence: u32,
    body: ReliableBody,
    tx: &Sender<FromClient>,
) -> bool {
    if !server.session_matches(from, session) {
        return false;
    }

    server.touch_client(from);

    let result = {
        let client = server.clients.get_mut(&from).unwrap();
        let channel = if stream == STREAM_STATE {
            &mut client.state
        } else {
            &mut client.reliable
        };

        channel.receive(sequence, body)
    };

    if !result.ack {
        return false;
    }

    let generation = match server.clients.get(&from) {
        Some(client) => client.generation,
        None => {
            return true;
        }
    };

    if packet_generation != generation {
        return true;
    }

    for payload in result.messages {
        if let Ok(event) = wincode::deserialize::<ClientToServer>(&payload) {
            let _ = tx.send(FromClient::Message { addr: from, event });
        }
    }

    true
}

#[cfg(feature = "server")]
fn apply_server_part(
    server: &mut NetworkServer,
    from: SocketAddr,
    session: u64,
    part: BundlePart,
    tx: &Sender<FromClient>,
) -> bool {
    match part {
        BundlePart::Reliable {
            stream,
            sequence,
            generation,
            payload,
        } => accept_from_client(
            server,
            from,
            stream,
            session,
            generation,
            sequence,
            ReliableBody::Complete(owned_payload(payload)),
            tx,
        ),
        BundlePart::Fragment {
            stream,
            sequence,
            generation,
            packet_id,
            fragment_idx,
            total_fragments,
            data,
        } => accept_from_client(
            server,
            from,
            stream,
            session,
            generation,
            sequence,
            ReliableBody::Fragment {
                packet_id,
                fragment_idx,
                total_fragments,
                data: owned_payload(data),
            },
            tx,
        ),
        BundlePart::Unreliable { sequence, payload } => {
            deliver_client_unreliable(server, from, session, sequence, owned_payload(payload), tx)
        }
        BundlePart::UnreliableFragment {
            sequence,
            fragment_idx,
            total_fragments,
            data,
        } => {
            if !server.session_matches(from, session) {
                return false;
            }

            let payload = {
                let Some(client) = server.clients.get_mut(&from) else {
                    return false;
                };

                client.unreliable_assembly.push(
                    &mut client.unreliable_in,
                    sequence,
                    fragment_idx,
                    total_fragments,
                    owned_payload(data),
                )
            };

            if let Some(payload) = payload {
                server.touch_client(from);

                if let Ok(event) = wincode::deserialize::<ClientToServer>(&payload) {
                    let _ = tx.send(FromClient::Message { addr: from, event });
                }
            }

            false
        }
    }
}

#[cfg(feature = "server")]
fn deliver_client_unreliable(
    server: &mut NetworkServer,
    from: SocketAddr,
    session: u64,
    sequence: u32,
    payload: Vec<u8>,
    tx: &Sender<FromClient>,
) -> bool {
    if !server.session_matches(from, session) {
        return false;
    }

    if let Some(payload) = take_client_unreliable(server, from, sequence, payload) {
        server.touch_client(from);

        if let Ok(event) = wincode::deserialize::<ClientToServer>(&payload) {
            let _ = tx.send(FromClient::Message { addr: from, event });
        }
    }

    false
}

#[cfg(feature = "server")]
fn take_client_unreliable(
    server: &mut NetworkServer,
    from: SocketAddr,
    sequence: u32,
    payload: Vec<u8>,
) -> Option<Vec<u8>> {
    let client = server.clients.get_mut(&from)?;

    crate::network::take_unreliable(
        &mut client.unreliable_in,
        &mut client.unreliable_assembly,
        sequence,
        payload,
    )
}

#[cfg(feature = "server")]
fn spawn_player(game: &mut GameState<FromClient, ServerToClient>, slot: usize) -> EntityHandle {
    let mut origin = game
        .brush_world
        .spawns()
        .first()
        .copied()
        .unwrap_or(Vector3::new(0.0, 28.0, 2.0));
    origin.x += slot as f64 * 0.9;
    let mut player = Player::new();
    player.base.position = origin;

    game.entities
        .spawn(Box::new(player))
        .unwrap_or(EntityHandle::NULL)
}

fn drop_player(
    game: &mut GameState<FromClient, ServerToClient>,
    addr: SocketAddr,
    players: &mut Vec<RemotePlayer>,
) {
    let mut idx = 0;

    while idx < players.len() {
        if players[idx].addr != addr {
            idx += 1;

            continue;
        }

        let handle = players[idx].player;
        game.entities.remove(handle);
        game.send_reliable(ServerToClient::EntityDespawned { handle });
        players.remove(idx);
    }
}

fn simulate_players(
    game: &mut GameState<FromClient, ServerToClient>,
    players: &mut [RemotePlayer],
) {
    let dt = game.tick_interval;
    let gravity = movement::gravity(&game.cvars);
    let mut idx = 0;

    while idx < players.len() {
        let Some(cmd) = players[idx].take_cmd() else {
            idx += 1;

            continue;
        };

        let handle = players[idx].player;
        let prev = players[idx].last_buttons;

        if apply_command(game, handle, &cmd, prev, dt, gravity) {
            players[idx].last_buttons = cmd.buttons;
            players[idx].ack = cmd.tick;
        }

        idx += 1;
    }
}

fn apply_command(
    game: &mut GameState<FromClient, ServerToClient>,
    handle: EntityHandle,
    cmd: &UserCommand,
    prev: InputButtons,
    dt: f64,
    gravity: f64,
) -> bool {
    let (mut position, mut velocity, mut angles) = {
        let Some(entity) = game.entities.get(handle) else {
            return false;
        };

        let base = entity.base();

        (base.position, base.velocity, base.angles)
    };
    movement::step(
        &mut position,
        &mut velocity,
        &mut angles,
        cmd,
        prev,
        dt,
        gravity,
        &game.brush_world,
        &game.voxel_world,
    );

    let Some(entity) = game.entities.get_mut(handle) else {
        return false;
    };

    let base = entity.base_mut();
    base.position = position;
    base.velocity = velocity;
    base.angles = angles;

    true
}

fn player_ack(players: &[RemotePlayer], handle: EntityHandle) -> u64 {
    let mut idx = 0;

    while idx < players.len() {
        if players[idx].player == handle {
            return players[idx].ack;
        }

        idx += 1;
    }

    0
}

fn emit_voxel_baseline(game: &GameState<FromClient, ServerToClient>, addr: SocketAddr) {
    game.send_state_to(
        addr,
        ServerToClient::VoxelScale {
            scale: game.voxel_world.scale(),
        },
    );

    for update in game.voxel_world.baseline() {
        game.send_state_to(addr, ServerToClient::VoxelChunk(update));
    }
}

#[cfg(feature = "server")]
fn emit_voxel_dirty(game: &mut GameState<FromClient, ServerToClient>, peers: &[SocketAddr]) {
    if let Some(scale) = game.voxel_world.take_scale() {
        for addr in peers {
            game.send_state_to(*addr, ServerToClient::VoxelScale { scale });
        }
    }

    let updates = game.voxel_world.take_dirty();

    for update in updates {
        for addr in peers {
            game.send_state_to(*addr, ServerToClient::VoxelChunk(update.clone()));
        }
    }
}

#[cfg(feature = "server")]
fn emit_snapshot(game: &GameState<FromClient, ServerToClient>, addr: SocketAddr, generation: u32) {
    let mut pending = Vec::new();
    for (handle, entity) in game.entities.iter() {
        let base = entity.base();
        pending.push(EntitySnapshot {
            handle,
            class_hash: entity.class_hash(),
            health: entity.net_health(),
            position: base.position,
            angles: base.angles,
            velocity: base.velocity,
        });
    }

    let mut batches = Vec::new();
    if pending.is_empty() {
        batches.push(Vec::new());
    }

    let mut batch = Vec::new();
    for entity in pending {
        batch.push(entity);
        if snapshot_fits(generation, true, u16::MAX, u16::MAX, &batch) {
            continue;
        }

        let overflow = batch.pop().unwrap();
        if !batch.is_empty() {
            batches.push(std::mem::take(&mut batch));
        }

        batch.push(overflow);
        if !snapshot_fits(generation, true, u16::MAX, u16::MAX, &batch) {
            println!("[sv] snapshot entity too large");
            batch.clear();
        }
    }

    if !batch.is_empty() {
        batches.push(batch);
    }

    if batches.is_empty() {
        batches.push(Vec::new());
    }

    let part_count = batches.len() as u16;
    for (idx, entities) in batches.into_iter().enumerate() {
        game.send_state_to(
            addr,
            ServerToClient::WorldSnapshot {
                generation,
                reset: idx == 0,
                part: idx as u16,
                parts: part_count,
                entities,
            },
        );
    }
}

#[cfg(feature = "server")]
fn emit_tick_state(game: &GameState<FromClient, ServerToClient>, players: &[RemotePlayer]) {
    let tick = game.tick_count;
    let mut pending = Vec::new();
    for (handle, entity) in game.entities.iter() {
        let base = entity.base();
        pending.push(NetTransform {
            handle,
            position: base.position,
            angles: base.angles,
            velocity: base.velocity,
            ack: player_ack(players, handle),
        });
    }

    if pending.is_empty() {
        return;
    }

    if tick_fits(tick, 0, 1, &pending) {
        game.send_unreliable(ServerToClient::TickState {
            tick,
            part: 0,
            parts: 1,
            transforms: pending,
        });

        return;
    }

    let mut batches = Vec::new();
    let mut batch = Vec::new();
    for transform in pending {
        batch.push(transform);
        if tick_fits(tick, u16::MAX, u16::MAX, &batch) {
            continue;
        }

        let overflow = batch.pop().unwrap();
        if !batch.is_empty() {
            batches.push(std::mem::take(&mut batch));
        }

        batch.push(overflow);
        if !tick_fits(tick, u16::MAX, u16::MAX, &batch) {
            println!("[sv] tick state too large");
            batch.clear();
        }
    }

    if !batch.is_empty() {
        batches.push(batch);
    }

    if batches.is_empty() {
        return;
    }

    let part_count = batches.len() as u16;
    for (idx, transforms) in batches.into_iter().enumerate() {
        game.send_unreliable(ServerToClient::TickState {
            tick,
            part: idx as u16,
            parts: part_count,
            transforms,
        });
    }
}

#[cfg(feature = "server")]
fn snapshot_fits(
    generation: u32,
    reset: bool,
    part: u16,
    parts: u16,
    entities: &[EntitySnapshot],
) -> bool {
    let event = ServerToClient::WorldSnapshot {
        generation,
        reset,
        part,
        parts,
        entities: entities.to_vec(),
    };
    let payload = wincode::serialize(&event).unwrap();

    encoded_packet_count(payload.len()).is_some()
}

#[cfg(feature = "server")]
fn tick_fits(tick: u64, part: u16, parts: u16, transforms: &[NetTransform]) -> bool {
    let event = ServerToClient::TickState {
        tick,
        part,
        parts,
        transforms: transforms.to_vec(),
    };
    let payload = wincode::serialize(&event).unwrap();

    payload.len() <= unreliable_message_limit()
}
