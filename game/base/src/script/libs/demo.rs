use crate::demo::{self, DemoCommand};
use mlua::Lua;
use r#macro::document;

#[document(
    kind = "library",
    name = "demo",
    realm = "shared",
    summary = "Record and play demos. record on a client writes what that client saw. record on the server writes every player's commands. play runs on the client."
)]
fn demo_lib() {}

#[document(
    parent = "demo",
    name = "record",
    kind = "function",
    realm = "shared",
    summary = "Starts recording demos/<name>.dem. The client writes a message log. The server writes a command tape.",
    params = {
        name = { ty = "string", desc = "File name without a path or extension." },
    },
    returns = { ty = "boolean", desc = "False when the name is rejected or this side is already recording." },
)]
fn demo_record() {}

#[document(
    parent = "demo",
    name = "stop",
    kind = "function",
    realm = "shared",
    summary = "Stops the recording on this side, or stops playback on the client.",
    returns = { ty = "boolean", desc = "False when no demo command could be queued." },
)]
fn demo_stop() {}

#[document(
    parent = "demo",
    name = "play",
    kind = "function",
    realm = "client",
    summary = "Plays demos/<name>.dem on the client with no live server traffic.",
    params = {
        name = { ty = "string", desc = "File name without a path or extension." },
    },
    returns = { ty = "boolean", desc = "False on the server, or when the command could not be queued." },
)]
fn demo_play() {}

#[document(
    parent = "demo",
    name = "pause",
    kind = "function",
    realm = "client",
    summary = "Toggles playback pause.",
    returns = { ty = "boolean", desc = "False on the server." },
)]
fn demo_pause() {}

#[document(
    parent = "demo",
    name = "timescale",
    kind = "function",
    realm = "client",
    summary = "Sets how fast demo ticks are consumed while unpaused. Sounds are not pitch-shifted.",
    params = {
        scale = { ty = "number", desc = "1 is recorded speed. 0 holds the clock without the pause flag." },
    },
    returns = { ty = "boolean", desc = "False on the server or when scale is negative." },
)]
fn demo_timescale() {}

#[document(
    parent = "demo",
    name = "seek",
    kind = "function",
    realm = "client",
    summary = "Jumps to a demo tick using the nearest checkpoint or keyframe.",
    params = {
        tick = { ty = "number", desc = "Simulation tick inside the demo." },
    },
    returns = { ty = "boolean", desc = "False on the server." },
)]
fn demo_seek() {}

#[document(
    parent = "demo",
    name = "loop",
    kind = "function",
    realm = "client",
    summary = "Loops playback back to the first checkpoint when the file ends.",
    params = {
        enabled = { ty = "boolean", desc = "True loops. False stops at the end." },
    },
    returns = { ty = "boolean", desc = "False on the server." },
)]
fn demo_loop() {}

#[document(
    parent = "demo",
    name = "cam",
    kind = "function",
    realm = "client",
    summary = "Sets the playback camera. first, chase, orbit, or free.",
    params = {
        mode = { ty = "string", desc = "first, chase, orbit, or free." },
    },
    returns = { ty = "boolean", desc = "False on the server or when the mode is unknown." },
)]
fn demo_cam() {}

#[document(
    parent = "demo",
    name = "view",
    kind = "function",
    realm = "client",
    summary = "Watches the player at this index in connect order.",
    params = {
        index = { ty = "number", desc = "Zero is the first player in the demo." },
    },
    returns = { ty = "boolean", desc = "False on the server." },
)]
fn demo_view() {}

fn queued(command: DemoCommand) -> bool {
    demo::request(command).is_ok()
}

pub fn register_demo_lib(lua: &Lua) {
    let table = lua.create_table().expect("[demo] Failed to create demo table");

    table
        .set(
            "record",
            lua.create_function(|_, name: String| Ok(queued(DemoCommand::Record { name })))
                .expect("[demo] Failed to create record"),
        )
        .expect("[demo] Failed setting record");

    table
        .set(
            "stop",
            lua.create_function(|_, ()| Ok(queued(DemoCommand::Stop)))
                .expect("[demo] Failed to create stop"),
        )
        .expect("[demo] Failed setting stop");

    table
        .set(
            "play",
            lua.create_function(|_, name: String| Ok(queued(DemoCommand::Play { name })))
                .expect("[demo] Failed to create play"),
        )
        .expect("[demo] Failed setting play");

    table
        .set(
            "pause",
            lua.create_function(|_, ()| Ok(queued(DemoCommand::Pause)))
                .expect("[demo] Failed to create pause"),
        )
        .expect("[demo] Failed setting pause");

    table
        .set(
            "timescale",
            lua.create_function(|_, scale: f64| {
                if !scale.is_finite() || scale < 0.0 {
                    return Ok(false);
                }

                Ok(queued(DemoCommand::Timescale { scale }))
            })
            .expect("[demo] Failed to create timescale"),
        )
        .expect("[demo] Failed setting timescale");

    table
        .set(
            "seek",
            lua.create_function(|_, tick: f64| {
                if !tick.is_finite() || tick < 0.0 {
                    return Ok(false);
                }

                Ok(queued(DemoCommand::Seek { tick: tick as u64 }))
            })
            .expect("[demo] Failed to create seek"),
        )
        .expect("[demo] Failed setting seek");

    table
        .set(
            "loop",
            lua.create_function(|_, enabled: bool| Ok(queued(DemoCommand::Loop { enabled })))
                .expect("[demo] Failed to create loop"),
        )
        .expect("[demo] Failed setting loop");

    table
        .set(
            "cam",
            lua.create_function(|_, mode: String| {
                let Ok(mode) = demo::parse_cam(&mode) else {
                    return Ok(false);
                };

                Ok(queued(DemoCommand::Cam { mode }))
            })
            .expect("[demo] Failed to create cam"),
        )
        .expect("[demo] Failed setting cam");

    table
        .set(
            "view",
            lua.create_function(|_, index: f64| {
                if !index.is_finite() || index < 0.0 {
                    return Ok(false);
                }

                Ok(queued(DemoCommand::View {
                    index: index as usize,
                }))
            })
            .expect("[demo] Failed to create view"),
        )
        .expect("[demo] Failed setting view");

    lua.globals()
        .set("demo", table)
        .expect("[demo] Failed to set demo table");
}
