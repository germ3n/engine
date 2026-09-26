use std::sync::{Arc, OnceLock};
use wincode::{SchemaWrite, SchemaRead};
use std::time::Duration;

pub const MAX_FRAGMENTS: u16 = 64;
pub const MAX_DATAGRAM: usize = 1200;
pub const STREAM_EVENT: u8 = 0;
pub const STREAM_STATE: u8 = 1;
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(2);
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum BundlePart {
    Reliable { stream: u8, sequence: u32, payload: Arc<Vec<u8>> },
    Fragment {
        stream: u8,
        sequence: u32,
        packet_id: u16,
        fragment_idx: u16,
        total_fragments: u16,
        data: Arc<Vec<u8>>,
    },
    Unreliable { sequence: u32, payload: Arc<Vec<u8>> },
    UnreliableFragment {
        sequence: u32,
        fragment_idx: u16,
        total_fragments: u16,
        data: Arc<Vec<u8>>,
    },
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub struct ReliableFrame {
    pub generation: u32,
    pub payload: Vec<u8>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum PacketType {
    Unreliable { session: u64, sequence: u32, payload: Arc<Vec<u8>> },
    Reliable { session: u64, stream: u8, sequence: u32, payload: Arc<Vec<u8>> },
    Ack {
        session: u64,
        cumulative: u32,
        selective: u32,
        state_cumulative: u32,
        state_selective: u32,
    },
    Connect { replace: Option<u64> },
    Challenge { token: u64 },
    ChallengeResponse { token: u64 },
    Connected { session: u64, generation: u32 },
    KeepAlive { session: u64 },
    Disconnect { session: u64 },
    Fragment {
        session: u64,
        stream: u8,
        sequence: u32,
        packet_id: u16,
        fragment_idx: u16,
        total_fragments: u16,
        data: Arc<Vec<u8>>,
    },
    Bundle {
        session: u64,
        ack: bool,
        cumulative: u32,
        selective: u32,
        state_cumulative: u32,
        state_selective: u32,
        parts: Vec<BundlePart>,
    },
}

pub fn reliable_payload_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        payload_limit(|len| bundled_one(BundlePart::Reliable {
            stream: STREAM_EVENT,
            sequence: 0,
            payload: Arc::new(vec![0u8; len]),
        }))
    })
}

pub fn fragment_payload_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        payload_limit(|len| bundled_one(BundlePart::Fragment {
            stream: STREAM_EVENT,
            sequence: 0,
            packet_id: 0,
            fragment_idx: 0,
            total_fragments: 1,
            data: Arc::new(vec![0u8; len]),
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
            payload: Arc::new(vec![0u8; len]),
        }))
    })
}

pub fn unreliable_fragment_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        payload_limit(|len| bundled_one(BundlePart::UnreliableFragment {
            sequence: 0,
            fragment_idx: 0,
            total_fragments: 1,
            data: Arc::new(vec![0u8; len]),
        }))
    })
}

pub fn unreliable_message_limit() -> usize {
    unreliable_fragment_limit().saturating_mul(MAX_FRAGMENTS as usize)
}

pub fn split_unreliable(sequence: u32, payload: Vec<u8>) -> Vec<BundlePart> {
    if payload.len() <= unreliable_payload_limit() {
        return vec![BundlePart::Unreliable { sequence, payload: Arc::new(payload) }];
    }

    let chunk_len = unreliable_fragment_limit();
    if chunk_len == 0 {
        println!("[net] unreliable payload too large");

        return Vec::new();
    }

    let count = payload.len().div_ceil(chunk_len);
    if count > MAX_FRAGMENTS as usize {
        println!("[net] unreliable payload too large");

        return Vec::new();
    }

    let mut parts = Vec::with_capacity(count);
    let mut offset = 0;
    let mut fragment_idx = 0u16;
    while offset < payload.len() {
        let end = (offset + chunk_len).min(payload.len());
        parts.push(BundlePart::UnreliableFragment {
            sequence,
            fragment_idx,
            total_fragments: count as u16,
            data: Arc::new(payload[offset..end].to_vec()),
        });
        offset = end;
        fragment_idx += 1;
    }

    parts
}

pub fn stamp(generation: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = wincode::serialize(&generation).unwrap();
    wincode::serialize_into(&mut bytes, payload).unwrap();

    bytes
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
        PacketType::Reliable { stream, sequence, payload, .. } => Some(BundlePart::Reliable {
            stream: *stream,
            sequence: *sequence,
            payload: Arc::clone(payload),
        }),
        PacketType::Fragment { stream, sequence, packet_id, fragment_idx, total_fragments, data, .. } => Some(BundlePart::Fragment {
            stream: *stream,
            sequence: *sequence,
            packet_id: *packet_id,
            fragment_idx: *fragment_idx,
            total_fragments: *total_fragments,
            data: Arc::clone(data),
        }),
        PacketType::Unreliable { sequence, payload, .. } => Some(BundlePart::Unreliable {
            sequence: *sequence,
            payload: Arc::clone(payload),
        }),
        _ => None,
    }
}

pub fn owned_payload(payload: Arc<Vec<u8>>) -> Vec<u8> {
    match Arc::try_unwrap(payload) {
        Ok(bytes) => bytes,
        Err(payload) => payload.as_ref().clone(),
    }
}

