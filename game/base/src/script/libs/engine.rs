use mlua::Lua;
use r#macro::document;

const ENGINE_TABLE: &str = "engine";

#[document(
    parent = "engine",
    name = "tick_interval",
    kind = "field",
    realm = "shared",
    summary = "Seconds between simulation ticks.",
    returns = { ty = "number", desc = "Fixed interval passed in when the Lua state is created." },
)]
fn engine_tick_interval() {}

#[document(
    parent = "engine",
    name = "curtime",
    kind = "field",
    realm = "shared",
    summary = "Simulation time in seconds.",
    returns = { ty = "number", desc = "Seconds since the server started simulating." },
)]
fn engine_curtime() {}

#[document(
    parent = "engine",
    name = "frametime",
    kind = "field",
    realm = "shared",
    summary = "Seconds since the previous frame.",
    returns = { ty = "number", desc = "Frame delta used for rendering, not the tick interval." },
)]
fn engine_frametime() {}

#[document(
    parent = "engine",
    name = "tick_count",
    kind = "field",
    realm = "shared",
    summary = "Simulation ticks since the server started.",
    returns = { ty = "number", desc = "Increments once per tick." },
)]
fn engine_tick_count() {}

#[document(
    parent = "engine",
    name = "first_time_predicted",
    kind = "field",
    realm = "shared",
    summary = "True during the first prediction of a command, and whenever the server runs predicted_think.",
    returns = { ty = "boolean", desc = "False while the client replays a saved command." },
    see_also = "Entity:predicted_think",
)]
fn engine_first_time_predicted() {}

pub fn publish_clock(lua: &Lua, cur_time: f64, frame_time: f64, tick_count: u64) {
    let engine: mlua::Table = lua.named_registry_value(ENGINE_TABLE).unwrap();
    engine.set("curtime", cur_time).unwrap();
    engine.set("frametime", frame_time).unwrap();
    engine.set("tick_count", tick_count).unwrap();
}

#[document(
    kind = "library",
    name = "engine",
    realm = "shared",
    summary = "Simulation clock. curtime and tick_count advance with the server. frametime is the last frame delta. tick_interval is fixed for the session."
)]
pub fn register_engine_lib(lua: &Lua, tick_interval: f64) {
    let engine_table = lua.create_table().expect("Failed to create engine table");
    engine_table
        .set("tick_interval", tick_interval)
        .expect("[engine] Failed setting tick_interval");
    engine_table
        .set("curtime", 0.0f64)
        .expect("[engine] Failed setting curtime");
    engine_table
        .set("frametime", 0.0f64)
        .expect("[engine] Failed setting frametime");
    engine_table
        .set("tick_count", 0u64)
        .expect("[engine] Failed setting tick_count");
    engine_table
        .set("first_time_predicted", true)
        .expect("[engine] Failed setting first_time_predicted");
    lua.set_named_registry_value(ENGINE_TABLE, engine_table.clone())
        .expect("Failed to store engine table");
    lua.globals()
        .set("engine", engine_table)
        .expect("[net] Failed to set engine table")
}
