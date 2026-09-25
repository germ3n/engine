use wincode::{SchemaWrite, SchemaRead};
use crate::script::libs::vector3::Vector3;
use crate::script::libs::angle3::Angle3;
use crate::r#enum::{InputButtons, EntityFlags};
use crate::entities::EntityHandle;

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum NetworkEvent {
    MapChange { map_name: String },
    GameStateChanged { state: u16 },
    ConVarReplicated { name: String, value: String },

    EntitySpawned { handle: EntityHandle, class_hash: u32, position: Vector3 },
    EntityDespawned { handle: EntityHandle },
    EntityParented { handle: EntityHandle, parent_handle: EntityHandle, attachment_point: u16 },

    PlayerConnected { handle: EntityHandle, name: String },
    PlayerDisconnected { handle: EntityHandle },
    PlayerSpawned { handle: EntityHandle },
    PlayerDamaged { handle: EntityHandle, attacker: EntityHandle, inflictor: EntityHandle, damage: u32, new_health: u32 },
    PlayerDied { handle: EntityHandle, killer: EntityHandle, inflictor: EntityHandle },

    ModelChanged { handle: EntityHandle, model: String },
    FlagsChanged { handle: EntityHandle, flags: EntityFlags },
    TransformUpdated { handle: EntityHandle, position: Option<Vector3>, angles: Option<Angle3>, velocity: Option<Vector3> },
    PlaySound { sound_hash: u32, entity_handle: Option<EntityHandle>, position: Vector3, volume: f32, pitch: f32 },
    PlayEffect { effect_hash: u32, position: Vector3, normal: Vector3 },
    AnimationTriggered { handle: EntityHandle, sequence_id: u16, playback_rate: f32 },
    // SERVER<->CLIENT Events
    UserMessage { hash: u32, data: Vec<u8> },
    ChatMessage { sender_handle: EntityHandle, team_only: bool, text: String },
    VoiceChunk { sender_handle: EntityHandle, data: Vec<u8> },

    Ping { client_time: u64 },
    Pong { client_time: u64, server_time: u64 },
    ServerTick { tick: u64 },
    ClientReady { tick: u64 },

    WeaponFired { entity_handle: EntityHandle, weapon_handle: EntityHandle },
    WeaponReloaded { entity_handle: EntityHandle },
    ItemEquipped { entity_handle: EntityHandle, slot: u8, item_handle: EntityHandle },
    // CLIENT->SERVER Events
    PlayerInput { tick: u64, buttons: InputButtons, movement: Vector3, viewangles: Angle3 },
}

#[derive(Clone, Debug)]
pub enum NetSend {
    Unreliable(NetworkEvent),
    Reliable(NetworkEvent),
}