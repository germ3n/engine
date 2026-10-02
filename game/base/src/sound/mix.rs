use crate::sound::clip::{self, StreamInfo};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const VOICES: usize = 128;
pub const STREAMS: usize = 8;
const RING: u32 = 65536;
const PERIOD: i32 = 512;

const COMB: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];

#[derive(Clone, Copy)]
pub struct VoiceMix {
    pub generation: u32,
    pub sample_ptr: usize,
    pub frames: u32,
    pub channels: u16,
    pub stream_slot: u8,
    pub looping: u8,
    pub gain_l: f32,
    pub gain_r: f32,
    pub pitch: f32,
    pub lowpass: f32,
    pub reverb: f32,
}

impl Default for VoiceMix {
    fn default() -> Self {
        Self {
            generation: 0,
            sample_ptr: 0,
            frames: 0,
            channels: 1,
            stream_slot: 255,
            looping: 0,
            gain_l: 0.0,
            gain_r: 0.0,
            pitch: 1.0,
            lowpass: 1.0,
            reverb: 0.0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct MixSnapshot {
    pub voices: [VoiceMix; VOICES],
    pub wet: f32,
    pub feedback: f32,
    pub damp: f32,
    pub scale: f32,
}

impl Default for MixSnapshot {
    fn default() -> Self {
        Self {
            voices: [VoiceMix::default(); VOICES],
            wet: 0.0,
            feedback: 0.4,
            damp: 0.2,
            scale: 1.0,
        }
    }
}

#[derive(Clone, Copy)]
struct VoiceRun {
    generation: u32,
    cursor: f64,
    lp_l: f32,
    lp_r: f32,
    frac: f32,
    prev: [f32; 2],
    next: [f32; 2],
    primed: u8,
}

impl Default for VoiceRun {
    fn default() -> Self {
        Self {
            generation: 0,
            cursor: 0.0,
            lp_l: 0.0,
            lp_r: 0.0,
            frac: 0.0,
            prev: [0.0, 0.0],
            next: [0.0, 0.0],
            primed: 0,
        }
    }
}

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    lp: f32,
}

struct Reverb {
    comb: [Comb; 8],
    silent: u32,
}

struct StreamSlot {
    samples: UnsafeCell<Box<[i16]>>,
    cap: u32,
    mask: u32,
    write: AtomicU32,
    read: AtomicU32,
    serial: AtomicU32,
    retire: AtomicU32,
    released: AtomicU32,
    finished: AtomicBool,
}

struct Shared {
    snapshots: [UnsafeCell<MixSnapshot>; 3],
    published: AtomicU32,
    reading: AtomicU32,
    runs: UnsafeCell<[VoiceRun; VOICES]>,
    reverb: UnsafeCell<Reverb>,
    done: [AtomicU32; VOICES],
    rings: [StreamSlot; STREAMS],
    stop: AtomicBool,
}

unsafe impl Sync for Shared {}

struct Job {
    quit: bool,
    slot: u8,
    serial: u32,
    info: Option<StreamInfo>,
    looping: bool,
}

pub struct Mixer {
    shared: Arc<Shared>,
    tx: Sender<Job>,
    thread: Option<JoinHandle<()>>,
    pub live: bool,
}

unsafe extern "C" {
    fn sound_device_start(
        callback: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *mut f32, i32)>,
        user: *mut std::ffi::c_void,
        sample_rate: i32,
        channels: i32,
        period_frames: i32,
    ) -> i32;

    fn sound_device_stop();
}

impl Mixer {
    pub fn new() -> Self {
        let shared = Arc::new(Shared::new());
        let (tx, rx) = mpsc::channel();
        let decode_shared = Arc::clone(&shared);
        let thread = thread::spawn(move || decode_loop(rx, decode_shared));
        let user = Arc::as_ptr(&shared) as *mut std::ffi::c_void;
        let opened = unsafe {
            sound_device_start(
                Some(audio_callback),
                user,
                clip::SAMPLE_RATE as i32,
                2,
                PERIOD,
            )
        };
        let live = opened == 0;

        if !live {
            log::warn!("[sound] device did not open");
        }

        return Self {
            shared,
            tx,
            thread: Some(thread),
            live,
        };
    }

