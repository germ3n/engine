use crate::platform::{
    ElementState, KeyCode, Modifiers, MouseButton, MouseScrollDelta, WindowEvent,
};
use crate::ui::backend::GfxWindow;
use crate::ui::gfx::{self, Book};
use crate::world::surface::{CpuImage, PixelFormat};
use egui::epaint::{ImageData, ImageDelta, Primitive};
use std::collections::HashMap;
use std::time::Instant;

const RETIRE_FRAMES: u32 = 4;
const CLIP_SLOTS: usize = 12;

struct Atlas {
    id: u32,
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

#[derive(Clone, Copy, Default)]
struct Corner {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    color: [f32; 4],
}

pub struct Gui {
    ctx: egui::Context,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    pointer: egui::Pos2,
    scale: f32,
    focused: bool,
    start: Instant,
    book: Book,
    atlases: HashMap<egui::TextureId, Atlas>,
    retired: Vec<(u32, u32)>,
    verts: Vec<f32>,
    free: egui::Rect,
}

impl Gui {
    pub fn new(scale: f32) -> Self {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| {
            options.zoom_with_keyboard = false;
        });

        Self {
            ctx,
            events: Vec::new(),
            modifiers: egui::Modifiers::default(),
            pointer: egui::Pos2::new(-1.0, -1.0),
            scale: sane_scale(scale),
            focused: true,
            start: Instant::now(),
            book: Book::new(),
            atlases: HashMap::new(),
            retired: Vec::new(),
            verts: Vec::new(),
            free: egui::Rect::EVERYTHING,
        }
    }

    pub fn context(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = sane_scale(scale);
    }

    pub fn wants_pointer(&self) -> bool {
        if self.ctx.is_using_pointer() {
            return true;
        }

        if let Some(layer) = self.ctx.layer_id_at(self.pointer) {
            if layer.order != egui::Order::Background {
                return true;
            }
        }

        !self.free.contains(self.pointer)
    }

    pub fn wants_keyboard(&self) -> bool {
        self.ctx.wants_keyboard_input()
    }

