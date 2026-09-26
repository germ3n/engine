use crate::network::reliable::{EnqueueStatus, ReliableChannel};
use crate::network::PacketType;
use std::net::{SocketAddr, UdpSocket};
use std::collections::HashMap;
use std::collections::hash_map::{DefaultHasher, RandomState};
use std::hash::{BuildHasher, Hash, Hasher};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use crate::network::packet::{CONNECTION_TIMEOUT, MAX_DATAGRAM, encoded_packet_count, unreliable_payload_limit};
use std::sync::Arc;

const CHALLENGE_WINDOW_SECS: u64 = 5;

pub struct ConnectedClient {
    pub reliable: ReliableChannel,
    pub last_seen: Instant,
    pub session: u64,
    pub unreliable_out: u32,
    pub unreliable_in: Option<u32>,
}

pub enum ReliableSendError {
    Missing,
    Full,
    TooLarge,
}

pub struct NetworkServer {
    pub port: u16,
    pub max_clients: u32,
    pub clients: HashMap<SocketAddr, ConnectedClient>,
    pub socket: UdpSocket,
    challenge_secret: u64,
    session_counter: u64,
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
            challenge_secret: random_secret(),
            session_counter: 0,
        }
    }

    pub fn is_connected(&self, addr: SocketAddr) -> bool {
        self.clients.contains_key(&addr)
    }

    pub fn challenge_for(&self, addr: SocketAddr) -> u64 {
        self.compute_challenge(addr, challenge_bucket())
    }

    pub fn verify_challenge(&self, addr: SocketAddr, token: u64) -> bool {
        let bucket = challenge_bucket();
        token == self.compute_challenge(addr, bucket)
            || token == self.compute_challenge(addr, bucket.wrapping_sub(1))
    }

    fn compute_challenge(&self, addr: SocketAddr, bucket: u64) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.challenge_secret.hash(&mut hasher);
        addr.hash(&mut hasher);
        bucket.hash(&mut hasher);
        hasher.finish()
    }

    pub fn touch_client(&mut self, addr: SocketAddr) {
        if let Some(client) = self.clients.get_mut(&addr) {
            client.last_seen = Instant::now();
        }
    }

    pub fn drop_idle_clients(&mut self) -> Vec<SocketAddr> {
        let timeout = CONNECTION_TIMEOUT;
        let idle: Vec<SocketAddr> = self.clients.iter()
            .filter(|(_, client)| client.last_seen.elapsed() >= timeout)
            .map(|(addr, _)| *addr)
            .collect();

        let mut dropped = Vec::new();
        for addr in idle {
            println!("[sv] timeout {}", addr);
            if self.disconnect_client(addr) {
                dropped.push(addr);
            }
        }

        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in addrs {
            if let Some(client) = self.clients.get_mut(&addr) {
                client.reliable.evict_stale_fragments();
            }
        }

        dropped
    }

    pub fn receive_message(&self) -> Result<(Vec<u8>, SocketAddr), String> {
        let mut buffer = [0; MAX_DATAGRAM];
        let (amt, src) = self.socket.recv_from(&mut buffer).map_err(|e| e.to_string())?;
        Ok((buffer[..amt].to_vec(), src))
    }

    pub fn add_client(&mut self, addr: SocketAddr) -> Option<u64> {
        if let Some(client) = self.clients.get(&addr) {
            return Some(client.session);
        }

        if self.clients.len() as u32 >= self.max_clients {
            return None;
        }

        let session = self.issue_session(addr);
        let mut reliable = ReliableChannel::new();
        reliable.set_session(session);
        self.clients.insert(addr, ConnectedClient {
            reliable,
            last_seen: Instant::now(),
            session,
            unreliable_out: 0,
            unreliable_in: None,
        });

        Some(session)
    }

    pub fn send_connected(&self, addr: SocketAddr) {
        let Some(session) = self.clients.get(&addr).map(|client| client.session) else {
            return;
        };

        let bytes = wincode::serialize(&PacketType::Connected { session }).unwrap();
        let _ = self.send_to(addr, &bytes);
    }

    pub fn touch_if_session(&mut self, addr: SocketAddr, session: u64) -> bool {
        let Some(client) = self.clients.get_mut(&addr) else {
            return false;
        };

        if client.session != session {
            return false;
        }

        client.last_seen = Instant::now();

        true
    }

    fn issue_session(&mut self, addr: SocketAddr) -> u64 {
        self.session_counter = self.session_counter.wrapping_add(1);

        let mut hasher = DefaultHasher::new();
        self.challenge_secret.hash(&mut hasher);
        addr.hash(&mut hasher);
        self.session_counter.hash(&mut hasher);
        hasher.finish()
    }

    pub fn session_matches(&self, addr: SocketAddr, session: u64) -> bool {
        match self.clients.get(&addr) {
            Some(client) => client.session == session,
            None => false,
        }
    }

    pub fn disconnect_client(&mut self, addr: SocketAddr) -> bool {
        let Some(session) = self.clients.get(&addr).map(|client| client.session) else {
            return false;
        };

        let bytes = wincode::serialize(&PacketType::Disconnect { session }).unwrap();
        let _ = self.send_to(addr, &bytes);
        self.clients.remove(&addr);

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

    pub fn enqueue_reliable(&mut self, addr: SocketAddr, payload: &[u8]) -> Result<(), ReliableSendError> {
        let Some(client) = self.clients.get_mut(&addr) else {
            return Err(ReliableSendError::Missing);
        };

        match client.reliable.enqueue(payload) {
            EnqueueStatus::Queued => Ok(()),
            EnqueueStatus::Full => Err(ReliableSendError::Full),
            EnqueueStatus::TooLarge => Err(ReliableSendError::TooLarge),
        }
    }

    pub fn broadcast_reliable(&mut self, payload: Vec<u8>) -> Vec<SocketAddr> {
        if encoded_packet_count(payload.len()).is_none() {
            println!("[sv] reliable payload too large");

            return Vec::new();
        }

        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        let mut stalled = Vec::new();
        for addr in addrs {
            if let Err(ReliableSendError::Full) = self.enqueue_reliable(addr, &payload) {
                stalled.push(addr);
            }
        }

        for addr in stalled.iter().copied() {
            println!("[sv] reliable window full {}", addr);
            self.disconnect_client(addr);
        }

        stalled
    }

    pub fn send_unreliable_to(&mut self, addr: SocketAddr, payload: Vec<u8>) {
        if payload.len() > unreliable_payload_limit() {
            println!("[sv] unreliable payload too large");

            return;
        }

        self.write_unreliable(addr, Arc::new(payload));
    }

    pub fn broadcast_unreliable(&mut self, payload: Vec<u8>) {
        if payload.len() > unreliable_payload_limit() {
            println!("[sv] unreliable payload too large");

            return;
        }

        let payload = Arc::new(payload);
        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in addrs {
            self.write_unreliable(addr, Arc::clone(&payload));
        }
    }

    fn write_unreliable(&mut self, addr: SocketAddr, payload: Arc<Vec<u8>>) {
        let Some(client) = self.clients.get_mut(&addr) else {
            return;
        };

        let sequence = client.unreliable_out;
        client.unreliable_out = client.unreliable_out.wrapping_add(1);
        let session = client.session;
        let packet = PacketType::Unreliable { session, sequence, payload };
        let bytes = wincode::serialize(&packet).unwrap();
        let _ = self.send_to(addr, &bytes);
    }

    pub fn pump_reliable(&mut self) {
        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in addrs {
            let packets: Vec<Vec<u8>> = {
                let client = self.clients.get_mut(&addr).unwrap();
                let mut packets = Vec::new();
                client.reliable.pump(|packet_bytes| {
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

fn random_secret() -> u64 {
    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    hasher.finish()
}

fn challenge_bucket() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / CHALLENGE_WINDOW_SECS
}