    pub fn publish(&self, snap: MixSnapshot) {
        let published = self.shared.published.load(Ordering::Acquire);
        let reading = self.shared.reading.load(Ordering::Acquire);
        let mut back = 0u32;
        let mut step = 0;

        while (back == published || back == reading) && step < 3 {
            back = (back + 1) % 3;
            step += 1;
        }

        unsafe {
            *self.shared.snapshots[back as usize].get() = snap;
        }

        self.shared.published.store(back, Ordering::Release);
    }

    pub fn finished(&self, idx: usize, generation: u32) -> bool {
        generation != 0 && self.shared.done[idx].load(Ordering::Acquire) == generation
    }

    pub fn released(&self, slot: usize) -> bool {
        let ring = &self.shared.rings[slot];

        return ring.released.load(Ordering::Acquire) == ring.retire.load(Ordering::Acquire);
    }

    pub fn begin_stream(&self, slot: usize, info: StreamInfo, looping: bool) {
        let ring = &self.shared.rings[slot];
        let serial = ring.serial.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        ring.read.store(0, Ordering::Release);
        ring.write.store(0, Ordering::Release);
        ring.finished.store(false, Ordering::Release);
        let _ = self.tx.send(Job {
            quit: false,
            slot: slot as u8,
            serial,
            info: Some(info),
            looping,
        });
    }

