use lewton::inside_ogg::OggStreamReader;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;

pub const SAMPLE_RATE: u32 = 48_000;
pub const PCM_LIMIT: usize = 256 * 1024;

#[derive(Clone)]
pub struct Pcm {
    pub samples: Arc<[i16]>,
    pub frames: u32,
    pub channels: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Wav,
    Ogg,
}

#[derive(Clone)]
pub struct StreamInfo {
    pub bytes: Arc<Vec<u8>>,
    pub kind: StreamKind,
    pub channels: u16,
    pub rate: u32,
    pub bits: u16,
    pub format: u16,
    pub data_start: usize,
    pub data_end: usize,
}

pub enum ClipBody {
    Missing,
    Pcm(Pcm),
    Stream(StreamInfo),
}

struct WavInfo {
    format: u16,
    channels: u16,
    rate: u32,
    bits: u16,
    block: usize,
    data_start: usize,
    data_end: usize,
}

struct Resampler {
    in_rate: u32,
    channels: usize,
    pos: f64,
    held: Vec<i16>,
}

pub struct Decoder {
    info: StreamInfo,
    resampler: Resampler,
    wav_pos: usize,
    ogg: Option<OggStreamReader<ArcCursor>>,
    pending: Vec<i16>,
    done: bool,
}

struct ArcCursor {
    bytes: Arc<Vec<u8>>,
    pos: usize,
}

impl Read for ArcCursor {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.bytes.len() {
            return Ok(0);
        }

        let rest = self.bytes.len() - self.pos;
        let count = rest.min(buf.len());
        buf[..count].copy_from_slice(&self.bytes[self.pos..self.pos + count]);
        self.pos += count;

        return Ok(count);
    }
}

impl Seek for ArcCursor {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let len = self.bytes.len() as i64;
        let next = match pos {
            SeekFrom::Start(value) => value as i64,
            SeekFrom::Current(value) => self.pos as i64 + value,
            SeekFrom::End(value) => len + value,
        };

        if next < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }

        self.pos = next as usize;

        return Ok(self.pos as u64);
    }
}

impl Resampler {
    fn new(in_rate: u32, channels: usize) -> Self {
        Self {
            in_rate,
            channels: channels.max(1),
            pos: 0.0,
            held: Vec::new(),
        }
    }

    fn push(&mut self, input: &[i16], out: &mut Vec<i16>) {
        if input.is_empty() {
            return;
        }

        if self.in_rate == SAMPLE_RATE {
            out.extend_from_slice(input);

            return;
        }

        self.held.extend_from_slice(input);
        self.emit(out, false);
    }

    fn finish(&mut self, out: &mut Vec<i16>) {
        if self.in_rate == SAMPLE_RATE || self.held.is_empty() {
            return;
        }

        let channels = self.channels;
        let start = self.held.len() - channels;
        let last = self.held[start..].to_vec();
        self.held.extend_from_slice(&last);
        self.emit(out, true);
    }

    fn emit(&mut self, out: &mut Vec<i16>, finishing: bool) {
        let channels = self.channels;
        let step = self.in_rate as f64 / SAMPLE_RATE as f64;
        let mut frames = self.held.len() / channels;

        if frames == 0 {
            return;
        }

        let limit = if finishing {
            frames as f64
        } else {
            frames as f64 - 1.0
        };

        while self.pos < limit {
            let idx = self.pos.floor() as usize;
            let frac = (self.pos - idx as f64) as f32;
            let next = if idx + 1 < frames { idx + 1 } else { idx };
            let mut channel = 0;

            while channel < channels {
                let first = self.held[idx * channels + channel] as f32;
                let second = self.held[next * channels + channel] as f32;
                let sample = first + (second - first) * frac;
                out.push(sample.round().clamp(-32768.0, 32767.0) as i16);
                channel += 1;
            }

            self.pos += step;
            frames = self.held.len() / channels;
        }

        let drop_frames = self.pos.floor() as usize;

        if drop_frames > 0 && drop_frames * channels <= self.held.len() {
            self.held.drain(0..drop_frames * channels);
            self.pos -= drop_frames as f64;
        }
    }
}

