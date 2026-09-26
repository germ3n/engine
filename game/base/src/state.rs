use crate::entities::EntityList;
use crate::network::NetSend;
use crate::console::{ConVar, ConVarValue};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::net::SocketAddr;
use std::collections::HashMap;
use crate::script::{ScriptEngine, Realm};
use std::sync::Arc;

pub struct GameState<In, Out> {
    pub realm: Realm,
    pub entities: EntityList,
    pub cvars: Arc<HashMap<String, Arc<ConVar>>>,
    pub tick_interval: f64,
    pub network_receiver: Receiver<In>,
    pub network_sender: SyncSender<NetSend<Out>>,
    pub script_engine: ScriptEngine,
    pub cur_time: f64,
    pub frame_time: f64,
    pub tick_count: u64,
}

impl<In, Out> GameState<In, Out> {
    pub fn new(
        realm: Realm,
        network_receiver: Receiver<In>,
        network_sender: SyncSender<NetSend<Out>>,
        tick_interval: f64
    ) -> Self {
        let mut cvars = HashMap::new();
        cvars.insert(
            "sv_gravity".to_string(),
            Arc::new(ConVar::new("sv_gravity", ConVarValue::Float(800.0), "World gravity", Some(false), Some(true))),
        );

        let cvars = Arc::new(cvars);
        let script_engine = ScriptEngine::new(
            realm,
            tick_interval,
            cvars.clone()
        );

        Self {
            realm,
            entities: EntityList::new(),
            cvars,
            tick_interval,
            network_receiver,
            network_sender,
            script_engine,
            cur_time: 0.0,
            frame_time: 0.0,
            tick_count: 0,
        }
    }

    pub fn run_hook<A: mlua::IntoLuaMulti>(&self, hook_name: &str, args: A) {
        self.script_engine.run_hook(hook_name, self.cur_time, self.frame_time, self.tick_count, args);
    }

    pub fn run_usermessage<A: mlua::IntoLuaMulti>(&self, hash: u32, args: A) {
        self.script_engine.run_usermessage(hash, self.cur_time, self.frame_time, self.tick_count, args);
    }

    pub fn send_reliable(&self, event: Out) {
        self.enqueue(NetSend::Reliable(event));
    }

    pub fn send_unreliable(&self, event: Out) {
        self.enqueue(NetSend::Unreliable(event));
    }

    pub fn send_reliable_to(&self, addr: SocketAddr, event: Out) {
        self.enqueue(NetSend::ReliableTo(addr, event));
    }

    pub fn send_unreliable_to(&self, addr: SocketAddr, event: Out) {
        self.enqueue(NetSend::UnreliableTo(addr, event));
    }

    pub fn send_state_to(&self, addr: SocketAddr, event: Out) {
        self.enqueue(NetSend::StateTo(addr, event));
    }

    fn enqueue(&self, message: NetSend<Out>) {
        match self.network_sender.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                println!("[net] outbound queue full");
            }
            Err(TrySendError::Disconnected(_)) => {
                println!("[net] outbound disconnected");
            }
        }
    }
}
