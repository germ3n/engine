use mlua::{Lua, RegistryKey, StdLib, LuaOptions};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::Receiver;
use crate::script::libs::{
    register_angle3_lib, register_convar_lib, register_engine_lib, 
    register_net_lib, register_surface_lib, register_vector3_lib
};
use crate::script::libs::engine::publish_clock;
use crate::ui::Color;
use std::collections::HashMap;
use crate::console::ConVar;

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
    pub render_queue: RenderQueue,
    usermsg_receiver: Receiver<(u32, Vec<u8>)>,
}

impl ScriptEngine {
    pub fn new(
        realm: Realm,
        tick_interval: f64,
        cvars: Arc<HashMap<String, Arc<ConVar>>>
    ) -> Self {
        let (usermsg_sender, usermsg_receiver) = std::sync::mpsc::channel();
        let lua = unsafe { Lua::unsafe_new_with(StdLib::ALL, LuaOptions::default()) };
        lua.globals().set("CLIENT", matches!(realm, Realm::Client)).expect("Failed to set CLIENT global");
        lua.globals().set("SERVER", matches!(realm, Realm::Server)).expect("Failed to set SERVER global");
        lua.globals().set("MENU", matches!(realm, Realm::Menu)).expect("Failed to set MENU global");

        {
            let hook_lib_data = include_bytes!("libs/hook.lua");
            lua.load(&hook_lib_data[..]).exec().expect("Failed to execute hook.lua");

            register_engine_lib(&lua, tick_interval);
            if !matches!(realm, Realm::Menu) {
                let net_lib_data = include_bytes!("libs/net.lua");
                lua.load(&net_lib_data[..]).exec().expect("Failed to execute net.lua");

                register_net_lib(&lua, usermsg_sender);
            } else {
                drop(usermsg_sender);
            }

            register_convar_lib(&lua, cvars);
            register_vector3_lib(&lua);
            register_angle3_lib(&lua);
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
            render_queue: render_queue.clone(),
            usermsg_receiver,
        }
    }

    pub fn poll_usermessage(&self) -> Option<(u32, Vec<u8>)> {
        self.usermsg_receiver.try_recv().ok()
    }

    pub fn run_hook<A: mlua::IntoLuaMulti>(&self, hook_name: &str, cur_time: f64, frame_time: f64, tick_count: u64, args: A) {
        publish_clock(&self.lua, cur_time, frame_time, tick_count);
        let call_fn: mlua::Function = self.lua.registry_value(&self.hook_caller).unwrap();

        if let Err(err) = call_fn.call::<()>((hook_name, args)) {
            eprintln!("[LUA HOOK ERROR]: {}", err);
        }
    }

    pub fn run_usermessage<A: mlua::IntoLuaMulti>(&self, hash: u32, cur_time: f64, frame_time: f64, tick_count: u64, args: A) {
        publish_clock(&self.lua, cur_time, frame_time, tick_count);
        let call_fn: mlua::Function = self.lua.registry_value(&self.net_caller).unwrap();

        if let Err(err) = call_fn.call::<()>((hash, args)) {
            eprintln!("[LUA HOOK ERROR]: {}", err);
        }
    }
}