use std::sync::Arc;
use wincode::{SchemaWrite, SchemaRead};
use std::collections::HashMap;

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum PacketType {
    Unreliable(Arc<Vec<u8>>),
    Reliable { sequence: u32, payload: Arc<Vec<u8>> },
    Ack { sequence: u32 },
    Connect,
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

    pub fn insert(&mut self, packet_id: u16, fragment_idx: u16, total_fragments: u16, data: Vec<u8>) -> Option<Vec<u8>> {
        let entry = self.incoming.entry(packet_id).or_insert_with(|| ReassemblyBuffer {
            total_fragments,
            received_count: 0,
            chunks: vec![None; total_fragments as usize],
        });

        let idx_usize = fragment_idx as usize;
        if idx_usize < entry.chunks.len() && entry.chunks[idx_usize].is_none() {
            entry.chunks[idx_usize] = Some(data);
            entry.received_count += 1;
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