use crate::anim::AnimAssets;
use crate::console::{ConVar, ConVarValue};
use crate::entities::{EntityHandle, EntityList, Player};
use crate::fs::Fs;
use crate::input::{binds_path, load_or_defaults, Binds};
use crate::movement::UserCommand;
use crate::network::events::{EntityAnimNet, EntityBones, EntityNetworked, NetVar};
use crate::network::NetSend;
use crate::network::NetWake;
use crate::physics::{PhysicsScope, PhysicsWorld};
use crate::platform::PadCache;
use crate::script::libs::engine::{publish_clock, WorldScope};
use crate::script::libs::ents::{AnimScope, EntityScope};
use crate::script::libs::nav::NavScope;
use crate::script::libs::vector3::Vector3;
use crate::script::{Realm, ScriptEngine};
use crate::sound::{Buses, SoundScope, SoundWorld};
use crate::world::gen::{ChunkHandle, VoxelGen};
use crate::world::nav::NavHost;
use crate::world::{BrushMap, ChunkPos, VoxelWorld};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

pub struct GameState<In, Out> {
    pub realm: Realm,
    pub entities: EntityList,
    pub voxel_world: VoxelWorld,
    pub brush_world: BrushMap,
    voxel_gen: Option<VoxelGen>,
    pub nav: NavHost,
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
    pub anims: AnimAssets,
    pub sound: Box<SoundWorld>,
    physics: PhysicsWorld,
    despawned: Vec<EntityHandle>,
    wake: NetWake,
    motion_ratio: f64,
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
        register_sound_cvars(&mut cvars);

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
        let mut sound = Box::new(SoundWorld::new(realm, matches!(realm, Realm::Client)));
        let script_engine = ScriptEngine::new(
            realm,
            tick_interval,
            cvars.clone(),
            binds.clone(),
            pads.clone(),
            &mut *sound,
        );