    pub fn on_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(focused) => {
                self.focused = *focused;
                self.events.push(egui::Event::WindowFocused(*focused));
            }
            WindowEvent::ModifiersChanged(next) => {
                self.modifiers = egui_modifiers(*next);
            }
            WindowEvent::CursorMoved { x, y } => {
                self.pointer = egui::Pos2::new(*x as f32 / self.scale, *y as f32 / self.scale);
                self.events.push(egui::Event::PointerMoved(self.pointer));
            }
            WindowEvent::MouseWheel { delta } => {
                let (unit, delta) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (egui::MouseWheelUnit::Line, egui::vec2(*x, *y))
                    }
                    MouseScrollDelta::PixelDelta(x, y) => (
                        egui::MouseWheelUnit::Point,
                        egui::vec2(*x as f32 / self.scale, *y as f32 / self.scale),
                    ),
                };
                self.events.push(egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers: self.modifiers,
                });
            }
            WindowEvent::MouseInput { state, button } => {
                let button = match button {
                    MouseButton::Left => egui::PointerButton::Primary,
                    MouseButton::Right => egui::PointerButton::Secondary,
                    MouseButton::Middle => egui::PointerButton::Middle,
                    MouseButton::Other(_) => return,
                };
                self.events.push(egui::Event::PointerButton {
                    pos: self.pointer,
                    button,
                    pressed: *state == ElementState::Pressed,
                    modifiers: self.modifiers,
                });
            }
            WindowEvent::KeyboardInput(input) => {
                let Some(key) = input.key_code.and_then(egui_key) else {
                    return;
                };
                let pressed = input.state == ElementState::Pressed;

                if pressed && self.modifiers.command {
                    if key == egui::Key::C {
                        self.events.push(egui::Event::Copy);
                    } else if key == egui::Key::X {
                        self.events.push(egui::Event::Cut);
                    }
                }

                self.events.push(egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: input.repeat,
                    modifiers: self.modifiers,
                });
            }
            WindowEvent::TextInput { text } => {
                if self.modifiers.command || self.modifiers.ctrl {
                    return;
                }

                self.events.push(egui::Event::Text(text.clone()));
            }
            _ => {}
        }
    }

    pub fn run(
        &mut self,
        width: u32,
        height: u32,
        build: impl FnMut(&egui::Context),
    ) -> egui::FullOutput {
        let mut build = build;
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width as f32 / self.scale, height as f32 / self.scale),
        ));
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(self.scale);
        raw.time = Some(self.start.elapsed().as_secs_f64());
        raw.modifiers = self.modifiers;
        raw.focused = self.focused;
        raw.max_texture_side = Some(4096);
        raw.events = std::mem::take(&mut self.events);
        let ctx = self.ctx.clone();
        let mut free = egui::Rect::EVERYTHING;
        let output = ctx.run(raw, |ctx| {
            build(ctx);
            free = ctx.available_rect();
        });
        self.free = free;

        output
    }

    pub fn paint(
        &mut self,
        window: &mut GfxWindow,
        output: egui::FullOutput,
        width: u32,
        height: u32,
    ) {
        for (id, delta) in &output.textures_delta.set {
            self.upload(window, *id, delta);
        }

        let ppp = output.pixels_per_point;
        let primitives = self.ctx.tessellate(output.shapes, ppp);
        let screen = [0.0, 0.0, width as f32, height as f32];
        let mut batch = 0u32;
        self.verts.clear();

        for primitive in primitives {
            let Primitive::Mesh(mesh) = primitive.primitive else {
                continue;
            };
            let Some(texture) = self.atlases.get(&mesh.texture_id).map(|atlas| atlas.id) else {
                continue;
            };
            let clip = [
                (primitive.clip_rect.min.x * ppp).max(screen[0]),
                (primitive.clip_rect.min.y * ppp).max(screen[1]),
                (primitive.clip_rect.max.x * ppp).min(screen[2]),
                (primitive.clip_rect.max.y * ppp).min(screen[3]),
            ];

            if clip[2] <= clip[0] || clip[3] <= clip[1] {
                continue;
            }

            if texture != batch && !self.verts.is_empty() {
                window.draw_screen(&self.verts, batch, gfx::SAMP_CLAMP);
                self.verts.clear();
            }

            batch = texture;
            push_mesh(&mut self.verts, &mesh, clip, ppp);
        }

        if !self.verts.is_empty() {
            window.draw_screen(&self.verts, batch, gfx::SAMP_CLAMP);
            self.verts.clear();
        }

        for id in &output.textures_delta.free {
            if let Some(atlas) = self.atlases.remove(id) {
                self.retired.push((atlas.id, RETIRE_FRAMES));
            }
        }

        self.age(window);
    }

    fn upload(&mut self, window: &mut GfxWindow, id: egui::TextureId, delta: &ImageDelta) {
        let width = delta.image.width();
        let height = delta.image.height();
        let pixels = image_bytes(&delta.image);
        let atlas = match delta.pos {
            Some(pos) => {
                let Some(atlas) = self.atlases.get_mut(&id) else {
                    return;
                };
                let mut row = 0;

                while row < height {
                    let dst_y = pos[1] + row;

                    if dst_y >= atlas.height {
                        break;
                    }

                    let span = width.min(atlas.width.saturating_sub(pos[0]));
                    let src = row * width * 4;
                    let dst = (dst_y * atlas.width + pos[0]) * 4;
                    atlas.pixels[dst..dst + span * 4].copy_from_slice(&pixels[src..src + span * 4]);
                    row += 1;
                }

                atlas
            }
            None => {
                let previous = self.atlases.insert(
                    id,
                    Atlas {
                        id: 0,
                        width,
                        height,
                        pixels,
                    },
                );

                if let Some(previous) = previous {
                    self.retired.push((previous.id, RETIRE_FRAMES));
                }

                self.atlases.get_mut(&id).expect("atlas")
            }
        };
        let image = CpuImage {
            width: atlas.width as u32,
            height: atlas.height as u32,
            format: PixelFormat::Rgba8,
            bytes: atlas.pixels.clone(),
            mips: Vec::new(),
        };
        let texture = self.book.alloc(gfx::KIND_TEXTURE);

        if texture == 0 {
            return;
        }

        window.create_image(texture, &image);

        if atlas.id != 0 {
            self.retired.push((atlas.id, RETIRE_FRAMES));
        }

        atlas.id = texture;
    }

    fn age(&mut self, window: &mut GfxWindow) {
        let mut idx = 0;

        while idx < self.retired.len() {
            if self.retired[idx].1 > 0 {
                self.retired[idx].1 -= 1;
                idx += 1;

                continue;
            }

            let id = self.retired[idx].0;
            self.retired.swap_remove(idx);

            if id == 0 {
                continue;
            }

            window.free_gpu(id);

            if self.book.doom(id) {
                self.book.recycle(id);
            }
        }
    }
}

