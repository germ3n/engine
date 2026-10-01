use crate::script::{pick_scripts, run_file, Realm};
use mlua::{Function, Lua, Table, Value};
use std::collections::BTreeMap;

const ENTITIES_PREFIX: &str = "lua/entities/";

pub fn register_scripted_ents_lib(lua: &Lua) {
    crate::script::exec(lua, "scripted_ents.lua", "lua/libs/scripted_ents.luac");
}

fn pick(files: &[String], stem: &str) -> Option<String> {
    let compiled = format!("/{stem}.luac");
    let source = format!("/{stem}.lua");

    files
        .iter()
        .find(|name| name.ends_with(&compiled) || name.ends_with(&source))
        .cloned()
}

fn load_class(lua: &Lua, class: &str, files: &[String], realm: Realm) -> Result<(), String> {
    let globals = lua.globals();
    let ent = lua.create_table().map_err(|err| err.to_string())?;
    globals.set("ENT", ent).map_err(|err| err.to_string())?;

    let realm_stem = match realm {
        Realm::Server => "init",
        Realm::Client => "cl_init",
        Realm::Menu => return Ok(()),
    };

    for stem in ["shared", realm_stem] {
        if let Some(path) = pick(files, stem) {
            run_file(lua, &path)?;
        }
    }

    let ent: Value = globals.get("ENT").map_err(|err| err.to_string())?;
    globals.set("ENT", Value::Nil).map_err(|err| err.to_string())?;

    let Value::Table(ent) = ent else {
        return Err("ENT is not a table".to_string());
    };

    let scripted_ents: Table = globals.get("scripted_ents").map_err(|err| err.to_string())?;
    let register: Function = scripted_ents.get("register").map_err(|err| err.to_string())?;

    register.call::<()>((ent, class)).map_err(|err| err.to_string())
}

pub fn load_entities(lua: &Lua, realm: Realm) {
    let Some(fs) = crate::fs::try_global() else {
        return;
    };

    let mut classes: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in fs.list_prefix(ENTITIES_PREFIX) {
        let Some(rest) = path.strip_prefix(ENTITIES_PREFIX) else {
            continue;
        };

        let mut parts = rest.split('/');
        let (Some(class), Some(_file), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };

        classes.entry(class.to_string()).or_default().push(path.clone());
    }

    let scripted_ents: Table = lua
        .globals()
        .get("scripted_ents")
        .expect("Failed to get scripted_ents");
    scripted_ents
        .set("_loading", true)
        .expect("[scripted_ents] Failed setting _loading");

    for (class, files) in &classes {
        let files = pick_scripts(&fs, files);

        match load_class(lua, class, &files, realm) {
            Ok(()) => log::info!("[scripted_ents] loaded {class}"),
            Err(err) => log::error!("[scripted_ents] {class}: {err}"),
        }

        let _ = lua.globals().set("ENT", Value::Nil);
    }

    scripted_ents
        .set("_loading", false)
        .expect("[scripted_ents] Failed setting _loading");
    let resolve_all: Function = scripted_ents
        .get("_resolve_all")
        .expect("Failed to get scripted_ents._resolve_all");

    if let Err(err) = resolve_all.call::<()>(()) {
        log::error!("[scripted_ents] resolve failed: {err}");
    }
}