        Self {
            realm,
            entities: EntityList::new(),
            voxel_world: VoxelWorld::new(),
            brush_world: BrushMap::new(),
            voxel_gen: None,
            nav: NavHost::new(),
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
            anims: AnimAssets::new(matches!(realm, Realm::Server)),
            sound,
            physics: PhysicsWorld::new(),
            despawned: Vec::new(),
            wake,
            motion_ratio: 1.0,
        }
    }

    fn with_entities<R>(&mut self, f: impl FnOnce(&ScriptEngine) -> R) -> R {
        let removed = self.entities.take_removed();
        self.sound.set_clock(self.tick_count);
        let entities: *mut EntityList = &mut self.entities;
        let anims: *mut AnimAssets = &mut self.anims;
        let sound: *mut SoundWorld = &mut *self.sound;
        let physics: *mut PhysicsWorld = if matches!(self.realm, Realm::Server) {
            &mut self.physics
        } else {
            std::ptr::null_mut()
        };
        let brush: *mut crate::world::BrushMap = &mut self.brush_world;
        let voxels: *mut crate::world::VoxelWorld = &mut self.voxel_world;
        let motion: *mut f64 = &mut self.motion_ratio;
        let nav_state: *mut crate::world::nav::NavState = &mut self.nav.state;
        let _scope = EntityScope::new(&self.script_engine.entity_access, entities);
        let _anim_scope = AnimScope::new(&self.script_engine.anim_access, anims);
        let _sound_scope = SoundScope::new(&self.script_engine.sound_access, sound);
        let _physics_scope = PhysicsScope::new(&self.script_engine.physics_access, physics);
        let _world_scope = WorldScope::new(
            &self.script_engine.brush_access,
            brush,
            &self.script_engine.voxel_access,
            voxels,
            &self.script_engine.motion_access,
            motion,
        );
        let _nav_scope = NavScope::new(&self.script_engine.nav_access, nav_state);

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

    pub fn take_motion(&mut self) -> Option<f64> {
        if (self.motion_ratio - 1.0).abs() <= 1e-12 {
            self.motion_ratio = 1.0;

            return None;
        }

        let ratio = self.motion_ratio;
        self.motion_ratio = 1.0;

        Some(ratio)
    }

    pub fn scale_maps(&mut self, ratio: f64) -> bool {
        if !matches!(self.realm, Realm::Server) {
            return false;
        }

        crate::scale::scale_both(
            &mut self.brush_world,
            &mut self.voxel_world,
            &mut self.entities,
            Some(&mut self.physics),
            &mut self.motion_ratio,
            ratio,
        )
    }

    pub fn step_physics(&mut self, players: &[EntityHandle]) {
        if !matches!(self.realm, Realm::Server) {
            return;
        }

        let gravity = crate::movement::gravity(&self.cvars);
        let dt = self.tick_interval;
        self.physics.step(
            &mut self.entities,
            players,
            dt,
            gravity,
            &self.brush_world,
            &mut self.voxel_world,
        );
    }

    pub fn think_entities(&mut self) {
        let (cur_time, frame_time, tick_count) = (self.cur_time, self.frame_time, self.tick_count);

        self.with_entities(|engine| engine.think_entities(cur_time, frame_time, tick_count));
    }

    pub fn begin_terrain(&mut self) {
        if !matches!(self.realm, Realm::Server) {
            return;
        }

        {
            let mut settings = self
                .script_engine
                .gen_settings
                .lock()
                .expect("gen settings");
            settings.map_name = self.map_name.clone();
        }

        if let Some(path) = crate::world::cwd_vmap_path(&self.map_name) {
            match self.voxel_world.load_resume(&path) {
                Ok(true) => {
                    let seed = self.voxel_world.seed();
                    self.script_engine
                        .gen_settings
                        .lock()
                        .expect("gen settings")
                        .adopt_seed(seed);
                    log::info!("[voxel] resumed {} seed {seed}", path.display());
                }
                Ok(false) => {}
                Err(err) => log::warn!("[voxel] {err}"),
            }
        }

        {
            let settings = self
                .script_engine
                .gen_settings
                .lock()
                .expect("gen settings");
            self.voxel_world.set_seed(settings.seed);
            self.voxel_world.replace_nonsolid(&settings.nonsolid);
        }
    }

    pub fn poll_nav(&mut self, centers: &[ChunkPos]) -> bool {
        let settled = if self.nav.needs_settled() {
            self.voxel_gen
                .as_ref()
                .map(|gen| gen.is_settled(&self.voxel_world, centers))
                .unwrap_or(true)
        } else {
            true
        };
        let map_name = self.map_name.clone();

        self.nav
            .poll(&map_name, &self.brush_world, &self.voxel_world, settled)
    }

    pub fn poll_voxel_gen(&mut self, centers: &[ChunkPos]) {
        let enabled = self
            .script_engine
            .gen_settings
            .lock()
            .expect("gen settings")
            .enabled;

        if self.voxel_gen.is_none() {
            if !enabled {
                return;
            }

            let settings = Arc::clone(&self.script_engine.gen_settings);
            self.voxel_gen = Some(VoxelGen::start(settings));
            self.script_engine
                .gen_settings
                .lock()
                .expect("gen settings")
                .running = true;
            log::info!("[voxel] generating seed {}", self.voxel_world.seed());
        }

        let Some(mut gen) = self.voxel_gen.take() else {
            return;
        };

        if enabled {
            gen.prepare(&mut self.voxel_world);
            gen.enqueue(&self.voxel_world, centers);
        }
        let mut ready = gen.take_commits(4);
        self.voxel_gen = Some(gen);
        let mut idx = 0;

        while idx < ready.len() {
            let shared = Arc::new(Mutex::new(std::mem::take(&mut ready[idx])));
            let handle = ChunkHandle(Arc::clone(&shared));
            self.run_hook::<_, ()>("VoxelChunkGenerated", handle);
            let draft = std::mem::take(&mut *shared.lock().expect("chunk"));
            let mut gen = self.voxel_gen.take().expect("voxel gen");
            gen.commit(&mut self.voxel_world, draft);
            self.voxel_gen = Some(gen);
            idx += 1;
        }
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
        let events = self.advance_predicted_anim(handle, cmd.tick);
        self.sound.set_command_tick(cmd.tick);

        self.with_entities(|engine| {
            publish_clock(&engine.lua, cur_time, frame_time, tick_count);

            if first_time {
                let mut idx = 0;

                while idx < events.len() {
                    engine.anim_event(events[idx].0, &events[idx].1);
                    idx += 1;
                }
            }

            engine.run_predicted(handle, cmd, first_time);
        });
        self.sound.set_command_tick(0);
    }

    pub fn update_sound(&mut self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32, dt: f32) {
        let buses = Buses {
            master: crate::console::float_cvar(&self.cvars, "snd_volume", 1.0).clamp(0.0, 4.0)
                as f32,
            sfx: crate::console::float_cvar(&self.cvars, "snd_sfxvolume", 1.0).clamp(0.0, 4.0)
                as f32,
            music: crate::console::float_cvar(&self.cvars, "snd_musicvolume", 1.0).clamp(0.0, 4.0)
                as f32,
            ui: crate::console::float_cvar(&self.cvars, "snd_uivolume", 1.0).clamp(0.0, 4.0) as f32,
            voice: crate::console::float_cvar(&self.cvars, "snd_voicevolume", 1.0).clamp(0.0, 4.0)
                as f32,
        };
        let max_distance =
            crate::console::float_cvar(&self.cvars, "snd_maxdistance", 48.0).max(0.5) as f32;
        self.sound.update(
            dt,
            Vector3::new(x, y, z),
            yaw,
            pitch,
            buses,
            max_distance,
            &self.brush_world,
            &self.voxel_world,
            &self.entities,
        );
    }

    fn advance_predicted_anim(
        &mut self,
        owner: EntityHandle,
        tick: u64,
    ) -> Vec<(EntityHandle, String)> {
        let dt = self.tick_interval;
        let mut handles = Vec::new();

        for (handle, entity) in self.entities.iter() {
            if entity.base().owner == owner && handle != owner {
                handles.push(handle);
            }
        }

        handles.insert(0, owner);
        let mut events = Vec::new();
        let mut idx = 0;

        while idx < handles.len() {
            let handle = handles[idx];
            let owned = handle != owner;

            if owned {
                self.apply_root(handle, tick, dt);
            }

            if let Some(entity) = self.entities.get_mut(handle) {
                self.anims
                    .finish_playback(&mut entity.base_mut().anim, tick, dt);
            }

            if tick > 0 {
                if let Some(entity) = self.entities.get(handle) {
                    let names =
                        self.anims
                            .events(&entity.base().anim, (tick - 1) as f64, tick as f64, dt);
                    let mut name_idx = 0;

                    while name_idx < names.len() {
                        events.push((handle, names[name_idx].clone()));
                        name_idx += 1;
                    }
                }
            }

            idx += 1;
        }

        events
    }

    pub fn apply_root(&mut self, handle: EntityHandle, tick: u64, dt: f64) {
        let Some((playback, yaw)) = self.entities.get(handle).map(|entity| {
            let base = entity.base();

            (base.anim, base.angles.y)
        }) else {
            return;
        };
        let Some(step) = self.anims.root_motion(&playback, yaw, tick, dt) else {
            return;
        };
        let Some(entity) = self.entities.get_mut(handle) else {
            return;
        };
        let base = entity.base_mut();
        crate::movement::root_move(
            &mut base.position,
            &mut base.velocity,
            &mut base.angles,
            step,
            dt,
            crate::movement::gravity(&self.cvars),
            &self.brush_world,
            &self.voxel_world,
        );
    }

    pub fn drive_free_anims(&mut self, players: &[EntityHandle]) -> Vec<(EntityHandle, String)> {
        let tick = self.tick_count;
        let dt = self.tick_interval;
        let mut handles = Vec::new();

        for (handle, entity) in self.entities.iter() {
            let base = entity.base();

            if players
                .iter()
                .any(|player| *player == handle || base.owner == *player)
            {
                continue;
            }

            if base.anim.mesh == crate::anim::NONE_ASSET {
                continue;
            }

            handles.push(handle);
        }

        let mut events = Vec::new();
        let mut idx = 0;

        while idx < handles.len() {
            let handle = handles[idx];
            self.apply_root(handle, tick, dt);

            if let Some(entity) = self.entities.get_mut(handle) {
                self.anims
                    .finish_playback(&mut entity.base_mut().anim, tick, dt);
            }

            if tick > 0 {
                if let Some(entity) = self.entities.get(handle) {
                    let names =
                        self.anims
                            .events(&entity.base().anim, (tick - 1) as f64, tick as f64, dt);
                    let mut name_idx = 0;

                    while name_idx < names.len() {
                        events.push((handle, names[name_idx].clone()));
                        name_idx += 1;
                    }
                }
            }

            idx += 1;
        }

        events
    }

    pub fn skin_batch(
        &mut self,
        local: EntityHandle,
        local_pos: Option<crate::script::libs::vector3::Vector3>,
        local_time: f64,
        cull: crate::anim::Cull,
        anchor: crate::anchor::Anchor,
    ) -> crate::ui::skin::SkinBatch {
        let dt = self.tick_interval;
        let mut inputs = Vec::new();

        for (handle, entity) in self.entities.iter() {
            let base = entity.base();

            if base.anim.mesh == crate::anim::NONE_ASSET {
                continue;
            }

            let class_hash = entity.class_hash();
            let position = if handle == local {
                local_pos.unwrap_or(base.position)
            } else {
                base.position
            };
            let mut pitch = base.angles.p;
            let mut roll = base.angles.r;

            if class_hash == Player::CLASS_HASH {
                pitch = 0.0;
                roll = 0.0;
            }

            let time = if handle == local {
                local_time
            } else {
                base.anim.draw_tick as f64 + f64::from(base.anim.draw_frac)
            };
            inputs.push(crate::anim::DrawInput {
                entity: handle.0,
                mesh: base.anim.mesh,
                clips: base.anim.clips,
                playback: base.anim,
                position: anchor.relative(position.x, position.y, position.z),
                pitch,
                yaw: base.angles.y,
                roll,
                time,
            });
        }

        self.anims.build_batch(&inputs, &cull, dt)
    }

    pub fn fire_anim_events(&mut self, events: Vec<(EntityHandle, String)>) {
        if events.is_empty() {
            return;
        }

        self.with_entities(|engine| {
            let mut idx = 0;

            while idx < events.len() {
                engine.anim_event(events[idx].0, &events[idx].1);
                idx += 1;
            }
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

    pub fn owned_anims(&self, owner: EntityHandle) -> Vec<EntityAnimNet> {
        let mut anims = Vec::new();

        for (handle, entity) in self.entities.iter() {
            if entity.base().owner != owner || handle == owner {
                continue;
            }

            if entity.base().anim.mesh == crate::anim::NONE_ASSET {
                continue;
            }

            anims.push(EntityAnimNet {
                handle,
                anim: entity.base().anim.snapshot(),
                bones: self.anims.entity_bones(handle.0).bones,
            });
        }

        anims
    }

    pub fn take_anim_bones(&mut self) -> Vec<EntityBones> {
        self.anims.take_dirty_bones()
    }

    pub fn anim_bones_baseline(&self) -> Vec<EntityBones> {
        self.anims.all_entity_bones()
    }

    pub fn take_anim_models(&mut self) -> Vec<crate::network::events::EntityModel> {
        let raw = std::mem::take(&mut self.anims.dirty);
        let mut models = Vec::new();
        let mut idx = 0;

        while idx < raw.len() {
            let handle = EntityHandle(raw[idx]);

            if let Some(entity) = self.entities.get(handle) {
                if let Some((mesh, clips)) = self.anims.model_paths(&entity.base().anim) {
                    models.push(crate::network::events::EntityModel {
                        handle,
                        mesh,
                        clips,
                    });
                }
            }

            idx += 1;
        }

        models
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

    pub fn try_send_state_to(&self, addr: SocketAddr, event: Out) -> bool {
        match self.network_sender.try_send(NetSend::StateTo(addr, event)) {
            Ok(()) => {
                self.wake.poke();

                true
            }
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => false,
        }
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

fn register_sound_cvars(cvars: &mut HashMap<String, Arc<ConVar>>) {
    let entries = [
        ("snd_volume", 1.0, "Master volume"),
        ("snd_sfxvolume", 1.0, "Effect volume"),
        ("snd_musicvolume", 1.0, "Music and soundscape volume"),
        ("snd_uivolume", 1.0, "Interface volume"),
        ("snd_voicevolume", 1.0, "Voice volume"),
        (
            "snd_maxdistance",
            48.0,
            "Distance where a level 75 sound falls silent",
        ),
    ];

    for (name, default, description) in entries {
        cvars.insert(
            name.to_string(),
            Arc::new(ConVar::new(
                name,
                ConVarValue::Float(default),
                description,
                Some(false),
                Some(false),
            )),
        );
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