fn sane_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.25 {
        scale
    } else {
        1.0
    }
}

fn image_bytes(image: &ImageData) -> Vec<u8> {
    let mut out = Vec::with_capacity(image.width() * image.height() * 4);

    match image {
        ImageData::Color(color) => {
            for pixel in color.pixels.iter() {
                out.extend_from_slice(&pixel.to_array());
            }
        }
        ImageData::Font(font) => {
            for pixel in font.srgba_pixels(None) {
                out.extend_from_slice(&pixel.to_array());
            }
        }
    }

    out
}

fn push_mesh(out: &mut Vec<f32>, mesh: &egui::Mesh, clip: [f32; 4], ppp: f32) {
    let mut idx = 0;

    while idx + 2 < mesh.indices.len() {
        let tri = [
            corner(mesh, mesh.indices[idx], ppp),
            corner(mesh, mesh.indices[idx + 1], ppp),
            corner(mesh, mesh.indices[idx + 2], ppp),
        ];
        push_clipped(out, tri, clip);
        idx += 3;
    }
}

fn corner(mesh: &egui::Mesh, index: u32, ppp: f32) -> Corner {
    let Some(vertex) = mesh.vertices.get(index as usize) else {
        return Corner::default();
    };
    let [r, g, b, a] = vertex.color.to_srgba_unmultiplied();

    Corner {
        x: vertex.pos.x * ppp,
        y: vertex.pos.y * ppp,
        u: vertex.uv.x,
        v: vertex.uv.y,
        color: [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ],
    }
}

fn push_clipped(out: &mut Vec<f32>, tri: [Corner; 3], clip: [f32; 4]) {
    let inside = |c: &Corner| c.x >= clip[0] && c.x <= clip[2] && c.y >= clip[1] && c.y <= clip[3];

    if inside(&tri[0]) && inside(&tri[1]) && inside(&tri[2]) {
        push_corner(out, &tri[0]);
        push_corner(out, &tri[1]);
        push_corner(out, &tri[2]);

        return;
    }

    if (tri[0].x < clip[0] && tri[1].x < clip[0] && tri[2].x < clip[0])
        || (tri[0].x > clip[2] && tri[1].x > clip[2] && tri[2].x > clip[2])
        || (tri[0].y < clip[1] && tri[1].y < clip[1] && tri[2].y < clip[1])
        || (tri[0].y > clip[3] && tri[1].y > clip[3] && tri[2].y > clip[3])
    {
        return;
    }

    let mut poly = [Corner::default(); CLIP_SLOTS];
    let mut count = 3;
    poly[0] = tri[0];
    poly[1] = tri[1];
    poly[2] = tri[2];
    let mut edge = 0;

    while edge < 4 {
        let mut next = [Corner::default(); CLIP_SLOTS];
        let mut kept = 0;
        let mut idx = 0;

        while idx < count {
            let current = poly[idx];
            let following = poly[(idx + 1) % count];
            let current_in = edge_inside(&current, edge, clip);
            let following_in = edge_inside(&following, edge, clip);

            if current_in && kept < CLIP_SLOTS {
                next[kept] = current;
                kept += 1;
            }

            if current_in != following_in && kept < CLIP_SLOTS {
                next[kept] = edge_cross(&current, &following, edge, clip);
                kept += 1;
            }

            idx += 1;
        }

        poly = next;
        count = kept;

        if count < 3 {
            return;
        }

        edge += 1;
    }

    let mut idx = 1;

    while idx + 1 < count {
        push_corner(out, &poly[0]);
        push_corner(out, &poly[idx]);
        push_corner(out, &poly[idx + 1]);
        idx += 1;
    }
}

