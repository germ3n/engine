use mlua::Lua;

const ENGINE_TABLE: &str = "engine";

pub fn publish_clock(lua: &Lua, cur_time: f64, frame_time: f64, tick_count: u64) {
    let engine: mlua::Table = lua.named_registry_value(ENGINE_TABLE).unwrap();
    engine.set("curtime", cur_time).unwrap();
    engine.set("frametime", frame_time).unwrap();
    engine.set("tick_count", tick_count).unwrap();
}

pub fn register_engine_lib(lua: &Lua, tick_interval: f64) {
    let engine_table = lua.create_table().expect("Failed to create engine table");
    engine_table.set("tick_interval", tick_interval).expect("[engine] Failed setting tick_interval");
    engine_table.set("curtime", 0.0f64).expect("[engine] Failed setting curtime");
    engine_table.set("frametime", 0.0f64).expect("[engine] Failed setting frametime");
    engine_table.set("tick_count", 0u64).expect("[engine] Failed setting tick_count");
    lua.set_named_registry_value(ENGINE_TABLE, engine_table.clone()).expect("Failed to store engine table");
    lua.globals().set("engine", engine_table).expect("[net] Failed to set engine table")
}
