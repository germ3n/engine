use crate::entities::{EntityHandle, Player};
use crate::movement::UserCommand;
use crate::network::events::{
    EntityModel, EntityNetworked, EntityOwnership, EntitySnapshot, LoopingSound, ServerToClient,
};
use crate::r#enum::InputButtons;
use crate::script::Realm;
use crate::state::GameState;
use crate::world::{BrushEdit, ChunkUpdate};
use std::cell::Cell;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use wincode::{SchemaRead, SchemaWrite};

pub const DEMO_MAGIC: &[u8; 4] = b"RDEM";
pub const DEMO_VERSION: u32 = 1;
pub const KIND_CLIENT: u8 = 1;
pub const KIND_SERVER: u8 = 2;
pub const SHOT_INTERVAL: f64 = 2.0;

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub struct DemoHeader {
    pub kind: u8,
    pub map_name: String,
    pub tickrate: u32,
    pub map_scale: f64,
    pub voxel_scale: f64,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub struct DemoPlayer {
    pub slot: u16,
    pub handle: EntityHandle,
    pub name: String,
    pub buttons: InputButtons,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub struct WorldShot {
    pub tick: u64,
    pub local: EntityHandle,
    pub players: Vec<DemoPlayer>,
    pub entities: Vec<EntitySnapshot>,
    pub networked: Vec<EntityNetworked>,
    pub owners: Vec<EntityOwnership>,
    pub models: Vec<EntityModel>,
    pub bones: Vec<crate::network::events::EntityBones>,
    pub voxels: Vec<ChunkUpdate>,
    pub voxel_scale: f64,
    pub brush_edits: Vec<BrushEdit>,
    pub brush_scale: f64,
    pub sounds: Vec<LoopingSound>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Copy, Debug)]
pub struct SlotInput {
    pub slot: u16,
    pub command: UserCommand,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum SlotEvent {
    UserMessage {
        slot: u16,
        hash: u32,
        data: Vec<u8>,
    },
    ScaleMaps {
        ratio: f64,
    },
    Join {
        slot: u16,
        handle: EntityHandle,
        name: String,
    },
    Leave {
        slot: u16,
    },
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum DemoFrame {
    ClientMsg {
        tick: u64,
        msg: ServerToClient,
    },
    LocalCmd(UserCommand),
    Keyframe(WorldShot),
    ServerTick {
        tick: u64,
        inputs: Vec<SlotInput>,
        events: Vec<SlotEvent>,
    },
    Checkpoint(WorldShot),
    NetVars {
        tick: u64,
        entities: Vec<EntityNetworked>,
    },
    Entities {
        tick: u64,
        entities: Vec<EntitySnapshot>,
    },
}

impl DemoFrame {
    pub fn tick(&self) -> u64 {
        match self {
            DemoFrame::ClientMsg { tick, .. } => *tick,
            DemoFrame::LocalCmd(cmd) => cmd.tick,
            DemoFrame::Keyframe(shot) => shot.tick,
            DemoFrame::ServerTick { tick, .. } => *tick,
            DemoFrame::Checkpoint(shot) => shot.tick,
            DemoFrame::NetVars { tick, .. } => *tick,
            DemoFrame::Entities { tick, .. } => *tick,
        }
    }

    pub fn shot(&self) -> Option<&WorldShot> {
        match self {
            DemoFrame::Keyframe(shot) | DemoFrame::Checkpoint(shot) => Some(shot),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IndexEntry {
    pub tick: u64,
    pub offset: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CamMode {
    First,
    Chase,
    Orbit,
    Free,
}

#[derive(Clone, Debug)]
pub enum DemoCommand {
    Record { name: String },
    Stop,
    Play { name: String },
    Pause,
    Timescale { scale: f64 },
    Seek { tick: u64 },
    Loop { enabled: bool },
    Cam { mode: CamMode },
    View { index: usize },
}

struct Queued {
    realm: Realm,
    command: DemoCommand,
}

thread_local! {
    static REALM: Cell<Option<Realm>> = const { Cell::new(None) };
}

static QUEUE: Mutex<Vec<Queued>> = Mutex::new(Vec::new());

pub fn bind_realm(realm: Realm) {
    set_realm(Some(realm));
}

pub fn realm() -> Option<Realm> {
    REALM.with(|cell| cell.get())
}

pub fn set_realm(realm: Option<Realm>) {
    REALM.with(|cell| cell.set(realm));
}

pub fn request(command: DemoCommand) -> Result<(), String> {
    let realm = REALM
        .with(|cell| cell.get())
        .ok_or("demo command has no realm")?;
    let playback = matches!(
        command,
        DemoCommand::Play { .. }
            | DemoCommand::Pause
            | DemoCommand::Timescale { .. }
            | DemoCommand::Seek { .. }
            | DemoCommand::Loop { .. }
            | DemoCommand::Cam { .. }
            | DemoCommand::View { .. }
    );

    if playback && matches!(realm, Realm::Server) {
        return Err("demo playback is client only".to_string());
    }

    QUEUE
        .lock()
        .map_err(|_| "demo queue poisoned".to_string())?
        .push(Queued { realm, command });

    Ok(())
}

pub fn drain(realm: Realm) -> Vec<DemoCommand> {
    let Ok(mut queue) = QUEUE.lock() else {
        return Vec::new();
    };
    let mut kept = Vec::new();
    let mut mine = Vec::new();
    let mut idx = 0;

    while idx < queue.len() {
        if queue[idx].realm == realm {
            mine.push(queue[idx].command.clone());
        } else {
            kept.push(Queued {
                realm: queue[idx].realm,
                command: queue[idx].command.clone(),
            });
        }

        idx += 1;
    }

    *queue = kept;

    mine
}

pub fn console_line(tokens: &[String]) -> Result<(), String> {
    if tokens.is_empty() {
        return Err("empty demo command".to_string());
    }

    match tokens[0].as_str() {
        "record" => {
            if tokens.len() != 2 {
                return Err("usage: record <name>".to_string());
            }

            request(DemoCommand::Record {
                name: tokens[1].clone(),
            })
        }
        "stop" => {
            if tokens.len() != 1 {
                return Err("usage: stop".to_string());
            }

            request(DemoCommand::Stop)
        }
        "playdemo" => {
            if tokens.len() != 2 {
                return Err("usage: playdemo <name>".to_string());
            }

            request(DemoCommand::Play {
                name: tokens[1].clone(),
            })
        }
        "demo_pause" => {
            if tokens.len() != 1 {
                return Err("usage: demo_pause".to_string());
            }

            request(DemoCommand::Pause)
        }
        "demo_timescale" => {
            if tokens.len() != 2 {
                return Err("usage: demo_timescale <scale>".to_string());
            }

            let scale = tokens[1]
                .parse::<f64>()
                .map_err(|_| "demo_timescale needs a number".to_string())?;

            if !scale.is_finite() || scale < 0.0 {
                return Err("demo_timescale needs a non-negative number".to_string());
            }

            request(DemoCommand::Timescale { scale })
        }
        "demo_seek" => {
            if tokens.len() != 2 {
                return Err("usage: demo_seek <tick>".to_string());
            }

            let tick = tokens[1]
                .parse::<u64>()
                .map_err(|_| "demo_seek needs a tick".to_string())?;

            request(DemoCommand::Seek { tick })
        }
        "demo_loop" => {
            if tokens.len() != 2 {
                return Err("usage: demo_loop <0|1>".to_string());
            }

            let enabled = match tokens[1].as_str() {
                "0" => false,
                "1" => true,
                _ => return Err("usage: demo_loop <0|1>".to_string()),
            };

            request(DemoCommand::Loop { enabled })
        }
        "demo_cam" => {
            if tokens.len() != 2 {
                return Err("usage: demo_cam <first|chase|orbit|free>".to_string());
            }

            let mode = parse_cam(&tokens[1])?;

            request(DemoCommand::Cam { mode })
        }
        "demo_view" => {
            if tokens.len() != 2 {
                return Err("usage: demo_view <index>".to_string());
            }

            let index = tokens[1]
                .parse::<usize>()
                .map_err(|_| "demo_view needs an index".to_string())?;

            request(DemoCommand::View { index })
        }
        other => Err(format!("unknown command '{other}'")),
    }
}

pub fn parse_cam(name: &str) -> Result<CamMode, String> {
    match name {
        "first" => Ok(CamMode::First),
        "chase" => Ok(CamMode::Chase),
        "orbit" => Ok(CamMode::Orbit),
        "free" => Ok(CamMode::Free),
        _ => Err("usage: demo_cam <first|chase|orbit|free>".to_string()),
    }
}

pub fn warn_header(
    header: &DemoHeader,
    map_name: &str,
    tickrate: u32,
    map_scale: f64,
    voxel_scale: f64,
) {
    if header.map_name != map_name {
        log::warn!("[demo] map is {map_name}, demo has {}", header.map_name);
    }

    if header.tickrate != tickrate {
        log::warn!(
            "[demo] tickrate is {tickrate}, demo has {}",
            header.tickrate
        );
    }

    if (header.map_scale - map_scale).abs() > 1e-6 {
        log::warn!(
            "[demo] map scale is {map_scale}, demo has {}",
            header.map_scale
        );
    }

    if (header.voxel_scale - voxel_scale).abs() > 1e-6 {
        log::warn!(
            "[demo] voxel scale is {voxel_scale}, demo has {}",
            header.voxel_scale
        );
    }
}

fn sane_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("bad demo name".to_string());
    }

    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("bad demo name".to_string());
    }

    Ok(())
}

pub fn demo_paths(name: &str) -> Result<(PathBuf, PathBuf), String> {
    sane_name(name)?;
    let dir = PathBuf::from("demos");
    std::fs::create_dir_all(&dir).map_err(|err| format!("demos: {err}"))?;
    let file = dir.join(format!("{name}.dem"));
    let index = dir.join(format!("{name}.dem.idx"));

    Ok((file, index))
}

pub struct DemoWriter {
    file: File,
    index: File,
    offset: u64,
    pending_flush: u32,
}

impl DemoWriter {
    pub fn create(name: &str, header: &DemoHeader) -> Result<Self, String> {
        let (file, index) = demo_paths(name)?;

        Self::create_at(&file, &index, header)
    }

    pub fn create_at(path: &Path, index_path: &Path, header: &DemoHeader) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|err| format!("demo: {err}"))?;
            }
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(|err| format!("demo {}: {err}", path.display()))?;
        let index = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(index_path)
            .map_err(|err| format!("demo {}: {err}", index_path.display()))?;
        let offset = write_header(&mut file, header)?;

        Ok(Self {
            file,
            index,
            offset,
            pending_flush: 0,
        })
    }

    pub fn write_frame(&mut self, frame: &DemoFrame) -> Result<(), String> {
        let payload = wincode::serialize(frame).map_err(|err| format!("demo: {err}"))?;

        if payload.len() > u32::MAX as usize {
            return Err("demo frame too large".to_string());
        }

        let len = (payload.len() as u32).to_le_bytes();
        self.file
            .write_all(&len)
            .map_err(|err| format!("demo: {err}"))?;
        self.file
            .write_all(&payload)
            .map_err(|err| format!("demo: {err}"))?;
        self.offset += 4 + payload.len() as u64;
        self.pending_flush += 1;

        if self.pending_flush >= 30 {
            self.flush()?;
        }

        Ok(())
    }

    pub fn write_mark(&mut self, frame: &DemoFrame) -> Result<(), String> {
        let offset = self.offset;
        let tick = frame.tick();
        self.write_frame(frame)?;
        self.index
            .write_all(&tick.to_le_bytes())
            .map_err(|err| format!("demo: {err}"))?;
        self.index
            .write_all(&offset.to_le_bytes())
            .map_err(|err| format!("demo: {err}"))?;
        self.flush()?;

        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), String> {
        self.file.flush().map_err(|err| format!("demo: {err}"))?;
        self.index.flush().map_err(|err| format!("demo: {err}"))?;
        self.pending_flush = 0;

        Ok(())
    }
}

impl Drop for DemoWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

pub struct DemoReader {
    file: File,
    header: DemoHeader,
    index: Vec<IndexEntry>,
    ahead: Option<DemoFrame>,
}

impl DemoReader {
    pub fn open(name: &str) -> Result<Self, String> {
        let (file, index) = demo_paths(name)?;

        Self::open_at(&file, &index)
    }

    pub fn open_at(path: &Path, index_path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|err| format!("demo {}: {err}", path.display()))?;
        let header = read_header(&mut file)?;
        let index = read_index(index_path)?;

        Ok(Self {
            file,
            header,
            index,
            ahead: None,
        })
    }

    pub fn header(&self) -> &DemoHeader {
        &self.header
    }

    pub fn index(&self) -> &[IndexEntry] {
        &self.index
    }

    pub fn mark_for(&self, tick: u64) -> Option<IndexEntry> {
        if self.index.is_empty() {
            return None;
        }

        let mut lo = 0;
        let mut hi = self.index.len();

        while lo < hi {
            let mid = (lo + hi) / 2;

            if self.index[mid].tick <= tick {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        if lo == 0 {
            return Some(self.index[0]);
        }

        Some(self.index[lo - 1])
    }

    pub fn seek_to(&mut self, offset: u64) -> Result<(), String> {
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|err| format!("demo: {err}"))?;
        self.ahead = None;

        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<DemoFrame>, String> {
        if let Some(frame) = self.ahead.take() {
            return Ok(Some(frame));
        }

        read_frame(&mut self.file)
    }

    pub fn peek_tick(&mut self) -> Result<Option<u64>, String> {
        if self.ahead.is_none() {
            self.ahead = read_frame(&mut self.file)?;
        }

        Ok(self.ahead.as_ref().map(|frame| frame.tick()))
    }
}

fn write_header(file: &mut File, header: &DemoHeader) -> Result<u64, String> {
    let payload = wincode::serialize(header).map_err(|err| format!("demo: {err}"))?;

    if payload.len() > u32::MAX as usize {
        return Err("demo header too large".to_string());
    }

    let mut bytes = Vec::with_capacity(12 + payload.len());
    bytes.extend_from_slice(DEMO_MAGIC);
    bytes.extend_from_slice(&DEMO_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&payload);
    file.write_all(&bytes)
        .map_err(|err| format!("demo: {err}"))?;

    Ok(bytes.len() as u64)
}

fn read_header(file: &mut File) -> Result<DemoHeader, String> {
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .map_err(|err| format!("demo: {err}"))?;

    if &magic != DEMO_MAGIC {
        return Err("demo header is invalid".to_string());
    }

    let mut version_buf = [0u8; 4];
    file.read_exact(&mut version_buf)
        .map_err(|err| format!("demo: {err}"))?;
    let version = u32::from_le_bytes(version_buf);

    if version != DEMO_VERSION {
        return Err(format!("demo version {version} is not supported"));
    }

    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)
        .map_err(|err| format!("demo: {err}"))?;
    let len = u32::from_le_bytes(len_buf) as usize;

    if len > 1024 * 1024 {
        return Err("demo header too large".to_string());
    }

    let mut payload = vec![0u8; len];
    file.read_exact(&mut payload)
        .map_err(|err| format!("demo: {err}"))?;

    wincode::deserialize(&payload).map_err(|err| format!("demo: {err}"))
}

fn read_index(path: &Path) -> Result<Vec<IndexEntry>, String> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("demo {}: {err}", path.display())),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| format!("demo: {err}"))?;
    let mut index = Vec::new();
    let mut cursor = 0;

    while cursor + 16 <= bytes.len() {
        let mut tick_buf = [0u8; 8];
        let mut off_buf = [0u8; 8];
        tick_buf.copy_from_slice(&bytes[cursor..cursor + 8]);
        off_buf.copy_from_slice(&bytes[cursor + 8..cursor + 16]);
        index.push(IndexEntry {
            tick: u64::from_le_bytes(tick_buf),
            offset: u64::from_le_bytes(off_buf),
        });
        cursor += 16;
    }

    Ok(index)
}

