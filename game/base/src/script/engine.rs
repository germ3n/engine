use crate::console::ConVar;
use crate::entities::EntityHandle;
use crate::input::Binds;
use crate::network::events::{networked_summary, vars_summary, EntityNetworked, NetVar};
use crate::physics::PhysicsAccess;
use crate::platform::PadCache;
use crate::script::libs::engine::publish_clock;
use crate::script::libs::ents::{AnimAccess, EntityAccess};
use crate::script::libs::{
    register_ai_lib, register_angle3_lib, register_biome_lib, register_console_lib,
    register_nav_lib,
    register_convar_lib, register_demo_lib, register_engine_lib, register_ents_lib,
    register_input_lib, register_net_lib, register_noise_lib, register_pad_lib,
    register_scripted_ents_lib, register_sound_lib, register_surface_lib,
    register_usercmd_lib, register_vector3_lib, register_webview_lib,
};
use crate::sound::SoundAccess;
use crate::ui::Color;
use mlua::{Lua, LuaOptions, RegistryKey, StdLib};
use std::collections::HashMap;
use std::sync::atomic::AtomicPtr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

pub enum DrawCommand {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
        texture: u32,
        pipeline: u32,
        sampler: u32,
    },
    OutlinedRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        thickness: f32,
        color: Color,
        texture: u32,
        pipeline: u32,
        sampler: u32,
    },
    Text {
        font: mlua::LuaString,
        text: mlua::LuaString,
        x: f32,
        y: f32,
        scale: f32,
        color: Color,
        texture: u32,
        pipeline: u32,
        sampler: u32,
    },
    CreateShader {
        id: u32,
        source: String,
    },
    CreateTexture {
        id: u32,
        path: String,
    },
    CreateImage {
        id: u32,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    },
    CreateMaterial {
        id: u32,
        name: String,
    },
    CreateTarget {
        id: u32,
        width: u32,
        height: u32,
    },
    CreateBuffer {
        id: u32,
        bytes: Vec<u8>,
    },
    CreateSampler {
        id: u32,
        linear: bool,
        repeat: bool,
    },
    CreatePipeline {
        id: u32,
        shader: u32,
        screen: bool,
    },
    CreateMesh {
        id: u32,
        verts: Vec<f32>,
        screen: bool,
    },
    Free {
        id: u32,
    },
    DrawMesh {
        mesh: u32,
        pipeline: u32,
        texture: u32,
        sampler: u32,
    },
    SetTarget {
        id: u32,
    },
    UpdateBuffer {
        id: u32,
        bytes: Vec<u8>,
    },
    UpdateMesh {
        id: u32,
        verts: Vec<f32>,
    },
    UpdateTexture {
        id: u32,
        path: String,
    },
    UpdateImage {
        id: u32,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    },
    UpdateTarget {
        id: u32,
        width: u32,
        height: u32,
    },
    SetScissor {
        rect: Option<[f32; 4]>,
    },
}

pub struct RenderState {
    pub commands: Vec<DrawCommand>,
    pub book: crate::ui::gfx::Book,
    pub width: u32,
    pub height: u32,
}

pub type RenderQueue = Arc<Mutex<RenderState>>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Realm {
    Client,
    Server,
    Menu,
}

pub struct ScriptEngine {
    pub lua: Lua,
    pub realm: Realm,
    pub hook_caller: RegistryKey,
    pub net_caller: RegistryKey,
    #[allow(dead_code)]
    pub tick_interval: f64,
    pub render_queue: RenderQueue,
    pub entity_access: EntityAccess,
    pub anim_access: AnimAccess,
    pub sound_access: SoundAccess,
    pub physics_access: PhysicsAccess,
    pub brush_access: crate::script::libs::engine::BrushAccess,
    pub voxel_access: crate::script::libs::engine::VoxelAccess,
    pub pointer: Arc<Mutex<crate::script::libs::input::Pointer>>,
    pub motion_access: crate::script::libs::engine::MotionAccess,
    pub gen_settings: Arc<Mutex<crate::world::gen::GenSettings>>,
    pub nav_access: crate::script::libs::nav::NavAccess,
    webviews: Option<Arc<Mutex<crate::ui::webview::Bank>>>,
    usermsg_receiver: Receiver<(u32, Vec<u8>)>,
}

