use mlua::{Lua};
use std::sync::Arc;
use std::sync::Mutex;
use crate::script::engine::{DynWindowPtr, RenderQueue, DrawCommand};
use crate::ui::Color;
//todo: stop using locks

pub fn register_surface_lib(
    lua: &Lua,
    render_queue: RenderQueue,
) {
    let surface_table = lua.create_table().unwrap();

    let render_queue_ = render_queue.clone();
    surface_table.set("draw_rect", lua.create_function(move |_, (x, y, w, h, r, g, b, a): (f32, f32, f32, f32, f32, f32, f32, f32)| {
        render_queue_.lock().expect("Couldn't lock render queue").push(DrawCommand::Rect {
            x,
            y,
            w,
            h,
            color: Color::ColorRGBAf { r: r / 255.0, g: g / 255.0, b: b / 255.0, a: a / 255.0 },
        });
        Ok(())
    }).expect("[surface] Failed to create draw_rect function"))
      .expect("[surface] Failed setting draw_rect function");

    let render_queue_ = render_queue.clone();
    surface_table.set("draw_outlined_rect", lua.create_function(move |_, (x, y, w, h, thickness, r, g, b, a): (f32, f32, f32, f32, f32, f32, f32, f32, f32)| {
        render_queue_.lock().expect("Couldn't lock render queue").push(DrawCommand::OutlinedRect {
            x,
            y,
            w,
            h,
            thickness,
            color: Color::ColorRGBAf { r: r / 255.0, g: g / 255.0, b: b / 255.0, a: a / 255.0 },
        });
        Ok(())
    }).expect("[surface] Failed to create draw_outlined_rect function"))
      .expect("[surface] Failed setting draw_outlined_rect function");

    let render_queue_ = render_queue.clone();
    surface_table.set("draw_text", lua.create_function(move |_, (font, text, x, y, scale, r, g, b, a): (mlua::LuaString, mlua::LuaString, f32, f32, f32, f32, f32, f32, f32)| {
        render_queue_.lock().expect("Couldn't lock render queue").push(DrawCommand::Text {
            font,
            text,
            x,
            y,
            scale,
            color: Color::ColorRGBAf { r: r / 255.0, g: g / 255.0, b: b / 255.0, a: a / 255.0 },
        });
        Ok(())
    }).expect("[surface] Failed to create draw_text function"))
      .expect("[surface] Failed setting draw_text function");

    lua.globals().set("surface", surface_table).unwrap();
}