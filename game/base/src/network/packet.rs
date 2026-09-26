use std::sync::{Arc, OnceLock};
use wincode::{SchemaWrite, SchemaRead};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const MAX_FRAGMENTS: u16 = 64;
pub const MAX_DATAGRAM: usize = 1200;
const MAX_CONCURRENT_BUFFERS: usize = 8;
const BUFFER_TIMEOUT: Duration = Duration::from_secs(5);
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(2);
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum BundlePart {
    Reliable { sequence: u32, payload: Vec<u8> },
    Fragment {
        sequence: u32,
        packet_id: u16,
        fragment_idx: u16,
        total_fragments: u16,
        data: Vec<u8>,
    },
    Unreliable { sequence: u32, payload: Vec<u8> },
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub struct ReliableFrame {
    pub generation: u32,
    pub payload: Vec<u8>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum PacketType {
    Unreliable { session: u64, sequence: u32, payload: Arc<Vec<u8>> },
    Reliable { session: u64, sequence: u32, payload: Arc<Vec<u8>> },
    Ack { session: u64, cumulative: u32, selective: u32 },
    Connect { replace: Option<u64> },
    Challenge { token: u64 },
    ChallengeResponse { token: u64 },
    Connected { session: u64, generation: u32 },
    KeepAlive { session: u64 },
    Disconnect { session: u64 },
    Fragment { 
        session: u64,
        sequence: u32,
        packet_id: u16, 
        fragment_idx: u16, 
        total_fragments: u16, 
        data: Arc<Vec<u8>> 
    },
    Bundle {
        session: u64,
        ack: bool,
        cumulative: u32,
        selective: u32,
        parts: Vec<BundlePart>,
    },
}

pub fn reliable_payload_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        payload_limit(|len| bundled_one(BundlePart::Reliable {
            sequence: 0,
            payload: vec![0u8; len],
        }))
    })
}

pub fn fragment_payload_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        payload_limit(|len| bundled_one(BundlePart::Fragment {
            sequence: 0,
            packet_id: 0,
            fragment_idx: 0,
            total_fragments: 1,
            data: vec![0u8; len],
        }))
    })
}

pub fn encoded_packet_count(payload_len: usize) -> Option<usize> {
    if payload_len <= reliable_payload_limit() {
        return Some(1);
    }

    let chunk_len = fragment_payload_limit();
    if chunk_len == 0 {
        return None;
    }

    let count = payload_len.div_ceil(chunk_len);
    if count > MAX_FRAGMENTS as usize {
        return None;
    }

    Some(count)
}

pub fn unreliable_payload_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        payload_limit(|len| bundled_one(BundlePart::Unreliable {
            sequence: 0,
            payload: vec![0u8; len],
        }))
    })
}

pub fn stamp(generation: u32, payload: &[u8]) -> Vec<u8> {
    wincode::serialize(&ReliableFrame {
        generation,
        payload: payload.to_vec(),
    }).unwrap()
}

pub fn unstamp(generation: u32, bytes: &[u8]) -> Option<Vec<u8>> {
    let frame = wincode::deserialize::<ReliableFrame>(bytes).ok()?;
    if frame.generation != generation {
        return None;
    }

    Some(frame.payload)
}

pub fn bundle_part(packet: &PacketType) -> Option<BundlePart> {
    match packet {
        PacketType::Reliable { sequence, payload, .. } => Some(BundlePart::Reliable {
            sequence: *sequence,
            payload: payload.to_vec(),
        }),
        PacketType::Fragment { sequence, packet_id, fragment_idx, total_fragments, data, .. } => Some(BundlePart::Fragment {
            sequence: *sequence,
            packet_id: *packet_id,
            fragment_idx: *fragment_idx,
            total_fragments: *total_fragments,
            data: data.to_vec(),
        }),
        PacketType::Unreliable { sequence, payload, .. } => Some(BundlePart::Unreliable {
            sequence: *sequence,
            payload: payload.to_vec(),
        }),
        _ => None,
    }
}

pub fn pack_bundles(session: u64, cumulative: u32, selective: u32, ack: bool, parts: Vec<BundlePart>) -> Vec<Vec<u8>> {
    if parts.is_empty() {
        if !ack {
            return Vec::new();
        }

        return vec![encode_bundle(session, cumulative, selective, true, &[])];
    }

    let mut datagrams = Vec::new();
    let mut start = 0;
    while start < parts.len() {
        let mut count = 1;
        while start + count <= parts.len()
            && encode_bundle(session, cumulative, selective, ack, &parts[start..start + count]).len() <= MAX_DATAGRAM
        {
            count += 1;
        }

        let fitted = count - 1;
        if fitted == 0 {
            println!("[net] bundle part too large");
            start += 1;

            continue;
        }

        datagrams.push(encode_bundle(session, cumulative, selective, ack, &parts[start..start + fitted]));
        start += fitted;
    }

    datagrams
}

fn bundled_one(part: BundlePart) -> PacketType {
    PacketType::Bundle {
        session: 0,
        ack: true,
        cumulative: 0,
        selective: 0,
        parts: vec![part],
    }
}

