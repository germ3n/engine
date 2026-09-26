use std::net::UdpSocket;
use std::net::SocketAddr;
use std::time::Duration;
use crate::network::packet::MAX_DATAGRAM;

pub struct NetworkClient {
    peer: SocketAddr,
    socket: UdpSocket,
}

impl NetworkClient {
    pub fn new(addr: SocketAddr) -> Self {
        let socket = UdpSocket::bind(addr).unwrap();
        socket.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        Self { peer: addr, socket }
    }

    pub fn from_socket(socket: UdpSocket) -> Self {
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

    pub fn receive_message(&self) -> Result<(Vec<u8>, SocketAddr), String> {
        let mut buffer = [0; MAX_DATAGRAM];
        let (amt, src) = self.socket.recv_from(&mut buffer).map_err(|e| e.to_string())?;
        Ok((buffer[..amt].to_vec(), src))
    }
}
