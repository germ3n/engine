use std::sync::mpsc::{Sender, Receiver};
use crate::network::{ClientToServer, ServerToClient};
use crate::state::GameState;
use std::time::{Instant, Duration};
use std::sync::atomic::Ordering;
use crate::network::server::NetworkServer;
use crate::network::{PacketType, NetSend, FromClient, ReliableBody, accept_unreliable};
use crate::network::usermessage::hash_usermessage_name;
use crate::network::usermessage::UserMsgReader;
use crate::network::server::ReliableSendError;
use crate::network::events::{EntitySnapshot, NetTransform};
use crate::network::packet::{encoded_packet_count, stamp, unreliable_payload_limit, BundlePart};
use crate::entities::context::FrameInfo;
use std::net::SocketAddr;

#[cfg(feature = "server")]
pub fn server_loop(mut game: GameState<FromClient, ServerToClient>) {
    let mut last_time = Instant::now();
    let mut accumulated_time = 0.0;
    let mut tick_idx = 0; 

    loop {
        let now = Instant::now();
        let dt = now.duration_since(last_time).as_secs_f64();
        last_time = now;

        accumulated_time += dt;

        let mut ticked = false;
        while accumulated_time >= game.tick_interval {
            accumulated_time -= game.tick_interval;

            let ct = game.cur_time() + game.tick_interval;
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
                    emit_snapshot(&game, addr, generation);
                }
                FromClient::Disconnected { addr } => {
                    println!("[sv] peer left {}", addr);
                }
                FromClient::Message { addr, event } => {
                    match event {
                        ClientToServer::UserMessage { hash, data } => {
                            println!("[sv] usermessage {hash} from {addr}");
                            game.script_engine.run_usermessage(hash, UserMsgReader::new(data));
                        }
                        _ => {}
                    }
                }
            }
        }

        while let Some((hash, data)) = game.script_engine.poll_usermessage() {
            game.send_reliable(ServerToClient::UserMessage { hash, data });
        }

        if ticked {
            emit_tick_state(&game);
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
pub fn server_network_loop(tx: Sender<FromClient>, rx: Receiver<NetSend<ServerToClient>>) {
    let mut server = NetworkServer::new(25400, 128);

    loop {
        while let Ok(outgoing) = rx.try_recv() {
            match outgoing {
                NetSend::Reliable(event) => {
                    let payload = wincode::serialize(&event).unwrap();
                    for addr in server.broadcast_reliable(&payload) {
                        let _ = tx.send(FromClient::Disconnected { addr });
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
                            println!("[sv] reliable window full {}", addr);
                            if server.disconnect_client(addr) {
                                let _ = tx.send(FromClient::Disconnected { addr });
                            }
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

        let mut ack_addrs = Vec::new();
        let mut got_packet = false;

        // Use match instead of unwrap to handle the timeout gracefully
        loop {
            let Some((data, from)) = server.poll_message() else {
                break;
            };

            got_packet = true;
            if let Ok(packet) = wincode::deserialize::<PacketType>(&data) {
                match packet {
                    PacketType::Connect { replace } => {
                        println!("[sv] connect");
                        let restart = replace.map(|session| server.session_matches(from, session)).unwrap_or(false);
                        if restart && server.disconnect_client(from) {
                            let _ = tx.send(FromClient::Disconnected { addr: from });
                        }

                        if server.is_connected(from) {
                            server.send_connected(from);
                        } else {
                            let token = server.challenge_for(from);
                            let challenge_bytes = wincode::serialize(&PacketType::Challenge { token }).unwrap();
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
                            let _ = tx.send(FromClient::Connected { addr: from, generation });
                        }
                        } else {
                            let token = server.challenge_for(from);
                            let challenge_bytes = wincode::serialize(&PacketType::Challenge { token }).unwrap();
                            let _ = server.send_to(from, &challenge_bytes);
                        }
                    }
                    PacketType::Challenge { .. } => {
                        println!("[sv] challenge");
                    }
                    PacketType::Connected { .. } => {
                    }
                    PacketType::Disconnect { session } => {
                        if server.retire_client(from, session) {
                            let _ = tx.send(FromClient::Disconnected { addr: from });
                        }
                    }
                    PacketType::Bundle { session, ack, cumulative, selective, parts } => {
                        if server.session_matches(from, session) {
                            server.touch_client(from);
                            if ack {
                                if let Some(client) = server.clients.get_mut(&from) {
                                    client.reliable.handle_ack(cumulative, selective);
                                }
                                if parts.is_empty() {
                                    println!("[sv] ack {}", cumulative);
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
                    PacketType::Reliable { session, sequence, payload } => {
                        if accept_from_client(
                            &mut server,
                            from,
                            session,
                            sequence,
                            ReliableBody::Complete(payload.to_vec()),
                            &tx,
                        ) {
                            remember_ack(&mut ack_addrs, from);
                        }
                    }
                    PacketType::Ack { session, cumulative, selective } => {
                        if server.session_matches(from, session) {
                            server.touch_client(from);

                            if let Some(client) = server.clients.get_mut(&from) {
                                client.reliable.handle_ack(cumulative, selective);
                            }
                            println!("[sv] ack {}", cumulative);
                        }
                    }
                    PacketType::Unreliable { session, sequence, payload } => {
                        if server.session_matches(from, session) {
                            let fresh = {
                                let client = server.clients.get_mut(&from).unwrap();
                                accept_unreliable(&mut client.unreliable_in, sequence)
                            };

                            if fresh {
                                server.touch_client(from);

                                if let Ok(event) = wincode::deserialize::<ClientToServer>(&payload) {
                                    let _ = tx.send(FromClient::Message { addr: from, event });
                                    println!("[sv] unreliable");
                                }
                            }
                        }
                    }
                    PacketType::Fragment { session, sequence, packet_id, fragment_idx, total_fragments, data } => {
                        if accept_from_client(
                            &mut server,
                            from,
                            session,
                            sequence,
                            ReliableBody::Fragment {
                                packet_id,
                                fragment_idx,
                                total_fragments,
                                data: data.to_vec(),
                            },
                            &tx,
                        ) {
                            remember_ack(&mut ack_addrs, from);
                        }
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
            std::thread::sleep(Duration::from_millis(2));
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
    session: u64,
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
        client.reliable.receive(sequence, body)
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

    for payload in result.messages {
        let Some(payload) = crate::network::packet::unstamp(generation, &payload) else {
            continue;
        };

        if let Ok(event) = wincode::deserialize::<ClientToServer>(&payload) {
            let _ = tx.send(FromClient::Message { addr: from, event });
            println!("[sv] deserialized and forwarded {}", sequence);
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
        BundlePart::Reliable { sequence, payload } => {
            accept_from_client(
                server,
                from,
                session,
                sequence,
                ReliableBody::Complete(payload),
                tx,
            )
        }
        BundlePart::Fragment { sequence, packet_id, fragment_idx, total_fragments, data } => {
            accept_from_client(
                server,
                from,
                session,
                sequence,
                ReliableBody::Fragment {
                    packet_id,
                    fragment_idx,
                    total_fragments,
                    data,
                },
                tx,
            )
        }
        BundlePart::Unreliable { sequence, payload } => {
            if !server.session_matches(from, session) {
                return false;
            }

            let fresh = {
                let client = server.clients.get_mut(&from).unwrap();
                accept_unreliable(&mut client.unreliable_in, sequence)
            };

            if fresh {
                server.touch_client(from);

                if let Ok(event) = wincode::deserialize::<ClientToServer>(&payload) {
                    let _ = tx.send(FromClient::Message { addr: from, event });
                    println!("[sv] unreliable");
                }
            }

            false
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

    if pending.is_empty() {
        game.send_reliable_to(addr, ServerToClient::WorldSnapshot {
            generation,
            reset: true,
            entities: Vec::new(),
        });

        return;
    }

    let mut batch = Vec::new();
    let mut reset = true;
    for entity in pending {
        batch.push(entity);
        if snapshot_fits(generation, reset, &batch) {
            continue;
        }

        let overflow = batch.pop().unwrap();
        if !batch.is_empty() {
            game.send_reliable_to(addr, ServerToClient::WorldSnapshot {
                generation,
                reset,
                entities: std::mem::take(&mut batch),
            });
            reset = false;
        }

        batch.push(overflow);
        if !snapshot_fits(generation, reset, &batch) {
            println!("[sv] snapshot entity too large");
            batch.clear();
        }
    }

    if !batch.is_empty() {
        game.send_reliable_to(addr, ServerToClient::WorldSnapshot {
            generation,
            reset,
            entities: batch,
        });
    }
}

#[cfg(feature = "server")]
fn emit_tick_state(game: &GameState<FromClient, ServerToClient>) {
    let tick = game.tick_count();
    let mut pending = Vec::new();
    for (handle, entity) in game.entities.iter() {
        let base = entity.base();
        pending.push(NetTransform {
            handle,
            position: base.position,
            angles: base.angles,
            velocity: base.velocity,
        });
    }

    if pending.is_empty() {
        return;
    }

    let mut batch = Vec::new();
    for transform in pending {
        batch.push(transform);
        if tick_fits(tick, &batch) {
            continue;
        }

        let overflow = batch.pop().unwrap();
        if !batch.is_empty() {
            game.send_unreliable(ServerToClient::TickState {
                tick,
                transforms: std::mem::take(&mut batch),
            });
        }

        batch.push(overflow);
        if !tick_fits(tick, &batch) {
            println!("[sv] tick state too large");
            batch.clear();
        }
    }

    if !batch.is_empty() {
        game.send_unreliable(ServerToClient::TickState {
            tick,
            transforms: batch,
        });
    }
}

#[cfg(feature = "server")]
fn snapshot_fits(generation: u32, reset: bool, entities: &[EntitySnapshot]) -> bool {
    let event = ServerToClient::WorldSnapshot {
        generation,
        reset,
        entities: entities.to_vec(),
    };
    let payload = wincode::serialize(&event).unwrap();
    let stamped = stamp(generation, &payload);

    encoded_packet_count(stamped.len()).is_some()
}

#[cfg(feature = "server")]
fn tick_fits(tick: u64, transforms: &[NetTransform]) -> bool {
    let event = ServerToClient::TickState {
        tick,
        transforms: transforms.to_vec(),
    };
    let payload = wincode::serialize(&event).unwrap();

    payload.len() <= unreliable_payload_limit()
}