fn read_frame(file: &mut File) -> Result<Option<DemoFrame>, String> {
    let mut len_buf = [0u8; 4];

    match file.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(format!("demo: {err}")),
    }

    let len = u32::from_le_bytes(len_buf) as usize;

    if len > 64 * 1024 * 1024 {
        return Err("demo frame too large".to_string());
    }

    let mut payload = vec![0u8; len];
    file.read_exact(&mut payload)
        .map_err(|err| format!("demo: {err}"))?;

    wincode::deserialize(&payload)
        .map(|frame| Some(frame))
        .map_err(|err| format!("demo: {err}"))
}

pub struct DemoSession {
    writer: DemoWriter,
    pub next_shot: f64,
    pub players: Vec<DemoPlayer>,
    pending: Vec<SlotEvent>,
    pub dead: bool,
}

impl DemoSession {
    pub fn create(name: &str, header: DemoHeader) -> Result<Self, String> {
        let writer = DemoWriter::create(name, &header)?;

        Ok(Self {
            writer,
            next_shot: 0.0,
            players: Vec::new(),
            pending: Vec::new(),
            dead: false,
        })
    }

    pub fn observe_client(&mut self, message: &ServerToClient) {
        match message {
            ServerToClient::PlayerConnected { handle, name } => {
                if self.players.iter().any(|player| player.handle == *handle) {
                    return;
                }

                let slot = self.players.len() as u16;
                self.players.push(DemoPlayer {
                    slot,
                    handle: *handle,
                    name: name.clone(),
                    buttons: InputButtons::NONE,
                });
            }
            ServerToClient::PlayerDisconnected { handle } => {
                self.players.retain(|player| player.handle != *handle);
            }
            _ => {}
        }
    }

