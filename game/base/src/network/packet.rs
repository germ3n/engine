use std::sync::Arc;
use wincode::{SchemaWrite, SchemaRead};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const MAX_FRAGMENTS: u16 = 64;
const MAX_CONCURRENT_BUFFERS: usize = 8;
const BUFFER_TIMEOUT: Duration = Duration::from_secs(5);
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(2);
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum PacketType {
    Unreliable(Arc<Vec<u8>>),
    Reliable { sequence: u32, payload: Arc<Vec<u8>> },
    Ack { sequence: u32 },
    Connect,
    Challenge { token: u64 },
    ChallengeResponse { token: u64 },
    Connected { session: u64 },
    KeepAlive { session: u64 },
    Fragment { 
        packet_id: u16, 
        fragment_idx: u16, 
        total_fragments: u16, 
        data: Arc<Vec<u8>> 
    },
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