use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering as AtomicOrdering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static LAG_MS: AtomicU32 = AtomicU32::new(0);
static JITTER_MS: AtomicU32 = AtomicU32::new(0);
static LOSS_PCT: AtomicU32 = AtomicU32::new(0);
static RNG: AtomicU64 = AtomicU64::new(0xC0FFEE);
static QUEUE: Mutex<BinaryHeap<Scheduled>> = Mutex::new(BinaryHeap::new());

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub lag_ms: u32,
    pub jitter_ms: u32,
    pub loss_pct: u32,
}

#[derive(Eq)]
struct Scheduled {
    at: Instant,
    seq: u64,
    packet: Packet,
}

#[derive(Eq, PartialEq)]
enum Packet {
    Client(Vec<u8>),
    Server { addr: SocketAddr, bytes: Vec<u8> },
}

impl PartialEq for Scheduled {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at && self.seq == other.seq
    }
}

impl Ord for Scheduled {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .at
            .cmp(&self.at)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for Scheduled {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn configure(settings: Settings) {
    let loss = settings.loss_pct.min(100);
    LAG_MS.store(settings.lag_ms, AtomicOrdering::Relaxed);
    JITTER_MS.store(settings.jitter_ms, AtomicOrdering::Relaxed);
    LOSS_PCT.store(loss, AtomicOrdering::Relaxed);
    let _ = RNG.compare_exchange(
        0xC0FFEE,
        seed(),
        AtomicOrdering::Relaxed,
        AtomicOrdering::Relaxed,
    );

    if settings.lag_ms > 0 || settings.jitter_ms > 0 || loss > 0 {
        log_settings();
    }
}

pub fn set_lag_ms(lag_ms: u32) {
    let previous = LAG_MS.swap(lag_ms, AtomicOrdering::Relaxed);
    if previous != lag_ms {
        log_settings();
    }
}

pub fn set_jitter_ms(jitter_ms: u32) {
    let previous = JITTER_MS.swap(jitter_ms, AtomicOrdering::Relaxed);
    if previous != jitter_ms {
        log_settings();
    }
}

pub fn set_loss_pct(loss_pct: u32) {
    let loss = loss_pct.min(100);
    let previous = LOSS_PCT.swap(loss, AtomicOrdering::Relaxed);
    if previous != loss {
        log_settings();
    }
}

pub fn settings() -> Settings {
    Settings {
        lag_ms: LAG_MS.load(AtomicOrdering::Relaxed),
        jitter_ms: JITTER_MS.load(AtomicOrdering::Relaxed),
        loss_pct: LOSS_PCT.load(AtomicOrdering::Relaxed),
    }
}

pub fn enabled() -> bool {
    let settings = settings();
    settings.lag_ms > 0 || settings.jitter_ms > 0 || settings.loss_pct > 0
}

fn log_settings() {
    let settings = settings();
    log::info!(
        "[netsim] lag={}ms jitter={}ms loss={}%",
        settings.lag_ms,
        settings.jitter_ms,
        settings.loss_pct
    );
}

pub fn enqueue_client(bytes: Vec<u8>) -> Option<Vec<u8>> {
    match schedule(Packet::Client(bytes)) {
        Some(Packet::Client(bytes)) => Some(bytes),
        _ => None,
    }
}

pub fn enqueue_server(addr: SocketAddr, bytes: Vec<u8>) -> Option<(SocketAddr, Vec<u8>)> {
    match schedule(Packet::Server { addr, bytes }) {
        Some(Packet::Server { addr, bytes }) => Some((addr, bytes)),
        _ => None,
    }
}

pub fn flush_client(mut send: impl FnMut(&[u8])) {
    let ready = take_ready(|packet| matches!(packet, Packet::Client(_)));
    for packet in ready {
        if let Packet::Client(bytes) = packet {
            send(&bytes);
        }
    }
}

pub fn flush_server(mut send: impl FnMut(SocketAddr, &[u8])) {
    let ready = take_ready(|packet| matches!(packet, Packet::Server { .. }));
    for packet in ready {
        if let Packet::Server { addr, bytes } = packet {
            send(addr, &bytes);
        }
    }
}

fn schedule(packet: Packet) -> Option<Packet> {
    if !enabled() {
        return Some(packet);
    }

    if roll_loss() {
        log::debug!("[netsim] dropped {}b", packet_len(&packet));

        return None;
    }

    let delay = delay_ms();
    if delay == 0 {
        return Some(packet);
    }

    let at = Instant::now() + Duration::from_millis(delay as u64);
    let seq = RNG.fetch_add(1, AtomicOrdering::Relaxed);
    let mut queue = QUEUE.lock().unwrap();
    queue.push(Scheduled { at, seq, packet });
    log::debug!("[netsim] delay {delay}ms queue={}", queue.len());

    None
}

fn take_ready(pred: impl Fn(&Packet) -> bool) -> Vec<Packet> {
    let now = Instant::now();
    let mut queue = QUEUE.lock().unwrap();
    let mut ready = Vec::new();
    let mut held = Vec::new();

    while let Some(next) = queue.peek() {
        if next.at > now {
            break;
        }

        let scheduled = queue.pop().unwrap();
        if pred(&scheduled.packet) {
            ready.push(scheduled.packet);
        } else {
            held.push(scheduled);
        }
    }

    for item in held {
        queue.push(item);
    }

    ready
}

fn delay_ms() -> u32 {
    let lag = LAG_MS.load(AtomicOrdering::Relaxed);
    let jitter = JITTER_MS.load(AtomicOrdering::Relaxed);
    if jitter == 0 {
        return lag;
    }

    let span = jitter.saturating_mul(2).saturating_add(1);
    let roll = next_u32() % span;
    lag.saturating_add(roll).saturating_sub(jitter)
}

fn roll_loss() -> bool {
    let loss = LOSS_PCT.load(AtomicOrdering::Relaxed);
    if loss == 0 {
        return false;
    }

    (next_u32() % 100) < loss
}

fn next_u32() -> u32 {
    let mut state = RNG.load(AtomicOrdering::Relaxed);
    loop {
        let mut next = state;
        next ^= next << 13;
        next ^= next >> 7;
        next ^= next << 17;
        if next == 0 {
            next = seed();
        }

        match RNG.compare_exchange_weak(
            state,
            next,
            AtomicOrdering::Relaxed,
            AtomicOrdering::Relaxed,
        ) {
            Ok(_) => return (next >> 32) as u32 ^ (next as u32),
            Err(current) => state = current,
        }
    }
}

fn seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0xDEADBEEF);
    nanos | 1
}

fn packet_len(packet: &Packet) -> usize {
    match packet {
        Packet::Client(bytes) => bytes.len(),
        Packet::Server { bytes, .. } => bytes.len(),
    }
}