pub fn load_bytes(bytes: Vec<u8>, force_stream: bool) -> Result<ClipBody, String> {
    if bytes.len() >= 4 && &bytes[0..4] == b"OggS" {
        let info = probe_ogg(&bytes)?;

        if force_stream || bytes.len() > 96 * 1024 {
            return Ok(ClipBody::Stream(info));
        }

        let pcm = decode_ogg(&bytes)?;

        if pcm_bytes(&pcm) > PCM_LIMIT {
            return Ok(ClipBody::Stream(info));
        }

        return Ok(ClipBody::Pcm(pcm));
    }

    let header = parse_wav(&bytes)?;
    let estimate = estimate_wav(&header);

    if force_stream || estimate > PCM_LIMIT {
        return Ok(ClipBody::Stream(StreamInfo {
            bytes: Arc::new(bytes),
            kind: StreamKind::Wav,
            channels: header.channels,
            rate: header.rate,
            bits: header.bits,
            format: header.format,
            data_start: header.data_start,
            data_end: header.data_end,
        }));
    }

    let pcm = decode_wav(&bytes, &header)?;

    return Ok(ClipBody::Pcm(pcm));
}

impl Decoder {
    pub fn open(info: &StreamInfo) -> Result<Self, String> {
        let channels = info.channels.max(1) as usize;
        let ogg = if info.kind == StreamKind::Ogg {
            Some(open_ogg(&info.bytes)?)
        } else {
            None
        };

        return Ok(Self {
            info: info.clone(),
            resampler: Resampler::new(info.rate.max(1), channels),
            wav_pos: info.data_start,
            ogg,
            pending: Vec::new(),
            done: false,
        });
    }

    pub fn pull(&mut self, max_frames: usize) -> Result<Vec<i16>, String> {
        let channels = self.info.channels.max(1) as usize;
        let mut out = Vec::new();

        if !self.pending.is_empty() {
            out.append(&mut self.pending);
        }

        while out.len() / channels < max_frames && !self.done {
            let source = self.read_source(1024)?;

            if source.is_empty() {
                self.resampler.finish(&mut out);
                self.done = true;

                break;
            }

            self.resampler.push(&source, &mut out);
        }

        let max_samples = max_frames.max(1) * channels;

        if out.len() > max_samples {
            self.pending.extend_from_slice(&out[max_samples..]);
            out.truncate(max_samples);
        }

        return Ok(out);
    }

    pub fn rewind(&mut self) -> Result<(), String> {
        self.done = false;
        self.pending.clear();
        self.resampler = Resampler::new(self.info.rate.max(1), self.info.channels.max(1) as usize);
        self.wav_pos = self.info.data_start;

        if self.info.kind == StreamKind::Ogg {
            self.ogg = Some(open_ogg(&self.info.bytes)?);
        }

        return Ok(());
    }

    fn read_source(&mut self, frames: usize) -> Result<Vec<i16>, String> {
        if self.info.kind == StreamKind::Wav {
            return Ok(read_wav_frames(&self.info, &mut self.wav_pos, frames));
        }

        let Some(reader) = self.ogg.as_mut() else {
            return Ok(Vec::new());
        };

        match reader.read_dec_packet_itl() {
            Ok(Some(packet)) => Ok(packet),
            Ok(None) => Ok(Vec::new()),
            Err(err) => Err(format!("ogg: {err}")),
        }
    }
}

fn pcm_bytes(pcm: &Pcm) -> usize {
    pcm.samples.len() * 2
}

fn probe_ogg(bytes: &[u8]) -> Result<StreamInfo, String> {
    let reader = open_ogg_bytes(bytes)?;
    let channels = reader.ident_hdr.audio_channels as u16;
    let rate = reader.ident_hdr.audio_sample_rate;

    if channels == 0 || channels > 2 || rate == 0 {
        return Err("ogg has an unsupported channel count or rate".to_string());
    }

    return Ok(StreamInfo {
        bytes: Arc::new(bytes.to_vec()),
        kind: StreamKind::Ogg,
        channels,
        rate,
        bits: 16,
        format: 1,
        data_start: 0,
        data_end: bytes.len(),
    });
}

