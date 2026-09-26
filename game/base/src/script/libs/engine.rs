use mlua::Lua;

const ENGINE_CLOCK: &str = "engine_clock";

pub fn publish_clock(lua: &Lua, cur_time: f64, frame_time: f64, tick_count: u64) {
    let clock: mlua::Table = lua.named_registry_value(ENGINE_CLOCK).unwrap();
    clock.set("curtime", cur_time).unwrap();
    clock.set("frametime", frame_time).unwrap();
    clock.set("tick_count", tick_count).unwrap();
}

pub fn register_engine_lib(lua: &Lua, tick_interval: f64) {
    let clock = lua.create_table().expect("Failed to create engine clock");
    clock.set("curtime", 0.0f64).expect("Failed to set engine clock");
    clock.set("frametime", 0.0f64).expect("Failed to set engine clock");
    clock.set("tick_count", 0u64).expect("Failed to set engine clock");
    lua.set_named_registry_value(ENGINE_CLOCK, clock).expect("Failed to store engine clock");

    let engine_table = lua.create_table().expect("Failed to create engine table");

    engine_table.set("tick_interval", lua.create_function(move |_, (): ()| {
        Ok(tick_interval)
    }).expect("[engine] Failed to create tick_interval function"))
      .expect("[engine] Failed setting tick_interval function");

    engine_table.set("curtime", lua.create_function(|lua, (): ()| -> mlua::Result<f64> {
        let clock: mlua::Table = lua.named_registry_value(ENGINE_CLOCK)?;
        clock.get("curtime")
    }).expect("[engine] Failed to create curtime function"))
      .expect("[engine] Failed setting curtime function");

    engine_table.set("frametime", lua.create_function(|lua, (): ()| -> mlua::Result<f64> {
        let clock: mlua::Table = lua.named_registry_value(ENGINE_CLOCK)?;
        clock.get("frametime")
    }).expect("[engine] Failed to create frametime function"))
      .expect("[engine] Failed setting frametime function");

    engine_table.set("tick_count", lua.create_function(|lua, (): ()| -> mlua::Result<u64> {
        let clock: mlua::Table = lua.named_registry_value(ENGINE_CLOCK)?;
        clock.get("tick_count")
    }).expect("[engine] Failed to create tick_count function"))
      .expect("[engine] Failed setting tick_count function");

    lua.globals().set("engine", engine_table).expect("[net] Failed to set engine table")
}