    pub fn retire_stream(&self, slot: usize) {
        let ring = &self.shared.rings[slot];
        let serial = ring.serial.load(Ordering::Acquire);
        ring.retire.store(serial, Ordering::Release);
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        unsafe {
            sound_device_stop();
        }

        self.shared.stop.store(true, Ordering::Release);
        let _ = self.tx.send(Job {
            quit: true,
            slot: 0,
            serial: 0,
            info: None,
            looping: false,
        });

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Shared {
    fn new() -> Self {
        Self {
            snapshots: [
                UnsafeCell::new(MixSnapshot::default()),
                UnsafeCell::new(MixSnapshot::default()),
                UnsafeCell::new(MixSnapshot::default()),
            ],
            published: AtomicU32::new(0),
            reading: AtomicU32::new(0),
            runs: UnsafeCell::new([VoiceRun::default(); VOICES]),
            reverb: UnsafeCell::new(Reverb::new()),
            done: std::array::from_fn(|_| AtomicU32::new(0)),
            rings: std::array::from_fn(|_| StreamSlot::new()),
            stop: AtomicBool::new(false),
        }
    }
}

impl StreamSlot {
    fn new() -> Self {
        Self {
            samples: UnsafeCell::new(vec![0; RING as usize].into_boxed_slice()),
            cap: RING,
            mask: RING - 1,
            write: AtomicU32::new(0),
            read: AtomicU32::new(0),
            serial: AtomicU32::new(0),
            retire: AtomicU32::new(0),
            released: AtomicU32::new(0),
            finished: AtomicBool::new(false),
        }
    }
}

impl Reverb {
    fn new() -> Self {
        Self {
            comb: std::array::from_fn(|_| Comb {
                buf: vec![0.0; 8192],
                idx: 0,
                lp: 0.0,
            }),
            silent: 0,
        }
    }

    fn step(&mut self, input: f32, feedback: f32, damp: f32, scale: f32) -> (f32, f32) {
        let feedback = feedback.clamp(0.0, 0.9);
        let damp = damp.clamp(0.0, 0.95);
        let scale = scale.clamp(0.25, 4.0);
        let mut left = 0.0;
        let mut right = 0.0;
        let mut idx = 0;

        while idx < 8 {
            let delay = ((COMB[idx] as f32) * scale) as usize;
            let sample = comb_step(&mut self.comb[idx], input, feedback, damp, delay);

            if idx % 2 == 0 {
                left += sample;
            } else {
                right += sample;
            }

            idx += 1;
        }

        return (left * 0.25, right * 0.25);
    }
}

fn comb_step(comb: &mut Comb, input: f32, feedback: f32, damp: f32, delay: usize) -> f32 {
    let len = comb.buf.len();
    let delay = delay.clamp(1, len - 1);
    let read = (comb.idx + len - delay) % len;
    let delayed = comb.buf[read];
    let filtered = delayed + damp * (comb.lp - delayed);
    comb.lp = filtered;
    comb.buf[comb.idx] = input + filtered * feedback;
    comb.idx += 1;

    if comb.idx >= len {
        comb.idx = 0;
    }

    return delayed;
}

unsafe extern "C" fn audio_callback(user: *mut std::ffi::c_void, out: *mut f32, frames: i32) {
    if user.is_null() || out.is_null() || frames <= 0 {
        return;
    }

    let shared = &*(user as *const Shared);
    let count = frames as usize;
    let output = std::slice::from_raw_parts_mut(out, count * 2);
    let mut idx = 0;

    while idx < output.len() {
        output[idx] = 0.0;
        idx += 1;
    }

    render_shared(shared, output);
}

fn render_shared(shared: &Shared, output: &mut [f32]) {
    let published = shared.published.load(Ordering::Acquire);
    shared.reading.store(published, Ordering::Release);
    let snap = unsafe { &*shared.snapshots[(published as usize) % 3].get() };
    let runs = unsafe { &mut *shared.runs.get() };
    let reverb = unsafe { &mut *shared.reverb.get() };
    let frames = output.len() / 2;
    let mut offset = 0;

    while offset < frames {
        let count = (frames - offset).min(512);
        render_chunk(
            output,
            offset,
            count,
            snap,
            runs,
            reverb,
            &shared.rings,
            &shared.done,
        );
        offset += count;
    }

    let mut slot = 0;

    while slot < STREAMS {
        let mut used = false;
        let mut voice = 0;

        while voice < VOICES {
            let mix = snap.voices[voice];

            if mix.generation != 0 && mix.stream_slot as usize == slot {
                used = true;

                break;
            }

            voice += 1;
        }

        if !used {
            let retire = shared.rings[slot].retire.load(Ordering::Acquire);
            shared.rings[slot].released.store(retire, Ordering::Release);
        }

        slot += 1;
    }
}

fn render_chunk(
    output: &mut [f32],
    offset: usize,
    count: usize,
    snap: &MixSnapshot,
    runs: &mut [VoiceRun; VOICES],
    reverb: &mut Reverb,
    rings: &[StreamSlot],
    done: &[AtomicU32; VOICES],
) {
    let mut send = [0.0f32; 512];
    let mut voice = 0;

    while voice < VOICES {
        let mix = snap.voices[voice];

        if mix.generation != 0 {
            if runs[voice].generation != mix.generation {
                runs[voice] = VoiceRun::default();
                runs[voice].generation = mix.generation;
            }

            if mix.stream_slot != 255 {
                mix_stream(
                    voice,
                    &mix,
                    &mut runs[voice],
                    output,
                    offset,
                    count,
                    &mut send,
                    rings,
                    done,
                );
            } else {
                mix_cached(
                    voice,
                    &mix,
                    &mut runs[voice],
                    output,
                    offset,
                    count,
                    &mut send,
                    done,
                );
            }
        }

        voice += 1;
    }

    let mut frame = 0;

    while frame < count {
        let (left, right) = reverb.step(send[frame], snap.feedback, snap.damp, snap.scale);
        let at = (offset + frame) * 2;
        output[at] += left * snap.wet;
        output[at + 1] += right * snap.wet;
        frame += 1;
    }

    if snap.wet < 0.0001 {
        reverb.silent = reverb.silent.saturating_add(count as u32);

        if reverb.silent > clip::SAMPLE_RATE {
            let mut idx = 0;

            while idx < 8 {
                reverb.comb[idx].buf.fill(0.0);
                reverb.comb[idx].lp = 0.0;
                reverb.comb[idx].idx = 0;
                idx += 1;
            }

            reverb.silent = 0;
        }
    } else {
        reverb.silent = 0;
    }
}

fn mix_cached(
    voice: usize,
    mix: &VoiceMix,
    run: &mut VoiceRun,
    output: &mut [f32],
    offset: usize,
    count: usize,
    send: &mut [f32],
    done: &[AtomicU32; VOICES],
) {
    if mix.sample_ptr == 0 || mix.frames == 0 {
        done[voice].store(mix.generation, Ordering::Release);

        return;
    }

    let mut frame = 0;

    while frame < count {
        if run.cursor >= mix.frames as f64 {
            if mix.looping == 0 {
                done[voice].store(mix.generation, Ordering::Release);

                break;
            }

            run.cursor %= mix.frames as f64;
        }

        let (left, right) = cached_sample(mix, run.cursor);
        let (low_l, low_r) = lowpass(run, mix.lowpass, left, right);
        let at = (offset + frame) * 2;
        output[at] += low_l * mix.gain_l;
        output[at + 1] += low_r * mix.gain_r;
        send[frame] += (low_l + low_r) * 0.5 * mix.reverb;
        run.cursor += mix.pitch as f64;
        frame += 1;
    }
}

fn mix_stream(
    voice: usize,
    mix: &VoiceMix,
    run: &mut VoiceRun,
    output: &mut [f32],
    offset: usize,
    count: usize,
    send: &mut [f32],
    rings: &[StreamSlot],
    done: &[AtomicU32; VOICES],
) {
    let slot = mix.stream_slot as usize;

    if slot >= STREAMS {
        return;
    }

    let ring = &rings[slot];
    let channels = (mix.channels as usize).clamp(1, 2);
    let mut frame = 0;

    while frame < count {
        if run.primed == 0 {
            if !read_frame(ring, channels, &mut run.next) {
                if ring.finished.load(Ordering::Acquire) && mix.looping == 0 {
                    done[voice].store(mix.generation, Ordering::Release);
                }

                break;
            }

            run.prev = run.next;
            run.primed = 1;
        }

        let frac = run.frac;
        let left = run.prev[0] + (run.next[0] - run.prev[0]) * frac;
        let right = run.prev[1] + (run.next[1] - run.prev[1]) * frac;
        let (low_l, low_r) = lowpass(run, mix.lowpass, left, right);
        let at = (offset + frame) * 2;
        output[at] += low_l * mix.gain_l;
        output[at + 1] += low_r * mix.gain_r;
        send[frame] += (low_l + low_r) * 0.5 * mix.reverb;
        run.frac += mix.pitch;

        while run.frac >= 1.0 {
            run.prev = run.next;

            if !read_frame(ring, channels, &mut run.next) {
                run.frac = 0.0;

                if ring.finished.load(Ordering::Acquire) && mix.looping == 0 {
                    done[voice].store(mix.generation, Ordering::Release);
                }

                break;
            }

            run.frac -= 1.0;
        }

        frame += 1;
    }
}

fn lowpass(run: &mut VoiceRun, coeff: f32, left: f32, right: f32) -> (f32, f32) {
    let coeff = coeff.clamp(0.0, 1.0);
    run.lp_l += coeff * (left - run.lp_l);
    run.lp_r += coeff * (right - run.lp_r);

    return (run.lp_l, run.lp_r);
}

fn cached_sample(mix: &VoiceMix, cursor: f64) -> (f32, f32) {
    let frames = mix.frames;

    if frames == 0 || mix.sample_ptr == 0 {
        return (0.0, 0.0);
    }

    let idx = (cursor.floor() as u32).min(frames - 1);
    let next = (idx + 1).min(frames - 1);
    let frac = (cursor - idx as f64) as f32;
    let channels = if mix.channels <= 1 { 1 } else { 2 };
    let ptr = mix.sample_ptr as *const i16;
    let first = unsafe { *ptr.add(idx as usize * channels) } as f32;
    let second = unsafe { *ptr.add(next as usize * channels) } as f32;
    let left = (first + (second - first) * frac) * (1.0 / 32768.0);

    if channels == 1 {
        return (left, left);
    }

    let first_r = unsafe { *ptr.add(idx as usize * channels + 1) } as f32;
    let second_r = unsafe { *ptr.add(next as usize * channels + 1) } as f32;
    let right = (first_r + (second_r - first_r) * frac) * (1.0 / 32768.0);

    return (left, right);
}

fn read_frame(ring: &StreamSlot, channels: usize, dst: &mut [f32; 2]) -> bool {
    let write = ring.write.load(Ordering::Acquire);
    let read = ring.read.load(Ordering::Relaxed);
    let have = write.wrapping_sub(read);

    if have < channels as u32 {
        return false;
    }

    let ptr = unsafe { (*ring.samples.get()).as_ptr() };
    let mut channel = 0;

    while channel < channels {
        let at = (read.wrapping_add(channel as u32) & ring.mask) as usize;
        let sample = unsafe { *ptr.add(at) } as f32 * (1.0 / 32768.0);
        dst[channel] = sample;
        channel += 1;
    }

    if channels == 1 {
        dst[1] = dst[0];
    }

    ring.read
        .store(read.wrapping_add(channels as u32), Ordering::Release);

    return true;
}

fn ring_write(ring: &StreamSlot, data: &[i16]) -> usize {
    let write = ring.write.load(Ordering::Relaxed);
    let read = ring.read.load(Ordering::Acquire);
    let used = write.wrapping_sub(read);
    let free = ring.cap.wrapping_sub(used);

    if free == 0 || data.is_empty() {
        return 0;
    }

    let count = (data.len() as u32).min(free) as usize;
    let ptr = unsafe { (*ring.samples.get()).as_mut_ptr() };
    let mut idx = 0;

    while idx < count {
        let at = (write.wrapping_add(idx as u32) & ring.mask) as usize;
        unsafe {
            *ptr.add(at) = data[idx];
        }
        idx += 1;
    }

    ring.write
        .store(write.wrapping_add(count as u32), Ordering::Release);

    return count;
}

fn decode_loop(rx: Receiver<Job>, shared: Arc<Shared>) {
    while let Ok(job) = rx.recv() {
        if job.quit || shared.stop.load(Ordering::Acquire) {
            break;
        }

        decode_job(&job, &shared);
    }
}

fn decode_job(job: &Job, shared: &Shared) {
    let Some(info) = &job.info else {
        return;
    };

    let mut decoder = match clip::Decoder::open(info) {
        Ok(decoder) => decoder,
        Err(err) => {
            log::warn!("[sound] {err}");
            shared.rings[job.slot as usize]
                .finished
                .store(true, Ordering::Release);

            return;
        }
    };

    let ring = &shared.rings[job.slot as usize];
    let channels = info.channels.max(1) as usize;
    let mut empty_passes = 0;

    loop {
        if shared.stop.load(Ordering::Acquire) || ring.serial.load(Ordering::Acquire) != job.serial
        {
            return;
        }

        let write = ring.write.load(Ordering::Relaxed);
        let read = ring.read.load(Ordering::Acquire);
        let free = ring.cap.wrapping_sub(write.wrapping_sub(read));

        if free < 2048 {
            thread::sleep(Duration::from_millis(2));

            continue;
        }

        let frames = ((free as usize) / channels).min(1024);
        let samples = match decoder.pull(frames.max(1)) {
            Ok(samples) => samples,
            Err(err) => {
                log::warn!("[sound] {err}");
                ring.finished.store(true, Ordering::Release);

                return;
            }
        };

        if samples.is_empty() {
            empty_passes += 1;

            if job.looping && empty_passes < 2 && decoder.rewind().is_ok() {
                continue;
            }

            ring.finished.store(true, Ordering::Release);

            return;
        }

        empty_passes = 0;

        let mut offset = 0;

        while offset < samples.len() {
            if ring.serial.load(Ordering::Acquire) != job.serial {
                return;
            }

            let wrote = ring_write(ring, &samples[offset..]);

            if wrote == 0 {
                thread::sleep(Duration::from_millis(2));

                continue;
            }

            offset += wrote;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_voice_mixes_a_constant_sample() {
        let samples = vec![16384i16; 64];
        let mut snap = MixSnapshot::default();
        snap.voices[0] = VoiceMix {
            generation: 1,
            sample_ptr: samples.as_ptr() as usize,
            frames: 64,
            channels: 1,
            stream_slot: 255,
            looping: 0,
            gain_l: 1.0,
            gain_r: 1.0,
            pitch: 1.0,
            lowpass: 1.0,
            reverb: 0.0,
        };
        snap.wet = 0.0;
        let mut runs = [VoiceRun::default(); VOICES];
        let mut reverb = Reverb::new();
        let rings: [StreamSlot; STREAMS] = std::array::from_fn(|_| StreamSlot::new());
        let done = std::array::from_fn(|_| AtomicU32::new(0));
        let mut output = vec![0.0; 32];
        render_chunk(
            &mut output,
            0,
            16,
            &snap,
            &mut runs,
            &mut reverb,
            &rings,
            &done,
        );

        let mut idx = 0;

        while idx < output.len() {
            assert!((output[idx] - 0.5).abs() < 0.001, "{}", output[idx]);
            idx += 1;
        }
    }

    #[test]
    fn reverb_impulse_stays_finite() {
        let mut reverb = Reverb::new();
        let mut peak = 0.0f32;
        let mut idx = 0;

        while idx < 48000 {
            let input = if idx == 0 { 1.0 } else { 0.0 };
            let (left, right) = reverb.step(input, 0.7, 0.2, 1.0);
            assert!(left.is_finite() && right.is_finite());
            peak = peak.max(left.abs()).max(right.abs());
            idx += 1;
        }

        assert!(peak < 4.0);
    }
}
