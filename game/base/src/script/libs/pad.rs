use crate::input::PadButton;
use crate::platform::{GamepadState, PadCache, PadPower, PAD_COUNT};
use mlua::prelude::LuaUserDataMethods;
use mlua::{Error, Lua, UserData};
use r#macro::document;
use std::sync::{Arc, Mutex};

#[document(
    kind = "library",
    name = "pad",
    realm = "shared",
    summary = "Reads up to four gamepads. An empty slot reports zeros and released buttons."
)]
fn pad_lib() {}

#[document(
    parent = "pad",
    name = "get",
    kind = "function",
    realm = "shared",
    summary = "Reads one gamepad.",
    params = {
        index = { ty = "number", desc = "Slot from 0 to pad.count - 1." },
    },
    returns = { ty = "Pad", desc = "The current state of that slot." },
    panics = "Errors when the index is outside 0 to pad.count - 1.",
)]
fn pad_get() {}

#[document(
    parent = "pad",
    name = "count",
    kind = "field",
    realm = "shared",
    summary = "Number of gamepad slots.",
    returns = { ty = "number", desc = "Always 4." },
)]
fn pad_count() {}

#[document(
    kind = "class",
    name = "Pad",
    realm = "shared",
    summary = "One gamepad sample. Sticks are about -1 to 1. Pedal axes are 0 to 1. Button methods are true while held."
)]
fn pad_class() {}

#[document(
    parent = "Pad",
    name = "forward",
    kind = "method",
    realm = "shared",
    summary = "Left stick Y.",
    returns = { ty = "number", desc = "About -1 to 1. Positive is forward." },
)]
fn pad_forward() {}

#[document(
    parent = "Pad",
    name = "right",
    kind = "method",
    realm = "shared",
    summary = "Left stick X.",
    returns = { ty = "number", desc = "About -1 to 1. Positive is right." },
)]
fn pad_right() {}

#[document(
    parent = "Pad",
    name = "look_x",
    kind = "method",
    realm = "shared",
    summary = "Right stick X.",
    returns = { ty = "number", desc = "About -1 to 1." },
)]
fn pad_look_x() {}

#[document(
    parent = "Pad",
    name = "look_y",
    kind = "method",
    realm = "shared",
    summary = "Right stick Y.",
    returns = { ty = "number", desc = "About -1 to 1." },
)]
fn pad_look_y() {}

#[document(
    parent = "Pad",
    name = "gas",
    kind = "method",
    realm = "shared",
    summary = "Gas pedal axis.",
    returns = { ty = "number", desc = "0 to 1." },
)]
fn pad_gas() {}

#[document(
    parent = "Pad",
    name = "brake",
    kind = "method",
    realm = "shared",
    summary = "Brake pedal axis.",
    returns = { ty = "number", desc = "0 to 1." },
)]
fn pad_brake() {}

#[document(
    parent = "Pad",
    name = "clutch",
    kind = "method",
    realm = "shared",
    summary = "Clutch pedal axis.",
    returns = { ty = "number", desc = "0 to 1." },
)]
fn pad_clutch() {}

#[document(
    parent = "Pad",
    name = "a",
    kind = "method",
    realm = "shared",
    summary = "South face button.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_a() {}

#[document(
    parent = "Pad",
    name = "b",
    kind = "method",
    realm = "shared",
    summary = "East face button.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_b() {}

#[document(
    parent = "Pad",
    name = "x",
    kind = "method",
    realm = "shared",
    summary = "West face button.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_x() {}

#[document(
    parent = "Pad",
    name = "y",
    kind = "method",
    realm = "shared",
    summary = "North face button.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_y() {}

#[document(
    parent = "Pad",
    name = "lb",
    kind = "method",
    realm = "shared",
    summary = "Left bumper.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_lb() {}

#[document(
    parent = "Pad",
    name = "lt",
    kind = "method",
    realm = "shared",
    summary = "Left trigger button.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_lt() {}

#[document(
    parent = "Pad",
    name = "rb",
    kind = "method",
    realm = "shared",
    summary = "Right bumper.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_rb() {}

