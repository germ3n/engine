use crate::console::{self, AUTOCOMPLETE_KEY};
use crate::input::Binds;
use crate::script::Realm;
use mlua::{Error, Lua, Table, Value};
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

#[document(
    parent = "console",
    name = "submit",
    kind = "function",
    realm = "shared",
    summary = "Runs a line on the server, then on the client if the server does not know it. Used by the in-game console.",
    params = {
        line = { ty = "string", desc = "Command line. Semicolons split commands." },
    },
    returns = { ty = "table", desc = "Rows with side, text, detail, and error." },
)]
fn console_submit() {}

#[document(
    parent = "console",
    name = "complete",
    kind = "function",
    realm = "shared",
    summary = "Tab-completes a line the same way the terminal does, including this realm's autocomplete callback.",
    params = {
        line = { ty = "string", desc = "Text currently in the console field." },
    },
    returns = { ty = "string", desc = "The completed line, then a list of matches when several remain." },
)]
fn console_complete() {}

pub fn register_console_lib(lua: &Lua, binds: Arc<Mutex<Binds>>, realm: Realm) {
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

    table
        .set(
            "submit",
            lua.create_function(|lua, line: String| {
                let outcomes = console::submit_shared(&line);
                let quit = outcomes.iter().any(|outcome| outcome.quit);
                let rows = rows_from(lua, &outcomes)?;

                if quit {
                    std::process::exit(0);
                }

                Ok(rows)
            })
            .expect("[engine] Failed to create console.submit"),
        )
        .expect("[engine] Failed setting console.submit");

    table
        .set(
            "complete",
            lua.create_function(move |lua, line: String| {
                let (text, matches) = console::complete_shared(&line, lua, realm);
                let list = lua.create_table()?;
                let mut idx = 0;

                while idx < matches.len() {
                    list.set(idx + 1, matches[idx].as_str())?;
                    idx += 1;
                }

                Ok((text, list))
            })
            .expect("[engine] Failed to create console.complete"),
        )
        .expect("[engine] Failed setting console.complete");

    lua.globals()
        .set("console", table)
        .expect("[engine] Failed to set console table");
}

fn rows_from(lua: &Lua, outcomes: &[console::Outcome]) -> Result<Table, Error> {
    let rows = lua.create_table()?;
    let mut idx = 0;

    while idx < outcomes.len() {
        let outcome = &outcomes[idx];
        let row = lua.create_table()?;
        row.set("side", outcome.side)?;
        row.set("text", outcome.line.as_str())?;
        row.set("detail", outcome.detail.clone().unwrap_or_default())?;
        row.set("error", outcome.error.clone().unwrap_or_default())?;
        rows.set(idx + 1, row)?;
        idx += 1;
    }

    Ok(rows)
}
