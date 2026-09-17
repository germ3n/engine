use std::time::{Duration, Instant};
use std::collections::{HashMap, HashSet};
use crate::network::PacketType;

const RECV_WINDOW: u32 = 1024;

struct PendingPacket {
    data: Vec<u8>,
    sent_at: Instant,
    last_sent: Instant,
}

pub struct ReliableChannel {
    outgoing_seq: u32,
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
            pending_acknowledgements: HashMap::new(),
            received_sequences: HashSet::new(),
            highest_received: 0,
            smoothed_rtt: Duration::from_millis(50),
            rtt_var: Duration::from_millis(25),
            current_rto: Duration::from_millis(150),
        }
    }

    pub fn create_reliable_packet(&mut self, payload: Vec<u8>) -> (u32, Vec<u8>) {
        let seq = self.outgoing_seq;
        self.outgoing_seq += 1;

        let packet = PacketType::Reliable { sequence: seq, payload: payload.into() };
        let bytes = wincode::serialize(&packet).unwrap();
        let now = Instant::now();

        self.pending_acknowledgements.insert(seq, PendingPacket {
            data: bytes.clone(),
            sent_at: now,
            last_sent: now,
        });

        (seq, bytes)
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

    pub fn check_resends<F>(&mut self, mut send_fn: F) 
    where F: FnMut(&[u8]) {
        let now = Instant::now();
        let timeout = self.current_rto;
        
        for (_, pending) in self.pending_acknowledgements.iter_mut() {
            if now.duration_since(pending.last_sent) > timeout {
                send_fn(&pending.data);
                pending.last_sent = now;
            }
        }
    }
}