#[document(
    parent = "Pad",
    name = "rt",
    kind = "method",
    realm = "shared",
    summary = "Right trigger button.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_rt() {}

#[document(
    parent = "Pad",
    name = "ls",
    kind = "method",
    realm = "shared",
    summary = "Left stick click.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_ls() {}

#[document(
    parent = "Pad",
    name = "rs",
    kind = "method",
    realm = "shared",
    summary = "Right stick click.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_rs() {}

#[document(
    parent = "Pad",
    name = "select",
    kind = "method",
    realm = "shared",
    summary = "Back or select button.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_select() {}

#[document(
    parent = "Pad",
    name = "start",
    kind = "method",
    realm = "shared",
    summary = "Start button.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_start() {}

#[document(
    parent = "Pad",
    name = "dpad_up",
    kind = "method",
    realm = "shared",
    summary = "D-pad up.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_dpad_up() {}

#[document(
    parent = "Pad",
    name = "dpad_down",
    kind = "method",
    realm = "shared",
    summary = "D-pad down.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_dpad_down() {}

#[document(
    parent = "Pad",
    name = "dpad_left",
    kind = "method",
    realm = "shared",
    summary = "D-pad left.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_dpad_left() {}

#[document(
    parent = "Pad",
    name = "dpad_right",
    kind = "method",
    realm = "shared",
    summary = "D-pad right.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_dpad_right() {}

#[document(
    parent = "Pad",
    name = "gas_pressed",
    kind = "method",
    realm = "shared",
    summary = "Gas pedal as a button.",
    returns = { ty = "boolean", desc = "True while held." },
    see_also = "Pad:gas",
)]
fn pad_gas_pressed() {}

#[document(
    parent = "Pad",
    name = "brake_pressed",
    kind = "method",
    realm = "shared",
    summary = "Brake pedal as a button.",
    returns = { ty = "boolean", desc = "True while held." },
    see_also = "Pad:brake",
)]
fn pad_brake_pressed() {}

#[document(
    parent = "Pad",
    name = "clutch_pressed",
    kind = "method",
    realm = "shared",
    summary = "Clutch pedal as a button.",
    returns = { ty = "boolean", desc = "True while held." },
    see_also = "Pad:clutch",
)]
fn pad_clutch_pressed() {}

#[document(
    parent = "Pad",
    name = "paddle1",
    kind = "method",
    realm = "shared",
    summary = "First wheel paddle.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_paddle1() {}

#[document(
    parent = "Pad",
    name = "paddle2",
    kind = "method",
    realm = "shared",
    summary = "Second wheel paddle.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_paddle2() {}

#[document(
    parent = "Pad",
    name = "paddle3",
    kind = "method",
    realm = "shared",
    summary = "Third wheel paddle.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_paddle3() {}

#[document(
    parent = "Pad",
    name = "paddle4",
    kind = "method",
    realm = "shared",
    summary = "Fourth wheel paddle.",
    returns = { ty = "boolean", desc = "True while held." },
)]
fn pad_paddle4() {}

#[document(
    parent = "Pad",
    name = "power",
    kind = "method",
    realm = "shared",
    summary = "Battery or cable state.",
    returns = { ty = "string", desc = "unknown, wired, discharging, charging, or charged. A second number is the percent while charging or discharging." },
)]
fn pad_power() {}

struct LuaPad {
    state: GamepadState,
}

impl LuaPad {
    fn down(&self, button: PadButton) -> bool {
        self.state.buttons.contains(button)
    }
}

impl UserData for LuaPad {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("forward", |_, this, ()| Ok(this.state.forward as f64));
        methods.add_method("right", |_, this, ()| Ok(this.state.right as f64));
        methods.add_method("look_x", |_, this, ()| Ok(this.state.look_x as f64));
        methods.add_method("look_y", |_, this, ()| Ok(this.state.look_y as f64));
        methods.add_method("gas", |_, this, ()| Ok(this.state.gas as f64));
        methods.add_method("brake", |_, this, ()| Ok(this.state.brake as f64));
        methods.add_method("clutch", |_, this, ()| Ok(this.state.clutch as f64));