fn payload_limit(make: impl Fn(usize) -> PacketType) -> usize {
    let mut low = 0;
    let mut high = MAX_DATAGRAM;
    while low < high {
        let mid = low + (high - low + 1) / 2;
        let bytes = wincode::serialize(&make(mid)).unwrap();
        if bytes.len() <= MAX_DATAGRAM {
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    low
}

fn encode_bundle(session: u64, cumulative: u32, selective: u32, ack: bool, parts: &[BundlePart]) -> Vec<u8> {
    let packet = PacketType::Bundle {
        session,
        ack,
        cumulative,
        selective,
        parts: parts.to_vec(),
    };

    wincode::serialize(&packet).unwrap()
}

struct ReassemblyBuffer {
    total_fragments: u16,
    received_count: u16,
    chunks: Vec<Option<Vec<u8>>>,
    last_update: Instant,
}

pub struct FragmentAssembler {
    incoming: HashMap<u16, ReassemblyBuffer>,
    next_packet_id: u16,
}

impl FragmentAssembler {
    pub fn new() -> Self {
        Self {
            incoming: HashMap::new(),
            next_packet_id: 0,
        }
    }

    pub fn next_id(&mut self) -> u16 {
        let id = self.next_packet_id;
        self.next_packet_id = self.next_packet_id.wrapping_add(1);
        id
    }

    pub fn evict_stale(&mut self) {
        self.incoming.retain(|_, buf| buf.last_update.elapsed() < BUFFER_TIMEOUT);
    }

    pub fn insert(&mut self, packet_id: u16, fragment_idx: u16, total_fragments: u16, data: Vec<u8>) -> Option<Vec<u8>> {
        self.evict_stale();

        if total_fragments == 0 || total_fragments > MAX_FRAGMENTS {
            return None;
        }

        if fragment_idx >= total_fragments {
            return None;
        }

        if !self.incoming.contains_key(&packet_id) {
            if self.incoming.len() >= MAX_CONCURRENT_BUFFERS {
                if let Some(oldest_id) = self.incoming.iter()
                    .min_by_key(|(_, buf)| buf.last_update)
                    .map(|(&id, _)| id)
                {
                    self.incoming.remove(&oldest_id);
                }
            }

            self.incoming.insert(packet_id, ReassemblyBuffer {
                total_fragments,
                received_count: 0,
                chunks: vec![None; total_fragments as usize],
                last_update: Instant::now(),
            });
        }

        let entry = self.incoming.get_mut(&packet_id)?;

        if entry.total_fragments != total_fragments {
            return None;
        }

        let idx = fragment_idx as usize;
        if idx < entry.chunks.len() && entry.chunks[idx].is_none() {
            entry.chunks[idx] = Some(data);
            entry.received_count += 1;
            entry.last_update = Instant::now();
        }

        if entry.received_count == entry.total_fragments {
            let mut full_payload = Vec::new();
            let mut completed = self.incoming.remove(&packet_id).unwrap();
            for chunk in completed.chunks.drain(..) {
                full_payload.extend(chunk.unwrap());
            }

            return Some(full_payload);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_joins_small_parts_and_piggybacks_the_ack() {
        let parts: Vec<BundlePart> = (0..4).map(|idx| BundlePart::Reliable {
            sequence: idx,
            payload: vec![idx as u8; 8],
        }).collect();
        let datagrams = pack_bundles(7, 3, 1, true, parts);
        assert_eq!(datagrams.len(), 1);
        assert!(datagrams[0].len() <= MAX_DATAGRAM);

        let packet = wincode::deserialize::<PacketType>(&datagrams[0]).unwrap();
        match packet {
            PacketType::Bundle { session, ack, cumulative, selective, parts } => {
                assert_eq!(session, 7);
                assert!(ack);
                assert_eq!(cumulative, 3);
                assert_eq!(selective, 1);
                assert_eq!(parts.len(), 4);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn pack_splits_full_reliable_parts() {
        let limit = reliable_payload_limit();
        let parts = vec![
            BundlePart::Reliable { sequence: 0, payload: vec![1u8; limit] },
            BundlePart::Reliable { sequence: 1, payload: vec![2u8; limit] },
        ];
        let datagrams = pack_bundles(1, 0, 0, true, parts);
        assert_eq!(datagrams.len(), 2);
        assert!(datagrams.iter().all(|bytes| bytes.len() <= MAX_DATAGRAM));
    }

    #[test]
    fn pack_ack_only_when_there_are_no_parts() {
        let datagrams = pack_bundles(1, 4, 0, true, Vec::new());
        assert_eq!(datagrams.len(), 1);
        let packet = wincode::deserialize::<PacketType>(&datagrams[0]).unwrap();
        let PacketType::Bundle { ack, parts, cumulative, .. } = packet else {
            panic!("expected bundle");
        };
        assert!(ack);
        assert!(parts.is_empty());
        assert_eq!(cumulative, 4);
    }

    #[test]
    fn reliable_limit_is_one_bundle() {
        let limit = reliable_payload_limit();
        let fitted = pack_bundles(1, 1, 0, true, vec![BundlePart::Reliable {
            sequence: 1,
            payload: vec![9u8; limit],
        }]);
        assert_eq!(fitted.len(), 1);
        assert!(fitted[0].len() <= MAX_DATAGRAM);

        let over = pack_bundles(1, 1, 0, true, vec![BundlePart::Reliable {
            sequence: 1,
            payload: vec![9u8; limit + 1],
        }]);
        assert!(over.is_empty());
    }

    #[test]
    fn unstamp_rejects_a_stale_generation() {
        let bytes = stamp(2, b"spawn");
        assert_eq!(unstamp(2, &bytes).unwrap(), b"spawn");
        assert!(unstamp(1, &bytes).is_none());
    }
}