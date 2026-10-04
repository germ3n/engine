use crate::demo::{self, DemoFrame, DemoPlayer, DemoSession, SlotEvent, SlotInput};
use crate::entities::context::FrameInfo;
use crate::entities::{EntityHandle, Player};
use crate::movement::{self, UserCommand};
use crate::network::events::{
    EntityBones, EntityModel, EntityNetworked, EntityOwnership, EntitySnapshot,
};
use crate::network::packet::STREAM_STATE;
use crate::network::packet::{
    encoded_packet_count, owned_payload, unreliable_message_limit, BundlePart, CONNECTION_TIMEOUT,
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
use crate::script::Realm;
use crate::state::GameState;
use crate::world::ChunkPos;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

struct RemotePlayer {
    addr: SocketAddr,
    player: EntityHandle,
    slot: u16,
    steam_id: u64,
    name: String,
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
    demo::bind_realm(Realm::Server);
    let mut last_time = Instant::now();
    let mut accumulated_time = 0.0;
    let mut peers = Vec::new();
    let mut joined = Vec::new();
    let mut players: Vec<RemotePlayer> = Vec::new();
    let mut recording: Option<DemoSession> = None;
    let mut nav_feed = NavFeed::new();
    let mut next_slot: u16 = 0;
    let _: () = game.run_hook("Initialize", ());
    let _ = game.take_motion();
    game.begin_terrain();

    loop {
        let now = Instant::now();
        let dt = now.duration_since(last_time).as_secs_f64();
        last_time = now;

        accumulated_time += dt;

        crate::console::poll_autocomplete(Realm::Server, &game.script_engine.lua);
        poll_demo(&mut game, &players, &mut recording);
        poll_nav_commands(&mut game, &peers);
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
            let inputs = simulate_players(&mut game, &mut players);
            game.step_physics(&player_handles);

            if let Some(session) = recording.as_mut() {
                let events = session.take_events();
                session.write_frame(&DemoFrame::ServerTick {
                    tick: game.tick_count,
                    inputs,
                    events,
                });
                let props = demo::capture_props(&game);

                if !props.is_empty() {
                    session.write_frame(&DemoFrame::Entities {
                        tick: game.tick_count,
                        entities: props,
                    });
                }

                if game.cur_time >= session.next_shot {
                    let listed = listed_players(&players);
                    let tick = game.tick_count;
                    let shot = demo::capture_world(&mut game, tick, EntityHandle::NULL, &listed);
                    session.write_mark(&DemoFrame::Checkpoint(shot));

                    while session.next_shot <= game.cur_time {
                        session.next_shot += demo::SHOT_INTERVAL;
                    }
                }
            }

            ticked = true;
        }

        if recording.as_ref().is_some_and(|session| session.dead) {
            log::warn!("[demo] recording stopped");
            recording = None;
        }

        while let Some((hash, data)) = game.script_engine.poll_usermessage() {
            game.send_reliable(ServerToClient::UserMessage { hash, data });
        }

        while let Ok(net_event) = game.network_receiver.try_recv() {
            match net_event {
                FromClient::Connected {
                    addr,
                    generation,
                    steam_id,
                    name,
                } => {
                    log::info!("[sv] peer {}", addr);
                    if !peers.contains(&addr) {
                        peers.push(addr);
                    }

                    joined.push(addr);

                    if !players.iter().any(|player| player.addr == addr) {
                        let handle = spawn_player(&mut game, players.len());

                        if !handle.is_null() {
                            let slot = next_slot;
                            next_slot = next_slot.saturating_add(1);
                            players.push(RemotePlayer {
                                addr,
                                player: handle,
                                slot,
                                steam_id,
                                name: name.clone(),
                                pending: VecDeque::new(),
                                last_buttons: InputButtons::NONE,
                                ack: 0,
                            });

                            if let Some(session) = recording.as_mut() {
                                session.push_event(SlotEvent::Join {
                                    slot,
                                    handle,
                                    name: name.clone(),
                                });
                            }
                            let joined_player = &players[players.len() - 1];
                            log::info!(
                                "[sv] spawn {:?} {} {}",
                                joined_player.player,
                                joined_player.steam_id,
                                joined_player.name
                            );
                            game.send_reliable(ServerToClient::PlayerConnected { handle, name });
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
                    emit_brush_baseline(&game, addr);
                    // emit_nav_to(&game, addr, &mut nav_feed);

                    if let Some(player) = players.iter().find(|player| player.addr == addr) {
                        let handle = player.player;
                        game.send_state_to(
                            addr,
                            ServerToClient::PlayerSpawned { handle },
                        );
                        let _: () = game.run_hook("PlayerSpawned", handle);
                    }
                }
                FromClient::Disconnected { addr } => {
                    log::info!("[sv] peer left {}", addr);
                    peers.retain(|peer| *peer != addr);
                    nav_feed.drop_peer(addr);

                    if let Some(session) = recording.as_mut() {
                        let mut idx = 0;

                        while idx < players.len() {
                            if players[idx].addr == addr {
                                session.push_event(SlotEvent::Leave {
                                    slot: players[idx].slot,
                                });
                            }

                            idx += 1;
                        }
                    }

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
                            if let Some(session) = recording.as_mut() {
                                session.push_event(SlotEvent::UserMessage {
                                    slot: slot_for(&players, addr),
                                    hash,
                                    data: data.clone(),
                                });
                            }

                            game.run_usermessage(hash, UserMsgReader::new(data));
                        }
                        ClientToServer::ScaleMaps { ratio } => {
                            if let Some(session) = recording.as_mut() {
                                session.push_event(SlotEvent::ScaleMaps { ratio });
                            }

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

        let networked = emit_entity_changes(&mut game);

        if let Some(session) = recording.as_mut() {
            if !networked.is_empty() {
                session.write_frame(&DemoFrame::NetVars {
                    tick: game.tick_count,
                    entities: networked,
                });
            }
        }

        if recording.as_ref().is_some_and(|session| session.dead) {
            log::warn!("[demo] recording stopped");
            recording = None;
        }
        emit_anim_models(&mut game);
        emit_anim_bones(&mut game);
        flush_sounds(&mut game);

        emit_scale_dirty(&mut game, &peers);
        emit_brush_edits(&mut game, &peers, &joined);
        emit_motion(&mut game, &peers, &joined);
        let centers = terrain_centers(&game, &players);
        game.poll_voxel_gen(&centers);
        poll_nav_bake(&mut game, &centers, &peers, &mut nav_feed);
        // nav_feed.pump(&game);

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
struct PendingJoin {
    steam_id: u64,
    name: String,
    started: Instant,
}

#[cfg(feature = "server")]
fn send_challenge(server: &NetworkServer, addr: SocketAddr) {
    let token = server.challenge_for(addr);
    let (secure, host_steam_id) = crate::network::steam::host_challenge();
    let bytes = wincode::serialize(&PacketType::Challenge {
        token,
        secure,
        host_steam_id,
    })
    .unwrap();
    let _ = server.send_to(addr, &bytes);
}

#[cfg(feature = "server")]
fn forget_auth(
    addr: SocketAddr,
    authed: &mut HashMap<SocketAddr, u64>,
    pending: &mut HashMap<SocketAddr, PendingJoin>,
) {
    authed.remove(&addr);
    pending.remove(&addr);
    crate::network::steam::end_session(addr);
}

#[cfg(feature = "server")]
fn admit(
    server: &mut NetworkServer,
    tx: &Sender<FromClient>,
    addr: SocketAddr,
    steam_id: u64,
    name: String,
) -> bool {
    let Some((_session, generation)) = server.add_client(addr) else {
        return false;
    };

    server.send_connected(addr);
    log::info!("[sv] connect {}", addr);
    let _ = tx.send(FromClient::Connected {
        addr,
        generation,
        steam_id,
        name,
    });

    true
}

#[cfg(feature = "server")]
fn poll_auth(
    server: &mut NetworkServer,
    tx: &Sender<FromClient>,
    authed: &mut HashMap<SocketAddr, u64>,
    pending: &mut HashMap<SocketAddr, PendingJoin>,
) {
    while let Some(update) = crate::network::steam::poll_auth() {
        match update {
            crate::network::steam::AuthUpdate::Accepted { addr, steam_id } => {
                let Some(join) = pending.remove(&addr) else {
                    crate::network::steam::end_session(addr);

                    continue;
                };

                if join.steam_id != steam_id {
                    forget_auth(addr, authed, pending);

                    continue;
                }

                let old = authed.iter().find_map(|(peer, id)| {
                    if *peer != addr && *id == steam_id {
                        Some(*peer)
                    } else {
                        None
                    }
                });
                if let Some(old) = old {
                    if server.disconnect_client(old) {
                        let _ = tx.send(FromClient::Disconnected { addr: old });
                    }

                    forget_auth(old, authed, pending);
                }

                authed.insert(addr, steam_id);
                if !admit(server, tx, addr, steam_id, join.name) {
                    forget_auth(addr, authed, pending);
                }
            }
            crate::network::steam::AuthUpdate::Rejected { addr } => {
                log::info!("[sv] auth rejected {}", addr);
                forget_auth(addr, authed, pending);
            }
            crate::network::steam::AuthUpdate::Revoked { addr } => {
                log::info!("[sv] auth revoked {}", addr);
                let gone = server.disconnect_client(addr);
                forget_auth(addr, authed, pending);
                if gone {
                    let _ = tx.send(FromClient::Disconnected { addr });
                }
            }
        }
    }
}

#[cfg(feature = "server")]
fn expire_auth(
    authed: &mut HashMap<SocketAddr, u64>,
    pending: &mut HashMap<SocketAddr, PendingJoin>,
) {
    let stale: Vec<SocketAddr> = pending
        .iter()
        .filter(|(_, join)| join.started.elapsed() >= CONNECTION_TIMEOUT)
        .map(|(addr, _)| *addr)
        .collect();
    for addr in stale {
        log::info!("[sv] auth timeout {}", addr);
        forget_auth(addr, authed, pending);
    }
}

#[cfg(feature = "server")]
fn trim_name(name: &str) -> String {
    name.chars()
        .filter(|ch| !ch.is_control())
        .take(32)
        .collect()
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
    let mut authed: HashMap<SocketAddr, u64> = HashMap::new();
    let mut pending: HashMap<SocketAddr, PendingJoin> = HashMap::new();

    loop {
        while let Ok(outgoing) = rx.try_recv() {
            handle_server_send(&mut server, outgoing);
        }

        poll_auth(&mut server, &tx, &mut authed, &mut pending);
        expire_auth(&mut authed, &mut pending);

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
                        forget_auth(from, &mut authed, &mut pending);
                        let _ = tx.send(FromClient::Disconnected { addr: from });
                    }

                    if server.is_connected(from) {
                        server.send_connected(from);
                    } else {
                        send_challenge(&server, from);
                    }
                }
                PacketType::ChallengeResponse {
                    token,
                    steam_id,
                    ticket,
                    name,
                } => {
                    log::info!("[sv] challenge response");
                    if server.is_connected(from) {
                        server.send_connected(from);
                    } else if server.verify_challenge(from, token) {
                        let (secure, _) = crate::network::steam::host_challenge();
                        if !secure {
                            let _ = admit(&mut server, &tx, from, 0, String::new());
                        } else if ticket.is_empty()
                            || ticket.len() > 1024
                            || name.len() > 128
                            || crate::network::steam::peer_id(from)
                                .is_some_and(|peer| peer != steam_id)
                        {
                            log::info!("[sv] auth rejected {}", from);
                        } else if !pending.contains_key(&from) {
                            let name = trim_name(&name);
                            pending.insert(
                                from,
                                PendingJoin {
                                    steam_id,
                                    name,
                                    started: Instant::now(),
                                },
                            );
                            crate::network::steam::begin_auth(from, steam_id, &ticket);
                        }
                    } else {
                        send_challenge(&server, from);
                    }
                }
                PacketType::Challenge { .. } => {
                    log::info!("[sv] challenge");
                }
                PacketType::Connected { .. } => {}
                PacketType::Disconnect { session } => {
                    if server.retire_client(from, session) {
                        forget_auth(from, &mut authed, &mut pending);
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
            forget_auth(addr, &mut authed, &mut pending);
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
) -> Vec<SlotInput> {
    let dt = game.tick_interval;
    let gravity = movement::gravity(&game.cvars);
    let mut recorded = Vec::new();
    let mut idx = 0;

    while idx < players.len() {
        let Some(cmd) = players[idx].take_cmd() else {
            idx += 1;

            continue;
        };

        let handle = players[idx].player;
        let prev = players[idx].last_buttons;
        let slot = players[idx].slot;

        if apply_command(game, handle, &cmd, prev, dt, gravity) {
            players[idx].last_buttons = cmd.buttons;
            players[idx].ack = cmd.tick;
            recorded.push(SlotInput { slot, command: cmd });
        }

        idx += 1;
    }

    recorded
}

fn listed_players(players: &[RemotePlayer]) -> Vec<DemoPlayer> {
    let mut listed = Vec::new();
    let mut idx = 0;

    while idx < players.len() {
        listed.push(DemoPlayer {
            slot: players[idx].slot,
            handle: players[idx].player,
            name: String::new(),
            buttons: players[idx].last_buttons,
        });
        idx += 1;
    }

    listed
}

fn slot_for(players: &[RemotePlayer], addr: SocketAddr) -> u16 {
    let mut idx = 0;

    while idx < players.len() {
        if players[idx].addr == addr {
            return players[idx].slot;
        }

        idx += 1;
    }

    0
}

fn poll_demo(
    game: &mut GameState<FromClient, ServerToClient>,
    players: &[RemotePlayer],
    recording: &mut Option<DemoSession>,
) {
    let commands = demo::drain(Realm::Server);
    let mut idx = 0;

    while idx < commands.len() {
        match &commands[idx] {
            demo::DemoCommand::Record { name } => {
                if recording.is_some() {
                    log::warn!("[demo] already recording");
                } else {
                    let rate = (1.0 / game.tick_interval).round() as u32;
                    let header = demo::DemoHeader {
                        kind: demo::KIND_SERVER,
                        map_name: game.map_name.clone(),
                        tickrate: rate,
                        map_scale: game.brush_world.scale(),
                        voxel_scale: game.voxel_world.scale(),
                    };

                    match DemoSession::create(name, header) {
                        Ok(mut session) => {
                            let listed = listed_players(players);
                            session.players = listed.clone();
                            let tick = game.tick_count;
                            let shot = demo::capture_world(game, tick, EntityHandle::NULL, &listed);
                            session.write_mark(&DemoFrame::Checkpoint(shot));
                            session.next_shot = game.cur_time + demo::SHOT_INTERVAL;
                            log::info!("[demo] recording {name}");
                            *recording = Some(session);
                        }
                        Err(err) => log::warn!("[demo] {err}"),
                    }
                }
            }
            demo::DemoCommand::Stop => {
                if recording.take().is_some() {
                    log::info!("[demo] stopped");
                } else {
                    log::warn!("[demo] not recording");
                }
            }
            _ => {}
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

//send nav
fn poll_nav_commands(game: &mut GameState<FromClient, ServerToClient>, peers: &[SocketAddr]) {
    let _ = peers;
    let commands = crate::world::nav::drain();
    let mut idx = 0;

    while idx < commands.len() {
        match &commands[idx] {
            crate::world::nav::NavCommand::Build { input, params } => {
                game.nav.request_build(*input, *params);
            }
            crate::world::nav::NavCommand::Show { enabled } => {
                let changed = game.nav.state.show != *enabled;
                game.nav.state.set_show(*enabled);

                if changed {
                    // emit_nav_show(game, peers);
                }
            }
            crate::world::nav::NavCommand::Path { start, goal } => {
                let points = game.nav.state.query(*start, *goal);
                game.nav.state.set_debug_path(points.clone());
                // emit_nav_path(game, peers, false, points);
                let _ = points;
            }
        }

        idx += 1;
    }
}

fn poll_nav_bake(
    game: &mut GameState<FromClient, ServerToClient>,
    centers: &[crate::world::ChunkPos],
    peers: &[SocketAddr],
    feed: &mut NavFeed,
) {
    let _ = (feed, peers);

    if game.poll_nav(centers) {
        // feed.set_parts(nav_wire_parts(game), peers);
    }

    if let Some(points) = game.nav.take_follow() {
        // emit_nav_path(game, peers, true, points);
        let _ = points;
    }
}

struct NavFeed {
    parts: Vec<Vec<u8>>,
    cursor: Vec<(SocketAddr, usize)>,
}

impl NavFeed {
    fn new() -> Self {
        Self {
            parts: Vec::new(),
            cursor: Vec::new(),
        }
    }

    fn set_parts(&mut self, parts: Vec<Vec<u8>>, peers: &[SocketAddr]) {
        self.parts = parts;
        self.cursor.clear();
        let mut idx = 0;

        while idx < peers.len() {
            self.cursor.push((peers[idx], 0));
            idx += 1;
        }

        if !self.parts.is_empty() {
            log::info!("[nav] sending mesh in {} parts", self.parts.len());
        }
    }

    fn add_peer(&mut self, addr: SocketAddr) {
        if self.parts.is_empty() {
            return;
        }

        self.cursor.retain(|item| item.0 != addr);
        self.cursor.push((addr, 0));
    }

    fn drop_peer(&mut self, addr: SocketAddr) {
        self.cursor.retain(|item| item.0 != addr);
    }

    fn pump(&mut self, game: &GameState<FromClient, ServerToClient>) {
        let batch = 8usize;
        let mut idx = 0;

        while idx < self.cursor.len() {
            let mut sent = 0;

            while sent < batch && self.cursor[idx].1 < self.parts.len() {
                let part = self.cursor[idx].1;
                let addr = self.cursor[idx].0;
                let ok = game.try_send_state_to(
                    addr,
                    ServerToClient::NavMesh {
                        part: part as u16,
                        parts: self.parts.len() as u16,
                        bytes: self.parts[part].clone(),
                    },
                );

                if !ok {
                    return;
                }

                self.cursor[idx].1 += 1;
                sent += 1;

                if self.cursor[idx].1 == self.parts.len() || self.cursor[idx].1 % 512 == 0 {
                    log::info!(
                        "[nav] mesh {}/{} to {addr}",
                        self.cursor[idx].1,
                        self.parts.len()
                    );
                }
            }

            idx += 1;
        }

        let total = self.parts.len();
        self.cursor.retain(|item| item.1 < total);
    }
}

fn nav_wire_parts(game: &GameState<FromClient, ServerToClient>) -> Vec<Vec<u8>> {
    if !game.nav.state.loaded || game.nav.state.file_bytes.is_empty() {
        return Vec::new();
    }

    let packed = crate::world::nav::compress_nav(&game.nav.state.file_bytes);
    let limit = crate::network::packet::reliable_payload_limit()
        .saturating_sub(256)
        .max(256);
    let parts = crate::world::nav::split_wire(&packed, limit);

    if parts.len() > u16::MAX as usize {
        log::warn!("[nav] mesh is too large to send");

        return Vec::new();
    }

    parts
}

fn emit_nav_to(
    game: &GameState<FromClient, ServerToClient>,
    addr: SocketAddr,
    feed: &mut NavFeed,
) {
    if game.nav.state.loaded {
        if feed.parts.is_empty() {
            feed.set_parts(nav_wire_parts(game), &[addr]);
        } else {
            feed.add_peer(addr);
        }
    }

    game.send_state_to(
        addr,
        ServerToClient::NavShow {
            enabled: game.nav.state.show,
        },
    );

    if !game.nav.state.debug_path.is_empty() {
        game.send_state_to(
            addr,
            ServerToClient::NavPath {
                follow: false,
                points: game.nav.state.debug_path.clone(),
            },
        );
    }

    if !game.nav.state.follow_path.is_empty() {
        game.send_state_to(
            addr,
            ServerToClient::NavPath {
                follow: true,
                points: game.nav.state.follow_path.clone(),
            },
        );
    }
}

fn emit_nav_show(game: &GameState<FromClient, ServerToClient>, peers: &[SocketAddr]) {
    let mut idx = 0;

    while idx < peers.len() {
        game.send_state_to(
            peers[idx],
            ServerToClient::NavShow {
                enabled: game.nav.state.show,
            },
        );
        idx += 1;
    }
}

fn emit_nav_path(
    game: &GameState<FromClient, ServerToClient>,
    peers: &[SocketAddr],
    follow: bool,
    points: Vec<Vector3>,
) {
    let mut idx = 0;

    while idx < peers.len() {
        game.send_state_to(
            peers[idx],
            ServerToClient::NavPath {
                follow,
                points: points.clone(),
            },
        );
        idx += 1;
    }
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
fn emit_brush_baseline(game: &GameState<FromClient, ServerToClient>, addr: SocketAddr) {
    let edits = game.brush_world.edits();
    let mut idx = 0;

    while idx < edits.len() {
        game.send_state_to(addr, ServerToClient::BrushEdit(edits[idx].clone()));
        idx += 1;
    }
}

#[cfg(feature = "server")]
fn emit_brush_edits(
    game: &mut GameState<FromClient, ServerToClient>,
    peers: &[SocketAddr],
    skip: &[SocketAddr],
) {
    let edits = game.brush_world.take_edits();
    let mut idx = 0;

    while idx < edits.len() {
        let mut peer = 0;

        while peer < peers.len() {
            if !skip.contains(&peers[peer]) {
                game.send_state_to(peers[peer], ServerToClient::BrushEdit(edits[idx].clone()));
            }

            peer += 1;
        }

        idx += 1;
    }
}

fn terrain_centers(
    game: &GameState<FromClient, ServerToClient>,
    players: &[RemotePlayer],
) -> Vec<ChunkPos> {
    let mut centers = Vec::new();
    let spawns = game.brush_world.spawns();

    if spawns.is_empty() {
        centers.push(
            game.voxel_world
                .block_at(Vector3::new(0.0, 28.0, 2.0))
                .chunk(),
        );
    } else {
        let mut idx = 0;

        while idx < spawns.len() {
            centers.push(game.voxel_world.block_at(spawns[idx]).chunk());
            idx += 1;
        }
    }

    let mut idx = 0;

    while idx < players.len() {
        if let Some(entity) = game.entities.get(players[idx].player) {
            centers.push(game.voxel_world.block_at(entity.base().position).chunk());
        }

        idx += 1;
    }

    centers
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
        let bones = {
            let bones = game.anims.entity_bones(handle.0);

            if bones.bones.is_empty() {
                None
            } else {
                Some(bones)
            }
        };
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
            bones,
        ));
    }

    pending.sort_by_key(|(_, _, owner, _, _)| owner.is_some());

    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut networked = Vec::new();
    let mut owners = Vec::new();
    let mut models = Vec::new();
    let mut bones = Vec::new();
    for (entity, state, owner, model, bone) in pending {
        let has_state = state.is_some();
        let has_owner = owner.is_some();
        let has_model = model.is_some();
        let has_bones = bone.is_some();
        batch.push(entity);
        networked.extend(state);
        owners.extend(owner);
        models.extend(model);
        bones.extend(bone);
        if snapshot_fits(
            generation,
            true,
            u16::MAX,
            u16::MAX,
            &batch,
            &networked,
            &owners,
            &models,
            &bones,
        ) {
            continue;
        }

        let overflow = batch.pop().unwrap();
        let overflow_state = if has_state { networked.pop() } else { None };
        let overflow_owner = if has_owner { owners.pop() } else { None };
        let overflow_model = if has_model { models.pop() } else { None };
        let overflow_bones = if has_bones { bones.pop() } else { None };
        if !batch.is_empty() {
            batches.push((
                std::mem::take(&mut batch),
                std::mem::take(&mut networked),
                std::mem::take(&mut owners),
                std::mem::take(&mut models),
                std::mem::take(&mut bones),
            ));
        }

        batch.push(overflow);
        networked.extend(overflow_state);
        owners.extend(overflow_owner);
        models.extend(overflow_model);
        bones.extend(overflow_bones);
        if !snapshot_fits(
            generation,
            true,
            u16::MAX,
            u16::MAX,
            &batch,
            &networked,
            &owners,
            &models,
            &bones,
        ) {
            log::warn!("[sv] snapshot entity too large");
            batch.clear();
            networked.clear();
            owners.clear();
            models.clear();
            bones.clear();
        }
    }

    if !batch.is_empty() {
        batches.push((batch, networked, owners, models, bones));
    }

    if batches.is_empty() {
        batches.push((Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()));
    }

    let part_count = batches.len() as u16;
    for (idx, (entities, networked, owners, models, bones)) in batches.into_iter().enumerate() {
        log::info!(
            "[sv] WorldSnapshot to {} gen={} reset={} part={}/{} ents={} networked={} owners={} models={} bones={}",
            addr,
            generation,
            idx == 0,
            idx,
            part_count,
            entities.len(),
            networked.len(),
            owners.len(),
            models.len(),
            bones.len()
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
                bones,
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
fn emit_anim_bones(game: &mut GameState<FromClient, ServerToClient>) {
    for bones in game.take_anim_bones() {
        game.send_reliable(ServerToClient::AnimBones {
            handle: bones.handle,
            bones: bones.bones,
        });
    }
}

#[cfg(feature = "server")]
fn emit_entity_changes(game: &mut GameState<FromClient, ServerToClient>) -> Vec<EntityNetworked> {
    let spawned = game.entities.take_net_spawned();
    let (removed, updates): (Vec<_>, Vec<_>) = game
        .collect_networked()
        .into_iter()
        .partition(|entity| !game.entities.is_valid(entity.handle));
    let removed = removed
        .into_iter()
        .filter(|entity| !spawned.contains(&entity.handle))
        .collect();
    let mut sent = send_networked(game, removed);

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

    sent.extend(send_networked(game, updates));

    sent
}

#[cfg(feature = "server")]
fn send_networked(
    game: &mut GameState<FromClient, ServerToClient>,
    updates: Vec<EntityNetworked>,
) -> Vec<EntityNetworked> {
    if updates.is_empty() {
        return Vec::new();
    }

    let mut sent = Vec::new();

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

        sent.push(entity.clone());

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

    sent
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
    bones: &[EntityBones],
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
        bones: bones.to_vec(),
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
