use wincode::{SchemaWrite, SchemaRead};

#[derive(SchemaWrite, SchemaRead, Clone, Debug)]
pub enum NetworkEvent {
    // SERVER->CLIENT Events
    PlayerSpawned { id: u32, position: [f32; 3] },
    PlayerDisconnected { id: u32 },
    // SERVER<->CLIENT Events
    UserMessage { hash: u32, data: Vec<u8>  }
}

#[derive(Clone, Debug)]
pub enum NetSend {
    Unreliable(NetworkEvent),
    Reliable(NetworkEvent),
}