use crate::network::reliable::{EnqueueStatus, ReliableChannel};
use crate::network::{PacketType, UnreliableInbox};
use std::net::{SocketAddr, UdpSocket};
use std::collections::HashMap;
use std::collections::hash_map::{DefaultHasher, RandomState};
use std::hash::{BuildHasher, Hash, Hasher};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use crate::network::packet::{bundle_part, encoded_packet_count, pack_bundles, stamp, unreliable_payload_limit, BundlePart, CONNECTION_TIMEOUT, MAX_DATAGRAM};

const CHALLENGE_WINDOW_SECS: u64 = 5;

pub struct ConnectedClient {
    pub reliable: ReliableChannel,
    pub last_seen: Instant,
    pub session: u64,
    pub generation: u32,
    pub unreliable_out: u32,
    pub unreliable_in: UnreliableInbox,
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
    generations: HashMap<SocketAddr, (u32, Instant)>,
    unreliable_parts: HashMap<SocketAddr, Vec<BundlePart>>,
}

impl NetworkServer {
    pub fn new(port: u16, max_clients: u32) -> Self {
        //let socket = UdpSocket::bind(format!("0.0.0.0:{}", port)).unwrap();
        let socket = UdpSocket::bind(format!("[::]:{}", port)).unwrap();
        socket.set_nonblocking(true).unwrap();
        Self {
            port,
            max_clients,
            clients: HashMap::new(),
            socket,
            challenge_secret: random_secret(),
            session_counter: 0,
            generations: HashMap::new(),
            unreliable_parts: HashMap::new(),
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
        self.generations.retain(|addr, slot| {
            self.clients.contains_key(addr) || slot.1.elapsed() < CONNECTION_TIMEOUT
        });

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

    pub fn poll_message(&self) -> Option<(Vec<u8>, SocketAddr)> {
        let mut buffer = [0; MAX_DATAGRAM];
        match self.socket.recv_from(&mut buffer) {
            Ok((amt, src)) => Some((buffer[..amt].to_vec(), src)),
            Err(_) => None,
        }
    }

    pub fn add_client(&mut self, addr: SocketAddr) -> Option<(u64, u32)> {
        if let Some(client) = self.clients.get(&addr) {
            return Some((client.session, client.generation));
        }

        if self.clients.len() as u32 >= self.max_clients {
            return None;
        }

        let session = self.issue_session(addr);
        let generation = self.bump_generation(addr);
        let mut reliable = ReliableChannel::new();
        reliable.set_session(session);
        self.clients.insert(addr, ConnectedClient {
            reliable,
            last_seen: Instant::now(),
            session,
            generation,
            unreliable_out: 0,
            unreliable_in: UnreliableInbox::new(),
        });

        Some((session, generation))
    }

    pub fn send_connected(&self, addr: SocketAddr) {
        let Some((session, generation)) = self.clients.get(&addr).map(|client| (client.session, client.generation)) else {
            return;
        };

        let bytes = wincode::serialize(&PacketType::Connected { session, generation }).unwrap();
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

        self.touch_generation(addr);
        let bytes = wincode::serialize(&PacketType::Disconnect { session }).unwrap();
        let _ = self.send_to(addr, &bytes);
        self.unreliable_parts.remove(&addr);
        self.clients.remove(&addr);

        true
    }

    pub fn retire_client(&mut self, addr: SocketAddr, session: u64) -> bool {
        if !self.session_matches(addr, session) {
            return false;
        }

        self.touch_generation(addr);
        self.unreliable_parts.remove(&addr);
        self.clients.remove(&addr);

        true
    }

    pub fn remove_client(&mut self, addr: SocketAddr) {
        self.generations.remove(&addr);
        self.unreliable_parts.remove(&addr);
        self.clients.remove(&addr);
    }

    pub fn send_selective_ack(&self, addr: SocketAddr) {
        let Some(client) = self.clients.get(&addr) else {
            return;
        };

        let ack = client.reliable.selective_ack();
        let bytes = wincode::serialize(&PacketType::Ack {
            session: client.session,
            cumulative: ack.cumulative,
            selective: ack.selective,
        }).unwrap();
        let _ = self.send_to(addr, &bytes);
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
        let generation = match self.clients.get(&addr) {
            Some(client) => client.generation,
            None => {
                return Err(ReliableSendError::Missing);
            }
        };

        let stamped = stamp(generation, payload);
        let Some(client) = self.clients.get_mut(&addr) else {
            return Err(ReliableSendError::Missing);
        };

        match client.reliable.enqueue(&stamped) {
            EnqueueStatus::Queued => Ok(()),
            EnqueueStatus::Full => Err(ReliableSendError::Full),
            EnqueueStatus::TooLarge => Err(ReliableSendError::TooLarge),
        }
    }

    pub fn broadcast_reliable(&mut self, payload: &[u8]) -> Vec<SocketAddr> {
        let stamped = stamp(1, payload);
        if encoded_packet_count(stamped.len()).is_none() {
            println!("[sv] reliable payload too large");

            return Vec::new();
        }

        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        let mut stalled = Vec::new();
        for addr in addrs {
            if let Err(ReliableSendError::Full) = self.enqueue_reliable(addr, payload) {
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

        self.write_unreliable(addr, payload);
    }

    pub fn broadcast_unreliable(&mut self, payload: Vec<u8>) {
        if payload.len() > unreliable_payload_limit() {
            println!("[sv] unreliable payload too large");

            return;
        }

        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in addrs {
            self.write_unreliable(addr, payload.clone());
        }
    }

    fn write_unreliable(&mut self, addr: SocketAddr, payload: Vec<u8>) {
        if payload.len() > unreliable_payload_limit() {
            println!("[sv] unreliable payload too large");

            return;
        }

        let sequence = {
            let Some(client) = self.clients.get_mut(&addr) else {
                return;
            };

            let sequence = client.unreliable_out;
            client.unreliable_out = client.unreliable_out.wrapping_add(1);

            sequence
        };

        self.unreliable_parts.entry(addr).or_default().push(BundlePart::Unreliable {
            sequence,
            payload,
        });
    }

    pub fn flush_client(&mut self, addr: SocketAddr, force_ack: bool) -> Vec<Vec<u8>> {
        let mut parts = self.unreliable_parts.remove(&addr).unwrap_or_default();
        let reliable = {
            let Some(client) = self.clients.get_mut(&addr) else {
                return Vec::new();
            };

            let mut packets = Vec::new();
            client.reliable.pump(|packet| {
                packets.push(packet);
            });

            packets
        };

        let mut reliable_parts = Vec::new();
        for packet in &reliable {
            if let Some(part) = bundle_part(packet) {
                reliable_parts.push(part);
            }
        }

        reliable_parts.append(&mut parts);
        let parts = reliable_parts;
        if parts.is_empty() && !force_ack {
            return Vec::new();
        }

        let Some(client) = self.clients.get(&addr) else {
            return Vec::new();
        };

        let ack = client.reliable.selective_ack();
        let session = client.session;

        pack_bundles(session, ack.cumulative, ack.selective, true, parts)
    }

    fn bump_generation(&mut self, addr: SocketAddr) -> u32 {
        let slot = self.generations.entry(addr).or_insert((0, Instant::now()));
        slot.0 = slot.0.wrapping_add(1);
        if slot.0 == 0 {
            slot.0 = 1;
        }

        slot.1 = Instant::now();

        slot.0
    }

    fn touch_generation(&mut self, addr: SocketAddr) {
        if let Some(slot) = self.generations.get_mut(&addr) {
            slot.1 = Instant::now();
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
