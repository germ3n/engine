use crate::network::crypto::{self, Channel, Role, KEY_LEN};
use crate::network::packet::{
    bundle_part, pack_bundles, split_unreliable, BundlePart, CONNECTION_TIMEOUT, MAX_DATAGRAM,
    STREAM_STATE,
};
use crate::network::reliable::{EnqueueStatus, ReliableChannel, UnreliableAssembly};
use crate::network::{PacketType, UnreliableInbox, OUTBOUND_CAP};
use std::collections::hash_map::{DefaultHasher, RandomState};
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, Hash, Hasher};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CHALLENGE_WINDOW_SECS: u64 = 5;
const MAX_CRYPTO_SESSIONS: usize = 4096;
const CRYPTO_IDLE: Duration = Duration::from_secs(60);
const CRYPTO_REPLACE_IDLE: Duration = Duration::from_secs(3);
const HELLOS_PER_SEC: u32 = 2000;
const RESETS_PER_SEC: u32 = 2000;
const RECV_SPIN: usize = 64;

struct CryptoSession {
    channel: Channel,
    client_public: [u8; KEY_LEN],
    server_public: [u8; KEY_LEN],
    last_valid: Instant,
}

struct Budget {
    window: Instant,
    used: u32,
}

impl Budget {
    fn new() -> Self {
        Self {
            window: Instant::now(),
            used: 0,
        }
    }

    fn take(&mut self, limit: u32) -> bool {
        if self.window.elapsed() >= Duration::from_secs(1) {
            self.window = Instant::now();
            self.used = 0;
        }

        if self.used >= limit {
            return false;
        }

        self.used += 1;
        true
    }
}

