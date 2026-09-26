use std::time::{Duration, Instant};
use std::collections::{HashMap, HashSet, VecDeque};
use crate::network::PacketType;
use crate::network::packet::{encoded_packet_count, fragment_payload_limit, reliable_payload_limit};

const RECV_WINDOW: u32 = 1024;
const SEND_WINDOW: usize = 32;
const MAX_UNSENT: usize = 128;

struct PendingPacket {
    data: Vec<u8>,
    sent_at: Instant,
    last_sent: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueStatus {
    Queued,
    Full,
    TooLarge,
}

pub struct ReliableChannel {
    outgoing_seq: u32,
    next_fragment_id: u16,
    unsent: VecDeque<(u32, Vec<u8>)>,
    pending_acknowledgements: HashMap<u32, PendingPacket>,
    received_sequences: HashSet<u32>,
    highest_received: u32,
    smoothed_rtt: Duration,
    rtt_var: Duration,
    current_rto: Duration,
}

impl ReliableChannel {
    pub fn new() -> Self {
        Self {
            outgoing_seq: 0,
            next_fragment_id: 0,
            unsent: VecDeque::new(),
            pending_acknowledgements: HashMap::new(),
            received_sequences: HashSet::new(),
            highest_received: 0,
            smoothed_rtt: Duration::from_millis(50),
            rtt_var: Duration::from_millis(25),
            current_rto: Duration::from_millis(150),
        }
    }

    pub fn enqueue(&mut self, payload: &[u8]) -> EnqueueStatus {
        let Some(count) = encoded_packet_count(payload.len()) else {
            return EnqueueStatus::TooLarge;
        };

        if self.unsent.len() + count > MAX_UNSENT {
            return EnqueueStatus::Full;
        }

        self.encode(payload);

        EnqueueStatus::Queued
    }

    pub fn handle_ack(&mut self, seq: u32) {
        if let Some(pending) = self.pending_acknowledgements.remove(&seq) {
            let rtt = pending.sent_at.elapsed();
            
            if self.smoothed_rtt.is_zero() {
                self.smoothed_rtt = rtt;
                self.rtt_var = rtt / 2;
            } else {
                let diff = if rtt > self.smoothed_rtt {
                    rtt - self.smoothed_rtt
                } else {
                    self.smoothed_rtt - rtt
                };
                
                self.rtt_var = (self.rtt_var * 3 + diff) / 4;
                self.smoothed_rtt = (self.smoothed_rtt * 7 + rtt) / 8;
            }

            self.current_rto = self.smoothed_rtt + self.rtt_var * 4;
            if self.current_rto < Duration::from_millis(20) {
                self.current_rto = Duration::from_millis(20);
            }
        }
    }

    pub fn is_duplicate_and_track(&mut self, seq: u32) -> bool {
        if self.received_sequences.contains(&seq) {
            return true;
        }

        self.received_sequences.insert(seq);

        if seq > self.highest_received {
            self.highest_received = seq;
        }

        let cutoff = self.highest_received.saturating_sub(RECV_WINDOW);
        self.received_sequences.retain(|&s| s >= cutoff);

        false
    }

    pub fn pump<F>(&mut self, mut send_fn: F) 
    where F: FnMut(&[u8]) {
        let now = Instant::now();

        while self.pending_acknowledgements.len() < SEND_WINDOW {
            let Some((seq, data)) = self.unsent.pop_front() else {
                break;
            };

            send_fn(&data);
            self.pending_acknowledgements.insert(seq, PendingPacket {
                data,
                sent_at: now,
                last_sent: now,
            });
        }

        let timeout = self.current_rto;
        for pending in self.pending_acknowledgements.values_mut() {
            if now.duration_since(pending.last_sent) > timeout {
                send_fn(&pending.data);
                pending.last_sent = now;
            }
        }
    }

    fn alloc_seq(&mut self) -> u32 {
        let seq = self.outgoing_seq;
        self.outgoing_seq = self.outgoing_seq.wrapping_add(1);

        seq
    }

