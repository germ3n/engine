use super::{Mods, RawFrame, WebHost};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::time::{Duration, Instant};

extern "C" {
    fn engine_web_new() -> *mut std::ffi::c_void;
    fn engine_web_free(host: *mut std::ffi::c_void);
    fn engine_web_create(host: *mut std::ffi::c_void, width: u32, height: u32) -> u64;
    fn engine_web_destroy(host: *mut std::ffi::c_void, view_id: u64);
    fn engine_web_load_html(host: *mut std::ffi::c_void, view_id: u64, html: *const c_char);
    fn engine_web_load_url(host: *mut std::ffi::c_void, view_id: u64, url: *const c_char);
    fn engine_web_resize(host: *mut std::ffi::c_void, view_id: u64, width: u32, height: u32);
    fn engine_web_run_js(host: *mut std::ffi::c_void, view_id: u64, code: *const c_char);
    fn engine_web_mouse_move(host: *mut std::ffi::c_void, view_id: u64, x: f32, y: f32);
    fn engine_web_mouse_button(host: *mut std::ffi::c_void, view_id: u64, button: i32, down: i32);
    fn engine_web_mouse_wheel(host: *mut std::ffi::c_void, view_id: u64, dx: f32, dy: f32);
    fn engine_web_key(
        host: *mut std::ffi::c_void,
        view_id: u64,
        code: u16,
        chars: *const c_char,
        down: i32,
        repeat: i32,
        mods: u32,
    );
    fn engine_web_text(host: *mut std::ffi::c_void, view_id: u64, text: *const c_char);
    fn engine_web_focus(host: *mut std::ffi::c_void, view_id: u64, on: i32);
    fn engine_web_is_live(host: *mut std::ffi::c_void, view_id: u64) -> i32;
    fn engine_web_boost(host: *mut std::ffi::c_void, view_id: u64) -> i32;
    fn engine_web_clear_boost(host: *mut std::ffi::c_void, view_id: u64);
    fn engine_web_busy(host: *mut std::ffi::c_void, view_id: u64) -> i32;
    fn engine_web_request_snapshot(host: *mut std::ffi::c_void, view_id: u64);
    fn engine_web_poll_frame(
        host: *mut std::ffi::c_void,
        view_id: *mut u64,
        width: *mut u32,
        height: *mut u32,
        bytes: *mut *mut u8,
        length: *mut u32,
    ) -> i32;
    fn engine_web_free_bytes(bytes: *mut u8);
    fn engine_web_poll_message(
        host: *mut std::ffi::c_void,
        view_id: *mut u64,
        text: *mut *mut c_char,
    ) -> i32;
    fn engine_web_free_text(text: *mut c_char);
    fn engine_web_debug_message(host: *mut std::ffi::c_void, view_id: u64, text: *const c_char);
}

struct Meta {
    next_shot: Instant,
}

pub struct MacHost {
    raw: *mut std::ffi::c_void,
    meta: HashMap<u64, Meta>,
}

unsafe impl Send for MacHost {}

impl MacHost {
    pub fn new() -> Self {
        Self {
            raw: unsafe { engine_web_new() },
            meta: HashMap::new(),
        }
    }
}

impl Drop for MacHost {
    fn drop(&mut self) {
        if self.raw.is_null() {
            return;
        }

        unsafe { engine_web_free(self.raw) };
        self.raw = std::ptr::null_mut();
    }
}

impl WebHost for MacHost {
    fn create(&mut self, width: u32, height: u32) -> u64 {
        let id = unsafe { engine_web_create(self.raw, width, height) };

        if id == 0 {
            return 0;
        }

        self.meta.insert(
            id,
            Meta {
                next_shot: Instant::now(),
            },
        );

        id
    }

    fn remove(&mut self, id: u64) {
        self.meta.remove(&id);
        unsafe { engine_web_destroy(self.raw, id) };
    }

