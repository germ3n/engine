use std::time::{Duration, Instant};
use std::collections::{HashMap, VecDeque};
use crate::network::PacketType;
use crate::network::packet::{FragmentAssembler, encoded_packet_count, fragment_payload_limit, reliable_payload_limit};

const RECV_WINDOW: u32 = 1024;
const SEND_WINDOW: usize = 32;
pub(crate) const MAX_UNSENT: usize = 128;
const MIN_RTO: Duration = Duration::from_millis(20);
const MAX_RTO: Duration = Duration::from_millis(1000);

struct PendingPacket {
    data: Vec<u8>,
    first_sent: Instant,
    last_sent: Instant,
    retransmitted: bool,
}

struct OutPacket {
    seq: u32,
    kind: OutKind,
}

enum OutKind {
    Complete(Vec<u8>),
    Fragment {
        packet_id: u16,
        fragment_idx: u16,
        total_fragments: u16,
        data: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueStatus {
    Queued,
    Full,
    TooLarge,
}

pub enum ReliableBody {
    Complete(Vec<u8>),
    Fragment {
        packet_id: u16,
        fragment_idx: u16,
        total_fragments: u16,
        data: Vec<u8>,
    },
}

pub struct RecvResult {
    pub ack: bool,
    pub messages: Vec<Vec<u8>>,
}

pub struct ReliableChannel {
    session: Option<u64>,
    outgoing_seq: u32,
    next_fragment_id: u16,
    unsent: VecDeque<Vec<u8>>,
    encoded: VecDeque<OutPacket>,
    pending_acknowledgements: HashMap<u32, PendingPacket>,
    next_recv: u32,
    recv_buffer: HashMap<u32, ReliableBody>,
    assembler: FragmentAssembler,
    smoothed_rtt: Duration,
    rtt_var: Duration,
    current_rto: Duration,
}

impl ReliableChannel {
    pub fn new() -> Self {
        Self {
            session: None,
            outgoing_seq: 0,
            next_fragment_id: 0,
            unsent: VecDeque::new(),
            encoded: VecDeque::new(),
            pending_acknowledgements: HashMap::new(),
            next_recv: 0,
            recv_buffer: HashMap::new(),
            assembler: FragmentAssembler::new(),
            smoothed_rtt: Duration::from_millis(50),
            rtt_var: Duration::from_millis(25),
            current_rto: Duration::from_millis(150),
        }
    }

    pub fn set_session(&mut self, session: u64) {
        self.session = Some(session);
    }

    pub fn enqueue(&mut self, payload: &[u8]) -> EnqueueStatus {
        let Some(count) = encoded_packet_count(payload.len()) else {
            return EnqueueStatus::TooLarge;
        };

        if self.queued_packets() + count > MAX_UNSENT {
            return EnqueueStatus::Full;
        }

        self.unsent.push_back(payload.to_vec());

        EnqueueStatus::Queued
    }

    pub fn take_unsent(&mut self) -> Vec<Vec<u8>> {
        self.encoded.clear();

        self.unsent.drain(..).collect()
    }

    pub fn handle_ack(&mut self, seq: u32) {
        let Some(pending) = self.pending_acknowledgements.remove(&seq) else {
            return;
        };

        if pending.retransmitted {
            return;
        }

        let rtt = pending.first_sent.elapsed();
        let diff = if rtt > self.smoothed_rtt {
            rtt - self.smoothed_rtt
        } else {
            self.smoothed_rtt - rtt
        };

        self.rtt_var = (self.rtt_var * 3 + diff) / 4;
        self.smoothed_rtt = (self.smoothed_rtt * 7 + rtt) / 8;
        self.current_rto = self.smoothed_rtt + self.rtt_var * 4;

        if self.current_rto < MIN_RTO {
            self.current_rto = MIN_RTO;
        }

        if self.current_rto > MAX_RTO {
            self.current_rto = MAX_RTO;
        }
    }

    pub fn receive(&mut self, seq: u32, body: ReliableBody) -> RecvResult {
        let ahead = seq.wrapping_sub(self.next_recv);
        if ahead >= RECV_WINDOW {
            let behind = self.next_recv.wrapping_sub(seq);

            return RecvResult {
                ack: behind < 0x8000_0000,
                messages: Vec::new(),
            };
        }

        if self.recv_buffer.contains_key(&seq) {
            return RecvResult { ack: true, messages: Vec::new() };
        }

        self.recv_buffer.insert(seq, body);

        RecvResult {
            ack: true,
            messages: self.drain_ready(),
        }
    }

    pub fn evict_stale_fragments(&mut self) {
        self.assembler.evict_stale();
    }

    pub fn pump<F>(&mut self, mut send_fn: F)
    where
        F: FnMut(&[u8]),
    {
        if self.session.is_none() {
            return;
        }

        let now = Instant::now();

        while self.pending_acknowledgements.len() < SEND_WINDOW {
            if self.encoded.is_empty() {
                let Some(payload) = self.unsent.pop_front() else {
                    break;
                };

                self.encode(&payload);
            }

            let Some(packet) = self.encoded.pop_front() else {
                break;
            };

            let seq = packet.seq;
            let data = self.serialize(&packet);
            send_fn(&data);
            self.pending_acknowledgements.insert(seq, PendingPacket {
                data,
                first_sent: now,
                last_sent: now,
                retransmitted: false,
            });
        }

        let timeout = self.current_rto;
        let mut retransmitted = false;
        for pending in self.pending_acknowledgements.values_mut() {
            if now.duration_since(pending.last_sent) > timeout {
                send_fn(&pending.data);
                pending.last_sent = now;
                pending.retransmitted = true;
                retransmitted = true;
            }
        }

        if retransmitted && self.current_rto < MAX_RTO {
            self.current_rto = (self.current_rto * 2).min(MAX_RTO);
        }
    }

    fn queued_packets(&self) -> usize {
        let mut total = self.encoded.len();
        for payload in &self.unsent {
            total += encoded_packet_count(payload.len()).unwrap_or(0);
        }

        total
    }

    fn alloc_seq(&mut self) -> u32 {
        let seq = self.outgoing_seq;
        self.outgoing_seq = self.outgoing_seq.wrapping_add(1);

        seq
    }

    fn encode(&mut self, payload: &[u8]) {
        if payload.len() <= reliable_payload_limit() {
            let seq = self.alloc_seq();
            self.encoded.push_back(OutPacket {
                seq,
                kind: OutKind::Complete(payload.to_vec()),
            });

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
            self.encoded.push_back(OutPacket {
                seq,
                kind: OutKind::Fragment {
                    packet_id,
                    fragment_idx,
                    total_fragments,
                    data: payload[offset..end].to_vec(),
                },
            });
            offset = end;
            fragment_idx += 1;
        }
    }

    fn serialize(&self, packet: &OutPacket) -> Vec<u8> {
        let session = self.session.unwrap();
        let wire = match &packet.kind {
            OutKind::Complete(payload) => PacketType::Reliable {
                session,
                sequence: packet.seq,
                payload: payload.clone().into(),
            },
            OutKind::Fragment { packet_id, fragment_idx, total_fragments, data } => PacketType::Fragment {
                session,
                sequence: packet.seq,
                packet_id: *packet_id,
                fragment_idx: *fragment_idx,
                total_fragments: *total_fragments,
                data: data.clone().into(),
            },
        };

        wincode::serialize(&wire).unwrap()
    }

    fn drain_ready(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();

        while let Some(body) = self.recv_buffer.remove(&self.next_recv) {
            self.next_recv = self.next_recv.wrapping_add(1);

            match body {
                ReliableBody::Complete(payload) => {
                    out.push(payload);
                }
                ReliableBody::Fragment { packet_id, fragment_idx, total_fragments, data } => {
                    if let Some(full) = self.assembler.insert(packet_id, fragment_idx, total_fragments, data) {
                        out.push(full);
                    }
                }
            }
        }

        out
    }
}

pub fn accept_unreliable(highest: &mut Option<u32>, sequence: u32) -> bool {
    match *highest {
        Some(prev) => {
            let ahead = sequence.wrapping_sub(prev);
            if ahead == 0 || ahead >= 0x8000_0000 {
                return false;
            }

            *highest = Some(sequence);

            true
        }
        None => {
            *highest = Some(sequence);

            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::packet::{MAX_DATAGRAM, MAX_FRAGMENTS, fragment_payload_limit};

    fn push_fragment(recv: &mut ReliableChannel, packet: &PacketType) -> RecvResult {
        let PacketType::Fragment { sequence, packet_id, fragment_idx, total_fragments, data, .. } = packet else {
            panic!("expected fragment");
        };

        recv.receive(*sequence, ReliableBody::Fragment {
            packet_id: *packet_id,
            fragment_idx: *fragment_idx,
            total_fragments: *total_fragments,
            data: data.to_vec(),
        })
    }

    #[test]
    fn small_payload_is_one_reliable_packet() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
        let payload = vec![7u8; 32];
        assert_eq!(channel.enqueue(&payload), EnqueueStatus::Queued);

        let mut sent = Vec::new();
        channel.pump(|bytes| sent.push(bytes.to_vec()));
        assert_eq!(sent.len(), 1);

        let packet = wincode::deserialize::<PacketType>(&sent[0]).unwrap();
        match packet {
            PacketType::Reliable { session, sequence, payload: body } => {
                assert_eq!(session, 1);
                assert_eq!(sequence, 0);
                assert_eq!(body.as_slice(), payload.as_slice());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn large_payload_roundtrips_through_fragments() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
        let payload = vec![9u8; MAX_DATAGRAM * 2];
        assert_eq!(channel.enqueue(&payload), EnqueueStatus::Queued);

        let mut sent = Vec::new();
        channel.pump(|bytes| sent.push(bytes.to_vec()));
        assert!(sent.len() > 1);
        assert!(sent.iter().all(|bytes| bytes.len() <= MAX_DATAGRAM));

        let mut recv = ReliableChannel::new();
        let packets: Vec<PacketType> = sent.iter().map(|bytes| wincode::deserialize(bytes).unwrap()).collect();
        let last = packets.len() - 1;
        let early = push_fragment(&mut recv, &packets[last]);
        assert!(early.ack);
        assert!(early.messages.is_empty());

        let mut built = None;
        for (idx, packet) in packets.iter().enumerate() {
            if idx == last {
                continue;
            }

            let result = push_fragment(&mut recv, packet);
            assert!(result.ack);
            if let Some(full) = result.messages.into_iter().next() {
                built = Some(full);
            }
        }

        assert_eq!(built.unwrap(), payload);
    }

    #[test]
    fn send_window_bounds_pending() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
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

    #[test]
    fn out_of_order_payloads_deliver_in_sequence() {
        let mut channel = ReliableChannel::new();
        let early = channel.receive(1, ReliableBody::Complete(b"b".to_vec()));
        assert!(early.ack);
        assert!(early.messages.is_empty());

        let ready = channel.receive(0, ReliableBody::Complete(b"a".to_vec()));
        assert_eq!(ready.messages, vec![b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn duplicate_sequence_is_not_delivered_again() {
        let mut channel = ReliableChannel::new();
        let first = channel.receive(0, ReliableBody::Complete(b"a".to_vec()));
        assert_eq!(first.messages, vec![b"a".to_vec()]);

        let second = channel.receive(0, ReliableBody::Complete(b"b".to_vec()));
        assert!(second.ack);
        assert!(second.messages.is_empty());
    }

    #[test]
    fn far_sequence_does_not_move_the_window() {
        let mut channel = ReliableChannel::new();
        let rejected = channel.receive(RECV_WINDOW, ReliableBody::Complete(vec![1]));
        assert!(!rejected.ack);
        assert!(rejected.messages.is_empty());

        let accepted = channel.receive(0, ReliableBody::Complete(b"first".to_vec()));
        assert!(accepted.ack);
        assert_eq!(accepted.messages, vec![b"first".to_vec()]);
    }

    #[test]
    fn sequence_wrap_stays_ordered() {
        let mut channel = ReliableChannel::new();
        channel.next_recv = u32::MAX;

        let first = channel.receive(u32::MAX, ReliableBody::Complete(b"a".to_vec()));
        assert_eq!(first.messages, vec![b"a".to_vec()]);

        let early = channel.receive(1, ReliableBody::Complete(b"c".to_vec()));
        assert!(early.messages.is_empty());

        let ready = channel.receive(0, ReliableBody::Complete(b"b".to_vec()));
        assert_eq!(ready.messages, vec![b"b".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn retransmitted_ack_does_not_sample_rtt() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
        channel.enqueue(&[1, 2, 3, 4]);
        channel.pump(|_| {});

        let pending = channel.pending_acknowledgements.get_mut(&0).unwrap();
        pending.last_sent = Instant::now() - Duration::from_secs(5);
        channel.current_rto = Duration::from_millis(1);
        channel.pump(|_| {});
        assert!(channel.pending_acknowledgements.get(&0).unwrap().retransmitted);

        let before = channel.smoothed_rtt;
        channel.handle_ack(0);
        assert_eq!(channel.smoothed_rtt, before);
    }

    #[test]
    fn unreliable_sequence_drops_old_packets() {
        let mut highest = None;
        assert!(accept_unreliable(&mut highest, 0));
        assert!(accept_unreliable(&mut highest, 2));
        assert!(!accept_unreliable(&mut highest, 1));
        assert!(!accept_unreliable(&mut highest, 2));
        assert!(accept_unreliable(&mut highest, 3));
    }
}
