use std::sync::mpsc::{Receiver, Sender};
use crate::{GameState, client};
use crate::network::{NetworkEvent, NetworkClient};
use crate::ui::{opengl::OpenGLWindow, window::Window};
use winit::event::{WindowEvent, Event};
use glutin::prelude::GlSurface;
use winit::event_loop::ControlFlow;
use glow::HasContext;
use crate::ui::menu::draw_menu;
use crate::script::engine::DrawCommand;
use std::time::Duration;
use std::sync::atomic::Ordering;
use core::net::SocketAddr;
use std::str::FromStr;
use crate::network::{PacketType, ReliableChannel, NetSend};
use crate::network::packet::FragmentAssembler;
use crate::network::usermessage::UserMsgReader;

pub fn client_loop(mut game: GameState) {
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
                    let mut ticked = false;
                    
                    // Use a while loop to catch up if a frame lags
                    while accumulated_time >= game.tick_interval {
                        accumulated_time -= game.tick_interval;

                        game.entities.tick_all();

                        let tc = game.tick_count.load(Ordering::Relaxed);
                        game.tick_count.store(tc + 1, Ordering::Relaxed);
                        ticked = true;
                    }

                    if !ticked {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }

                while let Ok(net_event) = game.network_receiver.try_recv() {
                    match net_event {
                        NetworkEvent::PlayerSpawned { id, position } => {
                            game.script_engine.run_hook("PlayerSpawned", (id, position));
                        }
                        NetworkEvent::PlayerDisconnected { id } => {
                            game.script_engine.run_hook("PlayerDisconnected", id);
                        }
                        NetworkEvent::UserMessage { hash, data } => {
                            game.script_engine.run_usermessage(hash, UserMsgReader::new(data));
                        }
                    }
                }

                client_window.window.request_redraw();
            },
            _ => (),
        }
    }).unwrap();
}

#[cfg(feature = "client")]
pub fn client_network_loop(tx: Sender<NetworkEvent>, rx: Receiver<NetSend>) {
    let mut client = NetworkClient::new(SocketAddr::from_str("0.0.0.0:0").unwrap());
    client.connect(SocketAddr::from_str("127.0.0.1:25400").unwrap()).unwrap();
    let mut reliable_chan = ReliableChannel::new();
    let mut connected = false;
    let mut assembler = FragmentAssembler::new(); 

    let connect_bytes = wincode::serialize(&PacketType::Connect).unwrap();
    let _ = client.send_message(&connect_bytes);

    loop {
        while let Ok(outgoing) = rx.try_recv() {
            match outgoing {
                NetSend::Reliable(event) => {
                    let payload = wincode::serialize(&event).unwrap();
                    let (_, bytes) = reliable_chan.create_reliable_packet(payload);
                    let _ = client.send_message(&bytes);
                }
                NetSend::Unreliable(event) => {
                    let payload = wincode::serialize(&event).unwrap();
                    let packet = PacketType::Unreliable(payload.into());
                    let bytes = wincode::serialize(&packet).unwrap();
                    let _ = client.send_message(&bytes);
                }
            }
        }

        match client.receive_message() {
            Ok((data, _from)) => {
                if let Ok(packet) = wincode::deserialize::<PacketType>(&data) {
                    match packet {
                        PacketType::Connect => {
                            connected = true;
                            println!("[cl] connected");
                        }
                        PacketType::Reliable { sequence, payload } => {
                            connected = true;
                            // Send ACK back to server
                            let ack_packet = PacketType::Ack { sequence };
                            let ack_bytes = wincode::serialize(&ack_packet).unwrap();
                            let _ = client.send_message(&ack_bytes);

                            // Forward event
                            if !reliable_chan.is_duplicate_and_track(sequence) {
                                if let Ok(event) = wincode::deserialize::<NetworkEvent>(&payload) {
                                    let _ = tx.send(event);
                                    println!("[cl] deserialized and forwarded {}", sequence);
                                }
                            }
                        }
                        PacketType::Ack { sequence } => {
                            connected = true;
                            reliable_chan.handle_ack(sequence);
                            println!("[cl] ack {}", sequence);
                        }
                        PacketType::Unreliable(payload) => {
                            connected = true;
                            if let Ok(event) = wincode::deserialize::<NetworkEvent>(&payload) {
                                let _ = tx.send(event);
                                println!("[cl] unreliable");
                            }
                        }
                        PacketType::Fragment { packet_id, fragment_idx, total_fragments, data } => {
                            connected = true;
                            if let Some(full_payload) = assembler.insert(packet_id, fragment_idx, total_fragments, data.to_vec()) {
                                if let Ok(event) = wincode::deserialize::<NetworkEvent>(&full_payload) {
                                    let _ = tx.send(event);
                                    println!("[cl] reassembled and forwarded fragment packet {}", packet_id);
                                }
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // Timeout hit, allows resends to execute cleanly without spamming logs
                if !connected {
                    let _ = client.send_message(&connect_bytes);
                }
            }
        }

        reliable_chan.check_resends(|packet_bytes| {
            let _ = client.send_message(packet_bytes);
        });
    }
}