fn open_ogg(bytes: &Arc<Vec<u8>>) -> Result<OggStreamReader<ArcCursor>, String> {
    OggStreamReader::new(ArcCursor {
        bytes: Arc::clone(bytes),
        pos: 0,
    })
    .map_err(|err| format!("ogg: {err}"))
}

fn open_ogg_bytes(bytes: &[u8]) -> Result<OggStreamReader<Cursor<Vec<u8>>>, String> {
    OggStreamReader::new(Cursor::new(bytes.to_vec())).map_err(|err| format!("ogg: {err}"))
}

fn decode_ogg(bytes: &[u8]) -> Result<Pcm, String> {
    let mut reader = open_ogg_bytes(bytes)?;
    let channels = reader.ident_hdr.audio_channels as usize;
    let rate = reader.ident_hdr.audio_sample_rate;

    if channels == 0 || channels > 2 || rate == 0 {
        return Err("ogg has an unsupported channel count or rate".to_string());
    }

    let mut source = Vec::new();

    loop {
        match reader.read_dec_packet_itl() {
            Ok(Some(packet)) => source.extend_from_slice(&packet),
            Ok(None) => break,
            Err(err) => return Err(format!("ogg: {err}")),
        }
    }

    return Ok(finish_pcm(source, channels, rate));
}

fn parse_wav(bytes: &[u8]) -> Result<WavInfo, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a wav file".to_string());
    }

    let mut pos = 12usize;
    let mut format = 0u16;
    let mut channels = 0u16;
    let mut rate = 0u32;
    let mut bits = 0u16;
    let mut block = 0usize;
    let mut data_start = 0usize;
    let mut data_end = 0usize;
    let mut found_fmt = false;
    let mut found_data = false;

    while pos + 8 <= bytes.len() {
        let id = [bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]];
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let start = pos + 8;

        if start + size > bytes.len() {
            return Err("wav chunk exceeds the file".to_string());
        }

        if &id == b"fmt " && size >= 16 {
            format = u16::from_le_bytes([bytes[start], bytes[start + 1]]);
            channels = u16::from_le_bytes([bytes[start + 2], bytes[start + 3]]);
            rate = u32::from_le_bytes([
                bytes[start + 4],
                bytes[start + 5],
                bytes[start + 6],
                bytes[start + 7],
            ]);
            block = u16::from_le_bytes([bytes[start + 12], bytes[start + 13]]) as usize;
            bits = u16::from_le_bytes([bytes[start + 14], bytes[start + 15]]);

            if format == 0xFFFE && size >= 26 {
                format = u16::from_le_bytes([bytes[start + 24], bytes[start + 25]]);
            }

            found_fmt = true;
        }

        if &id == b"data" {
            data_start = start;
            data_end = start + size;
            found_data = true;
        }

        pos = start + size + (size & 1);
    }

    if !found_fmt || !found_data {
        return Err("wav is missing fmt or data".to_string());
    }

    if channels == 0 || channels > 2 || rate == 0 || block == 0 {
        return Err("wav has an unsupported layout".to_string());
    }

    if format != 1 && format != 3 {
        return Err("wav is compressed".to_string());
    }

    if bits != 8 && bits != 16 && bits != 24 && bits != 32 {
        return Err("wav has an unsupported bit depth".to_string());
    }

    let sample_bytes = (bits / 8) as usize;

    if sample_bytes * channels as usize > block {
        return Err("wav block align is too small".to_string());
    }

    return Ok(WavInfo {
        format,
        channels,
        rate,
        bits,
        block,
        data_start,
        data_end,
    });
}

fn estimate_wav(header: &WavInfo) -> usize {
    let frames = (header.data_end - header.data_start) / header.block;
    let scaled = (frames as u64 * SAMPLE_RATE as u64) / header.rate.max(1) as u64;

    return (scaled as usize) * header.channels as usize * 2;
}

