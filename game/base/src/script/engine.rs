use crate::ui::window::Window;
use mlua::{Lua, RegistryKey};
use std::sync::{Arc, Mutex, atomic::{AtomicU64}};
use crate::script::libs::{register_engine_lib, register_surface_lib};
use crate::script::libs::register_net_lib;
use crate::ui::Color;

pub enum DrawCommand {
    Rect { x: f32, y: f32, w: f32, h: f32, color: Color },
    OutlinedRect { x: f32, y: f32, w: f32, h: f32, thickness: f32, color: Color },
    Text { font: mlua::LuaString, text: mlua::LuaString, x: f32, y: f32, scale: f32, color: Color },
}

pub type RenderQueue = Arc<Mutex<Vec<DrawCommand>>>;

#[derive(Clone, Copy)]
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
    pub tick_interval: f64,
    pub cur_time: Arc<AtomicU64>,
    pub frame_time: Arc<AtomicU64>,
    pub tick_count: Arc<AtomicU64>,
    pub render_queue: RenderQueue,
}

impl ScriptEngine {
    pub fn new(
        realm: Realm, 
        tick_interval: f64,
        cur_time: Arc<AtomicU64>,
        frame_time: Arc<AtomicU64>,
        tick_count: Arc<AtomicU64>
    ) -> Self {
        let lua = Lua::new();
        lua.globals().set("CLIENT", matches!(realm, Realm::Client)).expect("Failed to set CLIENT global");
        lua.globals().set("SERVER", matches!(realm, Realm::Server)).expect("Failed to set SERVER global");
        lua.globals().set("MENU", matches!(realm, Realm::Menu)).expect("Failed to set MENU global");

        {
            let hook_lib_data = include_bytes!("libs/hook.lua");
            lua.load(&hook_lib_data[..]).exec().expect("Failed to execute hook.lua");

            register_engine_lib(&lua, tick_interval, cur_time.clone(), frame_time.clone(), tick_count.clone());
            if !matches!(realm, Realm::Menu) {
                let net_lib_data = include_bytes!("libs/net.lua");
                lua.load(&net_lib_data[..]).exec().expect("Failed to execute net.lua");

                register_net_lib(&lua);
            }
        }

        let render_queue = Arc::new(Mutex::new(Vec::new()));
        if !matches!(realm, Realm::Server) {
            register_surface_lib(&lua, render_queue.clone());
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
            cur_time,
            frame_time,
            tick_count,
            render_queue: render_queue.clone(),
        }
    }

    pub fn run_hook<A: mlua::IntoLuaMulti>(&self, hook_name: &str, args: A) {
        let call_fn: mlua::Function = self.lua.registry_value(&self.hook_caller).unwrap();
        
        if let Err(err) = call_fn.call::<()>((hook_name, args)) {
            eprintln!("[LUA HOOK ERROR]: {}", err);
        }
    }

    pub fn run_usermessage<A: mlua::IntoLuaMulti>(&self, hash: u32, args: A) {
        let call_fn: mlua::Function = self.lua.registry_value(&self.net_caller).unwrap();
        
        if let Err(err) = call_fn.call::<()>((hash, args)) {
            eprintln!("[LUA HOOK ERROR]: {}", err);
        }
    }
}