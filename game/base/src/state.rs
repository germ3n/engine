use crate::console::{ConVar, ConVarValue};
use crate::entities::EntityList;
use crate::input::{binds_path, load_or_defaults, Binds};
use crate::network::NetSend;
use crate::network::NetWake;
use crate::script::{Realm, ScriptEngine};
use crate::world::{BrushMap, VoxelWorld};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

pub struct GameState<In, Out> {
    pub realm: Realm,
    pub entities: EntityList,
    pub voxel_world: VoxelWorld,
    pub brush_world: BrushMap,
    pub map_name: String,
    pub cvars: Arc<HashMap<String, Arc<ConVar>>>,
    pub binds: Arc<Mutex<Binds>>,
    pub tick_interval: f64,
    pub network_receiver: Receiver<In>,
    pub network_sender: SyncSender<NetSend<Out>>,
    pub script_engine: ScriptEngine,
    pub cur_time: f64,
    pub frame_time: f64,
    pub tick_count: u64,
    wake: NetWake,
}

impl<In, Out> GameState<In, Out> {
    pub fn new(
        realm: Realm,
        network_receiver: Receiver<In>,
        network_sender: SyncSender<NetSend<Out>>,
        tick_interval: f64,
        wake: NetWake,
    ) -> Self {
        let mut cvars = HashMap::new();
        cvars.insert(
            "sv_gravity".to_string(),
            Arc::new(ConVar::new(
                "sv_gravity",
                ConVarValue::Float(24.0),
                "World gravity",
                Some(false),
                Some(true),
            )),
        );

        for idx in 0..4 {
            let entries = [
                (
                    format!("pad{idx}_deadzone_left"),
                    0.15,
                    "Left stick deadzone",
                ),
                (
                    format!("pad{idx}_deadzone_right"),
                    0.15,
                    "Right stick deadzone",
                ),
                (format!("pad{idx}_deadzone_gas"), 0.05, "Gas pedal deadzone"),
                (
                    format!("pad{idx}_deadzone_brake"),
                    0.05,
                    "Brake pedal deadzone",
                ),
                (
                    format!("pad{idx}_deadzone_clutch"),
                    0.05,
                    "Clutch pedal deadzone",
                ),
            ];

            for (name, default, description) in entries {
                cvars.insert(
                    name.clone(),
                    Arc::new(ConVar::new(
                        &name,
                        ConVarValue::Float(default),
                        description,
                        Some(false),
                        Some(false),
                    )),
                );
            }
        }

        let cvars = Arc::new(cvars);
        let binds = Arc::new(Mutex::new(load_or_defaults(&binds_path())));
        let script_engine = ScriptEngine::new(realm, tick_interval, cvars.clone(), binds.clone());

        Self {
            realm,
            entities: EntityList::new(),
            voxel_world: VoxelWorld::new(),
            brush_world: BrushMap::new(),
            map_name: String::new(),
            cvars,
            binds,
            tick_interval,
            network_receiver,
            network_sender,
            script_engine,
            cur_time: 0.0,
            frame_time: 0.0,
            tick_count: 0,
            wake,
        }
    }

    pub fn run_hook<A: mlua::IntoLuaMulti>(&self, hook_name: &str, args: A) {
        self.script_engine.run_hook(
            hook_name,
            self.cur_time,
            self.frame_time,
            self.tick_count,
            args,
        );
    }

    pub fn run_usermessage<A: mlua::IntoLuaMulti>(&self, hash: u32, args: A) {
        self.script_engine.run_usermessage(
            hash,
            self.cur_time,
            self.frame_time,
            self.tick_count,
            args,
        );
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
            Ok(()) => self.wake.poke(),
            Err(TrySendError::Full(_)) => {
                println!("[net] outbound queue full");
            }
            Err(TrySendError::Disconnected(_)) => {
                println!("[net] outbound disconnected");
            }
        }
    }
}
