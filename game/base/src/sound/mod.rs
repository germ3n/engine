mod clip;
mod mix;

use crate::entities::{EntityHandle, EntityList};
use crate::network::usermessage::hash_usermessage_name;
use crate::script::libs::vector3::Vector3;
use crate::script::Realm;
use crate::world::{BrushMap, VoxelWorld};
use clip::{ClipBody, Pcm, StreamInfo};
use mix::{MixSnapshot, Mixer, VoiceMix, STREAMS, VOICES};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

pub const CENTER: f32 = std::f32::consts::FRAC_1_SQRT_2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Channel
{
    Auto,
    Weapon,
    Voice,
    Item,
    Body,
    Stream,
    Static,
}

impl Channel
{
    fn parse(name: &str) -> Self
    {
        match name
        {
            "weapon" => Self::Weapon,
            "voice" => Self::Voice,
            "item" => Self::Item,
            "body" => Self::Body,
            "stream" => Self::Stream,
            "static" => Self::Static,
            _ => Self::Auto,
        }
    }

    fn replaces(self) -> bool
    {
        match self
        {
            Self::Auto | Self::Static => false,
            _ => true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bus
{
    Sfx,
    Music,
    Ui,
    Voice,
}

impl Bus
{
    fn parse(name: &str) -> Self
    {
        match name
        {
            "music" => Self::Music,
            "ui" => Self::Ui,
            "voice" => Self::Voice,
            _ => Self::Sfx,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Room
{
    None,
    Room,
    Hall,
    Underwater,
}

impl Room
{
    fn parse(name: &str) -> Self
    {
        match name
        {
            "room" => Self::Room,
            "hall" => Self::Hall,
            "underwater" => Self::Underwater,
            _ => Self::None,
        }
    }

    fn target(self) -> (f32, f32, f32, f32)
    {
        match self
        {
            Self::None => (0.0, 0.35, 0.2, 1.0),
            Self::Room => (0.18, 0.5, 0.28, 1.0),
            Self::Hall => (0.28, 0.7, 0.18, 2.4),
            Self::Underwater => (0.45, 0.55, 0.72, 0.65),
        }
    }
}

#[derive(Clone)]
struct SoundDef
{
    channel: Channel,
    level: f32,
    volume_min: f32,
    volume_max: f32,
    pitch_min: f32,
    pitch_max: f32,
    waves: Vec<u32>,
    bus: Bus,
    looping: bool,
    stream: bool,
}

struct Scape
{
    room: Room,
    sounds: Vec<u32>,
}

struct ScapeBox
{
    scape: u32,
    min: Vector3,
    max: Vector3,
}

struct Catalog
{
    paths: HashMap<u32, String>,
    clips: HashMap<u32, ClipBody>,
    defs: HashMap<u32, SoundDef>,
    scapes: HashMap<u32, Scape>,
    boxes: Vec<ScapeBox>,
}

enum Ready
{
    Pcm
    {
        ptr: usize,
        frames: u32,
        channels: u16,
    },
    Stream(StreamInfo),
}

struct Start
{
    sound_hash: u32,
    def_hash: u32,
    entity: EntityHandle,
    position: Vector3,
    volume: f32,
    pitch: f32,
    tick: u64,
    channel: Channel,
    level: f32,
    bus: Bus,
    looping: bool,
    positional: bool,
    scape: bool,
    fade: f32,
    stream: bool,
}

struct LogicalVoice
{
    active: bool,
    generation: u32,
    entity: EntityHandle,
    channel: Channel,
    def_hash: u32,
    sound_hash: u32,
    bus: Bus,
    volume: f32,
    pitch: f32,
    level: f32,
    position: Vector3,
    positional: bool,
    looping: bool,
    scape: bool,
    fade: f32,
    fade_target: f32,
    occlusion: f32,
    occlusion_target: f32,
    age: f32,
    score: f32,
    sample_ptr: usize,
    frames: u32,
    channels: u16,
    stream_slot: u8,
}

impl Default for LogicalVoice
{
    fn default() -> Self
    {
        Self {
            active: false,
            generation: 0,
            entity: EntityHandle::NULL,
            channel: Channel::Auto,
            def_hash: 0,
            sound_hash: 0,
            bus: Bus::Sfx,
            volume: 1.0,
            pitch: 100.0,
            level: 75.0,
            position: Vector3::new(0.0, 0.0, 0.0),
            positional: false,
            looping: false,
            scape: false,
            fade: 1.0,
            fade_target: 1.0,
            occlusion: 0.0,
            occlusion_target: 0.0,
            age: 0.0,
            score: 0.0,
            sample_ptr: 0,
            frames: 0,
            channels: 1,
            stream_slot: 255,
        }
    }
}

pub struct NetPlay
{
    pub sound_hash: u32,
    pub def_hash: u32,
    pub entity: EntityHandle,
    pub position: Vector3,
    pub volume: f32,
    pub pitch: f32,
    pub tick: u64,
    pub looping: bool,
    pub positional: bool,
}

pub struct NetStop
{
    pub def_hash: u32,
    pub sound_hash: u32,
    pub entity: EntityHandle,
}

pub enum Pending
{
    Play(NetPlay),
    Stop(NetStop),
}

struct Tracked
{
    play: NetPlay,
    channel: Channel,
}

pub struct Buses
{
    pub master: f32,
    pub sfx: f32,
    pub music: f32,
    pub ui: f32,
    pub voice: f32,
}

pub struct SoundWorld
{
    mixer: Option<Mixer>,
    catalog: Catalog,
    voices: [LogicalVoice; VOICES],
    loops: Vec<Tracked>,
    pending: Vec<Pending>,
    dedup: VecDeque<(u64, u32, u32, u32)>,
    realm: Realm,
    command_tick: u64,
    sim_tick: u64,
    next_gen: u32,
    trace_cursor: usize,
    room: Room,
    wet: f32,
    feedback: f32,
    damp: f32,
    scale: f32,
    active_scape: u32,
}

pub type SoundAccess = Arc<AtomicPtr<SoundWorld>>;

pub struct SoundScope<'a>
{
    access: &'a AtomicPtr<SoundWorld>,
    previous: *mut SoundWorld,
}

impl<'a> SoundScope<'a>
{
    pub fn new(access: &'a AtomicPtr<SoundWorld>, sound: *mut SoundWorld) -> Self
    {
        let previous = access.swap(sound, Ordering::Relaxed);

        return Self { access, previous };
    }
}

impl Drop for SoundScope<'_>
{
    fn drop(&mut self)
    {
        self.access.store(self.previous, Ordering::Relaxed);
    }
}

pub fn sound_hash(name: &str) -> u32
{
    let hash = hash_usermessage_name(name);

    if hash == 0
    {
        return 1;
    }

    return hash;
}

pub fn reach(level: f32, max_distance: f32) -> f32
{
    (max_distance * (level / 75.0)).max(0.5)
}

pub fn distance_gain(dist: f32, level: f32, max_distance: f32) -> f32
{
    let max_dist = reach(level, max_distance);

    if dist <= 0.5
    {
        return 1.0;
    }

    if dist >= max_dist
    {
        return 0.0;
    }

    let fade = 1.0 - (dist - 0.5) / (max_dist - 0.5);

    return (0.5 / dist) * fade;
}

pub fn lowpass_coeff(dist: f32, level: f32, max_distance: f32, occlusion: f32) -> f32
{
    let max_dist = reach(level, max_distance).max(0.51);
    let along = ((dist - 0.5) / (max_dist - 0.5)).clamp(0.0, 1.0);
    let air = 18000.0 + (2000.0 - 18000.0) * along;
    let blocked = occlusion.clamp(0.0, 1.0);
    let cutoff = air + (800.0 - air) * blocked;
    let coeff = 1.0 - (-2.0 * std::f32::consts::PI * cutoff / clip::SAMPLE_RATE as f32).exp();

    return coeff.clamp(0.0, 1.0);
}

pub fn roll(tick: u64, entity: u32, name: u32, salt: u32) -> u32
{
    let mut hash = 2166136261u32;
    let parts = [
        tick.to_le_bytes().to_vec(),
        entity.to_le_bytes().to_vec(),
        name.to_le_bytes().to_vec(),
        salt.to_le_bytes().to_vec(),
    ];
    let mut part = 0;

    while part < parts.len()
    {
        let mut idx = 0;

        while idx < parts[part].len()
        {
            hash ^= parts[part][idx] as u32;
            hash = hash.wrapping_mul(16777619);
            idx += 1;
        }

        part += 1;
    }

    return hash;
}

impl SoundWorld
{
    pub fn new(realm: Realm, device: bool) -> Self
    {
        let mixer = if device { Some(Mixer::new()) } else { None };
        let mut world = Self {
            mixer,
            catalog: Catalog {
                paths: HashMap::new(),
                clips: HashMap::new(),
                defs: HashMap::new(),
                scapes: HashMap::new(),
                boxes: Vec::new(),
            },
            voices: std::array::from_fn(|_| LogicalVoice::default()),
            loops: Vec::new(),
            pending: Vec::new(),
            dedup: VecDeque::new(),
            realm,
            command_tick: 0,
            sim_tick: 0,
            next_gen: 0,
            trace_cursor: 0,
            room: Room::None,
            wet: 0.0,
            feedback: 0.35,
            damp: 0.2,
            scale: 1.0,
            active_scape: 0,
        };
        world.index_files();

        return world;
    }

    pub fn set_clock(&mut self, tick: u64)
    {
        self.sim_tick = tick;
    }

    pub fn set_command_tick(&mut self, tick: u64)
    {
        self.command_tick = tick;
    }

    pub fn allows_local(&self, first_time: bool) -> bool
    {
        if matches!(self.realm, Realm::Client)
        {
            return first_time;
        }

        return true;
    }

    pub fn add_def(
        &mut self,
        name: &str,
        channel: &str,
        level: f32,
        volume_min: f32,
        volume_max: f32,
        pitch_min: f32,
        pitch_max: f32,
        waves: &[String],
        bus: &str,
        looping: bool,
        stream: bool,
    )
    {
        if name.is_empty() || waves.is_empty()
        {
            return;
        }

        let mut files = Vec::new();
        let mut idx = 0;

        while idx < waves.len()
        {
            let hash = sound_hash(&waves[idx]);
            self.catalog.paths.entry(hash).or_insert_with(|| waves[idx].clone());
            files.push(hash);
            idx += 1;
        }

        self.catalog.defs.insert(
            sound_hash(name),
            SoundDef {
                channel: Channel::parse(channel),
                level: level.clamp(1.0, 140.0),
                volume_min: volume_min.min(volume_max).clamp(0.0, 4.0),
                volume_max: volume_min.max(volume_max).clamp(0.0, 4.0),
                pitch_min: pitch_min.min(pitch_max).clamp(1.0, 255.0),
                pitch_max: pitch_min.max(pitch_max).clamp(1.0, 255.0),
                waves: files,
                bus: Bus::parse(bus),
                looping,
                stream,
            },
        );
    }

    pub fn add_scape(&mut self, name: &str, room: &str, sounds: &[String])
    {
        if name.is_empty()
        {
            return;
        }

        let mut defs = Vec::new();
        let mut idx = 0;

        while idx < sounds.len()
        {
            defs.push(sound_hash(&sounds[idx]));
            idx += 1;
        }

        self.catalog.scapes.insert(
            sound_hash(name),
            Scape {
                room: Room::parse(room),
                sounds: defs,
            },
        );
    }

    pub fn add_box(&mut self, name: &str, min: Vector3, max: Vector3)
    {
        let scape = sound_hash(name);

        if !self.catalog.scapes.contains_key(&scape)
        {
            return;
        }

        self.catalog.boxes.push(ScapeBox { scape, min, max });
    }

    pub fn set_room(&mut self, name: &str)
    {
        self.room = Room::parse(name);
    }

    pub fn play(
        &mut self,
        name: &str,
        position: Option<Vector3>,
        volume: f32,
        pitch: f32,
        entity: EntityHandle,
        channel: &str,
    )
    {
        let hash = sound_hash(name);
        let roll_tick = if self.command_tick != 0 { self.command_tick } else { self.sim_tick };
        let def = self.catalog.defs.get(&hash).cloned();
        let start = if let Some(def) = def
        {
            if def.waves.is_empty()
            {
                return;
            }

            let wave_roll = roll(roll_tick, entity.0, hash, 1);
            let wave = def.waves[(wave_roll as usize) % def.waves.len()];
            let vol = if volume >= 0.0
            {
                volume
            }
            else
            {
                lerp(def.volume_min, def.volume_max, unit(roll(roll_tick, entity.0, hash, 2)))
            };
            let pit = if pitch >= 0.0
            {
                pitch
            }
            else
            {
                lerp(def.pitch_min, def.pitch_max, unit(roll(roll_tick, entity.0, hash, 3)))
            };
            let chosen = if channel.is_empty() { def.channel } else { Channel::parse(channel) };
            let positional = position.is_some();

            Start {
                sound_hash: wave,
                def_hash: hash,
                entity,
                position: position.unwrap_or_else(|| Vector3::new(0.0, 0.0, 0.0)),
                volume: vol.clamp(0.0, 4.0),
                pitch: pit.clamp(1.0, 255.0),
                tick: self.command_tick,
                channel: chosen,
                level: def.level,
                bus: if positional { def.bus } else { Bus::Ui },
                looping: def.looping,
                positional,
                scape: false,
                fade: 1.0,
                stream: def.stream,
            }
        }
        else
        {
            self.catalog.paths.entry(hash).or_insert_with(|| name.to_string());
            let positional = position.is_some();

            Start {
                sound_hash: hash,
                def_hash: 0,
                entity,
                position: position.unwrap_or_else(|| Vector3::new(0.0, 0.0, 0.0)),
                volume: if volume >= 0.0 { volume.clamp(0.0, 4.0) } else { 1.0 },
                pitch: if pitch >= 0.0 { pitch.clamp(1.0, 255.0) } else { 100.0 },
                tick: self.command_tick,
                channel: if channel.is_empty() { Channel::Auto } else { Channel::parse(channel) },
                level: 75.0,
                bus: if positional { Bus::Sfx } else { Bus::Ui },
                looping: false,
                positional,
                scape: false,
                fade: 1.0,
                stream: false,
            }
        };

        self.dispatch(start);
    }

    pub fn stop(&mut self, entity: EntityHandle, name: Option<&str>)
    {
        let (def_hash, sound_hash) = match name
        {
            None => (0, 0),
            Some(name) =>
            {
                let hash = sound_hash(name);

                if self.catalog.defs.contains_key(&hash)
                {
                    (hash, 0)
                }
                else
                {
                    (0, hash)
                }
            }
        };

        if matches!(self.realm, Realm::Server)
        {
            self.loops.retain(|item| !stop_match(&item.play, entity, def_hash, sound_hash));
            self.pending.push(Pending::Stop(NetStop {
                def_hash,
                sound_hash,
                entity,
            }));

            return;
        }

        self.stop_voices(entity, def_hash, sound_hash);
    }

    pub fn forget_entity(&mut self, entity: EntityHandle)
    {
        self.loops.retain(|item| item.play.entity != entity);
        self.stop_voices(entity, 0, 0);
    }

    pub fn hear_play(
        &mut self,
        sound_hash: u32,
        def_hash: u32,
        entity: EntityHandle,
        position: Vector3,
        volume: f32,
        pitch: f32,
        tick: u64,
        force_loop: bool,
        positional: bool,
    )
    {
        if tick != 0 && self.duplicate(tick, entity.0, def_hash, sound_hash)
        {
            return;
        }

        let def = self.catalog.defs.get(&def_hash).cloned();
        let start = Start {
            sound_hash,
            def_hash,
            entity,
            position,
            volume,
            pitch,
            tick: 0,
            channel: def.as_ref().map(|item| item.channel).unwrap_or(Channel::Auto),
            level: def.as_ref().map(|item| item.level).unwrap_or(75.0),
            bus: if positional { def.as_ref().map(|item| item.bus).unwrap_or(Bus::Sfx) } else { Bus::Ui },
            looping: force_loop || def.as_ref().map(|item| item.looping).unwrap_or(false),
            positional,
            scape: false,
            fade: 1.0,
            stream: def.as_ref().map(|item| item.stream).unwrap_or(false),
        };
        self.play_local(start);
    }

    pub fn hear_stop(&mut self, entity: EntityHandle, def_hash: u32, sound_hash: u32)
    {
        self.stop_voices(entity, def_hash, sound_hash);
    }

    pub fn take_pending(&mut self) -> Vec<Pending>
    {
        std::mem::take(&mut self.pending)
    }

    pub fn baseline(&self) -> Vec<NetPlay>
    {
        let mut out = Vec::new();
        let mut idx = 0;

        while idx < self.loops.len()
        {
            out.push(self.loops[idx].play.clone_play());
            idx += 1;
        }

        return out;
    }

    pub fn update(
        &mut self,
        dt: f32,
        origin: Vector3,
        yaw: f32,
        pitch: f32,
        buses: Buses,
        max_distance: f32,
        brushes: &BrushMap,
        voxels: &VoxelWorld,
        entities: &EntityList,
    )
    {
        if self.mixer.is_none()
        {
            return;
        }

        self.follow(entities);
        self.occlusion(dt, origin, brushes, voxels);
        self.scapes(dt, origin);
        self.reclaim(dt);
        self.smooth_room(dt);
        let snap = self.snapshot(origin, yaw, pitch, buses, max_distance);

        if let Some(mixer) = &self.mixer
        {
            mixer.publish(snap);
        }
    }

    fn index_files(&mut self)
    {
        let Some(fs) = crate::fs::try_global() else
        {
            return;
        };

        for path in fs.list_prefix("sound/")
        {
            if path.ends_with(".wav") || path.ends_with(".ogg")
            {
                let hash = sound_hash(&path);
                self.catalog.paths.entry(hash).or_insert(path);
            }
        }
    }

    fn dispatch(&mut self, start: Start)
    {
        match self.realm
        {
            Realm::Server =>
            {
                if start.looping
                {
                    self.note_loop(&start);
                }

                self.pending.push(Pending::Play(start.to_net()));
            }
            Realm::Client =>
            {
                if start.tick != 0
                {
                    self.remember(start.tick, start.entity.0, start.def_hash, start.sound_hash);
                }

                self.play_local(start);
            }
            Realm::Menu => {}
        }
    }

    fn note_loop(&mut self, start: &Start)
    {
        if start.channel.replaces() && !start.entity.is_null()
        {
            self.loops.retain(|item| item.play.entity != start.entity || item.channel != start.channel);
        }
        else
        {
            self.loops.retain(|item| {
                item.play.entity != start.entity
                    || item.play.def_hash != start.def_hash
                    || item.play.sound_hash != start.sound_hash
            });
        }

        self.loops.push(Tracked {
            play: start.to_net(),
            channel: start.channel,
        });
    }

    fn play_local(&mut self, start: Start)
    {
        if let Some(idx) = self.find_replace(&start)
        {
            let _ = self.occupy(idx, start);

            return;
        }

        let score = incoming_score(&start);
        let Some(idx) = self.find_slot(score, start.bus) else
        {
            return;
        };

        let _ = self.occupy(idx, start);
    }

    fn occupy(&mut self, idx: usize, start: Start) -> bool
    {
        let Some(ready) = self.prepare(start.sound_hash, start.stream) else
        {
            return false;
        };

        if self.voices[idx].active
        {
            self.stop_slot(idx);
        }

        let mut stream_slot = 255u8;
        let mut ptr = 0usize;
        let mut frames = 0u32;
        let channels = match ready
        {
            Ready::Pcm { ptr: sample_ptr, frames: sample_frames, channels: sample_channels } =>
            {
                ptr = sample_ptr;
                frames = sample_frames;
                sample_channels.max(1)
            }
            Ready::Stream(info) =>
            {
                let live = self.mixer.as_ref().map(|mixer| mixer.live).unwrap_or(false);

                if !live
                {
                    return false;
                }

                let Some(slot) = self.alloc_stream() else
                {
                    log::debug!("[sound] no stream slot");

                    return false;
                };

                if let Some(mixer) = &self.mixer
                {
                    mixer.begin_stream(slot, info.clone(), start.looping);
                }

                stream_slot = slot as u8;
                info.channels.max(1)
            }
        };

        let generation = self.bump();
        let voice = &mut self.voices[idx];
        voice.active = true;
        voice.generation = generation;
        voice.entity = start.entity;
        voice.channel = start.channel;
        voice.def_hash = start.def_hash;
        voice.sound_hash = start.sound_hash;
        voice.bus = start.bus;
        voice.volume = start.volume;
        voice.pitch = start.pitch;
        voice.level = start.level;
        voice.position = start.position;
        voice.positional = start.positional;
        voice.looping = start.looping;
        voice.scape = start.scape;
        voice.fade = start.fade;
        voice.fade_target = 1.0;
        voice.occlusion = 0.0;
        voice.occlusion_target = 0.0;
        voice.age = 0.0;
        voice.score = incoming_score(&start);
        voice.sample_ptr = ptr;
        voice.frames = frames;
        voice.channels = channels;
        voice.stream_slot = stream_slot;

        return true;
    }

    fn prepare(&mut self, hash: u32, force_stream: bool) -> Option<Ready>
    {
        if !self.catalog.clips.contains_key(&hash)
        {
            self.load_hash(hash, force_stream);
        }

        match self.catalog.clips.get(&hash)
        {
            Some(ClipBody::Pcm(pcm)) => Some(Ready::Pcm {
                ptr: pcm_ptr(pcm),
                frames: pcm.frames,
                channels: pcm.channels,
            }),
            Some(ClipBody::Stream(info)) => Some(Ready::Stream(info.clone())),
            _ => None,
        }
    }

    fn load_hash(&mut self, hash: u32, force_stream: bool)
    {
        let Some(path) = self.catalog.paths.get(&hash).cloned() else
        {
            log::warn!("[sound] unknown {hash}");
            self.catalog.clips.insert(hash, ClipBody::Missing);

            return;
        };

        let bytes = match crate::fs::read(&path)
        {
            Ok(bytes) => bytes,
            Err(err) =>
            {
                log::warn!("[sound] {path}: {err}");
                self.catalog.clips.insert(hash, ClipBody::Missing);

                return;
            }
        };

        match clip::load_bytes(bytes, force_stream)
        {
            Ok(body) =>
            {
                self.catalog.clips.insert(hash, body);
            }
            Err(err) =>
            {
                log::warn!("[sound] {path}: {err}");
                self.catalog.clips.insert(hash, ClipBody::Missing);
            }
        }
    }

    fn find_replace(&self, start: &Start) -> Option<usize>
    {
        if start.channel.replaces() && !start.entity.is_null()
        {
            let mut idx = 0;

            while idx < VOICES
            {
                let voice = &self.voices[idx];

                if voice.active && voice.entity == start.entity && voice.channel == start.channel
                {
                    return Some(idx);
                }

                idx += 1;
            }
        }

        return None;
    }

    fn find_slot(&self, score: f32, bus: Bus) -> Option<usize>
    {
        let mut idx = 0;

        while idx < VOICES
        {
            if !self.voices[idx].active
            {
                return Some(idx);
            }

            idx += 1;
        }

        let mut best: Option<(usize, f32)> = None;
        idx = 0;

        while idx < VOICES
        {
            let voice = &self.voices[idx];

            if voice.bus != Bus::Ui && best.map(|(_, quiet)| voice.score < quiet).unwrap_or(true)
            {
                best = Some((idx, voice.score));
            }

            idx += 1;
        }

        let (slot, quiet) = best?;

        if bus != Bus::Ui && score < quiet
        {
            return None;
        }

        return Some(slot);
    }

    fn alloc_stream(&self) -> Option<usize>
    {
        if self.mixer.is_none()
        {
            return None;
        }

        let mut idx = 0;

        while idx < STREAMS
        {
            if !self.stream_used(idx) && self.mixer.as_ref().map(|mixer| mixer.released(idx)).unwrap_or(false)
            {
                return Some(idx);
            }

            idx += 1;
        }

        return None;
    }

    fn stream_used(&self, slot: usize) -> bool
    {
        let mut idx = 0;

        while idx < VOICES
        {
            if self.voices[idx].active && self.voices[idx].stream_slot as usize == slot
            {
                return true;
            }

            idx += 1;
        }

        return false;
    }

    fn stop_slot(&mut self, idx: usize)
    {
        let slot = self.voices[idx].stream_slot;

        if slot != 255
        {
            if let Some(mixer) = &self.mixer
            {
                mixer.retire_stream(slot as usize);
            }
        }

        self.voices[idx] = LogicalVoice::default();
    }

    fn stop_voices(&mut self, entity: EntityHandle, def_hash: u32, sound_hash: u32)
    {
        let mut idx = 0;

        while idx < VOICES
        {
            if self.voices[idx].active && voice_match(&self.voices[idx], entity, def_hash, sound_hash)
            {
                self.stop_slot(idx);
            }

            idx += 1;
        }
    }

    fn follow(&mut self, entities: &EntityList)
    {
        let mut idx = 0;

        while idx < VOICES
        {
            let entity = self.voices[idx].entity;

            if self.voices[idx].active && !entity.is_null()
            {
                if let Some(found) = entities.get(entity)
                {
                    self.voices[idx].position = found.base().position;
                }
            }

            idx += 1;
        }
    }

    fn occlusion(&mut self, dt: f32, origin: Vector3, brushes: &BrushMap, voxels: &VoxelWorld)
    {
        let mut traced = 0;
        let mut idx = self.trace_cursor % VOICES;
        let start = idx;

        loop
        {
            if traced >= 8
            {
                break;
            }

            if self.voices[idx].active && self.voices[idx].positional
            {
                let blocked = trace_blocked(origin, self.voices[idx].position, brushes, voxels);
                self.voices[idx].occlusion_target = if blocked { 1.0 } else { 0.0 };
                traced += 1;
            }

            idx = (idx + 1) % VOICES;

            if idx == start
            {
                break;
            }
        }

        self.trace_cursor = idx;
        let blend = (dt / 0.1).clamp(0.0, 1.0);
        idx = 0;

        while idx < VOICES
        {
            if self.voices[idx].active
            {
                let target = self.voices[idx].occlusion_target;
                self.voices[idx].occlusion += (target - self.voices[idx].occlusion) * blend;
            }

            idx += 1;
        }
    }

    fn scapes(&mut self, dt: f32, origin: Vector3)
    {
        let picked = self.pick_scape(origin);

        if picked != self.active_scape
        {
            self.active_scape = picked;
            self.room = self.scape_room(picked);
            self.fade_beds();

            if picked != 0
            {
                self.start_beds(picked);
            }
        }

        let blend = (dt / 1.0).clamp(0.0, 1.0);
        let mut idx = 0;

        while idx < VOICES
        {
            if self.voices[idx].active && self.voices[idx].scape
            {
                let target = self.voices[idx].fade_target;
                self.voices[idx].fade += (target - self.voices[idx].fade) * blend;

                if self.voices[idx].fade <= 0.001 && self.voices[idx].fade_target <= 0.0
                {
                    self.stop_slot(idx);
                }
            }

            idx += 1;
        }
    }

    fn pick_scape(&self, origin: Vector3) -> u32
    {
        let mut idx = 0;

        while idx < self.catalog.boxes.len()
        {
            let box_ = &self.catalog.boxes[idx];

            if inside(origin, box_.min, box_.max)
            {
                return box_.scape;
            }

            idx += 1;
        }

        return 0;
    }

    fn scape_room(&self, scape: u32) -> Room
    {
        self.catalog.scapes.get(&scape).map(|item| item.room).unwrap_or(Room::None)
    }

    fn fade_beds(&mut self)
    {
        let mut idx = 0;

        while idx < VOICES
        {
            if self.voices[idx].active && self.voices[idx].scape
            {
                self.voices[idx].fade_target = 0.0;
            }

            idx += 1;
        }
    }

    fn start_beds(&mut self, scape: u32)
    {
        let Some(sounds) = self.catalog.scapes.get(&scape).map(|item| item.sounds.clone()) else
        {
            return;
        };
        let mut idx = 0;

        while idx < sounds.len()
        {
            let def_hash = sounds[idx];
            let Some(def) = self.catalog.defs.get(&def_hash).cloned() else
            {
                idx += 1;

                continue;
            };

            if def.waves.is_empty()
            {
                idx += 1;

                continue;
            }

            let wave = def.waves[(roll(self.sim_tick, scape, def_hash, 1) as usize) % def.waves.len()];
            self.play_local(Start {
                sound_hash: wave,
                def_hash,
                entity: EntityHandle::NULL,
                position: Vector3::new(0.0, 0.0, 0.0),
                volume: def.volume_max,
                pitch: def.pitch_max,
                tick: 0,
                channel: Channel::Static,
                level: def.level,
                bus: Bus::Music,
                looping: true,
                positional: false,
                scape: true,
                fade: 0.0,
                stream: def.stream,
            });
            idx += 1;
        }
    }

    fn reclaim(&mut self, dt: f32)
    {
        let live = self.mixer.as_ref().map(|mixer| mixer.live).unwrap_or(false);
        let mut stop = Vec::new();
        let mut idx = 0;

        while idx < VOICES
        {
            if !self.voices[idx].active
            {
                idx += 1;

                continue;
            }

            if live
            {
                let generation = self.voices[idx].generation;

                if self.mixer.as_ref().map(|mixer| mixer.finished(idx, generation)).unwrap_or(false)
                {
                    stop.push(idx);
                }
            }
            else if !self.voices[idx].looping && self.voices[idx].stream_slot == 255 && self.voices[idx].frames > 0
            {
                self.voices[idx].age += dt;
                let rate = (self.voices[idx].pitch / 100.0).clamp(0.01, 4.0);
                let seconds = self.voices[idx].frames as f32 / clip::SAMPLE_RATE as f32 / rate;

                if self.voices[idx].age >= seconds
                {
                    stop.push(idx);
                }
            }

            idx += 1;
        }

        idx = 0;

        while idx < stop.len()
        {
            self.stop_slot(stop[idx]);
            idx += 1;
        }
    }

    fn smooth_room(&mut self, dt: f32)
    {
        let (wet, feedback, damp, scale) = self.room.target();
        let blend = (dt / 0.2).clamp(0.0, 1.0);
        self.wet += (wet - self.wet) * blend;
        self.feedback += (feedback - self.feedback) * blend;
        self.damp += (damp - self.damp) * blend;
        self.scale += (scale - self.scale) * blend;
    }

    fn snapshot(&mut self, origin: Vector3, yaw: f32, pitch: f32, buses: Buses, max_distance: f32) -> MixSnapshot
    {
        let right = listener_right(yaw, pitch);
        let mut snap = MixSnapshot::default();
        snap.wet = self.wet;
        snap.feedback = self.feedback;
        snap.damp = self.damp;
        snap.scale = self.scale;
        let mut idx = 0;

        while idx < VOICES
        {
            if !self.voices[idx].active
            {
                idx += 1;

                continue;
            }

            let voice = &mut self.voices[idx];
            let bus_gain = bus_volume(voice.bus, &buses) * buses.master;
            let (gain_l, gain_r, lowpass, reverb, score) = place_voice(voice, origin, right, bus_gain, max_distance);
            voice.score = score;
            snap.voices[idx] = VoiceMix {
                generation: voice.generation,
                sample_ptr: voice.sample_ptr,
                frames: voice.frames,
                channels: voice.channels,
                stream_slot: voice.stream_slot,
                looping: if voice.looping { 1 } else { 0 },
                gain_l,
                gain_r,
                pitch: (voice.pitch / 100.0).clamp(0.01, 4.0),
                lowpass,
                reverb,
            };
            idx += 1;
        }

        return snap;
    }

    fn bump(&mut self) -> u32
    {
        self.next_gen = self.next_gen.wrapping_add(1);

        if self.next_gen == 0
        {
            self.next_gen = 1;
        }

        return self.next_gen;
    }

    fn remember(&mut self, tick: u64, entity: u32, def_hash: u32, sound_hash: u32)
    {
        self.dedup.push_back((tick, entity, def_hash, sound_hash));

        while self.dedup.len() > 128
        {
            self.dedup.pop_front();
        }
    }

    fn duplicate(&self, tick: u64, entity: u32, def_hash: u32, sound_hash: u32) -> bool
    {
        let mut idx = 0;

        while idx < self.dedup.len()
        {
            if self.dedup[idx] == (tick, entity, def_hash, sound_hash)
            {
                return true;
            }

            idx += 1;
        }

        return false;
    }
}

impl Start
{
    fn to_net(&self) -> NetPlay
    {
        NetPlay {
            sound_hash: self.sound_hash,
            def_hash: self.def_hash,
            entity: self.entity,
            position: self.position,
            volume: self.volume,
            pitch: self.pitch,
            tick: self.tick,
            looping: self.looping,
            positional: self.positional,
        }
    }
}

impl NetPlay
{
    fn clone_play(&self) -> Self
    {
        Self {
            sound_hash: self.sound_hash,
            def_hash: self.def_hash,
            entity: self.entity,
            position: self.position,
            volume: self.volume,
            pitch: self.pitch,
            tick: self.tick,
            looping: self.looping,
            positional: self.positional,
        }
    }
}

fn pcm_ptr(pcm: &Pcm) -> usize
{
    pcm.samples.as_ptr() as usize
}

fn incoming_score(start: &Start) -> f32
{
    let mut score = start.volume.max(0.0);

    if start.bus == Bus::Ui
    {
        score += 8.0;
    }

    if start.scape
    {
        score *= 0.2;
    }
    else if start.looping
    {
        score *= 0.5;
    }

    return score;
}

fn bus_volume(bus: Bus, buses: &Buses) -> f32
{
    match bus
    {
        Bus::Sfx => buses.sfx,
        Bus::Music => buses.music,
        Bus::Ui => buses.ui,
        Bus::Voice => buses.voice,
    }
}

fn place_voice(
    voice: &LogicalVoice,
    origin: Vector3,
    right: Vector3,
    bus_gain: f32,
    max_distance: f32,
) -> (f32, f32, f32, f32, f32)
{
    let fade = voice.fade.clamp(0.0, 1.0);
    let loud = voice.volume * fade * bus_gain;

    if !voice.positional
    {
        let gain = if voice.channels <= 1 { loud * CENTER } else { loud };
        let score = incoming_voice_score(voice, 1.0);

        return (gain, gain, 1.0, 0.0, score);
    }

    let to = Vector3::new(
        voice.position.x - origin.x,
        voice.position.y - origin.y,
        voice.position.z - origin.z,
    );
    let dist = to.len() as f32;
    let gain = distance_gain(dist, voice.level, max_distance);
    let blocked = 1.0 - voice.occlusion.clamp(0.0, 1.0) * 0.8;
    let (pan_l, pan_r) = pan(to, right);
    let level = loud * gain * blocked;
    let lowpass = lowpass_coeff(dist, voice.level, max_distance, voice.occlusion);
    let reverb = if voice.bus == Bus::Ui || voice.scape { 0.0 } else { level * 0.35 };
    let score = incoming_voice_score(voice, gain * blocked);

    return (level * pan_l, level * pan_r, lowpass, reverb, score);
}

fn incoming_voice_score(voice: &LogicalVoice, spatial: f32) -> f32
{
    let mut score = voice.volume.max(0.0) * spatial.max(0.0);

    if voice.bus == Bus::Ui
    {
        score += 8.0;
    }

    if voice.scape
    {
        score *= 0.2;
    }
    else if voice.looping
    {
        score *= 0.5;
    }

    return score;
}

fn pan(to: Vector3, right: Vector3) -> (f32, f32)
{
    let dist = to.len();

    if dist < 0.0001
    {
        return (CENTER, CENTER);
    }

    let inv = 1.0 / dist;
    let dir = Vector3::new(to.x * inv, to.y * inv, to.z * inv);
    let side = dir.dot(right).clamp(-1.0, 1.0) as f32;
    let angle = (side + 1.0) * std::f32::consts::FRAC_PI_4;

    return (angle.cos(), angle.sin());
}

fn listener_right(yaw: f32, pitch: f32) -> Vector3
{
    let fx = pitch.cos() * yaw.cos();
    let fy = pitch.cos() * yaw.sin();
    let len = (fx * fx + fy * fy).sqrt();

    if len <= 0.0001
    {
        return Vector3::new(0.0, 1.0, 0.0);
    }

    return Vector3::new((fy / len) as f64, (-fx / len) as f64, 0.0);
}

fn inside(point: Vector3, min: Vector3, max: Vector3) -> bool
{
    point.x >= min.x
        && point.y >= min.y
        && point.z >= min.z
        && point.x <= max.x
        && point.y <= max.y
        && point.z <= max.z
}

fn lerp(min: f32, max: f32, t: f32) -> f32
{
    min + (max - min) * t.clamp(0.0, 1.0)
}

fn unit(hash: u32) -> f32
{
    (hash % 10000) as f32 / 9999.0
}

fn stop_match(play: &NetPlay, entity: EntityHandle, def_hash: u32, sound_hash: u32) -> bool
{
    if !entity.is_null() && play.entity != entity
    {
        return false;
    }

    if def_hash == 0 && sound_hash == 0
    {
        return !entity.is_null();
    }

    if def_hash != 0 && play.def_hash == def_hash
    {
        return true;
    }

    if sound_hash != 0 && play.sound_hash == sound_hash
    {
        return true;
    }

    return false;
}

fn voice_match(voice: &LogicalVoice, entity: EntityHandle, def_hash: u32, sound_hash: u32) -> bool
{
    if !entity.is_null() && voice.entity != entity
    {
        return false;
    }

    if def_hash == 0 && sound_hash == 0
    {
        return !entity.is_null();
    }

    if def_hash != 0 && voice.def_hash == def_hash
    {
        return true;
    }

    if sound_hash != 0 && voice.sound_hash == sound_hash
    {
        return true;
    }

    return false;
}

fn trace_blocked(start: Vector3, end: Vector3, brushes: &BrushMap, voxels: &VoxelWorld) -> bool
{
    let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
    let dist = delta.len();

    if dist < 0.2
    {
        return false;
    }

    if let Some(hit) = brushes.trace(start, end)
    {
        if hit.distance > 0.05 && hit.distance < dist - 0.05
        {
            return true;
        }
    }

    if let Some(hit) = voxels.trace(start, end)
    {
        if hit.distance > 0.05 && hit.distance < dist - 0.05
        {
            return true;
        }
    }

    return false;
}

pub fn world<'a>(access: &AtomicPtr<SoundWorld>) -> Option<&'a mut SoundWorld>
{
    let ptr = access.load(Ordering::Relaxed);

    return unsafe { ptr.as_mut() };
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn pcm_world() -> SoundWorld
    {
        let mut world = SoundWorld::new(Realm::Client, false);
        let pcm = Pcm {
            samples: Arc::from(vec![1000i16; 32].into_boxed_slice()),
            frames: 32,
            channels: 1,
        };
        world.catalog.clips.insert(sound_hash("sound/test.wav"), ClipBody::Pcm(pcm));
        world.catalog.paths.insert(sound_hash("sound/test.wav"), "sound/test.wav".to_string());
        world.add_def(
            "test.weapon",
            "weapon",
            75.0,
            1.0,
            1.0,
            100.0,
            100.0,
            &["sound/test.wav".to_string()],
            "sfx",
            false,
            false,
        );
        world.add_def(
            "test.static",
            "static",
            75.0,
            1.0,
            1.0,
            100.0,
            100.0,
            &["sound/test.wav".to_string()],
            "sfx",
            false,
            false,
        );

        return world;
    }

    fn active(world: &SoundWorld) -> usize
    {
        let mut count = 0;
        let mut idx = 0;

        while idx < VOICES
        {
            if world.voices[idx].active
            {
                count += 1;
            }

            idx += 1;
        }

        return count;
    }

    #[test]
    fn weapon_channel_replaces_the_previous_voice()
    {
        let mut world = pcm_world();
        let entity = EntityHandle::new(4, 1);
        world.play("test.weapon", Some(Vector3::new(1.0, 0.0, 0.0)), -1.0, -1.0, entity, "");
        world.play("test.weapon", Some(Vector3::new(2.0, 0.0, 0.0)), -1.0, -1.0, entity, "");

        assert_eq!(active(&world), 1);
        assert_eq!(world.voices.iter().find(|voice| voice.active).unwrap().position.x, 2.0);
    }

    #[test]
    fn static_channel_keeps_both_voices()
    {
        let mut world = pcm_world();
        let entity = EntityHandle::new(4, 1);
        world.play("test.static", Some(Vector3::new(0.0, 0.0, 0.0)), -1.0, -1.0, entity, "");
        world.play("test.static", Some(Vector3::new(1.0, 0.0, 0.0)), -1.0, -1.0, entity, "");

        assert_eq!(active(&world), 2);
    }

    #[test]
    fn predicted_echo_is_dropped()
    {
        let mut world = pcm_world();
        world.remember(9, 4, sound_hash("test.weapon"), sound_hash("sound/test.wav"));

        assert!(world.duplicate(9, 4, sound_hash("test.weapon"), sound_hash("sound/test.wav")));
        assert!(!world.duplicate(10, 4, sound_hash("test.weapon"), sound_hash("sound/test.wav")));
    }

    #[test]
    fn hash_index_uses_the_path()
    {
        let mut world = SoundWorld::new(Realm::Server, false);
        let hash = sound_hash("sound/a.wav");
        world.catalog.paths.insert(hash, "sound/a.wav".to_string());

        assert_eq!(world.catalog.paths.get(&hash).map(String::as_str), Some("sound/a.wav"));
        assert_ne!(roll(1, 2, 3, 4), roll(2, 2, 3, 4));
        assert_eq!(roll(1, 2, 3, 4), roll(1, 2, 3, 4));
    }

    #[test]
    fn distance_and_occlusion_change_the_filter()
    {
        assert!((distance_gain(0.5, 75.0, 48.0) - 1.0).abs() < 0.001);
        assert_eq!(distance_gain(48.0, 75.0, 48.0), 0.0);
        assert!(distance_gain(10.0, 75.0, 48.0) > 0.0);
        assert!(lowpass_coeff(10.0, 75.0, 48.0, 1.0) < lowpass_coeff(10.0, 75.0, 48.0, 0.0));
    }
}
