use crate::entities::context::FrameInfo;
use crate::entities::{EntityHandle, Player};
use crate::movement::{self, UserCommand};
use crate::network::events::{EntityModel, EntityNetworked, EntityOwnership, EntitySnapshot};
use crate::network::packet::STREAM_STATE;
use crate::network::packet::{
    encoded_packet_count, owned_payload, unreliable_message_limit, BundlePart,
};
use crate::network::server::NetworkServer;
use crate::network::server::ReliableSendError;
use crate::network::usermessage::UserMsgReader;
use crate::network::wait_socket;
use crate::network::{
    ClientToServer, FromClient, NetSend, PacketType, ReliableBody, ServerToClient, RECV_BUDGET,
};
use crate::r#enum::InputButtons;
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use crate::state::GameState;
use std::collections::{HashMap, VecDeque};
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
    let mut peers = Vec::new();
    let mut joined = Vec::new();
    let mut players: Vec<RemotePlayer> = Vec::new();
    let _: () = game.run_hook("Initialize", ());
    let _ = game.take_motion();

    loop {
        let now = Instant::now();
        let dt = now.duration_since(last_time).as_secs_f64();
        last_time = now;

        accumulated_time += dt;

        joined.clear();
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
            let mut player_handles = Vec::new();
            let mut player_idx = 0;

            while player_idx < players.len() {
                player_handles.push(players[player_idx].player);
                player_idx += 1;
            }

            let anim_events = game.drive_free_anims(&player_handles);
            game.fire_anim_events(anim_events);
            game.think_entities();
            simulate_players(&mut game, &mut players);
            game.step_physics(&player_handles);

            ticked = true;
        }

        while let Some((hash, data)) = game.script_engine.poll_usermessage() {
            game.send_reliable(ServerToClient::UserMessage { hash, data });
        }

        while let Ok(net_event) = game.network_receiver.try_recv() {
            match net_event {
                FromClient::Connected { addr, generation } => {
                    log::info!("[sv] peer {}", addr);
                    if !peers.contains(&addr) {
                        peers.push(addr);
                    }

                    joined.push(addr);

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
                            log::info!("[sv] spawn {handle:?}");
                            let _: () = game.run_hook("PlayerSpawned", handle);
                            let mut idx = 0;

                            while idx < players.len() {
                                if players[idx].addr != addr {
                                    let (origin, angles) = game
                                        .entities
                                        .get(handle)
                                        .map(|entity| {
                                            (entity.base().position, entity.base().angles)
                                        })
                                        .unwrap_or((
                                            Vector3::new(0.0, 28.0, 2.0),
                                            Angle3::default(),
                                        ));
                                    game.send_reliable_to(
                                        players[idx].addr,
                                        ServerToClient::EntitySpawned {
                                            handle,
                                            class_hash: Player::CLASS_HASH,
                                            position: origin,
                                            angles,
                                            owner: EntityHandle::NULL,
                                            networked: Vec::new(),
                                        },
                                    );
                                }

                                idx += 1;
                            }
                        }
                    }

                    emit_snapshot(&mut game, addr, generation);
                    emit_voxel_baseline(&game, addr);
                    emit_sound_baseline(&game, addr);
                    game.send_state_to(
                        addr,
                        ServerToClient::MapChange {
                            map_name: game.map_name.clone(),
                        },
                    );
                    game.send_state_to(
                        addr,
                        ServerToClient::BrushScale {
                            scale: game.brush_world.scale(),
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
                    log::info!("[sv] peer left {}", addr);
                    peers.retain(|peer| *peer != addr);
                    drop_player(&mut game, addr, &mut players);
                }
                FromClient::Message { addr, event } => {
                    match &event {
                        ClientToServer::PlayerInput { .. } => {
                            log::debug!("[sv] {} from {}", event.summary(), addr);
                        }
                        _ => {
                            log::info!("[sv] {} from {}", event.summary(), addr);
                        }
                    }

                    match event {
                        ClientToServer::UserMessage { hash, data } => {
                            game.run_usermessage(hash, UserMsgReader::new(data));
                        }
                        ClientToServer::ScaleMaps { ratio } => {
                            let _ = game.scale_maps(ratio);
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
                    }
                }
            }
        }

        while let Some((hash, data)) = game.script_engine.poll_usermessage() {
            game.send_reliable(ServerToClient::UserMessage { hash, data });
        }

        emit_entity_changes(&mut game);
        emit_anim_models(&mut game);
        flush_sounds(&mut game);

        emit_scale_dirty(&mut game, &peers);
        emit_motion(&mut game, &peers, &joined);

        if ticked {
            emit_tick_state(&game, &players);
            emit_predicted_state(&mut game, &players);
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
            log::warn!("[sv] {err}");
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
                    log::info!("[sv] connect");
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
                    log::info!("[sv] challenge response");
                    if server.is_connected(from) {
                        server.send_connected(from);
                    } else if server.verify_challenge(from, token) {
                        if let Some((_session, generation)) = server.add_client(from) {
                            server.send_connected(from);
                            log::info!("[sv] connect {}", from);
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
                    log::info!("[sv] challenge");
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

        server.flush_sim();
    }
}

#[cfg(feature = "server")]
fn handle_server_send(server: &mut NetworkServer, outgoing: NetSend<ServerToClient>) {
    match outgoing {
        NetSend::Reliable(event) => {
            let payload = wincode::serialize(&event).unwrap();
            for addr in server.broadcast_reliable(&payload) {
                log::warn!("[sv] reliable outbound full {}", addr);
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
                    log::warn!("[sv] reliable outbound full {}", addr);
                }
                Err(ReliableSendError::TooLarge) => {
                    log::warn!("[sv] reliable payload too large");
                }
                Err(ReliableSendError::Missing) => {}
            }
        }
        NetSend::StateTo(addr, event) => {
            let payload = wincode::serialize(&event).unwrap();
            match server.enqueue_state(addr, &payload) {
                Ok(()) => {}
                Err(ReliableSendError::Full) => {
                    log::warn!("[sv] reliable outbound full {}", addr);
                }
                Err(ReliableSendError::TooLarge) => {
                    log::warn!("[sv] reliable payload too large");
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
    let root = game
        .entities
        .get(handle)
        .map(|entity| entity.base().anim)
        .and_then(|playback| game.anims.root_motion(&playback, angles.y, cmd.tick, dt));
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
        root,
    );

    let Some(entity) = game.entities.get_mut(handle) else {
        return false;
    };

    let base = entity.base_mut();
    base.position = position;
    base.velocity = velocity;
    base.angles = angles;
    game.run_predicted(handle, cmd, true);

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
fn emit_sound_baseline(game: &GameState<FromClient, ServerToClient>, addr: SocketAddr) {
    let pending = game.sound.baseline();
    let mut sounds = Vec::new();
    let mut idx = 0;

    while idx < pending.len() {
        let play = &pending[idx];
        sounds.push(crate::network::events::LoopingSound {
            sound_hash: play.sound_hash,
            def_hash: play.def_hash,
            entity_handle: play.entity,
            position: play.position,
            volume: play.volume,
            pitch: play.pitch,
            positional: play.positional,
        });
        idx += 1;
    }

    game.send_reliable_to(addr, ServerToClient::SoundBaseline { sounds });
}

#[cfg(feature = "server")]
fn flush_sounds(game: &mut GameState<FromClient, ServerToClient>) {
    let pending = game.sound.take_pending();
    let mut idx = 0;

    while idx < pending.len() {
        match &pending[idx] {
            crate::sound::Pending::Play(play) => {
                let event = ServerToClient::PlaySound {
                    sound_hash: play.sound_hash,
                    entity_handle: if play.entity.is_null() {
                        None
                    } else {
                        Some(play.entity)
                    },
                    position: play.position,
                    volume: play.volume,
                    pitch: play.pitch,
                    def_hash: play.def_hash,
                    tick: play.tick,
                    positional: play.positional,
                };

                if play.looping {
                    game.send_reliable(event);
                } else {
                    game.send_unreliable(event);
                }
            }
            crate::sound::Pending::Stop(stop) => {
                game.send_reliable(ServerToClient::StopSound {
                    def_hash: stop.def_hash,
                    sound_hash: stop.sound_hash,
                    entity_handle: stop.entity,
                });
            }
        }

        idx += 1;
    }
}

#[cfg(feature = "server")]
fn emit_scale_dirty(game: &mut GameState<FromClient, ServerToClient>, peers: &[SocketAddr]) {
    if let Some(scale) = game.voxel_world.take_scale() {
        for addr in peers {
            game.send_state_to(*addr, ServerToClient::VoxelScale { scale });
        }
    }

    if let Some(scale) = game.brush_world.take_scale() {
        for addr in peers {
            game.send_state_to(*addr, ServerToClient::BrushScale { scale });
        }
    }
}

#[cfg(feature = "server")]
fn emit_motion(
    game: &mut GameState<FromClient, ServerToClient>,
    peers: &[SocketAddr],
    skip: &[SocketAddr],
) {
    let Some(ratio) = game.take_motion() else {
        return;
    };

    for addr in peers {
        if skip.contains(addr) {
            continue;
        }

        game.send_state_to(*addr, ServerToClient::WorldMotion { ratio });
    }
}

#[cfg(feature = "server")]
fn emit_voxel_dirty(game: &mut GameState<FromClient, ServerToClient>, peers: &[SocketAddr]) {
    let updates = game.voxel_world.take_dirty();

    for update in updates {
        for addr in peers {
            game.send_state_to(*addr, ServerToClient::VoxelChunk(update.clone()));
        }
    }
}

#[cfg(feature = "server")]
fn emit_snapshot(
    game: &mut GameState<FromClient, ServerToClient>,
    addr: SocketAddr,
    generation: u32,
) {
    let mut states: HashMap<EntityHandle, EntityNetworked> = game
        .networked_state(None)
        .into_iter()
        .map(|entity| (entity.handle, entity))
        .collect();
    let mut pending = Vec::new();
    for (handle, entity) in game.entities.iter() {
        if !entity.is_spawned() {
            continue;
        }

        let base = entity.base();
        let owner = if base.owner.is_null() {
            None
        } else {
            Some(EntityOwnership {
                handle,
                owner: base.owner,
            })
        };
        let model = game
            .anims
            .model_paths(&base.anim)
            .map(|(mesh, clips)| EntityModel {
                handle,
                mesh,
                clips,
            });
        pending.push((
            EntitySnapshot {
                handle,
                class_hash: entity.class_hash(),
                health: entity.net_health(),
                position: base.position,
                angles: base.angles,
                velocity: base.velocity,
                ack: 0,
                anim: base.anim.snapshot(),
            },
            states.remove(&handle),
            owner,
            model,
        ));
    }

    pending.sort_by_key(|(_, _, owner, _)| owner.is_some());

    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut networked = Vec::new();
    let mut owners = Vec::new();
    let mut models = Vec::new();
    for (entity, state, owner, model) in pending {
        let has_state = state.is_some();
        let has_owner = owner.is_some();
        let has_model = model.is_some();
        batch.push(entity);
        networked.extend(state);
        owners.extend(owner);
        models.extend(model);
        if snapshot_fits(
            generation,
            true,
            u16::MAX,
            u16::MAX,
            &batch,
            &networked,
            &owners,
            &models,
        ) {
            continue;
        }

        let overflow = batch.pop().unwrap();
        let overflow_state = if has_state { networked.pop() } else { None };
        let overflow_owner = if has_owner { owners.pop() } else { None };
        let overflow_model = if has_model { models.pop() } else { None };
        if !batch.is_empty() {
            batches.push((
                std::mem::take(&mut batch),
                std::mem::take(&mut networked),
                std::mem::take(&mut owners),
                std::mem::take(&mut models),
            ));
        }

        batch.push(overflow);
        networked.extend(overflow_state);
        owners.extend(overflow_owner);
        models.extend(overflow_model);
        if !snapshot_fits(
            generation,
            true,
            u16::MAX,
            u16::MAX,
            &batch,
            &networked,
            &owners,
            &models,
        ) {
            log::warn!("[sv] snapshot entity too large");
            batch.clear();
            networked.clear();
            owners.clear();
            models.clear();
        }
    }

    if !batch.is_empty() {
        batches.push((batch, networked, owners, models));
    }

    if batches.is_empty() {
        batches.push((Vec::new(), Vec::new(), Vec::new(), Vec::new()));
    }

    let part_count = batches.len() as u16;
    for (idx, (entities, networked, owners, models)) in batches.into_iter().enumerate() {
        log::info!(
            "[sv] WorldSnapshot to {} gen={} reset={} part={}/{} ents={} networked={} owners={} models={}",
            addr,
            generation,
            idx == 0,
            idx,
            part_count,
            entities.len(),
            networked.len(),
            owners.len(),
            models.len()
        );
        game.send_state_to(
            addr,
            ServerToClient::WorldSnapshot {
                generation,
                reset: idx == 0,
                part: idx as u16,
                parts: part_count,
                entities,
                networked,
                owners,
                models,
            },
        );
    }
}

#[cfg(feature = "server")]
fn emit_predicted_state(
    game: &mut GameState<FromClient, ServerToClient>,
    players: &[RemotePlayer],
) {
    let tick = game.tick_count;
    let mut idx = 0;

    while idx < players.len() {
        let handle = players[idx].player;
        let Some(player) = game.entities.get(handle).map(|entity| {
            let base = entity.base();

            EntitySnapshot {
                handle,
                class_hash: entity.class_hash(),
                health: entity.net_health(),
                position: base.position,
                angles: base.angles,
                velocity: base.velocity,
                ack: players[idx].ack,
                anim: base.anim.snapshot(),
            }
        }) else {
            idx += 1;

            continue;
        };

        let mut event = ServerToClient::PredictedState {
            tick,
            player,
            entities: game.predicted_state(handle),
            anims: game.owned_anims(handle),
        };
        let size = wincode::serialized_size(&event).unwrap() as usize;

        log::trace!(
            "[sv netvar] send predicted {:?} ack={} bytes={}",
            handle,
            players[idx].ack,
            size
        );

        if size > unreliable_message_limit() {
            log::warn!("[sv] predicted state too large for {:?}", handle);

            if let ServerToClient::PredictedState { entities, .. } = &mut event {
                entities.clear();
            }
        }

        game.send_unreliable_to(players[idx].addr, event);
        idx += 1;
    }
}

#[cfg(feature = "server")]
fn emit_anim_models(game: &mut GameState<FromClient, ServerToClient>) {
    for model in game.take_anim_models() {
        game.send_reliable(ServerToClient::AnimModel {
            handle: model.handle,
            mesh: model.mesh,
            clips: model.clips,
        });
    }
}

#[cfg(feature = "server")]
fn emit_entity_changes(game: &mut GameState<FromClient, ServerToClient>) {
    let spawned = game.entities.take_net_spawned();
    let (removed, updates): (Vec<_>, Vec<_>) = game
        .collect_networked()
        .into_iter()
        .partition(|entity| !game.entities.is_valid(entity.handle));
    let removed = removed
        .into_iter()
        .filter(|entity| !spawned.contains(&entity.handle))
        .collect();
    send_networked(game, removed);

    for handle in game.take_despawned() {
        game.sound.forget_entity(handle);
        game.send_reliable(ServerToClient::EntityDespawned { handle });
    }

    for handle in game.entities.take_owner_changed() {
        let Some(owner) = game.entities.get(handle).map(|entity| entity.base().owner) else {
            continue;
        };

        log::debug!("[sv netvar] send owner {:?} -> {:?}", handle, owner);
        game.send_reliable(ServerToClient::EntityOwner { handle, owner });
    }

    for handle in spawned {
        let Some((class_hash, position, angles, owner)) = game.entities.get(handle).map(|entity| {
            (
                entity.class_hash(),
                entity.base().position,
                entity.base().angles,
                entity.base().owner,
            )
        }) else {
            continue;
        };

        let networked = game
            .networked_state(Some(handle))
            .pop()
            .map(|entity| entity.vars)
            .unwrap_or_default();
        log::debug!(
            "[sv netvar] send spawn {:?} class={} owner={:?} vars={}",
            handle,
            class_hash,
            owner,
            networked.len()
        );
        game.send_reliable(ServerToClient::EntitySpawned {
            handle,
            class_hash,
            position,
            angles,
            owner,
            networked,
        });
    }

    send_networked(game, updates);
}

#[cfg(feature = "server")]
fn send_networked(game: &mut GameState<FromClient, ServerToClient>, updates: Vec<EntityNetworked>) {
    if updates.is_empty() {
        return;
    }

    let header = wincode::serialized_size(&ServerToClient::NetworkedUpdate {
        entities: Vec::new(),
    })
    .unwrap() as usize;
    let mut batch = Vec::new();
    let mut size = header;
    for entity in updates {
        let entity_size = wincode::serialized_size(&entity).unwrap() as usize;

        if encoded_packet_count(header + entity_size).is_none() {
            log::warn!("[sv] networked update too large");

            continue;
        }

        if encoded_packet_count(size + entity_size).is_none() {
            log::debug!(
                "[sv netvar] send update ents={} bytes={}",
                batch.len(),
                size
            );
            game.send_reliable(ServerToClient::NetworkedUpdate {
                entities: std::mem::take(&mut batch),
            });
            size = header;
        }

        batch.push(entity);
        size += entity_size;
    }

    if !batch.is_empty() {
        log::debug!(
            "[sv netvar] send update ents={} bytes={}",
            batch.len(),
            size
        );
        game.send_reliable(ServerToClient::NetworkedUpdate { entities: batch });
    }
}

#[cfg(feature = "server")]
fn emit_tick_state(game: &GameState<FromClient, ServerToClient>, players: &[RemotePlayer]) {
    let tick = game.tick_count;
    let mut pending = Vec::new();
    for (handle, entity) in game.entities.iter() {
        if !entity.is_spawned() {
            continue;
        }

        let base = entity.base();
        pending.push(EntitySnapshot {
            handle,
            class_hash: entity.class_hash(),
            health: entity.net_health(),
            position: base.position,
            angles: base.angles,
            velocity: base.velocity,
            ack: player_ack(players, handle),
            anim: base.anim.snapshot(),
        });
    }

    if pending.is_empty() {
        return;
    }

    log::debug!("[sv] tick {} state for {} entities", tick, pending.len());

    if tick_fits(tick, 0, 1, &pending) {
        game.send_unreliable(ServerToClient::TickState {
            tick,
            part: 0,
            parts: 1,
            entities: pending,
        });

        return;
    }

    let mut batches = Vec::new();
    let mut batch = Vec::new();
    for entity in pending {
        batch.push(entity);
        if tick_fits(tick, u16::MAX, u16::MAX, &batch) {
            continue;
        }

        let overflow = batch.pop().unwrap();
        if !batch.is_empty() {
            batches.push(std::mem::take(&mut batch));
        }

        batch.push(overflow);
        if !tick_fits(tick, u16::MAX, u16::MAX, &batch) {
            log::warn!("[sv] tick state too large");
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
    for (idx, entities) in batches.into_iter().enumerate() {
        log::debug!(
            "[sv] tick {} state part {}/{} ({} ents)",
            tick,
            idx,
            part_count,
            entities.len()
        );
        game.send_unreliable(ServerToClient::TickState {
            tick,
            part: idx as u16,
            parts: part_count,
            entities,
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
    networked: &[EntityNetworked],
    owners: &[EntityOwnership],
    models: &[EntityModel],
) -> bool {
    let event = ServerToClient::WorldSnapshot {
        generation,
        reset,
        part,
        parts,
        entities: entities.to_vec(),
        networked: networked.to_vec(),
        owners: owners.to_vec(),
        models: models.to_vec(),
    };
    let payload = wincode::serialize(&event).unwrap();

    encoded_packet_count(payload.len()).is_some()
}

#[cfg(feature = "server")]
fn tick_fits(tick: u64, part: u16, parts: u16, entities: &[EntitySnapshot]) -> bool {
    let event = ServerToClient::TickState {
        tick,
        part,
        parts,
        entities: entities.to_vec(),
    };
    let payload = wincode::serialize(&event).unwrap();

    payload.len() <= unreliable_message_limit()
}
