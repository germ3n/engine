use blake2::{Blake2b512, Digest};
use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::cell::Cell;
use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};

pub const TAG_HELLO: u8 = 0xE1;
pub const TAG_HELLO_ACK: u8 = 0xE2;
pub const TAG_DATA: u8 = 0xE3;
pub const TAG_RESET: u8 = 0xE4;

pub const KEY_LEN: usize = 32;
pub const HELLO_LEN: usize = 1 + KEY_LEN;
pub const DATA_HEADER: usize = 1 + 8;
pub const MAC_LEN: usize = 16;
pub const OVERHEAD: usize = DATA_HEADER + MAC_LEN;

const KDF_LABEL: &[u8] = b"engine-net-v1";
const WINDOW: u64 = 128;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Client,
    Server,
}

pub fn generate_secret() -> [u8; KEY_LEN] {
    let mut secret = [0u8; KEY_LEN];
    getrandom::fill(&mut secret).expect("system randomness unavailable");
    secret
}

pub fn public_key(secret: &[u8; KEY_LEN]) -> [u8; KEY_LEN] {
    x25519(*secret, X25519_BASEPOINT_BYTES)
}

pub fn hello_packet(tag: u8, public: &[u8; KEY_LEN]) -> [u8; HELLO_LEN] {
    let mut out = [0u8; HELLO_LEN];
    out[0] = tag;
    out[1..].copy_from_slice(public);
    out
}

pub fn parse_hello(bytes: &[u8]) -> Option<[u8; KEY_LEN]> {
    if bytes.len() != HELLO_LEN {
        return None;
    }

    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes[1..]);
    Some(key)
}

struct ReplayWindow {
    top: Option<u64>,
    bits: u128,
}

impl ReplayWindow {
    fn new() -> Self {
        Self { top: None, bits: 0 }
    }

    fn fresh(&self, counter: u64) -> bool {
        let Some(top) = self.top else {
            return true;
        };

        if counter > top {
            return true;
        }

        let age = top - counter;
        age < WINDOW && self.bits & (1u128 << age) == 0
    }

    fn mark(&mut self, counter: u64) {
        let Some(top) = self.top else {
            self.top = Some(counter);
            self.bits = 1;
            return;
        };

        if counter > top {
            let shift = counter - top;
            self.bits = if shift >= WINDOW {
                0
            } else {
                self.bits << shift
            };
            self.bits |= 1;
            self.top = Some(counter);
        } else {
            self.bits |= 1u128 << (top - counter);
        }
    }
}

pub struct Channel {
    send: ChaCha20Poly1305,
    recv: ChaCha20Poly1305,
    send_counter: Cell<u64>,
    window: ReplayWindow,
}

fn nonce(counter: u64) -> Nonce {
    let mut bytes = [0u8; 12];
    bytes[4..].copy_from_slice(&counter.to_le_bytes());
    Nonce::from(bytes)
}

impl Channel {
    pub fn derive(
        role: Role,
        secret: &[u8; KEY_LEN],
        peer_public: &[u8; KEY_LEN],
        client_public: &[u8; KEY_LEN],
        server_public: &[u8; KEY_LEN],
    ) -> Option<Self> {
        let shared = x25519(*secret, *peer_public);
        if shared.iter().all(|byte| *byte == 0) {
            return None;
        }

        let mut hasher = Blake2b512::new();
        hasher.update(KDF_LABEL);
        hasher.update(shared);
        hasher.update(client_public);
        hasher.update(server_public);
        let okm = hasher.finalize();

        let client_to_server = Key::try_from(&okm[..KEY_LEN]).ok()?;
        let server_to_client = Key::try_from(&okm[KEY_LEN..]).ok()?;
        let c2s = ChaCha20Poly1305::new(&client_to_server);
        let s2c = ChaCha20Poly1305::new(&server_to_client);
        let (send, recv) = match role {
            Role::Client => (c2s, s2c),
            Role::Server => (s2c, c2s),
        };

        Some(Self {
            send,
            recv,
            send_counter: Cell::new(0),
            window: ReplayWindow::new(),
        })
    }

    pub fn seal(&self, plain: &[u8]) -> Option<Vec<u8>> {
        let counter = self.send_counter.get();
        if counter == u64::MAX {
            return None;
        }
        self.send_counter.set(counter + 1);

        let mut out = Vec::with_capacity(plain.len() + OVERHEAD);
        out.push(TAG_DATA);
        out.extend_from_slice(&counter.to_le_bytes());
        let mut body = Vec::with_capacity(plain.len() + MAC_LEN);
        body.extend_from_slice(plain);
        self.send
            .encrypt_in_place(&nonce(counter), &[TAG_DATA], &mut body)
            .ok()?;
        out.extend_from_slice(&body);

        Some(out)
    }

    pub fn open(&mut self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() < OVERHEAD || packet[0] != TAG_DATA {
            return None;
        }

        let counter = u64::from_le_bytes(packet[1..DATA_HEADER].try_into().ok()?);
        if !self.window.fresh(counter) {
            return None;
        }

        let mut body = packet[DATA_HEADER..].to_vec();
        self.recv
            .decrypt_in_place(&nonce(counter), &[TAG_DATA], &mut body)
            .ok()?;
        self.window.mark(counter);

        Some(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (Channel, Channel) {
        let cs = generate_secret();
        let ss = generate_secret();
        let cp = public_key(&cs);
        let sp = public_key(&ss);
        let client = Channel::derive(Role::Client, &cs, &sp, &cp, &sp).unwrap();
        let server = Channel::derive(Role::Server, &ss, &cp, &cp, &sp).unwrap();
        (client, server)
    }

    #[test]
    fn round_trip_both_directions() {
        let (mut client, mut server) = pair();
        let wire = client.seal(b"hello server").unwrap();
        assert_eq!(wire.len(), 12 + OVERHEAD);
        assert_eq!(server.open(&wire).unwrap(), b"hello server");
        let wire = server.seal(b"hello client").unwrap();
        assert_eq!(client.open(&wire).unwrap(), b"hello client");
    }

    #[test]
    fn rejects_replay_and_tamper() {
        let (client, mut server) = pair();
        let wire = client.seal(b"once").unwrap();
        assert!(server.open(&wire).is_some());
        assert!(server.open(&wire).is_none());

        let mut bad = client.seal(b"tamper").unwrap();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(server.open(&bad).is_none());
    }

    #[test]
    fn accepts_reordering_within_window() {
        let (client, mut server) = pair();
        let a = client.seal(b"a").unwrap();
        let b = client.seal(b"b").unwrap();
        assert!(server.open(&b).is_some());
        assert!(server.open(&a).is_some());
        assert!(server.open(&a).is_none());
    }

    #[test]
    fn rejects_packets_older_than_window() {
        let (client, mut server) = pair();
        let old = client.seal(b"old").unwrap();
        for _ in 0..WINDOW + 1 {
            let wire = client.seal(b"x").unwrap();
            assert!(server.open(&wire).is_some());
        }
        assert!(server.open(&old).is_none());
    }

    #[test]
    fn directions_do_not_cross() {
        let (mut client, _server) = pair();
        let wire = client.seal(b"x").unwrap();
        assert!(client.open(&wire).is_none());
    }
}
