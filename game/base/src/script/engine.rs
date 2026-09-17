use crate::{network::usermessage::hash_usermessage_name, ui::Color};
use crate::ui::window::Window;
use mlua::{Lua, RegistryKey};
use std::sync::{Arc, Mutex, atomic::{AtomicU64, AtomicU32, Ordering}};
use crate::network::usermessage::UserMsgWriter;
//todo: move functions to seperate libs.rs

#[derive(Clone, Copy)]
pub enum Realm {
    Client,
    Server,
    Menu,
}

pub struct DynWindowPtr(pub Option<*mut dyn Window>);
unsafe impl Send for DynWindowPtr {}
unsafe impl Sync for DynWindowPtr {}

pub struct ScriptEngine {
    pub lua: Lua,
    pub realm: Realm,
    pub hook_caller: RegistryKey,
    pub net_caller: RegistryKey,
    pub window_ptr: Arc<Mutex<DynWindowPtr>>,
    pub tick_interval: f64,
    pub cur_time: Arc<AtomicU64>,
    pub frame_time: Arc<AtomicU64>,
    pub tick_count: Arc<AtomicU64>,
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
        let globals = lua.globals();

        globals.set("CLIENT", matches!(realm, Realm::Client)).unwrap();
        globals.set("SERVER", matches!(realm, Realm::Server)).unwrap();
        globals.set("MENU", matches!(realm, Realm::Menu)).unwrap();

        //let tick_interval = Arc::new(Mutex::new(tick_interval));

        // globally shared libs
        {
            let hook_lib_data = include_bytes!("../lua/hook.lua");
            lua.load(&hook_lib_data[..])
                .exec()
                .expect("Failed to execute hook.lua");

            let net_lib_data = include_bytes!("../lua/net.lua");
                lua.load(&net_lib_data[..])
                    .exec()
                    .expect("Failed to execute net.lua");

            {
                let engine_table = lua.create_table().expect("Failed to create engine table");
                
                engine_table.set("tick_interval", lua.create_function(move |_, (): ()| {
                    Ok(tick_interval)
                }).unwrap()).unwrap();

                let ct_clone = cur_time.clone();
                engine_table.set("curtime", lua.create_function(move |_, (): ()| {
                    Ok(f64::from_bits(ct_clone.load(Ordering::Relaxed)))
                }).unwrap()).unwrap();

                let ft_clone = frame_time.clone();
                engine_table.set("frametime", lua.create_function(move |_, (): ()| {
                    Ok(f64::from_bits(ft_clone.load(Ordering::Relaxed)))
                }).unwrap()).unwrap();

                let tc_clone = tick_count.clone();
                engine_table.set("tick_count", lua.create_function(move |_, (): ()| {
                    Ok(tc_clone.load(Ordering::Relaxed))
                }).unwrap()).unwrap();

                globals.set("engine", engine_table).unwrap()
            }

            {
                let net_table: mlua::Table = lua.globals().get("net").expect("Couldn't get net table");
                
                let writer_func = lua.create_function(move |_, capacity: Option<u32>| {
                    let writer = if let Some(cap) = capacity {
                        UserMsgWriter::with_capacity(cap as usize)
                    } else {
                        UserMsgWriter::new()
                    };
                    Ok(writer)
                }).expect("Failed to create writer function");
            
                net_table.set("writer", writer_func).expect("Failed to set writer");
            
                let send_func = lua.create_function(move |_, (msg_name, writer_data): (String, mlua::AnyUserData)| {
                    let msg_hash = hash_usermessage_name(msg_name.as_str());
                    let writer = writer_data.borrow::<UserMsgWriter>()?;

                    println!("{}", msg_hash);

                    Ok(())
                }).expect("Failed to create send function");
            
                net_table.set("send", send_func).expect("Failed to set send function");
            }
        }

        match realm { // per-realm logic
            Realm::Client => {},
            Realm::Server => {},
            Realm::Menu => {},
        }

        let window_ptr = Arc::new(Mutex::new(DynWindowPtr(None)));
        match realm { // mixed realm logic
            Realm::Client | Realm::Menu => {
                {   
                    { // surface lib
                        let surface_table = lua.create_table().unwrap();
                        let wp_clone = window_ptr.clone();
                        surface_table.set("draw_rect", lua.create_function(move |_, (x, y, w, h, r, g, b, a): (f32, f32, f32, f32, u8, u8, u8, u8)| {
                            let guard = wp_clone.lock().unwrap();
                            if let Some(ptr) = guard.0 {
                                unsafe { (*ptr).draw_rectangle(x, y, w, h, Color::ColorRGBA { r, g, b, a }); }
                            }
                            Ok(())
                        }).unwrap()).unwrap();

                        let wp_clone = window_ptr.clone();
                        surface_table.set("draw_outlined_rect", lua.create_function(move |_, (x, y, w, h, thickness, r, g, b, a): (f32, f32, f32, f32, f32, u8, u8, u8, u8)| {
                            let guard = wp_clone.lock().unwrap();
                            if let Some(ptr) = guard.0 {
                                unsafe { (*ptr).draw_outlined_rectangle(x, y, w, h, thickness, Color::ColorRGBA { r, g, b, a }); }
                            }
                            Ok(())
                        }).unwrap()).unwrap();

                        let wp_clone = window_ptr.clone();
                        surface_table.set("draw_text", lua.create_function(move |_, (font, text, x, y, scale, r, g, b, a): (String, String, f32, f32, f32, u8, u8, u8, u8)| {
                            let guard = wp_clone.lock().unwrap();
                            if let Some(ptr) = guard.0 {
                                unsafe { (*ptr).draw_text(&font, &text, x, y, scale, Color::ColorRGBA { r, g, b, a }); }
                            }
                            Ok(())
                        }).unwrap()).unwrap();

                        globals.set("surface", surface_table).unwrap();
                    }
                }
            },
            _ => {}
        };

        let hook_table: mlua::Table = globals.get("hook").unwrap();
        let hook_call_fn: mlua::Function = hook_table.get("call").unwrap();
        let hook_caller = lua.create_registry_value(hook_call_fn).unwrap();

        let net_table: mlua::Table = globals.get("net").unwrap();
        let net_call_fn: mlua::Function = net_table.get("call").unwrap();
        let net_caller = lua.create_registry_value(net_call_fn).unwrap();

        Self {
            lua,
            realm,
            hook_caller,
            net_caller,
            window_ptr,
            tick_interval,
            cur_time,
            frame_time,
            tick_count,
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