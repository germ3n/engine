use crate::input::Binds;
use mlua::{Error, Lua};
use std::sync::{Arc, Mutex};

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

    lua.globals()
        .set("console", table)
        .expect("[engine] Failed to set console table");
}