    fn encode(&mut self, payload: &[u8]) {
        if payload.len() <= reliable_payload_limit() {
            let seq = self.alloc_seq();
            let packet = PacketType::Reliable { sequence: seq, payload: payload.to_vec().into() };
            let bytes = wincode::serialize(&packet).unwrap();
            self.unsent.push_back((seq, bytes));

            return;
        }

        let chunk_len = fragment_payload_limit();
        let total_fragments = payload.len().div_ceil(chunk_len) as u16;
        let packet_id = self.next_fragment_id;
        self.next_fragment_id = self.next_fragment_id.wrapping_add(1);

        let mut offset = 0;
        let mut fragment_idx = 0u16;
        while offset < payload.len() {
            let end = (offset + chunk_len).min(payload.len());
            let seq = self.alloc_seq();
            let packet = PacketType::Fragment {
                sequence: seq,
                packet_id,
                fragment_idx,
                total_fragments,
                data: payload[offset..end].to_vec().into(),
            };
            let bytes = wincode::serialize(&packet).unwrap();
            self.unsent.push_back((seq, bytes));
            offset = end;
            fragment_idx += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::packet::{FragmentAssembler, MAX_DATAGRAM, MAX_FRAGMENTS, fragment_payload_limit};

    #[test]
    fn small_payload_is_one_reliable_packet() {
        let mut channel = ReliableChannel::new();
        let payload = vec![7u8; 32];
        assert_eq!(channel.enqueue(&payload), EnqueueStatus::Queued);

        let mut sent = Vec::new();
        channel.pump(|bytes| sent.push(bytes.to_vec()));
        assert_eq!(sent.len(), 1);

        let packet = wincode::deserialize::<PacketType>(&sent[0]).unwrap();
        match packet {
            PacketType::Reliable { sequence, payload: body } => {
                assert_eq!(sequence, 0);
                assert_eq!(body.as_slice(), payload.as_slice());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn large_payload_roundtrips_through_fragments() {
        let mut channel = ReliableChannel::new();
        let payload = vec![9u8; MAX_DATAGRAM * 2];
        assert_eq!(channel.enqueue(&payload), EnqueueStatus::Queued);

        let mut sent = Vec::new();
        channel.pump(|bytes| sent.push(bytes.to_vec()));
        assert!(sent.len() > 1);
        assert!(sent.iter().all(|bytes| bytes.len() <= MAX_DATAGRAM));

        let mut recv = ReliableChannel::new();
        let mut assembler = FragmentAssembler::new();
        let mut built = None;
        for bytes in sent {
            let packet = wincode::deserialize::<PacketType>(&bytes).unwrap();
            let PacketType::Fragment { sequence, packet_id, fragment_idx, total_fragments, data } = packet else {
                panic!("expected fragment");
            };
            assert!(!recv.is_duplicate_and_track(sequence));
            if let Some(full) = assembler.insert(packet_id, fragment_idx, total_fragments, data.to_vec()) {
                built = Some(full);
            }
        }

        assert_eq!(built.unwrap(), payload);
    }

    #[test]
    fn send_window_bounds_pending() {
        let mut channel = ReliableChannel::new();
        let payload = vec![1u8; 8];
        let mut queued = 0;
        while channel.enqueue(&payload) == EnqueueStatus::Queued {
            queued += 1;
        }

        assert_eq!(channel.enqueue(&payload), EnqueueStatus::Full);
        assert_eq!(queued, MAX_UNSENT);

        let mut sent = Vec::new();
        channel.pump(|bytes| sent.push(bytes.to_vec()));
        assert_eq!(sent.len(), SEND_WINDOW);
        assert_eq!(channel.pending_acknowledgements.len(), SEND_WINDOW);

        let mut resent = 0;
        channel.pump(|_| resent += 1);
        assert_eq!(resent, 0);
        assert_eq!(channel.pending_acknowledgements.len(), SEND_WINDOW);

        channel.handle_ack(0);
        let mut released = 0;
        channel.pump(|_| released += 1);
        assert_eq!(released, 1);
        assert_eq!(channel.pending_acknowledgements.len(), SEND_WINDOW);
    }

    #[test]
    fn oversized_payload_is_rejected() {
        let mut channel = ReliableChannel::new();
        let payload = vec![0u8; fragment_payload_limit() * (MAX_FRAGMENTS as usize + 1)];
        assert_eq!(channel.enqueue(&payload), EnqueueStatus::TooLarge);
        assert!(channel.unsent.is_empty());
    }
}
