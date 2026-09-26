use std::sync::{Arc, OnceLock};
use core::mem::MaybeUninit;
use wincode::config::Config;
use wincode::io::{Reader, Writer};
use wincode::{ReadResult, SchemaRead, SchemaWrite, WriteResult};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct SharedBytes {
    buf: Arc<Vec<u8>>,
    start: u32,
    end: u32,
}

impl SharedBytes {
    pub fn full(buf: Arc<Vec<u8>>) -> Self {
        let end = buf.len() as u32;

        Self { buf, start: 0, end }
    }

    pub fn range(buf: Arc<Vec<u8>>, start: usize, end: usize) -> Self {
        Self { buf, start: start as u32, end: end as u32 }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf[self.start as usize..self.end as usize]
    }
}

unsafe impl<C: Config> SchemaWrite<C> for SharedBytes {
    type Src = SharedBytes;

    fn size_of(src: &Self::Src) -> WriteResult<usize> {
        <[u8] as SchemaWrite<C>>::size_of(src.as_slice())
    }

    fn write(writer: impl Writer, src: &Self::Src) -> WriteResult<()> {
        <[u8] as SchemaWrite<C>>::write(writer, src.as_slice())
    }
}

unsafe impl<'de, C: Config> SchemaRead<'de, C> for SharedBytes {
    type Dst = SharedBytes;

    fn read(reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        let bytes = <Vec<u8> as SchemaRead<C>>::get(reader)?;
        let end = bytes.len() as u32;
        dst.write(SharedBytes { buf: Arc::new(bytes), start: 0, end });

        Ok(())
    }
}

pub const MAX_FRAGMENTS: u16 = 64;
pub const MAX_DATAGRAM: usize = 1200;
pub const STREAM_EVENT: u8 = 0;
pub const STREAM_STATE: u8 = 1;
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(2);
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum BundlePart {
    Reliable { stream: u8, sequence: u32, generation: u32, payload: SharedBytes },
    Fragment {
        stream: u8,
        sequence: u32,
        generation: u32,
        packet_id: u16,
        fragment_idx: u16,
        total_fragments: u16,
        data: SharedBytes,
    },
    Unreliable { sequence: u32, payload: SharedBytes },
    UnreliableFragment {
        sequence: u32,
        fragment_idx: u16,
        total_fragments: u16,
        data: SharedBytes,
    },
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum PacketType {
    Unreliable { session: u64, sequence: u32, payload: SharedBytes },
    Reliable { session: u64, stream: u8, sequence: u32, generation: u32, payload: SharedBytes },
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
        generation: u32,
        packet_id: u16,
        fragment_idx: u16,
        total_fragments: u16,
        data: SharedBytes,
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
            generation: 0,
            payload: SharedBytes::full(Arc::new(vec![0u8; len])),
        }))
    })
}

pub fn fragment_payload_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        payload_limit(|len| bundled_one(BundlePart::Fragment {
            stream: STREAM_EVENT,
            sequence: 0,
            generation: 0,
            packet_id: 0,
            fragment_idx: 0,
            total_fragments: 1,
            data: SharedBytes::full(Arc::new(vec![0u8; len])),
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
            payload: SharedBytes::full(Arc::new(vec![0u8; len])),
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
            data: SharedBytes::full(Arc::new(vec![0u8; len])),
        }))
    })
}

pub fn unreliable_message_limit() -> usize {
    unreliable_fragment_limit().saturating_mul(MAX_FRAGMENTS as usize)
}

pub fn split_unreliable(sequence: u32, payload: Arc<Vec<u8>>) -> Vec<BundlePart> {
    if payload.len() <= unreliable_payload_limit() {
        return vec![BundlePart::Unreliable { sequence, payload: SharedBytes::full(payload) }];
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
            data: SharedBytes::range(Arc::clone(&payload), offset, end),
        });
        offset = end;
        fragment_idx += 1;
    }

    parts
}

pub fn bundle_part(packet: &PacketType) -> Option<BundlePart> {
    match packet {
        PacketType::Reliable { stream, sequence, generation, payload, .. } => Some(BundlePart::Reliable {
            stream: *stream,
            sequence: *sequence,
            generation: *generation,
            payload: payload.clone(),
        }),
        PacketType::Fragment { stream, sequence, generation, packet_id, fragment_idx, total_fragments, data, .. } => Some(BundlePart::Fragment {
            stream: *stream,
            sequence: *sequence,
            generation: *generation,
            packet_id: *packet_id,
            fragment_idx: *fragment_idx,
            total_fragments: *total_fragments,
            data: data.clone(),
        }),
        PacketType::Unreliable { sequence, payload, .. } => Some(BundlePart::Unreliable {
            sequence: *sequence,
            payload: payload.clone(),
        }),
        _ => None,
    }
}

pub fn owned_payload(payload: SharedBytes) -> Vec<u8> {
    let start = payload.start as usize;
    let end = payload.end as usize;
    if start == 0 && end == payload.buf.len() {
        match Arc::try_unwrap(payload.buf) {
            Ok(bytes) => bytes,
            Err(buf) => buf.as_ref().clone(),
        }
    } else {
        payload.buf[start..end].to_vec()
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
            generation: 0,
            payload: SharedBytes::full(Arc::new(vec![idx as u8; 8])),
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
            BundlePart::Reliable { stream: STREAM_EVENT, sequence: 0, generation: 0, payload: SharedBytes::full(Arc::new(vec![1u8; limit])) },
            BundlePart::Reliable { stream: STREAM_EVENT, sequence: 1, generation: 0, payload: SharedBytes::full(Arc::new(vec![2u8; limit])) },
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
            generation: 0,
            payload: SharedBytes::full(Arc::new(vec![9u8; limit])),
        }]);
        assert_eq!(fitted.len(), 1);
        assert!(fitted[0].len() <= MAX_DATAGRAM);

        let over = pack_bundles(1, 1, 0, 0, 0, true, vec![BundlePart::Reliable {
            stream: STREAM_EVENT,
            sequence: 1,
            generation: 0,
            payload: SharedBytes::full(Arc::new(vec![9u8; limit + 1])),
        }]);
        assert!(over.is_empty());
    }
}