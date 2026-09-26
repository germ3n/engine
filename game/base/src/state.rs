use crate::entities::EntityList;
use crate::network::NetSend;
use crate::console::{ConVar, ConVarValue};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::net::SocketAddr;
use std::collections::HashMap;
use crate::script::{ScriptEngine, Realm};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

pub struct GameState<In, Out> {
    pub realm: Realm,
    pub entities: EntityList,
    pub cvars: Arc<HashMap<String, Arc<ConVar>>>,
    pub tick_interval: f64,
    pub network_receiver: Receiver<In>,
    pub network_sender: SyncSender<NetSend<Out>>,
    pub script_engine: ScriptEngine,
    pub cur_time: Arc<AtomicU64>,
    pub frame_time: Arc<AtomicU64>,
    pub tick_count: Arc<AtomicU64>,
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
        let cur_time = Arc::new(AtomicU64::new(0.0f64.to_bits()));
        let frame_time = Arc::new(AtomicU64::new(0.0f64.to_bits()));
        let tick_count = Arc::new(AtomicU64::new(0));

        let script_engine = ScriptEngine::new(
            realm,
            tick_interval, 
            cur_time.clone(), 
            frame_time.clone(), 
            tick_count.clone(),
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
