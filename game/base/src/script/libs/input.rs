use crate::input::{key_name, parse_key};
use crate::platform::{KeyCode, MouseButton};
use mlua::Lua;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub struct Pointer {
    pub x: f64,
    pub y: f64,
    pub buttons: u32,
    pub keys: HashSet<KeyCode>,
    pub pressed: HashSet<KeyCode>,
    pub typed: String,
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
    pub wheel_x: f64,
    pub wheel_y: f64,
    pub block_look: bool,
    pub captured: bool,
}

impl Pointer {
    pub fn new() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            buttons: 0,
            keys: HashSet::new(),
            pressed: HashSet::new(),
            typed: String::new(),
            shift: false,
            control: false,
            alt: false,
            super_key: false,
            wheel_x: 0.0,
            wheel_y: 0.0,
            block_look: false,
            captured: false,
        }
    }

    pub fn set_key(&mut self, code: KeyCode, down: bool) {
        if down {
            self.keys.insert(code);
            self.pressed.insert(code);

            return;
        }

        self.keys.remove(&code);
    }

    pub fn push_text(&mut self, text: &str) {
        self.typed.push_str(text);
    }

    pub fn end_frame(&mut self) {
        self.pressed.clear();
        self.typed.clear();
        self.wheel_x = 0.0;
        self.wheel_y = 0.0;
    }

    pub fn release_all(&mut self) {
        self.keys.clear();
        self.buttons = 0;
        self.shift = false;
        self.control = false;
        self.alt = false;
        self.super_key = false;
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

pub fn key_label(code: KeyCode) -> &'static str {
    key_name(code)
}

fn find_key(name: &str) -> Option<KeyCode> {
    parse_key(&name.to_ascii_lowercase())
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

    let shared = pointer.clone();
    table
        .set(
            "key_down",
            lua.create_function(move |_, name: String| {
                let pointer = shared.lock().expect("Couldn't lock pointer");
                let Some(code) = find_key(&name) else {
                    return Ok(false);
                };

                Ok(pointer.keys.contains(&code))
            })
            .expect("[input] Failed to create key_down function"),
        )
        .expect("[input] Failed setting key_down function");

    let shared = pointer.clone();
    table
        .set(
            "key_pressed",
            lua.create_function(move |_, name: String| {
                let pointer = shared.lock().expect("Couldn't lock pointer");
                let Some(code) = find_key(&name) else {
                    return Ok(false);
                };

                Ok(pointer.pressed.contains(&code))
            })
            .expect("[input] Failed to create key_pressed function"),
        )
        .expect("[input] Failed setting key_pressed function");

    let shared = pointer.clone();
    table
        .set(
            "typed",
            lua.create_function(move |_, ()| {
                let pointer = shared.lock().expect("Couldn't lock pointer");

                Ok(pointer.typed.clone())
            })
            .expect("[input] Failed to create typed function"),
        )
        .expect("[input] Failed setting typed function");

    let shared = pointer.clone();
    table
        .set(
            "shift",
            lua.create_function(move |_, ()| {
                let pointer = shared.lock().expect("Couldn't lock pointer");

                Ok(pointer.shift)
            })
            .expect("[input] Failed to create shift function"),
        )
        .expect("[input] Failed setting shift function");

    let shared = pointer.clone();
    table
        .set(
            "control",
            lua.create_function(move |_, ()| {
                let pointer = shared.lock().expect("Couldn't lock pointer");

                Ok(pointer.control)
            })
            .expect("[input] Failed to create control function"),
        )
        .expect("[input] Failed setting control function");

    let shared = pointer.clone();
    table
        .set(
            "alt",
            lua.create_function(move |_, ()| {
                let pointer = shared.lock().expect("Couldn't lock pointer");

                Ok(pointer.alt)
            })
            .expect("[input] Failed to create alt function"),
        )
        .expect("[input] Failed setting alt function");

    let shared = pointer.clone();
    table
        .set(
            "super",
            lua.create_function(move |_, ()| {
                let pointer = shared.lock().expect("Couldn't lock pointer");

                Ok(pointer.super_key)
            })
            .expect("[input] Failed to create super function"),
        )
        .expect("[input] Failed setting super function");

    let shared = pointer.clone();
    table
        .set(
            "wheel",
            lua.create_function(move |_, ()| {
                let pointer = shared.lock().expect("Couldn't lock pointer");

                Ok((pointer.wheel_x, pointer.wheel_y))
            })
            .expect("[input] Failed to create wheel function"),
        )
        .expect("[input] Failed setting wheel function");

    let shared = pointer.clone();
    table
        .set(
            "block_look",
            lua.create_function(move |_, blocked: bool| {
                shared.lock().expect("Couldn't lock pointer").block_look = blocked;

                Ok(())
            })
            .expect("[input] Failed to create block_look function"),
        )
            .expect("[input] Failed setting block_look function");

    let shared = pointer.clone();
    table
        .set(
            "captured",
            lua.create_function(move |_, ()| {
                Ok(shared.lock().expect("Couldn't lock pointer").captured)
            })
            .expect("[input] Failed to create captured function"),
        )
        .expect("[input] Failed setting captured function");

    lua.globals().set("input", table).unwrap();
}
