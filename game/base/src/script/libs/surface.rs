use mlua::{Lua, Table};
use std::sync::Arc;
use std::sync::Mutex;
use crate::script::engine::DynWindowPtr;
use crate::ui::Color;

pub fn register_surface_lib(
    lua: &Lua,
    window_ptr: Arc<Mutex<DynWindowPtr>>
) {
    let surface_table = lua.create_table().unwrap();
    let wp_clone = window_ptr.clone();
    surface_table.set("draw_rect", lua.create_function(move |_, (x, y, w, h, r, g, b, a): (f32, f32, f32, f32, u8, u8, u8, u8)| {
        let guard = wp_clone.lock().unwrap();
        if let Some(ptr) = guard.0 {
            unsafe { (*ptr).draw_rectangle(x, y, w, h, Color::ColorRGBA { r, g, b, a }); }
        }
        Ok(())
    }).expect("[surface] Failed to create draw_rect function"))
      .expect("[surface] Failed setting draw_rect function");

    let wp_clone = window_ptr.clone();
    surface_table.set("draw_outlined_rect", lua.create_function(move |_, (x, y, w, h, thickness, r, g, b, a): (f32, f32, f32, f32, f32, u8, u8, u8, u8)| {
        let guard = wp_clone.lock().unwrap();
        if let Some(ptr) = guard.0 {
            unsafe { (*ptr).draw_outlined_rectangle(x, y, w, h, thickness, Color::ColorRGBA { r, g, b, a }); }
        }
        Ok(())
    }).expect("[surface] Failed to create draw_outlined_rect function"))
      .expect("[surface] Failed setting draw_outlined_rect function");

    let wp_clone = window_ptr.clone();
    surface_table.set("draw_text", lua.create_function(move |_, (font, text, x, y, scale, r, g, b, a): (String, String, f32, f32, f32, u8, u8, u8, u8)| {
        let guard = wp_clone.lock().unwrap();
        if let Some(ptr) = guard.0 {
            unsafe { (*ptr).draw_text(&font, &text, x, y, scale, Color::ColorRGBA { r, g, b, a }); }
        }
        Ok(())
    }).expect("[surface] Failed to create draw_text function"))
      .expect("[surface] Failed setting draw_text function");

    lua.globals().set("surface", surface_table).unwrap();
}