pub fn pack_bundles(
    session: u64,
    cumulative: u32,
    selective: u32,
    state_cumulative: u32,
    state_selective: u32,
    ack: bool,
    parts: Vec<BundlePart>,
) -> Vec<Vec<u8>> {
    if parts.is_empty() {
        if !ack {
            return Vec::new();
        }

        return vec![encode_bundle(session, cumulative, selective, state_cumulative, state_selective, true, &[])];
    }

    let mut datagrams = Vec::new();
    let mut start = 0;
    while start < parts.len() {
        let fitted = fit_count(session, cumulative, selective, state_cumulative, state_selective, ack, &parts[start..]);
        if fitted == 0 {
            println!("[net] bundle part too large");
            start += 1;

            continue;
        }

        datagrams.push(encode_bundle(session, cumulative, selective, state_cumulative, state_selective, ack, &parts[start..start + fitted]));
        start += fitted;
    }

    datagrams
}

fn fit_count(
    session: u64,
    cumulative: u32,
    selective: u32,
    state_cumulative: u32,
    state_selective: u32,
    ack: bool,
    parts: &[BundlePart],
) -> usize {
    let mut low = 1;
    let mut high = parts.len();
    let mut fitted = 0;

    while low <= high {
        let mid = low + (high - low) / 2;
        let len = bundle_len(session, cumulative, selective, state_cumulative, state_selective, ack, &parts[..mid]);
        if len <= MAX_DATAGRAM {
            fitted = mid;
            low = mid + 1;
        } else if mid == 1 {
            break;
        } else {
            high = mid - 1;
        }
    }

    fitted
}

fn bundled_one(part: BundlePart) -> PacketType {
    bundle_packet(0, 0, 0, 0, 0, true, &[part])
}

fn bundle_packet(
    session: u64,
    cumulative: u32,
    selective: u32,
    state_cumulative: u32,
    state_selective: u32,
    ack: bool,
    parts: &[BundlePart],
) -> PacketType {
    PacketType::Bundle {
        session,
        ack,
        cumulative,
        selective,
        state_cumulative,
        state_selective,
        parts: parts.to_vec(),
    }
}

fn bundle_len(
    session: u64,
    cumulative: u32,
    selective: u32,
    state_cumulative: u32,
    state_selective: u32,
    ack: bool,
    parts: &[BundlePart],
) -> usize {
    wincode::serialized_size(&bundle_packet(session, cumulative, selective, state_cumulative, state_selective, ack, parts)).unwrap() as usize
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

fn encode_bundle(
    session: u64,
    cumulative: u32,
    selective: u32,
    state_cumulative: u32,
    state_selective: u32,
    ack: bool,
    parts: &[BundlePart],
) -> Vec<u8> {
    wincode::serialize(&bundle_packet(session, cumulative, selective, state_cumulative, state_selective, ack, parts)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_joins_small_parts_and_piggybacks_the_ack() {
        let parts: Vec<BundlePart> = (0..4).map(|idx| BundlePart::Reliable {
            stream: STREAM_EVENT,
            sequence: idx,
            payload: Arc::new(vec![idx as u8; 8]),
        }).collect();
        let datagrams = pack_bundles(7, 3, 1, 4, 2, true, parts);
        assert_eq!(datagrams.len(), 1);
        assert!(datagrams[0].len() <= MAX_DATAGRAM);

        let packet = wincode::deserialize::<PacketType>(&datagrams[0]).unwrap();
        match packet {
            PacketType::Bundle { session, ack, cumulative, selective, state_cumulative, state_selective, parts } => {
                assert_eq!(session, 7);
                assert!(ack);
                assert_eq!(cumulative, 3);
                assert_eq!(selective, 1);
                assert_eq!(state_cumulative, 4);
                assert_eq!(state_selective, 2);
                assert_eq!(parts.len(), 4);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn pack_splits_full_reliable_parts() {
        let limit = reliable_payload_limit();
        let parts = vec![
            BundlePart::Reliable { stream: STREAM_EVENT, sequence: 0, payload: Arc::new(vec![1u8; limit]) },
            BundlePart::Reliable { stream: STREAM_EVENT, sequence: 1, payload: Arc::new(vec![2u8; limit]) },
        ];
        let datagrams = pack_bundles(1, 0, 0, 0, 0, true, parts);
        assert_eq!(datagrams.len(), 2);
        assert!(datagrams.iter().all(|bytes| bytes.len() <= MAX_DATAGRAM));
    }

    #[test]
    fn pack_ack_only_when_there_are_no_parts() {
        let datagrams = pack_bundles(1, 4, 0, 0, 0, true, Vec::new());
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
        let fitted = pack_bundles(1, 1, 0, 0, 0, true, vec![BundlePart::Reliable {
            stream: STREAM_EVENT,
            sequence: 1,
            payload: Arc::new(vec![9u8; limit]),
        }]);
        assert_eq!(fitted.len(), 1);
        assert!(fitted[0].len() <= MAX_DATAGRAM);

        let over = pack_bundles(1, 1, 0, 0, 0, true, vec![BundlePart::Reliable {
            stream: STREAM_EVENT,
            sequence: 1,
            payload: Arc::new(vec![9u8; limit + 1]),
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