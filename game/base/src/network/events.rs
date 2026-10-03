use crate::entities::EntityHandle;
use crate::r#enum::{EntityFlags, InputButtons};
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use crate::world::{BrushEdit, ChunkUpdate};
use std::net::SocketAddr;
use wincode::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead, Clone, Copy, Debug, PartialEq)]
pub struct AnimSnapshot {
    pub sequence: u16,
    pub gesture: u16,
    pub sequence_tick: u64,
    pub gesture_tick: u64,
    pub sequence_rate: f32,
    pub gesture_rate: f32,
    pub gesture_weight: f32,
}

impl Default for AnimSnapshot {
    fn default() -> Self {
        Self {
            sequence: u16::MAX,
            gesture: u16::MAX,
            sequence_tick: 0,
            gesture_tick: 0,
            sequence_rate: 1.0,
            gesture_rate: 1.0,
            gesture_weight: 1.0,
        }
    }
}

#[derive(SchemaWrite, SchemaRead, Clone, Copy, Debug)]
pub struct EntityAnimNet {
    pub handle: EntityHandle,
    pub anim: AnimSnapshot,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub struct EntityModel {
    pub handle: EntityHandle,
    pub mesh: String,
    pub clips: String,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub struct EntitySnapshot {
    pub handle: EntityHandle,
    pub class_hash: u32,
    pub health: i32,
    pub position: Vector3,
    pub angles: Angle3,
    pub velocity: Vector3,
    pub ack: u64,
    pub anim: AnimSnapshot,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub enum NetValue {
    Nil,
    Bool(bool),
    Int(i32),
    Float(f64),
    String(String),
    Vector3(Vector3),
    Angle3(Angle3),
    Entity(EntityHandle),
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub struct NetVar {
    pub key: String,
    pub value: NetValue,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub struct EntityNetworked {
    pub handle: EntityHandle,
    pub vars: Vec<NetVar>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Copy, Debug)]
pub struct EntityOwnership {
    pub handle: EntityHandle,
    pub owner: EntityHandle,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub struct LoopingSound {
    pub sound_hash: u32,
    pub def_hash: u32,
    pub entity_handle: EntityHandle,
    pub position: Vector3,
    pub volume: f32,
    pub pitch: f32,
    pub positional: bool,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum ServerToClient {
    MapChange {
        map_name: String,
    },
    GameStateChanged {
        state: u16,
    },
    ConVarReplicated {
        name: String,
        value: String,
    },

    EntitySpawned {
        handle: EntityHandle,
        class_hash: u32,
        position: Vector3,
        angles: Angle3,
        owner: EntityHandle,
        networked: Vec<NetVar>,
    },
    EntityDespawned {
        handle: EntityHandle,
    },
    EntityOwner {
        handle: EntityHandle,
        owner: EntityHandle,
    },
    NetworkedUpdate {
        entities: Vec<EntityNetworked>,
    },
    PredictedState {
        tick: u64,
        player: EntitySnapshot,
        entities: Vec<EntityNetworked>,
        anims: Vec<EntityAnimNet>,
    },
    EntityParented {
        handle: EntityHandle,
        parent_handle: EntityHandle,
        attachment_point: u16,
    },

    PlayerConnected {
        handle: EntityHandle,
        name: String,
    },
    PlayerDisconnected {
        handle: EntityHandle,
    },
    PlayerSpawned {
        handle: EntityHandle,
    },
    PlayerDamaged {
        handle: EntityHandle,
        attacker: EntityHandle,
        inflictor: EntityHandle,
        damage: u32,
        new_health: u32,
    },
    PlayerDied {
        handle: EntityHandle,
        killer: EntityHandle,
        inflictor: EntityHandle,
    },

    ModelChanged {
        handle: EntityHandle,
        model: String,
    },
    AnimModel {
        handle: EntityHandle,
        mesh: String,
        clips: String,
    },
    FlagsChanged {
        handle: EntityHandle,
        flags: EntityFlags,
    },
    TransformUpdated {
        handle: EntityHandle,
        position: Option<Vector3>,
        angles: Option<Angle3>,
        velocity: Option<Vector3>,
    },
    PlaySound {
        sound_hash: u32,
        entity_handle: Option<EntityHandle>,
        position: Vector3,
        volume: f32,
        pitch: f32,
        def_hash: u32,
        tick: u64,
        positional: bool,
    },
    StopSound {
        def_hash: u32,
        sound_hash: u32,
        entity_handle: EntityHandle,
    },
    SoundBaseline {
        sounds: Vec<LoopingSound>,
    },
    PlayEffect {
        effect_hash: u32,
        position: Vector3,
        normal: Vector3,
    },
    AnimationTriggered {
        handle: EntityHandle,
        sequence_id: u16,
        playback_rate: f32,
    },

    // SERVER<->CLIENT Events
    UserMessage {
        hash: u32,
        data: Vec<u8>,
    },
    ChatMessage {
        sender_handle: EntityHandle,
        team_only: bool,
        text: String,
    },
    VoiceChunk {
        sender_handle: EntityHandle,
        data: Vec<u8>,
    },

    Pong {
        client_time: u64,
        server_time: u64,
    },
    ServerTick {
        tick: u64,
    },

    WeaponFired {
        entity_handle: EntityHandle,
        weapon_handle: EntityHandle,
    },
    WeaponReloaded {
        entity_handle: EntityHandle,
    },
    ItemEquipped {
        entity_handle: EntityHandle,
        slot: u8,
        item_handle: EntityHandle,
    },

    WorldSnapshot {
        generation: u32,
        reset: bool,
        part: u16,
        parts: u16,
        entities: Vec<EntitySnapshot>,
        networked: Vec<EntityNetworked>,
        owners: Vec<EntityOwnership>,
        models: Vec<EntityModel>,
    },
    TickState {
        tick: u64,
        part: u16,
        parts: u16,
        entities: Vec<EntitySnapshot>,
    },
    VoxelScale {
        scale: f64,
    },
    VoxelChunk(ChunkUpdate),
    BrushScale {
        scale: f64,
    },
    BrushEdit(BrushEdit),
    WorldMotion {
        ratio: f64,
    },
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum ClientToServer {
    // SERVER<->CLIENT Events
    UserMessage {
        hash: u32,
        data: Vec<u8>,
    },
    ChatMessage {
        sender_handle: EntityHandle,
        team_only: bool,
        text: String,
    },
    VoiceChunk {
        sender_handle: EntityHandle,
        data: Vec<u8>,
    },

    Ping {
        client_time: u64,
    },
    ClientReady {
        tick: u64,
    },

    // CLIENT->SERVER Events
    PlayerInput {
        tick: u64,
        buttons: InputButtons,
        movement: Vector3,
        viewangles: Angle3,
    },
    ScaleMaps {
        ratio: f64,
    },
}

#[derive(Clone, Debug)]
pub enum NetSend<E> {
    Unreliable(E),
    Reliable(E),
    UnreliableTo(SocketAddr, E),
    ReliableTo(SocketAddr, E),
    StateTo(SocketAddr, E),
}

#[derive(Clone, Debug)]
pub enum FromClient {
    Connected {
        addr: SocketAddr,
        generation: u32,
        steam_id: u64,
        name: String,
    },
    Disconnected {
        addr: SocketAddr,
    },
    Message {
        addr: SocketAddr,
        event: ClientToServer,
    },
}

#[derive(Clone, Debug)]
pub enum FromServer {
    Message(ServerToClient),
    Connected { generation: u32 },
    Disconnected,
}

impl NetValue {
    pub fn summary(&self) -> String {
        match self {
            NetValue::Nil => "nil".to_string(),
            NetValue::Bool(value) => value.to_string(),
            NetValue::Int(value) => value.to_string(),
            NetValue::Float(value) => format!("{value:.3}"),
            NetValue::String(value) => format!("{value:?}"),
            NetValue::Vector3(value) => format!("({:.2},{:.2},{:.2})", value.x, value.y, value.z),
            NetValue::Angle3(value) => format!("({:.2},{:.2},{:.2})", value.p, value.y, value.r),
            NetValue::Entity(handle) => format!("{handle:?}"),
        }
    }
}

pub fn vars_summary(vars: &[NetVar]) -> String {
    let mut out = String::new();
    let mut idx = 0;

    while idx < vars.len() {
        if idx > 0 {
            out.push(' ');
        }

        out.push_str(&vars[idx].key);
        out.push('=');
        out.push_str(&vars[idx].value.summary());
        idx += 1;
    }

    out
}

pub fn networked_summary(entities: &[EntityNetworked]) -> String {
    let mut out = String::new();
    let mut idx = 0;

    while idx < entities.len() {
        if idx > 0 {
            out.push_str(", ");
        }

        out.push_str(&format!(
            "{:?}[{}]",
            entities[idx].handle,
            vars_summary(&entities[idx].vars)
        ));
        idx += 1;
    }

    out
}

fn edit_summary(edit: &BrushEdit) -> String {
    match edit {
        BrushEdit::Box(brush) => format!(
            "box ({:.2},{:.2},{:.2})-({:.2},{:.2},{:.2}) mat={}",
            brush.min.x,
            brush.min.y,
            brush.min.z,
            brush.max.x,
            brush.max.y,
            brush.max.z,
            brush.material
        ),
        BrushEdit::Convex { planes, material } => {
            format!("convex {} planes mat={material}", planes.len())
        }
        BrushEdit::Remove(index) => format!("remove {index}"),
        BrushEdit::Move { index, delta } => {
            format!(
                "move {index} ({:.2},{:.2},{:.2})",
                delta.x, delta.y, delta.z
            )
        }
        BrushEdit::Clear => "clear".to_string(),
    }
}

impl ServerToClient {
    pub fn summary(&self) -> String {
        match self {
            ServerToClient::MapChange { map_name } => format!("MapChange({map_name})"),
            ServerToClient::GameStateChanged { state } => format!("GameStateChanged({state})"),
            ServerToClient::ConVarReplicated { name, value } => {
                format!("ConVarReplicated({name}={value})")
            }
            ServerToClient::EntitySpawned {
                handle,
                class_hash,
                position,
                networked,
                ..
            } => format!(
                "EntitySpawned({handle:?} class={class_hash} pos=({:.2},{:.2},{:.2}) vars={})",
                position.x,
                position.y,
                position.z,
                networked.len()
            ),
            ServerToClient::EntityDespawned { handle } => format!("EntityDespawned({handle:?})"),
            ServerToClient::NetworkedUpdate { entities } => {
                format!("NetworkedUpdate(ents={})", entities.len())
            }
            ServerToClient::EntityOwner { handle, owner } => {
                format!("EntityOwner({handle:?} owner={owner:?})")
            }
            ServerToClient::PredictedState {
                tick,
                player,
                entities,
                anims,
            } => format!(
                "PredictedState(tick={tick} ack={} player={:?} ents={} anims={})",
                player.ack,
                player.handle,
                entities.len(),
                anims.len()
            ),
            ServerToClient::EntityParented {
                handle,
                parent_handle,
                attachment_point,
            } => format!(
                "EntityParented({handle:?} -> {parent_handle:?} attach={attachment_point})"
            ),
            ServerToClient::PlayerConnected { handle, name } => {
                format!("PlayerConnected({handle:?} {name})")
            }
            ServerToClient::PlayerDisconnected { handle } => {
                format!("PlayerDisconnected({handle:?})")
            }
            ServerToClient::PlayerSpawned { handle } => format!("PlayerSpawned({handle:?})"),
            ServerToClient::PlayerDamaged {
                handle,
                attacker,
                damage,
                new_health,
                ..
            } => format!(
                "PlayerDamaged({handle:?} by {attacker:?} dmg={damage} hp={new_health})"
            ),
            ServerToClient::PlayerDied {
                handle,
                killer,
                ..
            } => format!("PlayerDied({handle:?} killer={killer:?})"),
            ServerToClient::ModelChanged { handle, model } => {
                format!("ModelChanged({handle:?} {model})")
            }
            ServerToClient::AnimModel { handle, mesh, clips } => {
                format!("AnimModel({handle:?} {mesh} {clips})")
            }
            ServerToClient::FlagsChanged { handle, flags } => {
                format!("FlagsChanged({handle:?} {flags:?})")
            }
            ServerToClient::TransformUpdated {
                handle,
                position,
                angles,
                velocity,
            } => format!(
                "TransformUpdated({handle:?} pos={} ang={} vel={})",
                option_vec(position),
                option_ang(angles),
                option_vec(velocity)
            ),
            ServerToClient::PlaySound {
                sound_hash,
                entity_handle,
                ..
            } => format!("PlaySound(hash={sound_hash} ent={entity_handle:?})"),
            ServerToClient::StopSound {
                def_hash,
                sound_hash,
                entity_handle,
            } => format!("StopSound(def={def_hash} hash={sound_hash} ent={entity_handle:?})"),
            ServerToClient::SoundBaseline { sounds } => {
                format!("SoundBaseline({} loops)", sounds.len())
            }
            ServerToClient::PlayEffect { effect_hash, .. } => {
                format!("PlayEffect(hash={effect_hash})")
            }
            ServerToClient::AnimationTriggered {
                handle,
                sequence_id,
                ..
            } => format!("AnimationTriggered({handle:?} seq={sequence_id})"),
            ServerToClient::UserMessage { hash, data } => {
                format!("UserMessage(hash={hash} {}b)", data.len())
            }
            ServerToClient::ChatMessage {
                sender_handle,
                team_only,
                text,
            } => format!("ChatMessage({sender_handle:?} team={team_only} {text})"),
            ServerToClient::VoiceChunk {
                sender_handle,
                data,
            } => format!("VoiceChunk({sender_handle:?} {}b)", data.len()),
            ServerToClient::Pong {
                client_time,
                server_time,
            } => format!("Pong(client={client_time} server={server_time})"),
            ServerToClient::ServerTick { tick } => format!("ServerTick({tick})"),
            ServerToClient::WeaponFired {
                entity_handle,
                weapon_handle,
            } => format!("WeaponFired(ent={entity_handle:?} wep={weapon_handle:?})"),
            ServerToClient::WeaponReloaded { entity_handle } => {
                format!("WeaponReloaded({entity_handle:?})")
            }
            ServerToClient::ItemEquipped {
                entity_handle,
                slot,
                item_handle,
            } => format!("ItemEquipped(ent={entity_handle:?} slot={slot} item={item_handle:?})"),
            ServerToClient::WorldSnapshot {
                generation,
                reset,
                part,
                parts,
                entities,
                networked,
                owners,
                models,
            } => format!(
                "WorldSnapshot(gen={generation} reset={reset} part={part}/{parts} ents={} networked={} owners={} models={})",
                entities.len(),
                networked.len(),
                owners.len(),
                models.len()
            ),
            ServerToClient::TickState {
                tick,
                part,
                parts,
                entities,
            } => format!(
                "TickState(tick={tick} part={part}/{parts} ents={})",
                entities.len()
            ),
            ServerToClient::VoxelScale { scale } => format!("VoxelScale({scale})"),
            ServerToClient::VoxelChunk(update) => {
                format!("VoxelChunk({} {} {})", update.x, update.y, update.z)
            }
            ServerToClient::BrushScale { scale } => format!("BrushScale({scale})"),
            ServerToClient::BrushEdit(edit) => format!("BrushEdit({})", edit_summary(edit)),
            ServerToClient::WorldMotion { ratio } => format!("WorldMotion({ratio})"),
        }
    }
}

impl ClientToServer {
    pub fn summary(&self) -> String {
        match self {
            ClientToServer::UserMessage { hash, data } => {
                format!("UserMessage(hash={hash} {}b)", data.len())
            }
            ClientToServer::ChatMessage {
                sender_handle,
                team_only,
                text,
            } => format!("ChatMessage({sender_handle:?} team={team_only} {text})"),
            ClientToServer::VoiceChunk {
                sender_handle,
                data,
            } => format!("VoiceChunk({sender_handle:?} {}b)", data.len()),
            ClientToServer::Ping { client_time } => format!("Ping({client_time})"),
            ClientToServer::ClientReady { tick } => format!("ClientReady({tick})"),
            ClientToServer::PlayerInput {
                tick,
                buttons,
                movement,
                viewangles,
            } => format!(
                "PlayerInput(tick={tick} buttons={buttons:?} wish=({:.2},{:.2},{:.2}) view=({:.1},{:.1},{:.1}))",
                movement.x,
                movement.y,
                movement.z,
                viewangles.p,
                viewangles.y,
                viewangles.r
            ),
            ClientToServer::ScaleMaps { ratio } => format!("ScaleMaps({ratio})"),
        }
    }
}

fn option_vec(value: &Option<Vector3>) -> String {
    match value {
        Some(v) => format!("({:.2},{:.2},{:.2})", v.x, v.y, v.z),
        None => "-".to_string(),
    }
}

fn option_ang(value: &Option<Angle3>) -> String {
    match value {
        Some(a) => format!("({:.1},{:.1},{:.1})", a.p, a.y, a.r),
        None => "-".to_string(),
    }
}