fn edge_inside(c: &Corner, edge: usize, clip: [f32; 4]) -> bool {
    match edge {
        0 => c.x >= clip[0],
        1 => c.x <= clip[2],
        2 => c.y >= clip[1],
        _ => c.y <= clip[3],
    }
}

fn edge_cross(a: &Corner, b: &Corner, edge: usize, clip: [f32; 4]) -> Corner {
    let t = match edge {
        0 => (clip[0] - a.x) / (b.x - a.x),
        1 => (clip[2] - a.x) / (b.x - a.x),
        2 => (clip[1] - a.y) / (b.y - a.y),
        _ => (clip[3] - a.y) / (b.y - a.y),
    };
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let lerp = |from: f32, to: f32| from + (to - from) * t;

    Corner {
        x: lerp(a.x, b.x),
        y: lerp(a.y, b.y),
        u: lerp(a.u, b.u),
        v: lerp(a.v, b.v),
        color: [
            lerp(a.color[0], b.color[0]),
            lerp(a.color[1], b.color[1]),
            lerp(a.color[2], b.color[2]),
            lerp(a.color[3], b.color[3]),
        ],
    }
}

fn push_corner(out: &mut Vec<f32>, c: &Corner) {
    out.extend_from_slice(&[
        c.x, c.y, c.u, c.v, c.color[0], c.color[1], c.color[2], c.color[3],
    ]);
}

fn egui_modifiers(modifiers: Modifiers) -> egui::Modifiers {
    let command = if cfg!(target_os = "macos") {
        modifiers.super_key
    } else {
        modifiers.control
    };

    egui::Modifiers {
        alt: modifiers.alt,
        ctrl: modifiers.control,
        shift: modifiers.shift,
        mac_cmd: cfg!(target_os = "macos") && modifiers.super_key,
        command,
    }
}