    pub fn push_event(&mut self, event: SlotEvent) {
        self.pending.push(event);
    }

    pub fn take_events(&mut self) -> Vec<SlotEvent> {
        std::mem::take(&mut self.pending)
    }

    pub fn write_frame(&mut self, frame: &DemoFrame) -> bool {
        if self.dead {
            return false;
        }

        if let Err(err) = self.writer.write_frame(frame) {
            log::warn!("[demo] {err}");
            self.dead = true;

            return false;
        }

        true
    }

    pub fn write_mark(&mut self, frame: &DemoFrame) -> bool {
        if self.dead {
            return false;
        }

        if let Err(err) = self.writer.write_mark(frame) {
            log::warn!("[demo] {err}");
            self.dead = true;

            return false;
        }

        true
    }

    pub fn note_client(&mut self, tick: u64, message: &ServerToClient) -> bool {
        if matches!(message, ServerToClient::Pong { .. }) {
            return true;
        }

        self.observe_client(message);
        self.write_frame(&DemoFrame::ClientMsg {
            tick,
            msg: message.clone(),
        })
    }
}

pub fn remember_players<In, Out>(players: &mut Vec<DemoPlayer>, game: &GameState<In, Out>) {
    for (handle, entity) in game.entities.iter() {
        if !entity.is_spawned() || entity.class_hash() != Player::CLASS_HASH {
            continue;
        }

        if players.iter().any(|player| player.handle == handle) {
            continue;
        }

        let slot = players.len() as u16;
        players.push(DemoPlayer {
            slot,
            handle,
            name: String::new(),
            buttons: InputButtons::NONE,
        });
    }
}

