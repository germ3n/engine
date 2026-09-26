use std::net::UdpSocket;
use std::net::SocketAddr;
use crate::network::packet::MAX_DATAGRAM;

pub struct NetworkClient {
    peer: SocketAddr,
    socket: UdpSocket,
}

impl NetworkClient {
    pub fn new(addr: SocketAddr) -> Self {
        let socket = UdpSocket::bind(addr).unwrap();
        socket.set_nonblocking(true).unwrap();
        Self { peer: addr, socket }
    }

    pub fn from_socket(socket: UdpSocket) -> Self {
        socket.set_nonblocking(true).unwrap();
        Self {
            peer: socket.local_addr().unwrap(),
            socket: socket,
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

    pub fn poll_message(&self) -> Option<(Vec<u8>, SocketAddr)> {
        let mut buffer = [0; MAX_DATAGRAM];
        match self.socket.recv_from(&mut buffer) {
            Ok((amt, src)) => Some((buffer[..amt].to_vec(), src)),
            Err(_) => None,
        }
    }
}
