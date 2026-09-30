use crate::input::PadButton;
use crate::platform::{GamepadState, PadCache, PadPower, PAD_COUNT};
use mlua::prelude::LuaUserDataMethods;
use mlua::{Error, Lua, UserData};
use std::sync::{Arc, Mutex};

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
        methods.add_method("dpad_down", |_, this, ()| Ok(this.down(PadButton::DPadDown)));
        methods.add_method("dpad_left", |_, this, ()| Ok(this.down(PadButton::DPadLeft)));
        methods.add_method("dpad_right", |_, this, ()| Ok(this.down(PadButton::DPadRight)));
        methods.add_method("gas_pressed", |_, this, ()| Ok(this.down(PadButton::PedalGas)));
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
