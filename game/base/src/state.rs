use crate::console::{ConVar, ConVarValue};
use crate::entities::EntityList;
use crate::fs::Fs;
use crate::input::{binds_path, load_or_defaults, Binds};
use crate::network::NetSend;
use crate::network::NetWake;
use crate::platform::PadCache;
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
    pub pads: Arc<Mutex<PadCache>>,
    pub tick_interval: f64,
    pub network_receiver: Receiver<In>,
    pub network_sender: SyncSender<NetSend<Out>>,
    pub script_engine: ScriptEngine,
    pub fs: Arc<Fs>,
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
        fs: Arc<Fs>,
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
        register_net_sim_cvars(&mut cvars);

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
        let pads = Arc::new(Mutex::new(PadCache::new()));
        let script_engine = ScriptEngine::new(
            realm,
            tick_interval,
            cvars.clone(),
            binds.clone(),
            pads.clone(),
        );

        Self {
            realm,
            entities: EntityList::new(),
            voxel_world: VoxelWorld::new(),
            brush_world: BrushMap::new(),
            map_name: String::new(),
            cvars,
            binds,
            pads,
            tick_interval,
            network_receiver,
            network_sender,
            script_engine,
            fs,
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
                log::warn!("[net] outbound queue full");
            }
            Err(TrySendError::Disconnected(_)) => {
                log::warn!("[net] outbound disconnected");
            }
        }
    }
}

fn register_net_sim_cvars(cvars: &mut HashMap<String, Arc<ConVar>>) {
    let settings = crate::network::sim::settings();
    insert_sim_cvar(
        cvars,
        "net_fakelag",
        settings.lag_ms,
        "Fake network lag in milliseconds",
        |value| crate::network::sim::set_lag_ms(cvar_u32(value)),
    );
    insert_sim_cvar(
        cvars,
        "net_fakejitter",
        settings.jitter_ms,
        "Fake network jitter in milliseconds",
        |value| crate::network::sim::set_jitter_ms(cvar_u32(value)),
    );
    insert_sim_cvar(
        cvars,
        "net_fakeloss",
        settings.loss_pct,
        "Fake packet loss percentage (0-100)",
        |value| crate::network::sim::set_loss_pct(cvar_u32(value)),
    );
}

fn insert_sim_cvar(
    cvars: &mut HashMap<String, Arc<ConVar>>,
    name: &str,
    default: u32,
    description: &str,
    on_change: impl Fn(&ConVarValue) + Send + Sync + 'static,
) {
    let cvar = Arc::new(ConVar::new(
        name,
        ConVarValue::Integer(default as i64),
        description,
        Some(false),
        Some(false),
    ));
    cvar.add_change_callback(on_change);
    cvars.insert(name.to_string(), cvar);
}

fn cvar_u32(value: &ConVarValue) -> u32 {
    match value {
        ConVarValue::Integer(value) => (*value).max(0) as u32,
        ConVarValue::Float(value) => (*value).max(0.0) as u32,
        _ => 0,
    }
}
