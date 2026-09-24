use std::sync::Arc;
use crate::console::{ConVar, ConVarValue};
use std::collections::HashMap;
use mlua::{Error, Function, Lua, UserData, Value};
use mlua::prelude::LuaUserDataMethods;

pub struct LuaConVar {
    pub cvar: Arc<ConVar>,
}

impl UserData for LuaConVar {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get_value_int", |_, this, ()| {
            let val = this.cvar.value.lock().unwrap();
            match *val {
                ConVarValue::Integer(v) => Ok(v),
                _ => Err(Error::RuntimeError("ConVar is not an integer".to_string())),
            }
        });

        methods.add_method("get_value_float", |_, this, ()| {
            let val = this.cvar.value.lock().unwrap();
            match *val {
                ConVarValue::Float(v) => Ok(v),
                _ => Err(Error::RuntimeError("ConVar is not a float".to_string())),
            }
        });

        methods.add_method("set_value_int", |lua, this, new_val: i64| {
            this.cvar.set_value(ConVarValue::Integer(new_val));
            notify_callbacks(lua, &this.cvar, Value::Integer(new_val))?;
            Ok(())
        });

        methods.add_method("set_value_float", |lua, this, new_val: f64| {
            this.cvar.set_value(ConVarValue::Float(new_val));
            notify_callbacks(lua, &this.cvar, Value::Number(new_val))?;
            Ok(())
        });

        methods.add_method("add_change_callback", |lua, this, func: Function| {
            let key = lua.create_registry_value(func)?;
            let mut callbacks = this.cvar.callbacks.lock().unwrap();
            callbacks.push(key);
            Ok(())
        });
    }
}

fn notify_callbacks(lua: &Lua, cvar: &ConVar, new_value: Value) -> Result<(), Error> {
    let callbacks = cvar.callbacks.lock().unwrap();
    for key in callbacks.iter() {
        let func: Function = lua.registry_value(key)?;
        func.call::<()>(new_value.clone())?;
    }
    Ok(())
}

pub fn register_convar_lib(lua: &Lua, cvars: Arc<HashMap<String, Arc<ConVar>>>) {
    let cvar_table = lua.create_table().expect("Failed to create cvar table");

    cvar_table
        .set(
            "get",
            lua.create_function(move |_, cvar_name: String| {
                if let Some(cvar) = cvars.get(&cvar_name) {
                    return Ok(LuaConVar {
                        cvar: cvar.clone(),
                    });
                }
                Err(Error::RuntimeError(format!("ConVar '{}' not found", cvar_name)))
            })
            .expect("[engine] Failed to create cvar.get function"),
        )
        .expect("[engine] Failed setting cvar.get function");

    lua.globals()
        .set("cvar", cvar_table)
        .expect("[net] Failed to set cvar table");
}