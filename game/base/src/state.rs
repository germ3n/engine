use crate::console::{ConVar, ConVarValue};
use crate::entities::{EntityHandle, EntityList};
use crate::fs::Fs;
use crate::input::{binds_path, load_or_defaults, Binds};
use crate::movement::UserCommand;
use crate::network::events::{EntityNetworked, NetVar};
use crate::network::NetSend;
use crate::network::NetWake;
use crate::platform::PadCache;
use crate::script::libs::engine::publish_clock;
use crate::script::libs::ents::EntityScope;
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
    despawned: Vec<EntityHandle>,
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
            despawned: Vec::new(),
            wake,
        }
    }

    fn with_entities<R>(&mut self, f: impl FnOnce(&ScriptEngine) -> R) -> R {
        let removed = self.entities.take_removed();
        let entities: *mut EntityList = &mut self.entities;
        let _scope = EntityScope::new(&self.script_engine.entity_access, entities);

        if !removed.is_empty() {
            if matches!(self.realm, Realm::Server) {
                for (handle, spawned) in &removed {
                    if *spawned {
                        self.despawned.push(*handle);
                    }
                }
            }

            self.script_engine.sync_removed(&removed);
        }

        f(&self.script_engine)
    }

    pub fn run_hook<A, R>(&mut self, hook_name: &str, args: A) -> R
    where
        A: mlua::IntoLuaMulti,
        R: mlua::FromLuaMulti,
    {
        let (cur_time, frame_time, tick_count) = (self.cur_time, self.frame_time, self.tick_count);

        self.with_entities(|engine| {
            engine.run_hook(hook_name, cur_time, frame_time, tick_count, args)
        })
    }

    pub fn run_usermessage<A: mlua::IntoLuaMulti>(&mut self, hash: u32, args: A) {
        let (cur_time, frame_time, tick_count) = (self.cur_time, self.frame_time, self.tick_count);

        self.with_entities(|engine| {
            engine.run_usermessage(hash, cur_time, frame_time, tick_count, args);
        });
    }

    pub fn think_entities(&mut self) {
        let (cur_time, frame_time, tick_count) = (self.cur_time, self.frame_time, self.tick_count);

        self.with_entities(|engine| engine.think_entities(cur_time, frame_time, tick_count));
    }

    pub fn sync_entities(&mut self) {
        self.with_entities(|_| ());
    }

    pub fn take_despawned(&mut self) -> Vec<EntityHandle> {
        self.sync_entities();

        std::mem::take(&mut self.despawned)
    }

    pub fn net_spawn(&mut self, handle: EntityHandle, vars: &[NetVar], time: f64) -> bool {
        self.with_entities(|engine| engine.net_spawn(handle, vars, Some(time)))
    }

    pub fn collect_networked(&mut self) -> Vec<EntityNetworked> {
        self.with_entities(|engine| engine.collect_networked())
    }

    pub fn networked_state(&mut self, handle: Option<EntityHandle>) -> Vec<EntityNetworked> {
        self.with_entities(|engine| engine.networked_state(handle))
    }

    pub fn apply_networked(&mut self, entities: &[EntityNetworked], time: f64) {
        self.with_entities(|engine| engine.apply_networked(entities, Some(time)));
    }

    pub fn present_networked(&mut self, time: f64) {
        self.with_entities(|engine| engine.present_networked(time));
    }

    pub fn run_predicted(&mut self, handle: EntityHandle, cmd: &UserCommand, first_time: bool) {
        let (cur_time, frame_time, tick_count) = (self.cur_time, self.frame_time, self.tick_count);

        self.with_entities(|engine| {
            publish_clock(&engine.lua, cur_time, frame_time, tick_count);
            engine.run_predicted(handle, cmd, first_time);
        });
    }

    pub fn predicted_state(&mut self, handle: EntityHandle) -> Vec<EntityNetworked> {
        self.with_entities(|engine| engine.predicted_state(handle))
    }

    pub fn begin_reconcile(&mut self, entities: &[EntityNetworked]) {
        self.with_entities(|engine| engine.begin_reconcile(entities));
    }

    pub fn end_reconcile(&mut self) {
        self.with_entities(|engine| engine.end_reconcile());
    }

    pub fn owner_changed(&mut self, handle: EntityHandle, owner: EntityHandle) {
        self.with_entities(|engine| engine.owner_changed(handle, owner));
    }

    pub fn set_local_player(&mut self, handle: EntityHandle) {
        self.with_entities(|engine| engine.set_local_player(handle));
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
