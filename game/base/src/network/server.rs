use crate::network::reliable::ReliableChannel;
use crate::network::PacketType;
use std::net::{SocketAddr, UdpSocket};
use std::collections::HashMap;
use std::time::Duration;
use crate::network::packet::FragmentAssembler;
use std::sync::Arc;

pub struct ConnectedClient {
    pub reliable: ReliableChannel,
    pub assembler: FragmentAssembler,
}

pub struct NetworkServer {
    pub port: u16,
    pub max_clients: u32,
    pub clients: HashMap<SocketAddr, ConnectedClient>,
    pub socket: UdpSocket,
}

impl NetworkServer {
    pub fn new(port: u16, max_clients: u32) -> Self {
        //let socket = UdpSocket::bind(format!("0.0.0.0:{}", port)).unwrap();
        let socket = UdpSocket::bind(format!("[::]:{}", port)).unwrap();
        socket.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        Self {
            port,
            max_clients,
            clients: HashMap::new(),
            socket,
        }
    }

    pub fn receive_message(&self) -> Result<(Vec<u8>, SocketAddr), String> {
        let mut buffer = [0; 65535];
        let (amt, src) = self.socket.recv_from(&mut buffer).map_err(|e| e.to_string())?;
        Ok((buffer[..amt].to_vec(), src))
    }

    pub fn add_client(&mut self, addr: SocketAddr) -> bool {
        if self.clients.contains_key(&addr) {
            return true;
        }

        if self.clients.len() as u32 >= self.max_clients {
            return false;
        }

        self.clients.insert(addr, ConnectedClient {
            reliable: ReliableChannel::new(),
            assembler: FragmentAssembler::new()
        });

        true
    }

    pub fn remove_client(&mut self, addr: SocketAddr) {
        self.clients.remove(&addr);
    }

    pub fn send_to(&self, addr: SocketAddr, message: &[u8]) -> Result<(), String> {
        self.socket.send_to(message, addr).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn send_message(&self, message: &[u8]) -> Result<(), String> {
        for addr in self.clients.keys() {
            self.socket.send_to(message, addr).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub fn broadcast_reliable(&mut self, payload: Vec<u8>) {
        let shared_payload = Arc::new(payload);
        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in addrs {
            let bytes = {
                let client = self.clients.get_mut(&addr).unwrap();
                client.reliable.create_reliable_packet((*shared_payload).clone()).1
            };
            let _ = self.socket.send_to(&bytes, addr);
        }
    }

    pub fn broadcast_unreliable(&self, payload: Vec<u8>) {
        let packet = PacketType::Unreliable(payload.into());
        let bytes = wincode::serialize(&packet).unwrap();
        let _ = self.send_message(&bytes);
    }

    pub fn check_resends(&mut self) {
        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in addrs {
            let packets: Vec<Vec<u8>> = {
                let client = self.clients.get_mut(&addr).unwrap();
                let mut packets = Vec::new();
                client.reliable.check_resends(|packet_bytes| {
                    packets.push(packet_bytes.to_vec());
                });
                packets
            };
            for packet in packets {
                let _ = self.socket.send_to(&packet, addr);
            }
        }
    }
}