pub fn capture_world<In, Out>(
    game: &mut GameState<In, Out>,
    tick: u64,
    local: EntityHandle,
    players: &[DemoPlayer],
) -> WorldShot {
    let networked = game.networked_state(None);
    let mut entities = Vec::new();
    let mut owners = Vec::new();
    let mut models = Vec::new();
    let mut bones = Vec::new();

    for (handle, entity) in game.entities.iter() {
        if !entity.is_spawned() {
            continue;
        }

        let base = entity.base();
        let model = game.anims.model_paths(&base.anim);
        entities.push(EntitySnapshot {
            handle,
            class_hash: entity.class_hash(),
            health: entity.net_health(),
            position: base.position,
            angles: base.angles,
            velocity: base.velocity,
            ack: 0,
            anim: base.anim.snapshot(),
        });

        if !base.owner.is_null() {
            owners.push(EntityOwnership {
                handle,
                owner: base.owner,
            });
        }

        if let Some((mesh, clips)) = model {
            models.push(EntityModel {
                handle,
                mesh,
                clips,
            });
        }

        let entity_bones = game.anims.entity_bones(handle.0);

        if !entity_bones.bones.is_empty() {
            bones.push(entity_bones);
        }
    }

    let pending = game.sound.baseline();
    let mut sounds = Vec::new();
    let mut idx = 0;

    while idx < pending.len() {
        let play = &pending[idx];
        sounds.push(LoopingSound {
            sound_hash: play.sound_hash,
            def_hash: play.def_hash,
            entity_handle: play.entity,
            position: play.position,
            volume: play.volume,
            pitch: play.pitch,
            positional: play.positional,
        });
        idx += 1;
    }

    WorldShot {
        tick,
        local,
        players: players.to_vec(),
        entities,
        networked,
        owners,
        models,
        bones,
        voxels: game.voxel_world.baseline(),
        voxel_scale: game.voxel_world.scale(),
        brush_edits: game.brush_world.edits().to_vec(),
        brush_scale: game.brush_world.scale(),
        sounds,
    }
}