        methods.add_method("a", |_, this, ()| Ok(this.down(PadButton::South)));
        methods.add_method("b", |_, this, ()| Ok(this.down(PadButton::East)));
        methods.add_method("x", |_, this, ()| Ok(this.down(PadButton::West)));
        methods.add_method("y", |_, this, ()| Ok(this.down(PadButton::North)));
        methods.add_method("lb", |_, this, ()| Ok(this.down(PadButton::LeftTrigger)));
        methods.add_method("lt", |_, this, ()| Ok(this.down(PadButton::LeftTrigger2)));
        methods.add_method("rb", |_, this, ()| Ok(this.down(PadButton::RightTrigger)));
        methods.add_method("rt", |_, this, ()| Ok(this.down(PadButton::RightTrigger2)));
        methods.add_method("ls", |_, this, ()| Ok(this.down(PadButton::LeftThumb)));
        methods.add_method("rs", |_, this, ()| Ok(this.down(PadButton::RightThumb)));
        methods.add_method("select", |_, this, ()| Ok(this.down(PadButton::Select)));
        methods.add_method("start", |_, this, ()| Ok(this.down(PadButton::Start)));
        methods.add_method("dpad_up", |_, this, ()| Ok(this.down(PadButton::DPadUp)));
        methods.add_method(
            "dpad_down",
            |_, this, ()| Ok(this.down(PadButton::DPadDown)),
        );
        methods.add_method(
            "dpad_left",
            |_, this, ()| Ok(this.down(PadButton::DPadLeft)),
        );
        methods.add_method("dpad_right", |_, this, ()| {
            Ok(this.down(PadButton::DPadRight))
        });
        methods.add_method("gas_pressed", |_, this, ()| {
            Ok(this.down(PadButton::PedalGas))
        });
        methods.add_method("brake_pressed", |_, this, ()| {
            Ok(this.down(PadButton::PedalBrake))
        });
        methods.add_method("clutch_pressed", |_, this, ()| {
            Ok(this.down(PadButton::PedalClutch))
        });
        methods.add_method("paddle1", |_, this, ()| Ok(this.down(PadButton::Paddle1)));
        methods.add_method("paddle2", |_, this, ()| Ok(this.down(PadButton::Paddle2)));
        methods.add_method("paddle3", |_, this, ()| Ok(this.down(PadButton::Paddle3)));
        methods.add_method("paddle4", |_, this, ()| Ok(this.down(PadButton::Paddle4)));

        methods.add_method("power", |_, this, ()| {
            Ok(match this.state.power {
                PadPower::Unknown => ("unknown", None),
                PadPower::Wired => ("wired", None),
                PadPower::Discharging(level) => ("discharging", Some(level as i64)),
                PadPower::Charging(level) => ("charging", Some(level as i64)),
                PadPower::Charged => ("charged", None),
            })
        });
    }
}

pub fn register_pad_lib(lua: &Lua, pads: Arc<Mutex<PadCache>>) {
    let table = lua.create_table().expect("Failed to create pad table");
    let shared = pads.clone();

    table
        .set(
            "get",
            lua.create_function(move |_, index: i64| {
                if index < 0 || index as usize >= PAD_COUNT {
                    return Err(Error::RuntimeError(format!(
                        "pad index {index} out of range (0..{})",
                        PAD_COUNT - 1
                    )));
                }

                let pads = shared
                    .lock()
                    .map_err(|_| Error::RuntimeError("pads lock poisoned".to_string()))?;

                Ok(LuaPad {
                    state: pads.get(index as usize),
                })
            })
            .expect("[engine] Failed to create pad.get"),
        )
        .expect("[engine] Failed setting pad.get");

    table
        .set("count", PAD_COUNT)
        .expect("[engine] Failed setting pad.count");

    lua.globals()
        .set("pad", table)
        .expect("[engine] Failed to set pad table");
}
