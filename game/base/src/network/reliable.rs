use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::{HashMap, VecDeque};
use crate::network::PacketType;
use crate::network::packet::{encoded_packet_count, fragment_payload_limit, reliable_payload_limit, MAX_FRAGMENTS, STREAM_EVENT};

const RECV_WINDOW: u32 = 1024;
const UNRELIABLE_JUMP: u32 = 1024;
const SEND_WINDOW: usize = 32;
const SELECTIVE_BITS: u32 = 32;
pub(crate) const MAX_UNSENT: usize = 128;
const MIN_RTO: Duration = Duration::from_millis(20);
const MAX_RTO: Duration = Duration::from_millis(1000);

struct PendingPacket {
    packet: PacketType,
    first_sent: Instant,
    last_sent: Instant,
    retransmitted: bool,
    fast_sent: bool,
}

struct OutPacket {
    seq: u32,
    kind: OutKind,
}

enum OutKind {
    Complete(Arc<Vec<u8>>),
    Fragment {
        packet_id: u16,
        fragment_idx: u16,
        total_fragments: u16,
        data: Arc<Vec<u8>>,
    },
}

struct InflightMessage {
    payload: Arc<Vec<u8>>,
    first_seq: u32,
    count: u16,
    acked: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct SelectiveAck {
    pub cumulative: u32,
    pub selective: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct UnreliableInbox {
    highest: Option<u32>,
    jump: Option<u32>,
}

impl UnreliableInbox {
    pub fn new() -> Self {
        Self {
            highest: None,
            jump: None,
        }
    }
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
    stream: u8,
    session: Option<u64>,
    outgoing_seq: u32,
    next_fragment_id: u16,
    unsent: VecDeque<Vec<u8>>,
    inflight: VecDeque<InflightMessage>,
    encoded: VecDeque<OutPacket>,
    pending_acknowledgements: HashMap<u32, PendingPacket>,
    next_recv: u32,
    recv_buffer: HashMap<u32, ReliableBody>,
    smoothed_rtt: Duration,
    rtt_var: Duration,
    current_rto: Duration,
    fast_seq: Option<u32>,
    retransmit_after: Instant,
}

impl ReliableChannel {
    pub fn new() -> Self {
        Self::with_stream(STREAM_EVENT)
    }

    pub fn with_stream(stream: u8) -> Self {
        Self {
            stream,
            session: None,
            outgoing_seq: 0,
            next_fragment_id: 0,
            unsent: VecDeque::new(),
            inflight: VecDeque::new(),
            encoded: VecDeque::new(),
            pending_acknowledgements: HashMap::new(),
            next_recv: 0,
            recv_buffer: HashMap::new(),
            smoothed_rtt: Duration::from_millis(50),
            rtt_var: Duration::from_millis(25),
            current_rto: Duration::from_millis(150),
            fast_seq: None,
            retransmit_after: Instant::now(),
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

    pub fn take_unacked(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for msg in self.inflight.drain(..) {
            if msg.acked < msg.count {
                out.push(msg.payload.as_ref().clone());
            }
        }

        out.extend(self.unsent.drain(..));
        self.encoded.clear();
        self.pending_acknowledgements.clear();

        out
    }

    pub fn queued_messages(&self) -> usize {
        self.inflight.len() + self.unsent.len()
    }

    pub fn selective_ack(&self) -> SelectiveAck {
        let cumulative = self.next_recv;
        let mut selective = 0u32;

        for bit in 0..SELECTIVE_BITS {
            let seq = cumulative.wrapping_add(1 + bit);
            if self.recv_buffer.contains_key(&seq) {
                selective |= 1u32 << bit;
            }
        }

        SelectiveAck { cumulative, selective }
    }

    pub fn handle_ack(&mut self, cumulative: u32, selective: u32) {
        let seqs: Vec<u32> = self.pending_acknowledgements.keys().copied().collect();

        for seq in seqs {
            if !ack_covers(cumulative, selective, seq) {
                continue;
            }

            let Some(pending) = self.pending_acknowledgements.remove(&seq) else {
                continue;
            };

            self.note_acked(seq);

            if pending.retransmitted {
                continue;
            }

            self.sample_rtt(pending.first_sent.elapsed());
        }

        self.arm_fast_retransmit(cumulative, selective);
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

    pub fn pump<F>(&mut self, mut emit: F)
    where
        F: FnMut(PacketType),
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
            let wire = self.to_packet(&packet);
            emit(wire.clone());
            self.pending_acknowledgements.insert(seq, PendingPacket {
                packet: wire,
                first_sent: now,
                last_sent: now,
                retransmitted: false,
                fast_sent: false,
            });
        }

        self.retransmit_timeout(now, &mut emit);
        self.retransmit_fast(now, &mut emit);
    }

    fn retransmit_timeout<F>(&mut self, now: Instant, emit: &mut F)
    where
        F: FnMut(PacketType),
    {
        if now < self.retransmit_after {
            return;
        }

        let origin = self.outgoing_seq;
        let mut overdue: Vec<u32> = self.pending_acknowledgements.iter().filter_map(|(seq, pending)| {
            if now.duration_since(pending.last_sent) > self.current_rto {
                Some(*seq)
            } else {
                None
            }
        }).collect();

        if overdue.is_empty() {
            return;
        }

        overdue.sort_by_key(|seq| std::cmp::Reverse(origin.wrapping_sub(*seq)));

        for seq in overdue {
            let Some(pending) = self.pending_acknowledgements.get_mut(&seq) else {
                continue;
            };

            emit(pending.packet.clone());
            pending.last_sent = now;
            pending.retransmitted = true;
            pending.fast_sent = true;
        }

        if self.current_rto < MAX_RTO {
            self.current_rto = (self.current_rto * 2).min(MAX_RTO);
        }

        self.retransmit_after = now + self.current_rto;
    }

    fn retransmit_fast<F>(&mut self, now: Instant, emit: &mut F)
    where
        F: FnMut(PacketType),
    {
        let Some(seq) = self.fast_seq.take() else {
            return;
        };

        let Some(pending) = self.pending_acknowledgements.get_mut(&seq) else {
            return;
        };

        if pending.fast_sent {
            return;
        }

        emit(pending.packet.clone());
        pending.last_sent = now;
        pending.retransmitted = true;
        pending.fast_sent = true;
    }

    fn arm_fast_retransmit(&mut self, cumulative: u32, selective: u32) {
        if selective == 0 {
            return;
        }

        let Some(oldest) = self.oldest_pending() else {
            return;
        };

        let later_acked = (1..=SELECTIVE_BITS).any(|ahead| {
            let seq = cumulative.wrapping_add(ahead);
            let newer = seq.wrapping_sub(oldest);
            if newer == 0 || newer >= 0x8000_0000 {
                return false;
            }

            let bit = ahead - 1;

            (selective & (1u32 << bit)) != 0 && !self.pending_acknowledgements.contains_key(&seq)
        });

        if !later_acked {
            return;
        }

        let Some(pending) = self.pending_acknowledgements.get(&oldest) else {
            return;
        };

        if pending.fast_sent {
            return;
        }

        self.fast_seq = Some(oldest);
    }

    fn oldest_pending(&self) -> Option<u32> {
        let origin = self.outgoing_seq;

        self.pending_acknowledgements
            .keys()
            .copied()
            .max_by_key(|seq| origin.wrapping_sub(*seq))
    }

    fn sample_rtt(&mut self, rtt: Duration) {
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

    fn note_acked(&mut self, seq: u32) {
        for msg in &mut self.inflight {
            let offset = seq.wrapping_sub(msg.first_seq);
            if offset < msg.count as u32 {
                msg.acked = msg.acked.saturating_add(1);

                break;
            }
        }

        self.inflight.retain(|msg| msg.acked < msg.count);
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
            let payload = Arc::new(payload.to_vec());
            self.encoded.push_back(OutPacket {
                seq,
                kind: OutKind::Complete(Arc::clone(&payload)),
            });
            self.track_inflight(payload, seq, 1);

            return;
        }

        let shared = Arc::new(payload.to_vec());
        let chunk_len = fragment_payload_limit();
        let total_fragments = shared.len().div_ceil(chunk_len) as u16;
        let packet_id = self.next_fragment_id;
        self.next_fragment_id = self.next_fragment_id.wrapping_add(1);
        let first_seq = self.outgoing_seq;

        let mut offset = 0;
        let mut fragment_idx = 0u16;
        while offset < shared.len() {
            let end = (offset + chunk_len).min(shared.len());
            let seq = self.alloc_seq();
            self.encoded.push_back(OutPacket {
                seq,
                kind: OutKind::Fragment {
                    packet_id,
                    fragment_idx,
                    total_fragments,
                    data: Arc::new(shared[offset..end].to_vec()),
                },
            });
            offset = end;
            fragment_idx += 1;
        }

        self.track_inflight(shared, first_seq, total_fragments);
    }

    fn track_inflight(&mut self, payload: Arc<Vec<u8>>, first_seq: u32, count: u16) {
        self.inflight.push_back(InflightMessage {
            payload,
            first_seq,
            count,
            acked: 0,
        });
    }

    fn to_packet(&self, packet: &OutPacket) -> PacketType {
        let session = self.session.unwrap();
        match &packet.kind {
            OutKind::Complete(payload) => PacketType::Reliable {
                session,
                stream: self.stream,
                sequence: packet.seq,
                payload: Arc::clone(payload),
            },
            OutKind::Fragment { packet_id, fragment_idx, total_fragments, data } => PacketType::Fragment {
                session,
                stream: self.stream,
                sequence: packet.seq,
                packet_id: *packet_id,
                fragment_idx: *fragment_idx,
                total_fragments: *total_fragments,
                data: Arc::clone(data),
            },
        }
    }

    fn drain_ready(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();

        loop {
            let seq = self.next_recv;
            let Some(body) = self.recv_buffer.get(&seq) else {
                break;
            };

            if matches!(body, ReliableBody::Complete(_)) {
                let Some(ReliableBody::Complete(payload)) = self.recv_buffer.remove(&seq) else {
                    break;
                };

                self.next_recv = seq.wrapping_add(1);
                out.push(payload);

                continue;
            }

            match self.fragment_run(seq) {
                FragmentRun::Pending => {
                    break;
                }
                FragmentRun::Invalid => {
                    println!("[net] dropped corrupt fragment");
                    self.recv_buffer.remove(&seq);
                    self.next_recv = seq.wrapping_add(1);
                }
                FragmentRun::Ready(total) => {
                    let mut full = Vec::new();
                    let mut consumed = 0u32;
                    for idx in 0..total {
                        let part_seq = seq.wrapping_add(idx);
                        let Some(ReliableBody::Fragment { data, .. }) = self.recv_buffer.remove(&part_seq) else {
                            break;
                        };

                        full.extend(data);
                        consumed = consumed.wrapping_add(1);
                    }

                    if consumed == 0 {
                        break;
                    }

                    self.next_recv = seq.wrapping_add(consumed);
                    if consumed == total {
                        out.push(full);
                    }
                }
            }
        }

        out
    }

    fn fragment_run(&self, seq: u32) -> FragmentRun {
        let Some(ReliableBody::Fragment { packet_id, fragment_idx, total_fragments, .. }) = self.recv_buffer.get(&seq) else {
            return FragmentRun::Invalid;
        };

        let packet_id = *packet_id;
        let fragment_idx = *fragment_idx;
        let total_fragments = *total_fragments;

        if total_fragments == 0 || total_fragments > MAX_FRAGMENTS || fragment_idx != 0 {
            return FragmentRun::Invalid;
        }

        let total = total_fragments as u32;
        for idx in 0..total {
            let part_seq = seq.wrapping_add(idx);
            match self.recv_buffer.get(&part_seq) {
                None => {
                    return FragmentRun::Pending;
                }
                Some(ReliableBody::Fragment {
                    packet_id: id,
                    fragment_idx: frag,
                    total_fragments: total_frag,
                    ..
                }) if *id == packet_id && *frag == idx as u16 && *total_frag == total_fragments => {}
                Some(_) => {
                    return FragmentRun::Invalid;
                }
            }
        }

        FragmentRun::Ready(total)
    }
}

enum FragmentRun {
    Pending,
    Invalid,
    Ready(u32),
}

fn ack_covers(cumulative: u32, selective: u32, seq: u32) -> bool {
    let before = cumulative.wrapping_sub(seq);
    if before > 0 && before <= RECV_WINDOW {
        return true;
    }

    let ahead = seq.wrapping_sub(cumulative);
    if ahead >= 1 && ahead <= SELECTIVE_BITS {
        let bit = ahead - 1;

        return (selective & (1u32 << bit)) != 0;
    }

    false
}

pub fn accept_unreliable(inbox: &mut UnreliableInbox, sequence: u32) -> bool {
    let Some(prev) = inbox.highest else {
        inbox.highest = Some(sequence);
        inbox.jump = None;

        return true;
    };

    let ahead = sequence.wrapping_sub(prev);
    if ahead == 0 || ahead >= 0x8000_0000 {
        inbox.jump = None;

        return false;
    }

    if ahead > UNRELIABLE_JUMP {
        if let Some(candidate) = inbox.jump {
            let step = sequence.wrapping_sub(candidate);
            if step > 0 && step <= UNRELIABLE_JUMP {
                inbox.highest = Some(sequence);
                inbox.jump = None;

                return true;
            }
        }

        inbox.jump = Some(sequence);

        return false;
    }

    inbox.highest = Some(sequence);
    inbox.jump = None;

    true
}

const MAX_PARTIAL_UNRELIABLE: usize = 8;

struct PartialUnreliable {
    total_fragments: u16,
    received_count: u16,
    chunks: Vec<Option<Vec<u8>>>,
}

pub struct UnreliableAssembly {
    incoming: HashMap<u32, PartialUnreliable>,
}

impl UnreliableAssembly {
    pub fn new() -> Self {
        Self {
            incoming: HashMap::new(),
        }
    }

    pub fn push(
        &mut self,
        inbox: &mut UnreliableInbox,
        sequence: u32,
        fragment_idx: u16,
        total_fragments: u16,
        data: Vec<u8>,
    ) -> Option<Vec<u8>> {
        self.discard_stale(inbox);

        if total_fragments == 0 || total_fragments > MAX_FRAGMENTS || fragment_idx >= total_fragments {
            return None;
        }

        if !unreliable_sequence_open(inbox, sequence) {
            return None;
        }

        if let Some(existing) = self.incoming.get(&sequence) {
            if existing.total_fragments != total_fragments {
                self.incoming.remove(&sequence);

                return None;
            }
        }

        if !self.incoming.contains_key(&sequence) && self.incoming.len() >= MAX_PARTIAL_UNRELIABLE {
            if let Some(drop_seq) = self.incoming.keys().copied().next() {
                self.incoming.remove(&drop_seq);
            }
        }

        let entry = self.incoming.entry(sequence).or_insert_with(|| PartialUnreliable {
            total_fragments,
            received_count: 0,
            chunks: vec![None; total_fragments as usize],
        });

        let idx = fragment_idx as usize;
        if entry.chunks[idx].is_none() {
            entry.chunks[idx] = Some(data);
            entry.received_count = entry.received_count.saturating_add(1);
        }

        if entry.received_count != entry.total_fragments {
            return None;
        }

        let Some(done) = self.incoming.remove(&sequence) else {
            return None;
        };

        let mut full = Vec::new();
        for chunk in done.chunks {
            if let Some(chunk) = chunk {
                full.extend(chunk);
            }
        }

        if !accept_unreliable(inbox, sequence) {
            return None;
        }

        self.discard_stale(inbox);

        Some(full)
    }

    fn discard_stale(&mut self, inbox: &UnreliableInbox) {
        self.incoming.retain(|sequence, _| unreliable_sequence_open(inbox, *sequence));
    }
}

pub fn take_unreliable(
    inbox: &mut UnreliableInbox,
    assembly: &mut UnreliableAssembly,
    sequence: u32,
    payload: Vec<u8>,
) -> Option<Vec<u8>> {
    if !accept_unreliable(inbox, sequence) {
        return None;
    }

    assembly.discard_stale(inbox);

    Some(payload)
}

fn unreliable_sequence_open(inbox: &UnreliableInbox, sequence: u32) -> bool {
    let Some(prev) = inbox.highest else {
        return true;
    };

    let ahead = sequence.wrapping_sub(prev);

    ahead > 0 && ahead <= UNRELIABLE_JUMP
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::packet::{bundle_part, owned_payload, pack_bundles, BundlePart, MAX_DATAGRAM, MAX_FRAGMENTS, fragment_payload_limit};

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
        channel.pump(|packet| sent.push(packet));
        assert_eq!(sent.len(), 1);

        match &sent[0] {
            PacketType::Reliable { session, sequence, payload: body, .. } => {
                assert_eq!(*session, 1);
                assert_eq!(*sequence, 0);
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
        channel.pump(|packet| sent.push(packet));
        assert!(sent.len() > 1);
        let packed = pack_bundles(1, 0, 0, 0, 0, true, sent.iter().filter_map(bundle_part).collect());
        assert!(packed.iter().all(|bytes| bytes.len() <= MAX_DATAGRAM));

        let mut recv = ReliableChannel::new();
        let packets = sent;
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
        channel.pump(|_| sent.push(()));
        assert_eq!(sent.len(), SEND_WINDOW);
        assert_eq!(channel.pending_acknowledgements.len(), SEND_WINDOW);

        let mut resent = 0;
        channel.pump(|_| resent += 1);
        assert_eq!(resent, 0);
        assert_eq!(channel.pending_acknowledgements.len(), SEND_WINDOW);

        channel.handle_ack(1, 0);
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
        channel.handle_ack(1, 0);
        assert_eq!(channel.smoothed_rtt, before);
    }

    #[test]
    fn cumulative_ack_retires_prefix() {
        let mut sender = ReliableChannel::new();
        sender.set_session(1);
        assert_eq!(sender.enqueue(b"a"), EnqueueStatus::Queued);
        assert_eq!(sender.enqueue(b"b"), EnqueueStatus::Queued);
        assert_eq!(sender.enqueue(b"c"), EnqueueStatus::Queued);
        sender.pump(|_| {});

        sender.handle_ack(2, 0);
        assert!(!sender.pending_acknowledgements.contains_key(&0));
        assert!(!sender.pending_acknowledgements.contains_key(&1));
        assert!(sender.pending_acknowledgements.contains_key(&2));
        assert_eq!(sender.take_unacked(), vec![b"c".to_vec()]);
    }

    #[test]
    fn selective_ack_retires_received_holes() {
        let mut sender = ReliableChannel::new();
        sender.set_session(1);
        assert_eq!(sender.enqueue(b"a"), EnqueueStatus::Queued);
        assert_eq!(sender.enqueue(b"b"), EnqueueStatus::Queued);
        assert_eq!(sender.enqueue(b"c"), EnqueueStatus::Queued);

        let mut sent = Vec::new();
        sender.pump(|packet| sent.push(packet));
        assert_eq!(sent.len(), 3);

        let mut recv = ReliableChannel::new();
        let packets = sent;
        let PacketType::Reliable { sequence: seq0, payload: body0, .. } = &packets[0] else {
            panic!("expected reliable");
        };
        let PacketType::Reliable { sequence: seq2, payload: body2, .. } = &packets[2] else {
            panic!("expected reliable");
        };

        let first = recv.receive(*seq0, ReliableBody::Complete(body0.to_vec()));
        assert!(first.ack);
        let third = recv.receive(*seq2, ReliableBody::Complete(body2.to_vec()));
        assert!(third.ack);
        assert!(third.messages.is_empty());

        let ack = recv.selective_ack();
        assert_eq!(ack.cumulative, seq0.wrapping_add(1));
        assert_ne!(ack.selective & 1, 0);

        sender.handle_ack(ack.cumulative, ack.selective);
        assert!(!sender.pending_acknowledgements.contains_key(&seq0));
        assert!(sender.pending_acknowledgements.contains_key(&seq0.wrapping_add(1)));
        assert!(!sender.pending_acknowledgements.contains_key(&seq2));
        assert_eq!(sender.take_unacked(), vec![b"b".to_vec()]);
    }

    #[test]
    fn take_unacked_keeps_inflight_and_unsent_payloads() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
        let mut count = 0u8;
        while channel.enqueue(&[count]) == EnqueueStatus::Queued {
            count += 1;
        }

        let mut sent = 0;
        channel.pump(|_| sent += 1);
        assert_eq!(sent, SEND_WINDOW);

        let recovered = channel.take_unacked();
        assert_eq!(recovered.len(), count as usize);
        for (idx, payload) in recovered.iter().enumerate() {
            assert_eq!(payload.as_slice(), &[idx as u8]);
        }
    }

    #[test]
    fn take_unacked_rebuilds_a_fragmented_payload() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
        let payload = vec![4u8; MAX_DATAGRAM * 2];
        assert_eq!(channel.enqueue(&payload), EnqueueStatus::Queued);
        channel.pump(|_| {});

        assert_eq!(channel.take_unacked(), vec![payload]);
    }

    #[test]
    fn unreliable_sequence_drops_old_packets() {
        let mut inbox = UnreliableInbox::new();
        assert!(accept_unreliable(&mut inbox, 0));
        assert!(accept_unreliable(&mut inbox, 2));
        assert!(!accept_unreliable(&mut inbox, 1));
        assert!(!accept_unreliable(&mut inbox, 2));
        assert!(accept_unreliable(&mut inbox, 3));
    }

    #[test]
    fn unreliable_far_jump_does_not_stick() {
        let mut inbox = UnreliableInbox::new();
        assert!(accept_unreliable(&mut inbox, 0));
        assert!(!accept_unreliable(&mut inbox, 50_000));
        assert!(accept_unreliable(&mut inbox, 1));
        assert!(!accept_unreliable(&mut inbox, 50_000));
        assert!(accept_unreliable(&mut inbox, 50_001));
        assert!(!accept_unreliable(&mut inbox, 50_000));
        assert!(accept_unreliable(&mut inbox, 50_002));
    }

    #[test]
    fn packed_datagram_delivers_in_order() {
        let mut sender = ReliableChannel::new();
        sender.set_session(4);
        assert_eq!(sender.enqueue(b"a"), EnqueueStatus::Queued);
        assert_eq!(sender.enqueue(b"b"), EnqueueStatus::Queued);
        assert_eq!(sender.enqueue(b"c"), EnqueueStatus::Queued);

        let mut packets = Vec::new();
        sender.pump(|packet| packets.push(packet));
        let datagrams = pack_bundles(4, 9, 0, 0, 0, true, packets.iter().filter_map(bundle_part).collect());
        assert_eq!(datagrams.len(), 1);
        assert!(datagrams[0].len() <= MAX_DATAGRAM);

        let packet = wincode::deserialize::<PacketType>(&datagrams[0]).unwrap();
        let PacketType::Bundle { ack, cumulative, parts, .. } = packet else {
            panic!("expected bundle");
        };
        assert!(ack);
        assert_eq!(cumulative, 9);

        let mut recv = ReliableChannel::new();
        let mut messages = Vec::new();
        for part in parts {
            let BundlePart::Reliable { sequence, payload, .. } = part else {
                panic!("expected reliable");
            };
            let result = recv.receive(sequence, ReliableBody::Complete(owned_payload(payload)));
            messages.extend(result.messages);
        }

        assert_eq!(messages, vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn fragment_gap_stays_until_the_run_is_complete() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
        let payload = vec![9u8; MAX_DATAGRAM * 2];
        assert_eq!(channel.enqueue(&payload), EnqueueStatus::Queued);

        let mut sent = Vec::new();
        channel.pump(|packet| sent.push(packet));
        assert!(sent.len() > 1);

        let mut recv = ReliableChannel::new();
        let last = sent.len() - 1;
        for packet in &sent[..last] {
            let result = push_fragment(&mut recv, packet);
            assert!(result.ack);
            assert!(result.messages.is_empty());
        }

        assert_eq!(recv.next_recv, 0);
        assert_eq!(recv.recv_buffer.len(), last);

        let done = push_fragment(&mut recv, &sent[last]);
        assert_eq!(done.messages, vec![payload]);
        assert_eq!(recv.next_recv, sent.len() as u32);
    }

    #[test]
    fn corrupt_fragment_does_not_stall_the_stream() {
        let mut channel = ReliableChannel::new();
        let dropped = channel.receive(0, ReliableBody::Fragment {
            packet_id: 1,
            fragment_idx: 1,
            total_fragments: 2,
            data: vec![1],
        });
        assert!(dropped.ack);
        assert!(dropped.messages.is_empty());

        let ready = channel.receive(1, ReliableBody::Complete(b"a".to_vec()));
        assert_eq!(ready.messages, vec![b"a".to_vec()]);
    }

    #[test]
    fn selective_ack_retransmits_the_hole_once() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
        assert_eq!(channel.enqueue(b"a"), EnqueueStatus::Queued);
        assert_eq!(channel.enqueue(b"b"), EnqueueStatus::Queued);
        assert_eq!(channel.enqueue(b"c"), EnqueueStatus::Queued);
        channel.pump(|_| {});

        channel.handle_ack(1, 1);
        let mut resent = Vec::new();
        channel.pump(|packet| resent.push(packet));
        assert_eq!(resent.len(), 1);
        let PacketType::Reliable { sequence, .. } = &resent[0] else {
            panic!("expected reliable");
        };
        assert_eq!(*sequence, 1);

        channel.handle_ack(1, 1);
        let mut again = 0;
        channel.pump(|_| again += 1);
        assert_eq!(again, 0);
    }

    #[test]
    fn timeout_retransmits_every_overdue_packet() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
        assert_eq!(channel.enqueue(b"a"), EnqueueStatus::Queued);
        assert_eq!(channel.enqueue(b"b"), EnqueueStatus::Queued);
        assert_eq!(channel.enqueue(b"c"), EnqueueStatus::Queued);
        channel.pump(|_| {});

        for pending in channel.pending_acknowledgements.values_mut() {
            pending.last_sent = Instant::now() - Duration::from_secs(5);
        }

        channel.current_rto = Duration::from_millis(1);
        channel.retransmit_after = Instant::now() - Duration::from_secs(1);

        let mut resent = Vec::new();
        channel.pump(|packet| resent.push(packet));
        assert_eq!(resent.len(), 3);
        let mut sequences: Vec<u32> = resent.iter().map(|packet| {
            let PacketType::Reliable { sequence, .. } = packet else {
                panic!("expected reliable");
            };

            *sequence
        }).collect();
        sequences.sort();
        assert_eq!(sequences, vec![0, 1, 2]);
        assert_eq!(channel.current_rto, Duration::from_millis(2));

        let mut again = 0;
        channel.pump(|_| again += 1);
        assert_eq!(again, 0);
    }

    #[test]
    fn timeout_leaves_a_fresh_packet() {
        let mut channel = ReliableChannel::new();
        channel.set_session(1);
        assert_eq!(channel.enqueue(b"a"), EnqueueStatus::Queued);
        assert_eq!(channel.enqueue(b"b"), EnqueueStatus::Queued);
        channel.pump(|_| {});

        channel.pending_acknowledgements.get_mut(&0).unwrap().last_sent = Instant::now() - Duration::from_secs(5);
        channel.current_rto = Duration::from_millis(1);
        channel.retransmit_after = Instant::now() - Duration::from_secs(1);

        let mut resent = Vec::new();
        channel.pump(|packet| resent.push(packet));
        assert_eq!(resent.len(), 1);
        let PacketType::Reliable { sequence, .. } = &resent[0] else {
            panic!("expected reliable");
        };
        assert_eq!(*sequence, 0);
    }

    #[test]
    fn unreliable_fragments_assemble_out_of_order() {
        use crate::network::packet::{split_unreliable, unreliable_payload_limit};

        let mut inbox = UnreliableInbox::new();
        let mut assembly = UnreliableAssembly::new();
        let payload = vec![3u8; unreliable_payload_limit() + 40];
        let mut parts = split_unreliable(0, payload.clone());
        assert!(parts.len() > 1);
        parts.reverse();

        let mut built = None;
        let last = parts.len() - 1;
        for (idx, part) in parts.into_iter().enumerate() {
            let BundlePart::UnreliableFragment { sequence, fragment_idx, total_fragments, data } = part else {
                panic!("expected fragment");
            };

            let result = assembly.push(&mut inbox, sequence, fragment_idx, total_fragments, owned_payload(data));
            if idx == last {
                built = result;
            } else {
                assert!(result.is_none());
            }
        }

        assert_eq!(built.unwrap(), payload);
    }
}