fn decode_wav(bytes: &[u8], header: &WavInfo) -> Result<Pcm, String> {
    let mut pos = header.data_start;
    let mut source = Vec::new();

    while pos + header.block <= header.data_end {
        let mut channel = 0;

        while channel < header.channels as usize {
            let at = pos + channel * (header.bits as usize / 8);
            source.push(read_sample(bytes, at, header.bits, header.format));
            channel += 1;
        }

        pos += header.block;
    }

    if source.is_empty() {
        return Err("wav has no samples".to_string());
    }

    return Ok(finish_pcm(source, header.channels as usize, header.rate));
}

fn read_wav_frames(info: &StreamInfo, pos: &mut usize, frames: usize) -> Vec<i16> {
    let channels = info.channels.max(1) as usize;
    let sample_bytes = (info.bits / 8) as usize;
    let block = sample_bytes * channels;
    let mut out = Vec::new();
    let mut count = 0;

    while count < frames && *pos + block <= info.data_end {
        let mut channel = 0;

        while channel < channels {
            let at = *pos + channel * sample_bytes;
            out.push(read_sample(&info.bytes, at, info.bits, info.format));
            channel += 1;
        }

        *pos += block;
        count += 1;
    }

    return out;
}

fn read_sample(bytes: &[u8], at: usize, bits: u16, format: u16) -> i16 {
    if format == 3 && bits == 32 {
        if at + 4 > bytes.len() {
            return 0;
        }

        let value = f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);

        return (value.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    }

    if bits == 8 {
        if at >= bytes.len() {
            return 0;
        }

        return ((bytes[at] as i16) - 128) << 8;
    }

    if bits == 16 {
        if at + 2 > bytes.len() {
            return 0;
        }

        return i16::from_le_bytes([bytes[at], bytes[at + 1]]);
    }

    if bits == 24 {
        if at + 3 > bytes.len() {
            return 0;
        }

        let mut value =
            (bytes[at] as i32) | ((bytes[at + 1] as i32) << 8) | ((bytes[at + 2] as i32) << 16);

        if value & 0x800000 != 0 {
            value |= !0xFFFFFF;
        }

        return (value >> 8) as i16;
    }

    if at + 4 > bytes.len() {
        return 0;
    }

    let value = i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);

    return (value >> 16) as i16;
}

fn finish_pcm(source: Vec<i16>, channels: usize, rate: u32) -> Pcm {
    let mut resampler = Resampler::new(rate, channels);
    let mut samples = Vec::new();
    resampler.push(&source, &mut samples);
    resampler.finish(&mut samples);
    let frames = if channels == 0 {
        0
    } else {
        samples.len() / channels
    };

    return Pcm {
        samples: Arc::from(samples.into_boxed_slice()),
        frames: frames as u32,
        channels: channels as u16,
    };
}

#[cfg(test)]
pub fn write_wav(samples: &[i16], channels: u16, rate: u32) -> Vec<u8> {
    let data_bytes = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_bytes);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    let block = channels as u32 * 2;
    out.extend_from_slice(&(rate * block).to_le_bytes());
    out.extend_from_slice(&(block as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_bytes as u32).to_le_bytes());

    let mut idx = 0;

    while idx < samples.len() {
        out.extend_from_slice(&samples[idx].to_le_bytes());
        idx += 1;
    }

    return out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_pcm_roundtrip_keeps_samples() {
        let source = [1000, -2000, 3000, -4000];
        let bytes = write_wav(&source, 1, SAMPLE_RATE);
        let body = load_bytes(bytes, false).expect("wav");
        let ClipBody::Pcm(pcm) = body else {
            panic!("expected pcm");
        };

        assert_eq!(pcm.channels, 1);
        assert_eq!(pcm.frames, 4);
        assert_eq!(&pcm.samples[..], &source);
    }

    #[test]
    fn compressed_wav_is_rejected() {
        let mut bytes = write_wav(&[1, 2], 1, SAMPLE_RATE);
        bytes[20] = 2;
        bytes[21] = 0;
        let err = match load_bytes(bytes, false) {
            Ok(_) => panic!("compressed wav was accepted"),
            Err(err) => err,
        };

        assert!(err.contains("compressed"));
    }
}
