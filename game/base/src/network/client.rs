use std::net::UdpSocket;
use std::net::SocketAddr;
use crate::network::packet::MAX_DATAGRAM;
use crate::network::PacketType;

pub struct NetworkClient {
    peer: SocketAddr,
    socket: UdpSocket,
    recv_buf: Vec<u8>,
}

impl NetworkClient {
    pub fn new(addr: SocketAddr) -> Self {
        let socket = UdpSocket::bind(addr).unwrap();
        socket.set_nonblocking(true).unwrap();
        Self { peer: addr, socket, recv_buf: Vec::with_capacity(MAX_DATAGRAM) }
    }

    pub fn from_socket(socket: UdpSocket) -> Self {
        socket.set_nonblocking(true).unwrap();
        Self {
            peer: socket.local_addr().unwrap(),
            socket: socket,
            recv_buf: Vec::with_capacity(MAX_DATAGRAM),
        }
    }

    pub fn connect(&mut self, addr: SocketAddr) -> Result<(), String> {
        self.socket.connect(addr).map_err(|e| e.to_string())?;
        self.peer = addr;
        Ok(())
    }

    pub fn send_message(&self, message: &[u8]) -> Result<(), String> {
        self.socket.send(message).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_address(&self) -> SocketAddr {
        self.peer
    }

    pub fn poll_packet(&mut self) -> Option<Result<PacketType, ()>> {
        if self.recv_buf.len() < MAX_DATAGRAM {
            self.recv_buf.resize(MAX_DATAGRAM, 0);
        }

        let (amt, _src) = match self.socket.recv_from(&mut self.recv_buf) {
            Ok(packet) => packet,
            Err(_) => {
                return None;
            }
        };

        Some(wincode::deserialize(&self.recv_buf[..amt]).map_err(|_| ()))
    }
}
