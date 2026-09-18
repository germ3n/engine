use std::sync::Arc;
use crate::console::ConVar;
use mlua::UserData;
use mlua::prelude::LuaUserDataMethods;
use crate::console::ConVarValue;
use mlua::Error;
use std::collections::HashMap;
use mlua::Lua;

pub struct LuaConVar {
    pub cvar: Arc<ConVar>
}

impl UserData for LuaConVar {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("get_value_int", |_, this, ()| {
            match this.cvar.value {
                ConVarValue::Integer(val) => Ok(val),
                _ => Err(Error::RuntimeError("ConVar is not an integer".to_string())),
            }
        });

        /*methods.add_method_mut("set_value_int", |_, this, value: i64| {
            this.cvar.set_value(ConVarValue::Integer(value));
            Ok(())
        });*/

        methods.add_method_mut("get_value_float", |_, this, ()| {
            match this.cvar.value {
                ConVarValue::Float(val) => Ok(val),
                _ => Err(Error::RuntimeError("ConVar is not a float".to_string())),
            }
        });

        /*methods.add_method_mut("set_value_float", |_, this, value: f64| {
            this.cvar.set_value(ConVarValue::Float(value));
            Ok(())
        });*/
    }
}

pub fn register_convar_lib(lua: &Lua, cvars: Arc<HashMap<String, Arc<ConVar>>>) {
    let cvar_table = lua.create_table().expect("Failed to create cvar table");  

    cvar_table.set("get", lua.create_function(move |_, cvar: String| {
        let cvar = cvars.get(&cvar);
        if let Some(cvar) = cvar {
            return Ok(LuaConVar {
                cvar: cvar.clone()
            })
        }
        
        Err(Error::RuntimeError("ConVar not found".to_string()))
    }).expect("[engine] Failed to create tick_count function"))
      .expect("[engine] Failed setting tick_count function");

    lua.globals().set("cvar", cvar_table).expect("[net] Failed to set engine table")
}