fn egui_key(code: KeyCode) -> Option<egui::Key> {
    use egui::Key;

    let key = match code {
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Delete => Key::Delete,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::Space => Key::Space,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::Insert => Key::Insert,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Digit0 | KeyCode::Numpad0 => Key::Num0,
        KeyCode::Digit1 | KeyCode::Numpad1 => Key::Num1,
        KeyCode::Digit2 | KeyCode::Numpad2 => Key::Num2,
        KeyCode::Digit3 | KeyCode::Numpad3 => Key::Num3,
        KeyCode::Digit4 | KeyCode::Numpad4 => Key::Num4,
        KeyCode::Digit5 | KeyCode::Numpad5 => Key::Num5,
        KeyCode::Digit6 | KeyCode::Numpad6 => Key::Num6,
        KeyCode::Digit7 | KeyCode::Numpad7 => Key::Num7,
        KeyCode::Digit8 | KeyCode::Numpad8 => Key::Num8,
        KeyCode::Digit9 | KeyCode::Numpad9 => Key::Num9,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyB => Key::B,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyF => Key::F,
        KeyCode::KeyG => Key::G,
        KeyCode::KeyH => Key::H,
        KeyCode::KeyI => Key::I,
        KeyCode::KeyJ => Key::J,
        KeyCode::KeyK => Key::K,
        KeyCode::KeyL => Key::L,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyN => Key::N,
        KeyCode::KeyO => Key::O,
        KeyCode::KeyP => Key::P,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyU => Key::U,
        KeyCode::KeyV => Key::V,
        KeyCode::KeyW => Key::W,
        KeyCode::KeyX => Key::X,
        KeyCode::KeyY => Key::Y,
        KeyCode::KeyZ => Key::Z,
        KeyCode::F1 => Key::F1,
        KeyCode::F2 => Key::F2,
        KeyCode::F3 => Key::F3,
        KeyCode::F4 => Key::F4,
        KeyCode::F5 => Key::F5,
        KeyCode::F6 => Key::F6,
        KeyCode::F7 => Key::F7,
        KeyCode::F8 => Key::F8,
        KeyCode::F9 => Key::F9,
        KeyCode::F10 => Key::F10,
        KeyCode::F11 => Key::F11,
        KeyCode::F12 => Key::F12,
        KeyCode::Minus | KeyCode::NumpadSubtract => Key::Minus,
        KeyCode::Equal => Key::Equals,
        KeyCode::NumpadAdd => Key::Plus,
        KeyCode::BracketLeft => Key::OpenBracket,
        KeyCode::BracketRight => Key::CloseBracket,
        KeyCode::Backslash => Key::Backslash,
        KeyCode::Semicolon => Key::Semicolon,
        KeyCode::Quote => Key::Quote,
        KeyCode::Backquote => Key::Backtick,
        KeyCode::Comma => Key::Comma,
        KeyCode::Period | KeyCode::NumpadDecimal => Key::Period,
        KeyCode::Slash | KeyCode::NumpadDivide => Key::Slash,
        _ => return None,
    };

    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(x0: f32, y0: f32, x1: f32, y1: f32) -> [Corner; 3] {
        let at = |x: f32, y: f32| Corner {
            x,
            y,
            u: 0.0,
            v: 0.0,
            color: [1.0; 4],
        };

        [at(x0, y0), at(x1, y0), at(x1, y1)]
    }

    #[test]
    fn inside_triangles_pass_through() {
        let mut out = Vec::new();
        push_clipped(
            &mut out,
            quad(10.0, 10.0, 20.0, 20.0),
            [0.0, 0.0, 100.0, 100.0],
        );

        assert_eq!(out.len(), 3 * gfx::SCREEN_FLOATS);
    }

    #[test]
    fn outside_triangles_are_dropped() {
        let mut out = Vec::new();
        push_clipped(
            &mut out,
            quad(200.0, 10.0, 220.0, 20.0),
            [0.0, 0.0, 100.0, 100.0],
        );

        assert!(out.is_empty());
    }

    #[test]
    fn straddling_triangles_stay_inside_the_clip() {
        let mut out = Vec::new();
        push_clipped(
            &mut out,
            quad(50.0, 50.0, 150.0, 150.0),
            [0.0, 0.0, 100.0, 100.0],
        );

        assert!(!out.is_empty());
        let mut idx = 0;

        while idx < out.len() {
            assert!(out[idx] <= 100.0 + 1e-3);
            assert!(out[idx + 1] <= 100.0 + 1e-3);
            idx += gfx::SCREEN_FLOATS;
        }
    }

    #[test]
    fn a_frame_tessellates_into_screen_vertices() {
        let mut gui = Gui::new(2.0);
        let probe = |ctx: &egui::Context| {
            egui::Window::new("probe").show(ctx, |ui| {
                ui.label("hello");
                let _ = ui.button("press");
            });
        };
        let first = gui.run(800, 600, probe);

        assert!(!first.textures_delta.set.is_empty());
        let output = gui.run(800, 600, probe);
        let primitives = gui.ctx.tessellate(output.shapes, output.pixels_per_point);
        let mut out = Vec::new();

        for primitive in primitives {
            if let Primitive::Mesh(mesh) = primitive.primitive {
                push_mesh(
                    &mut out,
                    &mesh,
                    [0.0, 0.0, 800.0, 600.0],
                    output.pixels_per_point,
                );
            }
        }

        assert!(out.len() > 100 * gfx::SCREEN_FLOATS);
        assert_eq!(out.len() % gfx::SCREEN_FLOATS, 0);
    }
}
