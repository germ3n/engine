use wincode::{SchemaWrite, SchemaRead};
use crate::script::libs::vector3::Vector3;
use crate::script::libs::angle3::Angle3;

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum NetworkEvent {
    MapChange { map_name: String },
    GameStateChanged { state: u16 },
    ConVarReplicated { name: String, value: String },

    EntitySpawned { id: u32, class_hash: u32, position: Vector3 },
    EntityDespawned { id: u32 },
    EntityParented { id: u32, parent_id: u32, attachment_point: u16 },

    PlayerConnected { id: u32, name: String },
    PlayerDisconnected { id: u32 },
    PlayerSpawned { id: u32 },
    PlayerDamaged { id: u32, attacker: u32, inflictor: u32, damage: u32, new_health: u32 },
    PlayerDied { id: u32, killer: u32, inflictor: u32 },

    ModelChanged { id: u32, model: String },
    PositionUpdated { id: u32, position: Vector3 },
    TransformUpdated { id: u32, position: Vector3, angles: Angle3, velocity: Vector3 },
    PlaySound { sound_hash: u32, entity_id: Option<u32>, position: Vector3, volume: f32, pitch: f32 },
    PlayEffect { effect_hash: u32, position: Vector3, normal: Vector3 },
    AnimationTriggered { entity_id: u32, sequence_id: u16, playback_rate: f32 },
    // SERVER<->CLIENT Events
    UserMessage { hash: u32, data: Vec<u8> },
    ChatMessage { sender_id: u32, team_only: bool, text: String },
    VoiceChunk { sender_id: u32, data: Vec<u8> },

    Ping { client_time: u64 },
    Pong { client_time: u64, server_time: u64 },
    ServerTick { tick: u64 },
    ClientReady { tick: u64 },

    WeaponFired { entity_id: u32, weapon_id: u32 },
    WeaponReloaded { entity_id: u32 },
    ItemEquipped { entity_id: u32, slot: u8, item_id: u32 },
    // CLIENT->SERVER Events
    PlayerInput { tick: u64, buttons: u64, movement: Vector3, viewangles: Angle3 },
}

#[derive(Clone, Debug)]
pub enum NetSend {
    Unreliable(NetworkEvent),
    Reliable(NetworkEvent),
}