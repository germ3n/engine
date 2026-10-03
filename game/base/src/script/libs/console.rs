use crate::console::AUTOCOMPLETE_KEY;
use crate::input::Binds;
use mlua::{Error, Lua, Value};
use r#macro::document;
use std::sync::{Arc, Mutex};

#[document(
    kind = "library",
    name = "console",
    realm = "shared",
    summary = "Runs console commands on this realm."
)]
fn console_lib() {}

#[document(
    parent = "console",
    name = "run",
    kind = "function",
    realm = "shared",
    summary = "Runs one console line on this realm.",
    params = {
        line = { ty = "string", desc = "Command line, without a trailing newline." },
    },
)]
fn console_run() {}

#[document(
    parent = "console",
    name = "autocomplete",
    kind = "function",
    realm = "shared",
    summary = "Sets this realm's optional Tab callback. Pass nil to clear it.",
    params = {
        callback = { ty = "function", desc = "Called as callback(line, prefix) and returns a list of strings. Nil clears it." },
    },
)]
fn console_autocomplete() {}

pub fn register_console_lib(lua: &Lua, binds: Arc<Mutex<Binds>>) {
    let table = lua.create_table().expect("Failed to create console table");
    let shared = binds.clone();

    table
        .set(
            "run",
            lua.create_function(move |_, line: String| {
                let mut binds = shared
                    .lock()
                    .map_err(|_| Error::RuntimeError("binds lock poisoned".to_string()))?;
                crate::console::exec_line(&line, &mut binds).map_err(Error::RuntimeError)?;
                Ok(())
            })
            .expect("[engine] Failed to create console.run"),
        )
        .expect("[engine] Failed setting console.run");

    table
        .set(
            "autocomplete",
            lua.create_function(|lua, value: Value| match value {
                Value::Nil => {
                    lua.unset_named_registry_value(AUTOCOMPLETE_KEY)?;

                    Ok(())
                }
                Value::Function(func) => {
                    lua.set_named_registry_value(AUTOCOMPLETE_KEY, func)?;

                    Ok(())
                }
                _ => Err(Error::RuntimeError(
                    "console.autocomplete expects a function or nil".to_string(),
                )),
            })
            .expect("[engine] Failed to create console.autocomplete"),
        )
        .expect("[engine] Failed setting console.autocomplete");

    lua.globals()
        .set("console", table)
        .expect("[engine] Failed to set console table");
}
