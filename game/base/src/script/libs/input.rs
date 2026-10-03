use crate::platform::MouseButton;
use mlua::Lua;
use std::sync::{Arc, Mutex};

pub struct Pointer {
    pub x: f64,
    pub y: f64,
    pub buttons: u32,
}

impl Pointer {
    pub fn new() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            buttons: 0,
        }
    }

    pub fn set_button(&mut self, button: MouseButton, down: bool) {
        let bit = button_bit(button);

        if bit == 0 {
            return;
        }

        if down {
            self.buttons |= bit;
        } else {
            self.buttons &= !bit;
        }
    }
}

pub fn button_index(button: MouseButton) -> i32 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 3,
        MouseButton::Other(index) => index as i32 + 1,
    }
}

fn button_bit(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 4,
        MouseButton::Other(3) => 8,
        MouseButton::Other(4) => 16,
        MouseButton::Other(_) => 0,
    }
}

fn down(buttons: u32, index: i32) -> bool {
    let bit = match index {
        1 => 1,
        2 => 2,
        3 => 4,
        4 => 8,
        5 => 16,
        _ => 0,
    };

    bit != 0 && buttons & bit != 0
}

pub fn register_input_lib(lua: &Lua, pointer: Arc<Mutex<Pointer>>) {
    let table = lua.create_table().expect("Failed to create input table");
    let shared = pointer.clone();
    table
        .set(
            "cursor",
            lua.create_function(move |_, ()| {
                let pointer = shared.lock().expect("Couldn't lock pointer");

                Ok((pointer.x, pointer.y))
            })
            .expect("[input] Failed to create cursor function"),
        )
        .expect("[input] Failed setting cursor function");

    let shared = pointer.clone();
    table
        .set(
            "mouse_down",
            lua.create_function(move |_, button: i32| {
                let pointer = shared.lock().expect("Couldn't lock pointer");

                Ok(down(pointer.buttons, button))
            })
            .expect("[input] Failed to create mouse_down function"),
        )
        .expect("[input] Failed setting mouse_down function");

    lua.globals().set("input", table).unwrap();
}
