use crate::entities::EntityList;
use crate::network::{NetworkEvent, NetSend};
use crate::console::{ConVar, ConVarValue};
use std::sync::mpsc::{Receiver, Sender};
use std::collections::HashMap;
use crate::script::{ScriptEngine, Realm};
use std::sync::{Arc, Mutex, atomic::{AtomicU64, AtomicU32, Ordering}};

pub struct GameState {
    pub realm: Realm,
    pub entities: EntityList,
    pub cvars: HashMap<String, ConVar>,
    pub tick_interval: f64,
    pub network_receiver: Receiver<NetworkEvent>,
    pub network_sender: Sender<NetSend>,
    pub script_engine: ScriptEngine,
    pub cur_time: Arc<AtomicU64>,
    pub frame_time: Arc<AtomicU64>,
    pub tick_count: Arc<AtomicU64>,
}

impl GameState {
    pub fn new(
        realm: Realm,
        network_receiver: Receiver<NetworkEvent>,
        network_sender: Sender<NetSend>,
        tick_interval: f64
    ) -> Self {
        let mut cvars = HashMap::new();
        cvars.insert(
            "sv_gravity".to_string(),
            ConVar::new("sv_gravity", ConVarValue::Float(800.0), "World gravity", Some(false), Some(true)),
        );

        let cur_time = Arc::new(AtomicU64::new(0.0f64.to_bits()));
        let frame_time = Arc::new(AtomicU64::new(0.0f64.to_bits()));
        let tick_count = Arc::new(AtomicU64::new(0));

        let script_engine = ScriptEngine::new(
            realm, 
            tick_interval, 
            cur_time.clone(), 
            frame_time.clone(), 
            tick_count.clone()
        );

        Self {
            realm,
            entities: EntityList::new(),
            cvars,
            tick_interval,
            network_receiver,
            network_sender,
            script_engine,
            cur_time,
            frame_time,
            tick_count,
        }
    }

    pub fn cur_time(&self) -> f64 {
        f64::from_bits(self.cur_time.load(Ordering::Relaxed))
    }

    pub fn frame_time(&self) -> f64 {
        f64::from_bits(self.frame_time.load(Ordering::Relaxed))
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count.load(Ordering::Relaxed)
    }

    pub fn send_reliable(&self, event: NetworkEvent) {
        let _ = self.network_sender.send(NetSend::Reliable(event));
    }

    pub fn send_unreliable(&self, event: NetworkEvent) {
        let _ = self.network_sender.send(NetSend::Unreliable(event));
    }
}