fn bind_port(port: u16) -> Result<UdpSocket, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let addr = format!("0.0.0.0:{port}");
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let addr = format!("[::]:{port}");

    let socket = UdpSocket::bind(&addr).map_err(|err| format!("bind {addr}: {err}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|err| format!("bind {addr}: {err}"))?;

    Ok(socket)
}

pub struct ConnectedClient {
    pub reliable: ReliableChannel,
    pub state: ReliableChannel,
    pub outbound: VecDeque<Vec<u8>>,
    pub state_outbound: VecDeque<Vec<u8>>,
    pub last_seen: Instant,
    pub session: u64,
    pub generation: u32,
    pub unreliable_out: u32,
    pub unreliable_in: UnreliableInbox,
    pub unreliable_assembly: UnreliableAssembly,
}

pub enum ReliableSendError {
    Missing,
    Full,
    TooLarge,
}

pub struct NetworkServer {
    #[allow(dead_code)]
    pub port: u16,
    pub max_clients: u32,
    pub clients: HashMap<SocketAddr, ConnectedClient>,
    pub socket: UdpSocket,
    recv_buf: Vec<u8>,
    challenge_secret: u64,
    session_counter: u64,
    generations: HashMap<SocketAddr, (u32, Instant)>,
    unreliable_parts: HashMap<SocketAddr, Vec<BundlePart>>,
    crypto: HashMap<SocketAddr, CryptoSession>,
    hello_budget: Budget,
    reset_budget: Budget,
}

impl NetworkServer {
    pub fn new(port: u16, max_clients: u32) -> Result<Self, String> {
        let socket = bind_port(port)?;

        Ok(Self {
            port,
            max_clients,
            clients: HashMap::new(),
            socket,
            recv_buf: Vec::with_capacity(MAX_DATAGRAM + crypto::OVERHEAD),
            challenge_secret: random_secret(),
            session_counter: 0,
            generations: HashMap::new(),
            unreliable_parts: HashMap::new(),
            crypto: HashMap::new(),
            hello_budget: Budget::new(),
            reset_budget: Budget::new(),
        })
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

        self.crypto
            .retain(|_, session| session.last_valid.elapsed() < CRYPTO_IDLE);

        let timeout = CONNECTION_TIMEOUT;
        let idle: Vec<SocketAddr> = self
            .clients
            .iter()
            .filter(|(_, client)| client.last_seen.elapsed() >= timeout)
            .map(|(addr, _)| *addr)
            .collect();

        let mut dropped = Vec::new();
        for addr in idle {
            log::warn!("[sv] timeout {}", addr);
            if self.disconnect_client(addr) {
                dropped.push(addr);
            }
        }

        dropped
    }

    pub fn poll_packet(&mut self) -> Option<(Result<PacketType, ()>, SocketAddr)> {
        if let Some((bytes, addr)) = crate::network::steam::pop_host() {
            return Some((wincode::deserialize(&bytes).map_err(|_| ()), addr));
        }

        let cap = MAX_DATAGRAM + crypto::OVERHEAD;
        if self.recv_buf.len() < cap {
            self.recv_buf.resize(cap, 0);
        }

        for _ in 0..RECV_SPIN {
            let (amt, src) = match self.socket.recv_from(&mut self.recv_buf) {
                Ok(packet) => packet,
                Err(_) => return None,
            };

            let packet = &self.recv_buf[..amt];
            match packet.first().copied() {
                Some(crypto::TAG_HELLO) => {
                    if let Some(key) = crypto::parse_hello(packet) {
                        self.handle_hello(src, key);
                    }
                }
                Some(crypto::TAG_DATA) => match self.crypto.get_mut(&src) {
                    Some(session) => {
                        if let Some(plain) = session.channel.open(packet) {
                            session.last_valid = Instant::now();
                            return Some((wincode::deserialize(&plain).map_err(|_| ()), src));
                        }
                    }
                    None => {
                        if self.reset_budget.take(RESETS_PER_SEC) {
                            let _ = self.socket.send_to(&[crypto::TAG_RESET], src);
                        }
                    }
                },
                _ => {}
            }
        }

        None
    }

    fn handle_hello(&mut self, src: SocketAddr, client_public: [u8; KEY_LEN]) {
        if let Some(session) = self.crypto.get(&src) {
            if session.client_public == client_public {
                let ack = crypto::hello_packet(crypto::TAG_HELLO_ACK, &session.server_public);
                let _ = self.socket.send_to(&ack, src);
                return;
            }

            if session.last_valid.elapsed() < CRYPTO_REPLACE_IDLE {
                return;
            }
        } else if self.crypto.len() >= MAX_CRYPTO_SESSIONS {
            self.crypto
                .retain(|_, session| session.last_valid.elapsed() < CRYPTO_IDLE);
            if self.crypto.len() >= MAX_CRYPTO_SESSIONS {
                return;
            }
        }

        if !self.hello_budget.take(HELLOS_PER_SEC) {
            return;
        }

        let secret = crypto::generate_secret();
        let server_public = crypto::public_key(&secret);
        let Some(channel) = Channel::derive(
            Role::Server,
            &secret,
            &client_public,
            &client_public,
            &server_public,
        ) else {
            return;
        };

        self.crypto.insert(
            src,
            CryptoSession {
                channel,
                client_public,
                server_public,
                last_valid: Instant::now(),
            },
        );
        let ack = crypto::hello_packet(crypto::TAG_HELLO_ACK, &server_public);
        let _ = self.socket.send_to(&ack, src);
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
        let mut state = ReliableChannel::with_stream(STREAM_STATE);
        state.set_session(session);
        self.clients.insert(
            addr,
            ConnectedClient {
                reliable,
                state,
                outbound: VecDeque::new(),
                state_outbound: VecDeque::new(),
                last_seen: Instant::now(),
                session,
                generation,
                unreliable_out: 0,
                unreliable_in: UnreliableInbox::new(),
                unreliable_assembly: UnreliableAssembly::new(),
            },
        );

        Some((session, generation))
    }

    pub fn send_connected(&self, addr: SocketAddr) {
        let Some((session, generation)) = self
            .clients
            .get(&addr)
            .map(|client| (client.session, client.generation))
        else {
            return;
        };

        let bytes = wincode::serialize(&PacketType::Connected {
            session,
            generation,
        })
        .unwrap();
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

    #[allow(dead_code)]
    pub fn remove_client(&mut self, addr: SocketAddr) {
        self.generations.remove(&addr);
        self.unreliable_parts.remove(&addr);
        self.clients.remove(&addr);
    }

    #[allow(dead_code)]
    pub fn send_selective_ack(&self, addr: SocketAddr) {
        let Some(client) = self.clients.get(&addr) else {
            return;
        };

        let ack = client.reliable.selective_ack();
        let state_ack = client.state.selective_ack();
        let bytes = wincode::serialize(&PacketType::Ack {
            session: client.session,
            cumulative: ack.cumulative,
            selective: ack.selective,
            state_cumulative: state_ack.cumulative,
            state_selective: state_ack.selective,
        })
        .unwrap();
        let _ = self.send_to(addr, &bytes);
    }

    pub fn send_to(&self, addr: SocketAddr, message: &[u8]) -> Result<(), String> {
        let Some((addr, message)) = crate::network::sim::enqueue_server(addr, message.to_vec())
        else {
            return Ok(());
        };

        self.send_raw(addr, &message)
    }

    pub fn flush_sim(&self) {
        crate::network::sim::flush_server(|addr, bytes| {
            let _ = self.send_raw(addr, bytes);
        });
    }

    fn send_raw(&self, addr: SocketAddr, message: &[u8]) -> Result<(), String> {
        if crate::network::steam::send_host(addr, message) {
            return Ok(());
        }

        let sealed = self
            .crypto
            .get(&addr)
            .and_then(|session| session.channel.seal(message))
            .ok_or_else(|| format!("no encrypted channel for {addr}"))?;

        self.socket
            .send_to(&sealed, addr)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn send_message(&self, message: &[u8]) -> Result<(), String> {
        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in addrs {
            self.send_to(addr, message)?;
        }

        Ok(())
    }

    pub fn enqueue_reliable(
        &mut self,
        addr: SocketAddr,
        payload: &[u8],
    ) -> Result<(), ReliableSendError> {
        self.enqueue_shared(addr, Arc::new(payload.to_vec()), false)
    }

    pub fn enqueue_state(
        &mut self,
        addr: SocketAddr,
        payload: &[u8],
    ) -> Result<(), ReliableSendError> {
        self.enqueue_shared(addr, Arc::new(payload.to_vec()), true)
    }

    pub fn broadcast_reliable(&mut self, payload: &[u8]) -> Vec<SocketAddr> {
        let shared = Arc::new(payload.to_vec());
        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        let mut stalled = Vec::new();
        let mut reported_large = false;
        for addr in addrs {
            match self.enqueue_shared(addr, Arc::clone(&shared), false) {
                Ok(()) => {}
                Err(ReliableSendError::Full) => {
                    stalled.push(addr);
                }
                Err(ReliableSendError::TooLarge) => {
                    if !reported_large {
                        log::warn!("[sv] reliable payload too large");
                        reported_large = true;
                    }
                }
                Err(ReliableSendError::Missing) => {}
            }
        }

        stalled
    }

    fn enqueue_shared(
        &mut self,
        addr: SocketAddr,
        payload: Arc<Vec<u8>>,
        state: bool,
    ) -> Result<(), ReliableSendError> {
        let generation = match self.clients.get(&addr) {
            Some(client) => client.generation,
            None => {
                return Err(ReliableSendError::Missing);
            }
        };

        if crate::network::packet::encoded_packet_count(payload.len()).is_none() {
            return Err(ReliableSendError::TooLarge);
        }

        let Some(client) = self.clients.get_mut(&addr) else {
            return Err(ReliableSendError::Missing);
        };

        let queued = if state {
            client.state_outbound.len() + client.state.queued_messages()
        } else {
            client.outbound.len() + client.reliable.queued_messages()
        };

        if queued >= OUTBOUND_CAP {
            return Err(ReliableSendError::Full);
        }

        let waiting = if state {
            !client.state_outbound.is_empty()
        } else {
            !client.outbound.is_empty()
        };

        if waiting {
            if state {
                client.state_outbound.push_back(payload.as_ref().clone());
            } else {
                client.outbound.push_back(payload.as_ref().clone());
            }

            return Ok(());
        }

        let status = {
            let channel = if state {
                &mut client.state
            } else {
                &mut client.reliable
            };
            channel.set_generation(generation);
            channel.enqueue_shared(&payload)
        };
        match status {
            EnqueueStatus::Queued => Ok(()),
            EnqueueStatus::Full => {
                if state {
                    client.state_outbound.push_back(payload.as_ref().clone());
                } else {
                    client.outbound.push_back(payload.as_ref().clone());
                }

                Ok(())
            }
            EnqueueStatus::TooLarge => Err(ReliableSendError::TooLarge),
        }
    }

    pub fn send_unreliable_to(&mut self, addr: SocketAddr, payload: Vec<u8>) {
        self.write_unreliable(addr, Arc::new(payload));
    }

    pub fn broadcast_unreliable(&mut self, payload: Vec<u8>) {
        let shared = Arc::new(payload);
        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in addrs {
            self.write_unreliable(addr, Arc::clone(&shared));
        }
    }

    fn write_unreliable(&mut self, addr: SocketAddr, payload: Arc<Vec<u8>>) {
        let sequence = {
            let Some(client) = self.clients.get(&addr) else {
                return;
            };

            client.unreliable_out
        };

        let parts = split_unreliable(sequence, payload);
        if parts.is_empty() {
            return;
        }

        {
            let Some(client) = self.clients.get_mut(&addr) else {
                return;
            };

            client.unreliable_out = client.unreliable_out.wrapping_add(1);
        }

        self.unreliable_parts.entry(addr).or_default().extend(parts);
    }

    pub fn flush_client(&mut self, addr: SocketAddr, force_ack: bool) -> Vec<Vec<u8>> {
        let Some(client) = self.clients.get_mut(&addr) else {
            self.unreliable_parts.remove(&addr);

            return Vec::new();
        };

        let generation = client.generation;
        drain_outbound(&mut client.outbound, &mut client.reliable, generation);
        drain_outbound(&mut client.state_outbound, &mut client.state, generation);

        let mut parts = Vec::new();
        client.reliable.pump(|packet| {
            if let Some(part) = bundle_part(&packet) {
                parts.push(part);
            }
        });
        client.state.pump(|packet| {
            if let Some(part) = bundle_part(&packet) {
                parts.push(part);
            }
        });

        let ack = client.reliable.selective_ack();
        let state_ack = client.state.selective_ack();
        let session = client.session;

        parts.append(&mut self.unreliable_parts.remove(&addr).unwrap_or_default());

        if parts.is_empty() && !force_ack {
            return Vec::new();
        }

        pack_bundles(
            session,
            ack.cumulative,
            ack.selective,
            state_ack.cumulative,
            state_ack.selective,
            true,
            parts,
        )
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

fn drain_outbound(
    outbound: &mut VecDeque<Vec<u8>>,
    channel: &mut ReliableChannel,
    generation: u32,
) {
    channel.set_generation(generation);
    loop {
        let Some(payload) = outbound.front().cloned() else {
            break;
        };

        match channel.enqueue_bytes(payload) {
            EnqueueStatus::Queued => {
                outbound.pop_front();
            }
            EnqueueStatus::Full => {
                break;
            }
            EnqueueStatus::TooLarge => {
                log::warn!("[sv] reliable payload too large");
                outbound.pop_front();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::NetworkClient;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::thread::sleep;

    fn loopback_server() -> (NetworkServer, SocketAddr) {
        let mut server = NetworkServer::new(0, 4).unwrap();
        server.socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        server.socket.set_nonblocking(true).unwrap();
        let addr = server.socket.local_addr().unwrap();
        (server, addr)
    }

    fn pump_server(server: &mut NetworkServer) -> Option<(PacketType, SocketAddr)> {
        for _ in 0..200 {
            if let Some((Ok(packet), from)) = server.poll_packet() {
                return Some((packet, from));
            }
            sleep(Duration::from_millis(5));
        }
        None
    }

    fn pump_client(client: &mut NetworkClient) -> Option<PacketType> {
        for _ in 0..200 {
            if let Some(Ok(packet)) = client.poll_packet() {
                return Some(packet);
            }
            sleep(Duration::from_millis(5));
        }
        None
    }

    #[test]
    fn encrypted_round_trip_and_server_restart() {
        let (mut server, addr) = loopback_server();
        let local = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
        let mut client = NetworkClient::new(local);
        client.connect(addr).unwrap();

        let ping = wincode::serialize(&PacketType::Connect { replace: None }).unwrap();
        let mut got = None;
        for _ in 0..50 {
            let _ = client.send_message(&ping);
            let _ = client.poll_packet();
            if let Some((packet, from)) = (0..10).find_map(|_| {
                let r = server.poll_packet();
                sleep(Duration::from_millis(5));
                r.and_then(|(p, f)| p.ok().map(|p| (p, f)))
            }) {
                got = Some((packet, from));
                break;
            }
        }
        let (packet, from) = got.expect("handshake and first packet");
        assert!(matches!(packet, PacketType::Connect { replace: None }));

        let reply = wincode::serialize(&PacketType::Challenge {
            token: 7,
            secure: false,
            host_steam_id: 0,
        })
        .unwrap();
        server.send_to(from, &reply).unwrap();
        assert!(matches!(
            pump_client(&mut client),
            Some(PacketType::Challenge { token: 7, .. })
        ));

        // Nothing on the wire is plaintext.
        let sealed = server.crypto.get(&from).unwrap().channel.seal(&reply).unwrap();
        assert!(!sealed.windows(reply.len()).any(|w| w == reply.as_slice()));

        // Server loses its state; the client must notice and re-handshake.
        server.crypto.clear();
        let mut recovered = false;
        for _ in 0..100 {
            let _ = client.send_message(&ping);
            let _ = client.poll_packet();
            if let Some((PacketType::Connect { .. }, _)) = pump_server_once(&mut server) {
                recovered = true;
                break;
            }
        }
        assert!(recovered, "client did not recover after server reset");
    }

    fn pump_server_once(server: &mut NetworkServer) -> Option<(PacketType, SocketAddr)> {
        sleep(Duration::from_millis(20));
        let (p, f) = server.poll_packet()?;
        Some((p.ok()?, f))
    }

    #[test]
    fn garbage_and_unknown_peers_are_ignored() {
        let (mut server, addr) = loopback_server();
        let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
        probe.send_to(&[0u8; 40], addr).unwrap();
        probe.send_to(&[crypto::TAG_DATA; 64], addr).unwrap();
        sleep(Duration::from_millis(20));
        assert!(pump_server(&mut server).is_none());
    }
}