impl ScriptEngine {
    pub fn new(
        realm: Realm,
        tick_interval: f64,
        cvars: Arc<HashMap<String, Arc<ConVar>>>,
        binds: Arc<Mutex<Binds>>,
        pads: Arc<Mutex<PadCache>>,
        sound: *mut crate::sound::SoundWorld,
    ) -> Self {
        let (usermsg_sender, usermsg_receiver) = std::sync::mpsc::channel();
        let lua = unsafe { Lua::unsafe_new_with(StdLib::ALL, LuaOptions::default()) };
        lua.globals()
            .set("CLIENT", matches!(realm, Realm::Client))
            .expect("Failed to set CLIENT global");
        lua.globals()
            .set("SERVER", matches!(realm, Realm::Server))
            .expect("Failed to set SERVER global");
        lua.globals()
            .set("MENU", matches!(realm, Realm::Menu))
            .expect("Failed to set MENU global");

        {
            crate::script::exec(&lua, "hook.lua", "lua/libs/hook.luac");

            if !matches!(realm, Realm::Menu) {
                crate::script::exec(&lua, "net.lua", "lua/libs/net.luac");

                register_net_lib(&lua, usermsg_sender);
            } else {
                drop(usermsg_sender);
            }

            register_convar_lib(&lua, cvars);
            register_console_lib(&lua, binds, realm);
            crate::script::libs::register_localize_lib(&lua);
            crate::script::libs::register_vpk_lib(&lua);
            register_pad_lib(&lua, pads);
            register_vector3_lib(&lua);
            register_angle3_lib(&lua);
            register_usercmd_lib(&lua);
        }

        let render_queue = Arc::new(Mutex::new(RenderState {
            commands: Vec::new(),
            book: crate::ui::gfx::Book::new(),
            width: 0,
            height: 0,
        }));
        let pointer = Arc::new(Mutex::new(crate::script::libs::input::Pointer::new()));
        let webviews = if matches!(realm, Realm::Server) {
            None
        } else {
            let bank = Arc::new(Mutex::new(crate::ui::webview::Bank::open()));
            register_webview_lib(&lua, bank.clone(), pointer.clone(), render_queue.clone());

            Some(bank)
        };

        if !matches!(realm, Realm::Server) {
            register_surface_lib(&lua, render_queue.clone());
            register_input_lib(&lua, pointer.clone());
            crate::script::exec(&lua, "gui.lua", "lua/libs/gui.luac");
        }

        let entity_access: EntityAccess = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
        let anim_access: AnimAccess = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
        let sound_access: SoundAccess = Arc::new(AtomicPtr::new(sound));
        let physics_access: PhysicsAccess = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
        let brush_access = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
        let voxel_access = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
        let motion_access = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
        let nav_access = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
        let gen_settings = Arc::new(Mutex::new(crate::world::gen::GenSettings::new()));
        register_engine_lib(
            &lua,
            tick_interval,
            realm,
            brush_access.clone(),
            voxel_access.clone(),
            entity_access.clone(),
            physics_access.clone(),
            motion_access.clone(),
            Arc::clone(&gen_settings),
        );
        register_noise_lib(&lua);
        if !matches!(realm, Realm::Menu) {
            register_nav_lib(&lua, nav_access.clone(), matches!(realm, Realm::Server));
        }
        if matches!(realm, Realm::Server) {
            register_ai_lib(&lua);
        }
        register_biome_lib(
            &lua,
            Arc::clone(&gen_settings),
            matches!(realm, Realm::Server),
        );
        if !matches!(realm, Realm::Menu) {
            register_demo_lib(&lua);
            register_sound_lib(&lua, sound_access.clone());
            register_ents_lib(
                &lua,
                entity_access.clone(),
                anim_access.clone(),
                physics_access.clone(),
            );
            register_scripted_ents_lib(&lua);
            crate::script::libs::scripted_ents::load_scripted(&lua, realm);
            crate::script::autorun::load_autorun(&lua, realm);
        }

        let hook_table: mlua::Table = lua.globals().get("hook").unwrap();
        let hook_call_fn: mlua::Function = hook_table.get("call").unwrap();
        let hook_caller = lua.create_registry_value(hook_call_fn).unwrap();

        let net_table: mlua::Table = lua.globals().get("net").unwrap();
        let net_call_fn: mlua::Function = net_table.get("call").unwrap();
        let net_caller = lua.create_registry_value(net_call_fn).unwrap();

        Self {
            lua,
            realm,
            hook_caller,
            net_caller,
            //window_ptr,
            tick_interval,
            render_queue: render_queue.clone(),
            entity_access,
            anim_access,
            sound_access,
            physics_access,
            brush_access,
            voxel_access,
            pointer,
            motion_access,
            gen_settings,
            nav_access,
            webviews,
            usermsg_receiver,
        }
    }