    fn load_html(&mut self, id: u64, html: &str) {
        let text = c_text(html);
        unsafe { engine_web_load_html(self.raw, id, text.as_ptr()) };
    }

    fn load_url(&mut self, id: u64, url: &str) {
        let text = c_text(url);
        unsafe { engine_web_load_url(self.raw, id, text.as_ptr()) };
    }

    fn resize(&mut self, id: u64, width: u32, height: u32) {
        unsafe { engine_web_resize(self.raw, id, width, height) };
    }

    fn run_js(&mut self, id: u64, code: &str) {
        let text = c_text(code);
        unsafe { engine_web_run_js(self.raw, id, text.as_ptr()) };
    }

    fn mouse_move(&mut self, id: u64, x: f32, y: f32) {
        unsafe { engine_web_mouse_move(self.raw, id, x, y) };
    }

    fn mouse_button(&mut self, id: u64, button: i32, down: bool) {
        unsafe { engine_web_mouse_button(self.raw, id, button, if down { 1 } else { 0 }) };
    }

    fn mouse_wheel(&mut self, id: u64, dx: f32, dy: f32) {
        unsafe { engine_web_mouse_wheel(self.raw, id, dx, dy) };
    }

    fn key(&mut self, id: u64, name: &str, down: bool, repeat: bool, mods: Mods) {
        let (code, chars) = mac_key(name, mods.shift);
        let text = c_text(&chars);
        unsafe {
            engine_web_key(
                self.raw,
                id,
                code,
                text.as_ptr(),
                if down { 1 } else { 0 },
                if repeat { 1 } else { 0 },
                mod_bits(mods, name),
            );
        }
    }

    fn text(&mut self, id: u64, text: &str) {
        let owned = c_text(text);
        unsafe { engine_web_text(self.raw, id, owned.as_ptr()) };
    }

    fn focus(&mut self, id: u64, on: bool) {
        unsafe { engine_web_focus(self.raw, id, if on { 1 } else { 0 }) };
    }

    fn pump(&mut self) {
        let now = Instant::now();
        let ids: Vec<u64> = self.meta.keys().copied().collect();
        let mut idx = 0;

        while idx < ids.len() {
            let id = ids[idx];
            idx += 1;

            if unsafe { engine_web_is_live(self.raw, id) } == 0 {
                continue;
            }

            if unsafe { engine_web_busy(self.raw, id) } != 0 {
                continue;
            }

            let boost = unsafe { engine_web_boost(self.raw, id) } != 0;
            let Some(meta) = self.meta.get_mut(&id) else {
                continue;
            };

            if !boost && now < meta.next_shot {
                continue;
            }

            unsafe { engine_web_request_snapshot(self.raw, id) };
            unsafe { engine_web_clear_boost(self.raw, id) };
            meta.next_shot = now + Duration::from_millis(66);
        }
    }

    fn poll_frames(&mut self) -> Vec<RawFrame> {
        let mut out = Vec::new();

        loop {
            let mut id = 0u64;
            let mut width = 0u32;
            let mut height = 0u32;
            let mut bytes = std::ptr::null_mut();
            let mut length = 0u32;
            let ok = unsafe {
                engine_web_poll_frame(
                    self.raw,
                    &mut id,
                    &mut width,
                    &mut height,
                    &mut bytes,
                    &mut length,
                )
            };

            if ok == 0 {
                break;
            }

            let slice = unsafe { std::slice::from_raw_parts(bytes, length as usize) };
            out.push(RawFrame {
                id,
                width,
                height,
                bytes: slice.to_vec(),
            });
            unsafe { engine_web_free_bytes(bytes) };
        }

        out
    }

    fn poll_messages(&mut self) -> Vec<(u64, String)> {
        let mut out = Vec::new();

        loop {
            let mut id = 0u64;
            let mut text = std::ptr::null_mut();
            let ok = unsafe { engine_web_poll_message(self.raw, &mut id, &mut text) };

            if ok == 0 {
                break;
            }

            let message = unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned();
            unsafe { engine_web_free_text(text) };
            out.push((id, message));
        }

        out
    }

