use crate::console::{ConVar, ConVarValue};
use mlua::prelude::LuaUserDataMethods;
use mlua::{Error, Function, Lua, UserData, Value};
use r#macro::document;
use std::collections::HashMap;
use std::sync::Arc;

#[document(
    kind = "library",
    name = "cvar",
    realm = "shared",
    summary = "Looks up console variables on this realm."
)]
fn cvar_lib() {}

#[document(
    parent = "cvar",
    name = "get",
    kind = "function",
    realm = "shared",
    summary = "Finds a console variable by name.",
    params = {
        name = { ty = "string", desc = "ConVar name." },
    },
    returns = { ty = "ConVar", desc = "The variable." },
    panics = "Errors when the name is not registered.",
)]
fn cvar_get() {}

#[document(
    kind = "class",
    name = "ConVar",
    realm = "shared",
    summary = "One console variable. Integer and float values have separate getters and setters."
)]
fn convar_class() {}

#[document(
    parent = "ConVar",
    name = "get_value_int",
    kind = "method",
    realm = "shared",
    summary = "Reads an integer value.",
    returns = { ty = "number", desc = "The stored integer." },
    panics = "Errors when the variable is not an integer.",
)]
fn convar_get_value_int() {}

#[document(
    parent = "ConVar",
    name = "get_value_float",
    kind = "method",
    realm = "shared",
    summary = "Reads a float value.",
    returns = { ty = "number", desc = "The stored float." },
    panics = "Errors when the variable is not a float.",
)]
fn convar_get_value_float() {}

#[document(
    parent = "ConVar",
    name = "set_value_int",
    kind = "method",
    realm = "shared",
    summary = "Writes an integer and runs change callbacks with that number.",
    params = {
        value = { ty = "number", desc = "New integer." },
    },
    panics = "Errors when a callback fails.",
)]
fn convar_set_value_int() {}

#[document(
    parent = "ConVar",
    name = "set_value_float",
    kind = "method",
    realm = "shared",
    summary = "Writes a float and runs change callbacks with that number.",
    params = {
        value = { ty = "number", desc = "New float." },
    },
    panics = "Errors when a callback fails.",
)]
fn convar_set_value_float() {}

#[document(
    parent = "ConVar",
    name = "add_change_callback",
    kind = "method",
    realm = "shared",
    summary = "Calls a function after set_value_int or set_value_float. The function receives the new number.",
    params = {
        callback = { ty = "function", desc = "function(value)" },
    },
)]
fn convar_add_change_callback() {}

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
                    return Ok(LuaConVar { cvar: cvar.clone() });
                }
                Err(Error::RuntimeError(format!(
                    "ConVar '{}' not found",
                    cvar_name
                )))
            })
            .expect("[engine] Failed to create cvar.get function"),
        )
        .expect("[engine] Failed setting cvar.get function");

    lua.globals()
        .set("cvar", cvar_table)
        .expect("[net] Failed to set cvar table");
}