pub fn capture_props<In, Out>(game: &GameState<In, Out>) -> Vec<EntitySnapshot> {
    let mut entities = Vec::new();

    for (handle, entity) in game.entities.iter() {
        if !entity.is_spawned() || entity.class_hash() == Player::CLASS_HASH {
            continue;
        }

        let base = entity.base();
        entities.push(EntitySnapshot {
            handle,
            class_hash: entity.class_hash(),
            health: entity.net_health(),
            position: base.position,
            angles: base.angles,
            velocity: base.velocity,
            ack: 0,
            anim: base.anim.snapshot(),
        });
    }

    entities
}

pub struct DemoPlay {
    pub reader: DemoReader,
    pub paused: bool,
    pub timescale: f64,
    pub loop_demo: bool,
    pub cam: CamMode,
    pub view_index: usize,
    pub tick: u64,
    pub acc: f64,
    pub players: Vec<DemoPlayer>,
    pub slots: Vec<SlotLink>,
    pub orbit_yaw: f32,
    pub orbit_pitch: f32,
    pub orbit_dist: f64,
    pub watched: EntityHandle,
}

#[derive(Clone, Copy, Debug)]
pub struct SlotLink {
    pub slot: u16,
    pub handle: EntityHandle,
    pub buttons: InputButtons,
}

impl DemoPlay {
    pub fn open(name: &str) -> Result<Self, String> {
        let reader = DemoReader::open(name)?;

        Ok(Self::from_reader(reader))
    }

    pub fn from_reader(reader: DemoReader) -> Self {
        Self {
            reader,
            paused: false,
            timescale: 1.0,
            loop_demo: false,
            cam: CamMode::First,
            view_index: 0,
            tick: 0,
            acc: 0.0,
            players: Vec::new(),
            slots: Vec::new(),
            orbit_yaw: 0.0,
            orbit_pitch: 0.25,
            orbit_dist: 3.2,
            watched: EntityHandle::NULL,
        }
    }

    pub fn kind(&self) -> u8 {
        self.reader.header().kind
    }

    pub fn sync_watched(&mut self) {
        if self.players.is_empty() {
            return;
        }

        if self.view_index >= self.players.len() {
            self.view_index = self.players.len() - 1;
        }

        self.watched = self.players[self.view_index].handle;
    }

