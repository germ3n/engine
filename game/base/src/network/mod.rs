pub mod client;
pub mod server;
pub mod events;
pub mod packet;
pub mod reliable;
pub mod usermessage;

pub use client::NetworkClient;
pub use server::NetworkServer;
pub use events::{ClientToServer, ServerToClient, NetSend, FromClient, FromServer};
pub use packet::PacketType;
pub use reliable::{EnqueueStatus, ReliableChannel, ReliableBody, SelectiveAck, UnreliableAssembly, UnreliableInbox, accept_unreliable, take_unreliable};

pub const OUTBOUND_CAP: usize = 1024;