    pub fn flush_webviews(&self) {
        let Some(webviews) = &self.webviews else {
            return;
        };
        let (uploads, messages) = webviews.lock().expect("Couldn't lock webview").drain();

        if !uploads.is_empty() {
            let mut queue = self.render_queue.lock().expect("Couldn't lock render queue");

            for upload in uploads {
                if !queue.book.live(upload.texture) {
                    continue;
                }

                queue.commands.push(DrawCommand::UpdateImage {
                    id: upload.texture,
                    width: upload.width,
                    height: upload.height,
                    bytes: upload.bytes,
                });
            }
        }

        if messages.is_empty() {
            return;
        }

        let Ok(webview) = self.lua.globals().get::<mlua::Table>("webview") else {
            return;
        };
        let Ok(listeners) = webview.get::<mlua::Table>("_listeners") else {
            return;
        };

        for (id, text) in messages {
            let Ok(func) = listeners.get::<mlua::Function>(id as i64) else {
                continue;
            };

            if let Err(err) = func.call::<()>(text) {
                log::warn!("[webview] {err}");
            }
        }
    }

    #[allow(dead_code)]
    pub fn webview_debug_message(&self, id: i64, text: &str) {
        let Some(webviews) = &self.webviews else {
            return;
        };

        webviews
            .lock()
            .expect("Couldn't lock webview")
            .debug_message(id as u64, text);
    }

    fn has_ents(&self) -> bool {
        !matches!(self.realm, Realm::Menu)
    }

