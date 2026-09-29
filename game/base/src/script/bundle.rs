pub const HOOK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lua/hook.luac"));
pub const NET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lua/net.luac"));
pub const VECTOR3: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lua/vector3.luac"));
pub const ANGLE3: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lua/angle3.luac"));
pub const MENU: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lua/menu.luac"));

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
    use std::sync::Arc;

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[test]
    fn bundled_scripts_are_bytecode() {
        let scripts = [HOOK, NET, VECTOR3, ANGLE3, MENU];

        for bytes in scripts {
            assert!(bytes.starts_with(b"\x1bLJ"));
        }

        assert!(!contains(HOOK, b"storage[event_id][identifier] = callback"));
        assert!(!contains(
            MENU,
            b"todo: maybe error when exceeding capacity"
        ));
    }

    #[test]
    fn bundled_scripts_run() {
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
        let engine = ScriptEngine::new(Realm::Server, 1.0 / 60.0, Arc::new(cvars));
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
        load_bytecode(&engine.lua, "menu.lua", MENU).exec().unwrap();
    }
}
