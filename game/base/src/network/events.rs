use wincode::{SchemaWrite, SchemaRead};

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum NetworkEvent {
    // SERVER->CLIENT Events
    PlayerConnected { id: u32, name: String },
    PlayerDisconnected { id: u32 },
    PlayerSpawned { id: u32 },
    PlayerDied { id: u32, killer: u32, inflictor: u32 },
    // SERVER<->CLIENT Events
    UserMessage { hash: u32, data: Vec<u8>  }
}

#[derive(Clone, Debug)]
pub enum NetSend {
    Unreliable(NetworkEvent),
    Reliable(NetworkEvent),
}