    fn debug_message(&mut self, id: u64, text: &str) {
        let owned = c_text(text);
        unsafe { engine_web_debug_message(self.raw, id, owned.as_ptr()) };
    }
}

fn c_text(text: &str) -> CString {
    match CString::new(text.replace('\0', "")) {
        Ok(text) => text,
        Err(_) => CString::new("").unwrap(),
    }
}

fn mod_bits(mods: Mods, name: &str) -> u32 {
    let mut bits = 0u32;
    let name = name.to_ascii_lowercase();

    if mods.shift || name == "lshift" || name == "rshift" {
        bits |= 1 << 17;
    }

    if mods.control || name == "lctrl" || name == "rctrl" || name == "lcontrol" || name == "rcontrol"
    {
        bits |= 1 << 18;
    }

    if mods.alt || name == "lalt" || name == "ralt" {
        bits |= 1 << 19;
    }

    if mods.command || name == "lsuper" || name == "rsuper" || name == "lcmd" || name == "rcmd" {
        bits |= 1 << 20;
    }

    bits
}

fn mac_key(name: &str, shift: bool) -> (u16, String) {
    let name = name.to_ascii_lowercase();

    match name.as_str() {
        "a" => (0x00, letter('a', shift)),
        "s" => (0x01, letter('s', shift)),
        "d" => (0x02, letter('d', shift)),
        "f" => (0x03, letter('f', shift)),
        "h" => (0x04, letter('h', shift)),
        "g" => (0x05, letter('g', shift)),
        "z" => (0x06, letter('z', shift)),
        "x" => (0x07, letter('x', shift)),
        "c" => (0x08, letter('c', shift)),
        "v" => (0x09, letter('v', shift)),
        "b" => (0x0B, letter('b', shift)),
        "q" => (0x0C, letter('q', shift)),
        "w" => (0x0D, letter('w', shift)),
        "e" => (0x0E, letter('e', shift)),
        "r" => (0x0F, letter('r', shift)),
        "y" => (0x10, letter('y', shift)),
        "t" => (0x11, letter('t', shift)),
        "1" => (0x12, shifted_digit('1', '!', shift)),
        "2" => (0x13, shifted_digit('2', '@', shift)),
        "3" => (0x14, shifted_digit('3', '#', shift)),
        "4" => (0x15, shifted_digit('4', '$', shift)),
        "6" => (0x16, shifted_digit('6', '^', shift)),
        "5" => (0x17, shifted_digit('5', '%', shift)),
        "9" => (0x19, shifted_digit('9', '(', shift)),
        "7" => (0x1A, shifted_digit('7', '&', shift)),
        "8" => (0x1C, shifted_digit('8', '*', shift)),
        "0" => (0x1D, shifted_digit('0', ')', shift)),
        "o" => (0x1F, letter('o', shift)),
        "u" => (0x20, letter('u', shift)),
        "i" => (0x22, letter('i', shift)),
        "p" => (0x23, letter('p', shift)),
        "l" => (0x25, letter('l', shift)),
        "j" => (0x26, letter('j', shift)),
        "k" => (0x28, letter('k', shift)),
        "n" => (0x2D, letter('n', shift)),
        "m" => (0x2E, letter('m', shift)),
        "enter" | "return" | "kp_enter" => (if name == "kp_enter" { 0x4C } else { 0x24 }, "\r".to_string()),
        "tab" => (0x30, "\t".to_string()),
        "space" => (0x31, " ".to_string()),
        "backspace" | "back" => (0x33, "\u{7f}".to_string()),
        "escape" | "esc" => (0x35, "\u{1b}".to_string()),
        "lsuper" | "lcmd" | "lwin" => (0x37, String::new()),
        "rsuper" | "rcmd" | "rwin" => (0x36, String::new()),
        "lshift" => (0x38, String::new()),
        "rshift" => (0x3C, String::new()),
        "capslock" | "caps" => (0x39, String::new()),
        "lalt" => (0x3A, String::new()),
        "ralt" => (0x3D, String::new()),
        "lctrl" | "lcontrol" => (0x3B, String::new()),
        "rctrl" | "rcontrol" => (0x3E, String::new()),
        "-" | "minus" => (0x1B, shifted_digit('-', '_', shift)),
        "=" | "equal" | "equals" => (0x18, shifted_digit('=', '+', shift)),
        "[" | "bracketleft" => (0x21, shifted_digit('[', '{', shift)),
        "]" | "bracketright" => (0x1E, shifted_digit(']', '}', shift)),
        "\\" | "backslash" => (0x2A, shifted_digit('\\', '|', shift)),
        ";" | "semicolon" => (0x29, shifted_digit(';', ':', shift)),
        "'" | "quote" => (0x27, shifted_digit('\'', '"', shift)),
        "`" | "backquote" => (0x32, shifted_digit('`', '~', shift)),
        "," | "comma" => (0x2B, shifted_digit(',', '<', shift)),
        "." | "period" => (0x2F, shifted_digit('.', '>', shift)),
        "/" | "slash" => (0x2C, shifted_digit('/', '?', shift)),
        "leftarrow" | "arrowleft" => (0x7B, "\u{F702}".to_string()),
        "rightarrow" | "arrowright" => (0x7C, "\u{F703}".to_string()),
        "downarrow" | "arrowdown" => (0x7D, "\u{F701}".to_string()),
        "uparrow" | "arrowup" => (0x7E, "\u{F700}".to_string()),
        "delete" | "del" => (0x75, "\u{F728}".to_string()),
        "home" => (0x73, "\u{F729}".to_string()),
        "end" => (0x77, "\u{F72B}".to_string()),
        "pgup" | "pageup" => (0x74, "\u{F72C}".to_string()),
        "pgdn" | "pagedown" => (0x79, "\u{F72D}".to_string()),
        "f1" => (0x7A, "\u{F704}".to_string()),
        "f2" => (0x78, "\u{F705}".to_string()),
        "f3" => (0x63, "\u{F706}".to_string()),
        "f4" => (0x76, "\u{F707}".to_string()),
        "f5" => (0x60, "\u{F708}".to_string()),
        "f6" => (0x61, "\u{F709}".to_string()),
        "f7" => (0x62, "\u{F70A}".to_string()),
        "f8" => (0x64, "\u{F70B}".to_string()),
        "f9" => (0x65, "\u{F70C}".to_string()),
        "f10" => (0x6D, "\u{F70D}".to_string()),
        "f11" => (0x67, "\u{F70E}".to_string()),
        "f12" => (0x6F, "\u{F70F}".to_string()),
        "kp_0" => (0x52, "0".to_string()),
        "kp_1" => (0x53, "1".to_string()),
        "kp_2" => (0x54, "2".to_string()),
        "kp_3" => (0x55, "3".to_string()),
        "kp_4" => (0x56, "4".to_string()),
        "kp_5" => (0x57, "5".to_string()),
        "kp_6" => (0x58, "6".to_string()),
        "kp_7" => (0x59, "7".to_string()),
        "kp_8" => (0x5B, "8".to_string()),
        "kp_9" => (0x5C, "9".to_string()),
        "kp_plus" => (0x45, "+".to_string()),
        "kp_minus" => (0x4E, "-".to_string()),
        "kp_multiply" => (0x43, "*".to_string()),
        "kp_divide" => (0x4B, "/".to_string()),
        "kp_decimal" => (0x41, ".".to_string()),
        _ => (0, String::new()),
    }
}

fn letter(ch: char, shift: bool) -> String {
    if shift {
        return ch.to_ascii_uppercase().to_string();
    }

    ch.to_string()
}

fn shifted_digit(plain: char, shifted: char, shift: bool) -> String {
    if shift {
        return shifted.to_string();
    }

    plain.to_string()
}
