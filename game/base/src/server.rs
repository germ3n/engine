use std::sync::mpsc::{Sender, Receiver};
use crate::network::NetworkEvent;
use crate::GameState;
use std::time::{Instant, Duration};
use std::sync::atomic::Ordering;
use crate::network::server::NetworkServer;
use crate::network::{PacketType, NetSend};
use crate::network::usermessage::hash_usermessage_name;

#[cfg(feature = "server")]
pub fn server_loop(game: GameState) {
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

            //game.entities.tick_all();

            let tc = game.tick_count.load(Ordering::Relaxed);
            game.tick_count.store(tc + 1, Ordering::Relaxed);
            
            if tick_idx % 100 == 0 {
                let hash = hash_usermessage_name("Test");
                let _ = game.network_sender.send(NetSend::Reliable(NetworkEvent::UserMessage {
                    hash,
                    data: [128; 256].to_vec(),
                }));
            }
            tick_idx += 1;

            ticked = true;
        }

        while let Ok(net_event) = game.network_receiver.try_recv() {
            match net_event {
                NetworkEvent::UserMessage { hash, data } => {
                    game.script_engine.run_usermessage(hash, data);
                }
                _ => {}
            }
        }

        if !ticked {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[cfg(feature = "server")]
pub fn server_network_loop(tx: Sender<NetworkEvent>, rx: Receiver<NetSend>) {
    let mut server = NetworkServer::new(25400, 128);

    loop {
        while let Ok(outgoing) = rx.try_recv() {
            match outgoing {
                NetSend::Reliable(event) => {
                    let payload = wincode::serialize(&event).unwrap();
                    server.broadcast_reliable(payload);
                }
                NetSend::Unreliable(event) => {
                    let payload = wincode::serialize(&event).unwrap();
                    server.broadcast_unreliable(payload);
                }
            }
        }

        // Use match instead of unwrap to handle the timeout gracefully
        match server.receive_message() {
            Ok((data, from)) => {
                if !server.add_client(from) {
                    continue;
                }

                if let Ok(packet) = wincode::deserialize::<PacketType>(&data) {
                    match packet {
                        PacketType::Connect => {
                            let connect_bytes = wincode::serialize(&PacketType::Connect).unwrap();
                            let _ = server.send_to(from, &connect_bytes);
                            println!("[sv] connect {}", from);
                        }
                        PacketType::Reliable { sequence, payload } => {
                            let ack_packet = PacketType::Ack { sequence };
                            let ack_bytes = wincode::serialize(&ack_packet).unwrap();
                            let _ = server.send_to(from, &ack_bytes);

                            let duplicate = {
                                let client = server.clients.get_mut(&from).unwrap();
                                client.reliable.is_duplicate_and_track(sequence)
                            };

                            if !duplicate {
                                if let Ok(event) = wincode::deserialize::<NetworkEvent>(&payload) {
                                    let _ = tx.send(event);
                                    println!("[sv] deserialized and forwarded {}", sequence);
                                }
                            }
                        }
                        PacketType::Ack { sequence } => {
                            if let Some(client) = server.clients.get_mut(&from) {
                                client.reliable.handle_ack(sequence);
                            }
                            println!("[sv] ack {}", sequence);
                        }
                        PacketType::Unreliable(payload) => {
                            if let Ok(event) = wincode::deserialize::<NetworkEvent>(&payload) {
                                let _ = tx.send(event);
                                println!("[sv] unreliable");
                            }
                        }
                        PacketType::Fragment { packet_id, fragment_idx, total_fragments, data } => {
                            let full_payload_opt = {
                                let client = server.clients.get_mut(&from).unwrap();
                                client.assembler.insert(packet_id, fragment_idx, total_fragments, data.to_vec())
                            };

                            if let Some(full_payload) = full_payload_opt {
                                if let Ok(event) = wincode::deserialize::<NetworkEvent>(&full_payload) {
                                    let _ = tx.send(event);
                                    println!("[sv] reassembled and forwarded fragment packet {}", packet_id);
                                }
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // This catches the WouldBlock timeout. 
                // Leaving this empty allows the loop to continue to check_resends().
            }
        }

        // Periodically check and resend unacknowledged reliable packets
        server.check_resends();
    }
}