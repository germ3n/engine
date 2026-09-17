use std::sync::{atomic::AtomicU64, atomic::Ordering, Arc};
use mlua::Lua;

pub fn register_engine_lib(
    lua: &Lua,
    tick_interval: f64,
    curtime: Arc<AtomicU64>, 
    frame_time: Arc<AtomicU64>, 
    tick_count: Arc<AtomicU64>
) {
    let engine_table = lua.create_table().expect("Failed to create engine table");
                
    engine_table.set("tick_interval", lua.create_function(move |_, (): ()| {
        Ok(tick_interval)
    }).expect("[engine] Failed to create tick_interval function"))
      .expect("[engine] Failed setting tick_interval function");

    engine_table.set("curtime", lua.create_function(move |_, (): ()| {
        Ok(f64::from_bits(curtime.load(Ordering::Relaxed)))
    }).expect("[engine] Failed to create curtime function"))
      .expect("[engine] Failed setting curtime function");

    engine_table.set("frametime", lua.create_function(move |_, (): ()| {
        Ok(f64::from_bits(frame_time.load(Ordering::Relaxed)))
    }).expect("[engine] Failed to create frametime function"))
      .expect("[engine] Failed setting frametime function");

    engine_table.set("tick_count", lua.create_function(move |_, (): ()| {
        Ok(tick_count.load(Ordering::Relaxed))
    }).expect("[engine] Failed to create tick_count function"))
      .expect("[engine] Failed setting tick_count function");

    lua.globals().set("engine", engine_table).expect("[net] Failed to set engine table")
}