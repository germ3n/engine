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
                FromClient::Connected { addr } => {
                    println!("[sv] peer {}", addr);
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
                    for addr in server.broadcast_reliable(payload) {
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

        server.pump_reliable();

        // Use match instead of unwrap to handle the timeout gracefully
        match server.receive_message() {
            Ok((data, from)) => {
                if let Ok(packet) = wincode::deserialize::<PacketType>(&data) {
                    match packet {
                        PacketType::Connect => {
                            println!("[sv] connect");
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
                                if server.add_client(from).is_some() {
                                    server.send_connected(from);
                                    println!("[sv] connect {}", from);
                                    let _ = tx.send(FromClient::Connected { addr: from });
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
                        PacketType::Disconnect { .. } => {
                        }
                        PacketType::KeepAlive { session } => {
                            if server.touch_if_session(from, session) {
                                let bytes = wincode::serialize(&PacketType::KeepAlive { session }).unwrap();
                                let _ = server.send_to(from, &bytes);
                            }
                        }
                        PacketType::Reliable { session, sequence, payload } => {
                            accept_from_client(
                                &mut server,
                                from,
                                session,
                                sequence,
                                ReliableBody::Complete(payload.to_vec()),
                                &tx,
                            );
                        }
                        PacketType::Ack { session, sequence } => {
                            if server.session_matches(from, session) {
                                server.touch_client(from);

                                if let Some(client) = server.clients.get_mut(&from) {
                                    client.reliable.handle_ack(sequence);
                                }
                                println!("[sv] ack {}", sequence);
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
                            accept_from_client(
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
                            );
                        }
                    }
                }
            }
            Err(_) => {
                // This catches the WouldBlock timeout. 
                // Leaving this empty allows the loop to continue to pump_reliable().
            }
        }

        for addr in server.drop_idle_clients() {
            let _ = tx.send(FromClient::Disconnected { addr });
        }

        // Periodically check and resend unacknowledged reliable packets
        server.pump_reliable();
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
) {
    if !server.session_matches(from, session) {
        return;
    }

    server.touch_client(from);

    let result = {
        let client = server.clients.get_mut(&from).unwrap();
        client.reliable.receive(sequence, body)
    };

    if !result.ack {
        return;
    }

    let ack_packet = PacketType::Ack { session, sequence };
    let ack_bytes = wincode::serialize(&ack_packet).unwrap();
    let _ = server.send_to(from, &ack_bytes);

    for payload in result.messages {
        if let Ok(event) = wincode::deserialize::<ClientToServer>(&payload) {
            let _ = tx.send(FromClient::Message { addr: from, event });
            println!("[sv] deserialized and forwarded {}", sequence);
        }
    }
}