    pub fn adopt_players(&mut self, players: &[DemoPlayer], prefer: EntityHandle) {
        self.players = players.to_vec();
        self.slots.clear();
        let mut idx = 0;

        while idx < players.len() {
            self.slots.push(SlotLink {
                slot: players[idx].slot,
                handle: players[idx].handle,
                buttons: players[idx].buttons,
            });
            idx += 1;
        }

        if let Some(found) = self
            .players
            .iter()
            .position(|player| player.handle == prefer)
        {
            self.view_index = found;
        }

        self.sync_watched();
    }

    pub fn slot_link(&mut self, slot: u16) -> Option<&mut SlotLink> {
        let mut idx = 0;

        while idx < self.slots.len() {
            if self.slots[idx].slot == slot {
                return Some(&mut self.slots[idx]);
            }

            idx += 1;
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movement;
    use crate::network::events::{AnimSnapshot, NetValue, NetVar};
    use crate::script::libs::angle3::Angle3;
    use crate::script::libs::vector3::Vector3;
    use crate::world::{BrushMap, VoxelWorld};

    fn header() -> DemoHeader {
        DemoHeader {
            kind: KIND_SERVER,
            map_name: "hall".to_string(),
            tickrate: 60,
            map_scale: 1.0,
            voxel_scale: 1.0,
        }
    }

    fn temp_pair(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("engine-demo-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        (
            dir.join(format!("{name}.dem")),
            dir.join(format!("{name}.idx")),
        )
    }

    fn shot(tick: u64, position: Vector3) -> WorldShot {
        WorldShot {
            tick,
            local: EntityHandle::NULL,
            players: Vec::new(),
            entities: vec![EntitySnapshot {
                handle: EntityHandle::new(1, 1),
                class_hash: Player::CLASS_HASH,
                health: 100,
                position,
                angles: Angle3::new(0.0, 0.0, 0.0),
                velocity: Vector3::new(0.0, 0.0, 0.0),
                ack: 0,
                anim: Default::default(),
            }],
            networked: Vec::new(),
            owners: Vec::new(),
            models: Vec::new(),
            bones: Vec::new(),
            voxels: Vec::new(),
            voxel_scale: 1.0,
            brush_edits: Vec::new(),
            brush_scale: 1.0,
            sounds: Vec::new(),
        }
    }

    fn netvar(index: u32, key: &str, value: i32) -> EntityNetworked {
        EntityNetworked {
            handle: EntityHandle::new(index, 1),
            vars: vec![NetVar {
                key: key.to_string(),
                value: NetValue::Int(value),
            }],
        }
    }

    fn command(tick: u64) -> UserCommand {
        UserCommand {
            tick,
            buttons: InputButtons::NONE,
            wish: Vector3::new(1.0, 0.0, 0.0),
            view: Angle3::new(0.0, 15.0, 0.0),
        }
    }

    fn step_cmd(
        position: &mut Vector3,
        velocity: &mut Vector3,
        angles: &mut Angle3,
        prev: &mut InputButtons,
        cmd: &UserCommand,
        brushes: &BrushMap,
        voxels: &VoxelWorld,
    ) {
        movement::step(
            position,
            velocity,
            angles,
            cmd,
            *prev,
            1.0 / 60.0,
            24.0,
            brushes,
            voxels,
            None,
        );
        *prev = cmd.buttons;
    }

    #[test]
    fn header_roundtrip() {
        let (file, index) = temp_pair("header");
        let written = header();
        DemoWriter::create_at(&file, &index, &written).unwrap();
        let reader = DemoReader::open_at(&file, &index).unwrap();
        let read = reader.header();
        assert_eq!(read.kind, KIND_SERVER);
        assert_eq!(read.map_name, "hall");
        assert_eq!(read.tickrate, 60);
        assert!((read.map_scale - 1.0).abs() < 1e-9);
        assert!((read.voxel_scale - 1.0).abs() < 1e-9);
    }

    #[test]
    fn newer_version_is_refused() {
        let (file, index) = temp_pair("version");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DEMO_MAGIC);
        bytes.extend_from_slice(&99u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(&file, bytes).unwrap();
        std::fs::write(&index, []).unwrap();
        let opened = DemoReader::open_at(&file, &index);
        assert!(opened.is_err());
        let err = match opened {
            Ok(_) => String::new(),
            Err(err) => err,
        };
        assert!(err.contains("not supported"));
    }

    #[test]
    fn seek_lands_on_the_late_mark() {
        let (file, index) = temp_pair("seek");
        let mut writer = DemoWriter::create_at(&file, &index, &header()).unwrap();
        writer
            .write_mark(&DemoFrame::Checkpoint(shot(0, Vector3::new(0.0, 0.0, 0.0))))
            .unwrap();
        writer
            .write_frame(&DemoFrame::ServerTick {
                tick: 5,
                inputs: Vec::new(),
                events: Vec::new(),
            })
            .unwrap();
        writer
            .write_mark(&DemoFrame::Checkpoint(shot(
                10,
                Vector3::new(4.0, 0.0, 1.0),
            )))
            .unwrap();
        writer
            .write_frame(&DemoFrame::ServerTick {
                tick: 15,
                inputs: Vec::new(),
                events: Vec::new(),
            })
            .unwrap();
        drop(writer);

        let mut reader = DemoReader::open_at(&file, &index).unwrap();
        let mark = reader.mark_for(12).unwrap();
        assert_eq!(mark.tick, 10);
        reader.seek_to(mark.offset).unwrap();
        let frame = reader.next_frame().unwrap().unwrap();
        let shot = frame.shot().unwrap();
        assert_eq!(shot.tick, 10);
        assert!((shot.entities[0].position.x - 4.0).abs() < 1e-9);
        let next = reader.next_frame().unwrap().unwrap();
        assert_eq!(next.tick(), 15);
    }

    #[test]
    fn checkpoint_resim_matches_straight_resim() {
        let (file, index) = temp_pair("resim");
        let brushes = BrushMap::new();
        let voxels = VoxelWorld::new();
        let mut position = Vector3::new(0.0, 0.0, 1.0);
        let mut velocity = Vector3::new(0.0, 0.0, 0.0);
        let mut angles = Angle3::new(0.0, 0.0, 0.0);
        let mut prev = InputButtons::NONE;
        let mut full_pos = position;
        let mut full_vel = velocity;
        let mut full_ang = angles;
        let mut full_prev = prev;
        let mut writer = DemoWriter::create_at(&file, &index, &header()).unwrap();
        writer
            .write_mark(&DemoFrame::Checkpoint(shot(0, position)))
            .unwrap();
        let mut idx = 0;

        while idx < 8 {
            let cmd = command(idx as u64 + 1);
            step_cmd(
                &mut position,
                &mut velocity,
                &mut angles,
                &mut prev,
                &cmd,
                &brushes,
                &voxels,
            );
            step_cmd(
                &mut full_pos,
                &mut full_vel,
                &mut full_ang,
                &mut full_prev,
                &cmd,
                &brushes,
                &voxels,
            );
            writer
                .write_frame(&DemoFrame::ServerTick {
                    tick: cmd.tick,
                    inputs: vec![SlotInput {
                        slot: 0,
                        command: cmd,
                    }],
                    events: Vec::new(),
                })
                .unwrap();

            if idx + 1 == 4 {
                let mut marked = shot(4, position);
                marked.entities[0].velocity = velocity;
                marked.entities[0].angles = angles;
                writer.write_mark(&DemoFrame::Checkpoint(marked)).unwrap();
            }

            idx += 1;
        }

        drop(writer);

        let mut reader = DemoReader::open_at(&file, &index).unwrap();
        let mark = reader.mark_for(8).unwrap();
        assert_eq!(mark.tick, 4);
        reader.seek_to(mark.offset).unwrap();
        let frame = reader.next_frame().unwrap().unwrap();
        let restored = frame.shot().unwrap();
        let mut replay_pos = restored.entities[0].position;
        let mut replay_vel = restored.entities[0].velocity;
        let mut replay_ang = restored.entities[0].angles;
        let mut replay_prev = InputButtons::NONE;

        loop {
            let Some(frame) = reader.next_frame().unwrap() else {
                break;
            };

            let DemoFrame::ServerTick { inputs, .. } = frame else {
                continue;
            };
            let mut cmd_idx = 0;

            while cmd_idx < inputs.len() {
                step_cmd(
                    &mut replay_pos,
                    &mut replay_vel,
                    &mut replay_ang,
                    &mut replay_prev,
                    &inputs[cmd_idx].command,
                    &brushes,
                    &voxels,
                );
                cmd_idx += 1;
            }
        }

        assert!((replay_pos.x - full_pos.x).abs() < 1e-4);
        assert!((replay_pos.y - full_pos.y).abs() < 1e-4);
        assert!((replay_pos.z - full_pos.z).abs() < 1e-4);
        assert!((replay_vel.x - full_vel.x).abs() < 1e-4);
        assert!((replay_vel.z - full_vel.z).abs() < 1e-4);
    }

    #[test]
    fn prop_snapshots_roundtrip() {
        let (file, index) = temp_pair("props");
        let entity = EntitySnapshot {
            handle: EntityHandle::new(3, 1),
            class_hash: 9,
            health: 40,
            position: Vector3::new(1.0, 2.0, 3.0),
            angles: Angle3::new(0.0, 90.0, 0.0),
            velocity: Vector3::new(0.5, 0.0, 0.0),
            ack: 0,
            anim: AnimSnapshot {
                sequence: 2,
                gesture: 4,
                sequence_tick: 8,
                gesture_tick: 9,
                sequence_rate: 1.0,
                gesture_rate: 1.0,
                gesture_weight: 1.0,
            },
        };
        let mut writer = DemoWriter::create_at(&file, &index, &header()).unwrap();
        writer
            .write_frame(&DemoFrame::Entities {
                tick: 6,
                entities: vec![entity.clone()],
            })
            .unwrap();
        drop(writer);

        let mut reader = DemoReader::open_at(&file, &index).unwrap();
        let frame = reader.next_frame().unwrap().unwrap();
        assert_eq!(frame.tick(), 6);
        let DemoFrame::Entities { entities, .. } = frame else {
            panic!("expected entity frame");
        };
        assert_eq!(entities.len(), 1);
        assert!((entities[0].position.x - 1.0).abs() < 1e-9);
        assert_eq!(entities[0].anim.sequence, 2);
        assert_eq!(entities[0].anim.gesture, 4);
    }

    #[test]
    fn netvar_updates_roundtrip() {
        let (file, index) = temp_pair("netvars");
        let update = netvar(4, "health", 40);
        let mut writer = DemoWriter::create_at(&file, &index, &header()).unwrap();
        writer
            .write_frame(&DemoFrame::NetVars {
                tick: 7,
                entities: vec![update.clone()],
            })
            .unwrap();
        drop(writer);

        let mut reader = DemoReader::open_at(&file, &index).unwrap();
        let frame = reader.next_frame().unwrap().unwrap();
        assert_eq!(frame.tick(), 7);
        let DemoFrame::NetVars { entities, .. } = frame else {
            panic!("expected netvar frame");
        };
        assert_eq!(entities, vec![update]);
    }

    #[test]
    fn client_recording_stores_networked_updates() {
        let (file, index) = temp_pair("client-netvars");
        let update = netvar(2, "clip", 12);
        let mut session = DemoSession {
            writer: DemoWriter::create_at(&file, &index, &header()).unwrap(),
            next_shot: 0.0,
            players: Vec::new(),
            pending: Vec::new(),
            dead: false,
        };
        assert!(session.note_client(
            3,
            &ServerToClient::NetworkedUpdate {
                entities: vec![update.clone()],
            },
        ));
        assert!(session.note_client(
            3,
            &ServerToClient::Pong {
                client_time: 1,
                server_time: 2,
            },
        ));
        assert!(!session.dead);
        drop(session);

        let mut reader = DemoReader::open_at(&file, &index).unwrap();
        let frame = reader.next_frame().unwrap().unwrap();
        assert_eq!(frame.tick(), 3);
        let DemoFrame::ClientMsg { msg, .. } = frame else {
            panic!("expected client message");
        };
        let ServerToClient::NetworkedUpdate { entities } = msg else {
            panic!("expected networked update");
        };
        assert_eq!(entities, vec![update]);
        assert!(reader.next_frame().unwrap().is_none());
    }

    #[test]
    fn netvar_updates_replay_after_seek() {
        let (file, index) = temp_pair("netvar-seek");
        let baseline = netvar(1, "ammo", 10);
        let delta = netvar(1, "ammo", 4);
        let mut marked = shot(0, Vector3::new(0.0, 0.0, 0.0));
        marked.networked = vec![baseline.clone()];
        let mut writer = DemoWriter::create_at(&file, &index, &header()).unwrap();
        writer.write_mark(&DemoFrame::Checkpoint(marked)).unwrap();
        writer
            .write_frame(&DemoFrame::ServerTick {
                tick: 6,
                inputs: Vec::new(),
                events: Vec::new(),
            })
            .unwrap();
        writer
            .write_frame(&DemoFrame::NetVars {
                tick: 6,
                entities: vec![delta.clone()],
            })
            .unwrap();
        writer
            .write_mark(&DemoFrame::Checkpoint(shot(
                20,
                Vector3::new(1.0, 0.0, 0.0),
            )))
            .unwrap();
        drop(writer);

        let mut reader = DemoReader::open_at(&file, &index).unwrap();
        assert_eq!(reader.index().len(), 2);
        let mark = reader.mark_for(6).unwrap();
        assert_eq!(mark.tick, 0);
        reader.seek_to(mark.offset).unwrap();
        let frame = reader.next_frame().unwrap().unwrap();
        let restored = frame.shot().unwrap();
        assert_eq!(restored.networked, vec![baseline]);
        let tick = reader.next_frame().unwrap().unwrap();
        assert_eq!(tick.tick(), 6);
        assert!(matches!(tick, DemoFrame::ServerTick { .. }));
        let update = reader.next_frame().unwrap().unwrap();
        assert_eq!(update.tick(), 6);
        let DemoFrame::NetVars { entities, .. } = update else {
            panic!("expected netvar frame");
        };
        assert_eq!(entities, vec![delta]);
    }
}