    fn tag(&self) -> &'static str {
        match self.realm {
            Realm::Server => "sv",
            Realm::Client => "cl",
            Realm::Menu => "menu",
        }
    }

    pub fn think_entities(&self, cur_time: f64, frame_time: f64, tick_count: u64) {
        if !self.has_ents() {
            return;
        }

        publish_clock(&self.lua, cur_time, frame_time, tick_count);

        if let Err(err) = crate::script::libs::ents::think(&self.lua, cur_time) {
            log::error!("[LUA ENTS ERROR]: {}", err);
        }
    }

    pub fn sync_removed(&self, handles: &[(EntityHandle, bool)]) {
        if !self.has_ents() || handles.is_empty() {
            return;
        }

        if let Err(err) = crate::script::libs::ents::removed(&self.lua, handles) {
            log::error!("[LUA ENTS ERROR]: {}", err);
        }
    }

    pub fn net_spawn(&self, handle: EntityHandle, vars: &[NetVar], time: Option<f64>) -> bool {
        if !self.has_ents() {
            return false;
        }

        match crate::script::libs::ents::net_spawn(&self.lua, handle, vars, time) {
            Ok(known) => {
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "[{} netvar] spawn {:?} known={} [{}]",
                        self.tag(),
                        handle,
                        known,
                        vars_summary(vars)
                    );
                }

                known
            }
            Err(err) => {
                log::error!("[LUA ENTS ERROR]: {}", err);

                false
            }
        }
    }

    pub fn collect_networked(&self) -> Vec<EntityNetworked> {
        if !self.has_ents() {
            return Vec::new();
        }

        match crate::script::libs::ents::collect_networked(&self.lua) {
            Ok(entities) => {
                if !entities.is_empty() && log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "[{} netvar] dirty ents={} {}",
                        self.tag(),
                        entities.len(),
                        networked_summary(&entities)
                    );
                }

                entities
            }
            Err(err) => {
                log::error!("[LUA ENTS ERROR]: {}", err);

                Vec::new()
            }
        }
    }

    pub fn networked_state(&self, handle: Option<EntityHandle>) -> Vec<EntityNetworked> {
        if !self.has_ents() {
            return Vec::new();
        }

        match crate::script::libs::ents::networked_state(&self.lua, handle) {
            Ok(entities) => {
                if !entities.is_empty() && log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "[{} netvar] full state {:?} ents={} {}",
                        self.tag(),
                        handle,
                        entities.len(),
                        networked_summary(&entities)
                    );
                }

                entities
            }
            Err(err) => {
                log::error!("[LUA ENTS ERROR]: {}", err);

                Vec::new()
            }
        }
    }

    pub fn apply_networked(&self, entities: &[EntityNetworked], time: Option<f64>) {
        if !self.has_ents() || entities.is_empty() {
            return;
        }

        match crate::script::libs::ents::apply_networked(&self.lua, entities, time) {
            Ok((skipped, missing)) => {
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "[{} netvar] apply ents={} skipped_predicted={} missing={} {}",
                        self.tag(),
                        entities.len(),
                        skipped,
                        missing,
                        networked_summary(entities)
                    );
                }
            }
            Err(err) => {
                log::error!("[LUA ENTS ERROR]: {}", err);
            }
        }
    }

    pub fn present_networked(&self, time: f64) {
        if !self.has_ents() {
            return;
        }

        if let Err(err) = crate::script::libs::ents::present_interpolated(&self.lua, time) {
            log::error!("[LUA ENTS ERROR]: {}", err);
        }
    }

    pub fn anim_event(&self, handle: EntityHandle, name: &str) {
        if !self.has_ents() {
            return;
        }

        if let Err(err) = crate::script::libs::ents::anim_event(&self.lua, handle, name) {
            log::error!("[LUA ENTS ERROR]: {}", err);
        }
    }

    pub fn prepare_move(
        &self,
        handle: EntityHandle,
        cmd: mlua::Value,
        first_time: bool,
    ) -> Option<bool> {
        if !self.has_ents() {
            return None;
        }

        log::trace!(
            "[{} netvar] prepare_move {:?} first_time={}",
            self.tag(),
            handle,
            first_time
        );

        match crate::script::libs::ents::prepare_move(&self.lua, handle, cmd, first_time) {
            Ok(value) => Some(value),
            Err(err) => {
                log::error!("[LUA ENTS ERROR]: {}", err);

                None
            }
        }
    }

    pub fn finish_move(&self, handle: EntityHandle, cmd: mlua::Value) {
        if !self.has_ents() {
            return;
        }

        if let Err(err) = crate::script::libs::ents::finish_move(&self.lua, handle, cmd) {
            log::error!("[LUA ENTS ERROR]: {}", err);
        }
    }

    pub fn predicted_state(&self, handle: EntityHandle) -> Vec<EntityNetworked> {
        if !self.has_ents() {
            return Vec::new();
        }

        match crate::script::libs::ents::predicted_state(&self.lua, handle) {
            Ok(entities) => {
                if !entities.is_empty() && log::log_enabled!(log::Level::Trace) {
                    log::trace!(
                        "[{} netvar] predicted state {:?} ents={} {}",
                        self.tag(),
                        handle,
                        entities.len(),
                        networked_summary(&entities)
                    );
                }

                entities
            }
            Err(err) => {
                log::error!("[LUA ENTS ERROR]: {}", err);

                Vec::new()
            }
        }
    }

    pub fn begin_reconcile(&self, entities: &[EntityNetworked]) {
        if !self.has_ents() {
            return;
        }

        if !entities.is_empty() && log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "[{} netvar] reconcile begin ents={} {}",
                self.tag(),
                entities.len(),
                networked_summary(entities)
            );
        }

        if let Err(err) = crate::script::libs::ents::begin_reconcile(&self.lua, entities) {
            log::error!("[LUA ENTS ERROR]: {}", err);
        }
    }

    pub fn end_reconcile(&self) {
        if !self.has_ents() {
            return;
        }

        match crate::script::libs::ents::end_reconcile(&self.lua) {
            Ok(changed) => {
                if changed > 0 {
                    log::debug!(
                        "[{} netvar] reconcile mispredicted keys={}",
                        self.tag(),
                        changed
                    );
                }
            }
            Err(err) => {
                log::error!("[LUA ENTS ERROR]: {}", err);
            }
        }
    }

    pub fn owner_changed(&self, handle: EntityHandle, owner: EntityHandle) {
        if !self.has_ents() {
            return;
        }

        log::debug!("[{} netvar] owner {:?} -> {:?}", self.tag(), handle, owner);

        if let Err(err) = crate::script::libs::ents::owner_changed(&self.lua, handle, owner) {
            log::error!("[LUA ENTS ERROR]: {}", err);
        }
    }

    pub fn set_local_player(&self, handle: EntityHandle) {
        if !self.has_ents() {
            return;
        }

        log::debug!("[{} netvar] local player {:?}", self.tag(), handle);

        if let Err(err) = crate::script::libs::ents::set_local(&self.lua, handle) {
            log::error!("[LUA ENTS ERROR]: {}", err);
        }
    }

    pub fn poll_usermessage(&self) -> Option<(u32, Vec<u8>)> {
        self.usermsg_receiver.try_recv().ok()
    }

    pub fn run_hook<A, R>(
        &self,
        hook_name: &str,
        cur_time: f64,
        frame_time: f64,
        tick_count: u64,
        args: A,
    ) -> R
    where
        A: mlua::IntoLuaMulti,
        R: mlua::FromLuaMulti,
    {
        publish_clock(&self.lua, cur_time, frame_time, tick_count);
        let call_fn: mlua::Function = self.lua.registry_value(&self.hook_caller).unwrap();

        match call_fn.call::<R>((hook_name, args)) {
            Ok(ret) => ret,
            Err(err) => {
                log::error!("[LUA HOOK ERROR]: {}", err);
                self.lua
                    .load("return")
                    .eval()
                    .unwrap_or_else(|err| panic!("hook return recovery failed: {err}"))
            }
        }
    }

    pub fn run_usermessage<A: mlua::IntoLuaMulti>(
        &self,
        hash: u32,
        cur_time: f64,
        frame_time: f64,
        tick_count: u64,
        args: A,
    ) {
        publish_clock(&self.lua, cur_time, frame_time, tick_count);
        let call_fn: mlua::Function = self.lua.registry_value(&self.net_caller).unwrap();

        if let Err(err) = call_fn.call::<()>((hash, args)) {
            log::error!("[LUA HOOK ERROR]: {}", err);
        }
    }
}
