pub fn bytes(path: &str) -> Vec<u8> {
    match crate::fs::read(path) {
        Ok(bytes) => {
            log::info!("[lua] loaded {path} from fs ({} bytes)", bytes.len());

            bytes
        }
        Err(err) => panic!("[lua] failed to load {path}: {err}"),
    }
}

pub fn load_bytecode<'a>(lua: &mlua::Lua, name: &str, bytes: &'a [u8]) -> mlua::chunk::Chunk<'a> {
    lua.load(bytes)
        .set_name(name)
        .set_mode(mlua::chunk::ChunkMode::Binary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::{ConVar, ConVarValue};
    use crate::script::{Realm, ScriptEngine};
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
    fn bundled_scripts_are_bytecode() {
        boot_fs();
        let scripts = [
            "lua/libs/hook.luac",
            "lua/libs/net.luac",
            "lua/libs/vector3.luac",
            "lua/libs/angle3.luac",
            "lua/menu/menu.luac",
        ];

        for path in scripts {
            let bytes = bytes(path);
            assert!(bytes.starts_with(b"\x1bLJ"), "{path}");
            assert!(!contains(&bytes, b"storage[event_id][identifier] = callback"));
        }
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[test]
    fn bundled_scripts_run() {
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
        let engine = ScriptEngine::new(Realm::Server, 1.0 / 60.0, Arc::new(cvars), binds, pads);
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
        engine.run_hook("Ping", 0.0, 0.0, 1, ());
        let menu = bytes("lua/menu/menu.luac");
        load_bytecode(&engine.lua, "menu.lua", &menu)
            .exec()
            .unwrap();
    }
}
