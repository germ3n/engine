pub mod autorun;
pub mod engine;
pub mod libs;

pub use engine::Realm;
pub use engine::ScriptEngine;

use std::collections::BTreeMap;

pub fn run_file(lua: &mlua::Lua, path: &str) -> Result<(), String> {
    let bytes = crate::fs::read(path)?;
    let mode = if bytes.starts_with(b"\x1bLJ") {
        mlua::chunk::ChunkMode::Binary
    } else {
        mlua::chunk::ChunkMode::Text
    };

    lua.load(&bytes)
        .set_name(path)
        .set_mode(mode)
        .exec()
        .map_err(|err| err.to_string())
}

pub fn pick_scripts(fs: &crate::fs::Fs, paths: &[String]) -> Vec<String> {
    let mut picked: BTreeMap<&str, (&String, Option<usize>, bool)> = BTreeMap::new();

    for path in paths {
        let (stem, compiled) = if let Some(stem) = path.strip_suffix(".luac") {
            (stem, true)
        } else if let Some(stem) = path.strip_suffix(".lua") {
            (stem, false)
        } else {
            continue;
        };
        let priority = fs.priority(path);

        match picked.get(stem) {
            Some((_, best, best_compiled)) if (*best, *best_compiled) >= (priority, compiled) => {}
            _ => {
                picked.insert(stem, (path, priority, compiled));
            }
        }
    }

    picked
        .into_values()
        .map(|(path, _, _)| path.clone())
        .collect()
}

pub fn exec(lua: &mlua::Lua, name: &str, path: &str) {
    let bytes = read(path);
    lua.load(&bytes)
        .set_name(name)
        .set_mode(mlua::chunk::ChunkMode::Binary)
        .exec()
        .unwrap_or_else(|err| panic!("Failed to execute {name}: {err}"));
}

pub fn eval<R: mlua::FromLuaMulti>(lua: &mlua::Lua, name: &str, path: &str) -> R {
    let bytes = read(path);

    lua.load(&bytes)
        .set_name(name)
        .set_mode(mlua::chunk::ChunkMode::Binary)
        .eval()
        .unwrap_or_else(|err| panic!("Failed to evaluate {name}: {err}"))
}

fn read(path: &str) -> Vec<u8> {
    match crate::fs::read(path) {
        Ok(bytes) => {
            log::info!("[lua] loaded {path} from fs ({} bytes)", bytes.len());

            bytes
        }
        Err(err) => panic!("[lua] failed to load {path}: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::{ConVar, ConVarValue};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn boot_fs() {
        if crate::fs::try_global().is_some() {
            return;
        }

        let fs = crate::fs::Fs::boot().expect("fs boot");
        crate::fs::set_global(Arc::new(fs));
    }

    #[test]
    fn scripts_are_bytecode() {
        boot_fs();
        let scripts = [
            "lua/libs/hook.luac",
            "lua/libs/net.luac",
            "lua/libs/vector3.luac",
            "lua/libs/angle3.luac",
            "lua/libs/ents.luac",
            "lua/libs/scripted_ents.luac",
            "lua/menu/menu.luac",
        ];

        for path in scripts {
            let bytes = read(path);
            assert!(bytes.starts_with(b"\x1bLJ"), "{path}");
        }
    }

    #[test]
    fn scripts_run() {
        boot_fs();
        let mut cvars = HashMap::new();
        cvars.insert(
            "sv_gravity".to_string(),
            Arc::new(ConVar::new(
                "sv_gravity",
                ConVarValue::Float(24.0),
                "World gravity",
                Some(false),
                Some(true),
            )),
        );
        let binds = Arc::new(Mutex::new(crate::input::Binds::defaults()));
        let pads = Arc::new(Mutex::new(crate::platform::PadCache::new()));
        let engine = ScriptEngine::new(
            Realm::Server,
            1.0 / 60.0,
            Arc::new(cvars),
            binds,
            pads,
            std::ptr::null_mut(),
        );
        let len: f64 = engine
            .lua
            .load("return Vector3(3, 4, 0):len()")
            .eval()
            .unwrap();
        assert_eq!(len, 5.0);
        let yaw: f64 = engine.lua.load("return Angle3(0, 90, 0).y").eval().unwrap();
        assert_eq!(yaw, 90.0);
        engine
            .lua
            .load("hook.add('Ping', 'id', function() end)")
            .exec()
            .unwrap();
        let _: () = engine.run_hook("Ping", 0.0, 0.0, 1, ());
        engine
            .lua
            .load("hook.add('Skip', 'id', function() return true, 'nope', 3 end)")
            .exec()
            .unwrap();
        let (skip, reason, n): (bool, String, i32) = engine.run_hook("Skip", 0.0, 0.0, 1, ());
        assert!(skip);
        assert_eq!(reason, "nope");
        assert_eq!(n, 3);
        let none: Option<bool> = engine.run_hook("Missing", 0.0, 0.0, 1, ());
        assert_eq!(none, None);
        exec(&engine.lua, "menu.lua", "lua/menu/menu.luac");
    }
}
