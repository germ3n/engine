use crate::ui::vr::{self, EyeViews, Headset, VrInput};
use crate::ui::window::Window;
use crate::ui::voxel::SceneView;
use crate::ui::Color;
use glow::HasContext; // Exposes OpenGL methods
use glutin::{
    config::{ConfigTemplateBuilder, GlConfig},
    context::{ContextAttributesBuilder, PossiblyCurrentContext},
    display::GetGlDisplay,
    prelude::*,
    surface::{SurfaceAttributesBuilder, SwapInterval, WindowSurface},
};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasRawWindowHandle;
use std::num::NonZeroU32;
use winit::{
    event_loop::EventLoop,
    window::{Window as WinitWindow, WindowBuilder},
};
use glow_glyph::{GlyphBrush, GlyphBrushBuilder, Section, Text, ab_glyph::FontArc};
use std::collections::HashMap;
pub struct OpenGLWindow {
    pub window: WinitWindow,
    pub context: PossiblyCurrentContext,
    pub surface: glutin::surface::Surface<WindowSurface>,
    pub event_loop: Option<EventLoop<()>>,
    pub gl: glow::Context,
    pub shader_program: glow::Program,
    pub vao: glow::VertexArray,
    pub vbo: glow::Buffer,
    pub glyph_brushes: HashMap<String, GlyphBrush>,
    colored_mesh: ColoredMesh,
    vr: Option<Headset>,
    vr_failed: bool,
    vr_enable: bool,
    eyes: Option<GlEyes>,
    clear: [f32; 4],
}

struct GlEyes {
    width: i32,
    height: i32,
    color: [glow::NativeTexture; 2],
    depth: [glow::NativeRenderbuffer; 2],
    frame: [glow::NativeFramebuffer; 2],
}

impl Window for OpenGLWindow {
    fn create_window() -> Self {
        let event_loop = EventLoop::new().unwrap();
        let window_builder = WindowBuilder::new().with_title("Starting...");

        let template = ConfigTemplateBuilder::new().with_depth_size(24);
        let display_builder = DisplayBuilder::new().with_window_builder(Some(window_builder));

        let (window, gl_config) = display_builder
            .build(&event_loop, template, |configs| {
                configs.reduce(|accum, config| {
                    if config.num_samples() > accum.num_samples() { config } else { accum }
                }).unwrap()
            }).unwrap();

        let window = window.expect("Failed to create winit window");
        let raw_window_handle = window.raw_window_handle();
        let gl_display = gl_config.display();

        let context_attributes = ContextAttributesBuilder::new().build(Some(raw_window_handle));
        let not_current_gl_context = unsafe {
            gl_display.create_context(&gl_config, &context_attributes).expect("Failed to create OpenGL context")
        };

        let (width, height): (u32, u32) = window.inner_size().into();
        let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window_handle,
            NonZeroU32::new(width.max(1)).unwrap(),
            NonZeroU32::new(height.max(1)).unwrap(),
        );

        let surface = unsafe {
            gl_display.create_window_surface(&gl_config, &surface_attributes).unwrap()
        };
        let context = not_current_gl_context.make_current(&surface).unwrap();
        let _ = surface.set_swap_interval(&context, SwapInterval::DontWait);

        // 1. Initialize Glow
        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                let c_str = std::ffi::CString::new(s).unwrap();
                gl_display.get_proc_address(c_str.as_c_str())
            })
        };

        // 2. Setup Shaders and Buffers
        let (shader_program, vao, vbo) = unsafe {
            // Vertex Shader: Converts pixel coordinates to screen space (-1.0 to 1.0)
            let vs = gl.create_shader(glow::VERTEX_SHADER).unwrap();
            gl.shader_source(vs, r#"
                #version 330 core
                in vec2 aPos;
                uniform vec2 uResolution;
                void main() {
                    vec2 zeroToOne = aPos / uResolution;
                    vec2 zeroToTwo = zeroToOne * 2.0;
                    vec2 clipSpace = zeroToTwo - 1.0;
                    gl_Position = vec4(clipSpace.x, -clipSpace.y, 0.0, 1.0); // Flip Y so 0 is at top
                }
            "#);
            gl.compile_shader(vs);

            // Fragment Shader: Applies the color
            let fs = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
            gl.shader_source(fs, r#"
                #version 330 core
                out vec4 FragColor;
                uniform vec4 uColor;
                void main() {
                    FragColor = uColor;
                }
            "#);
            gl.compile_shader(fs);

            // Link program
            let program = gl.create_program().unwrap();
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);

            // Create VAO and VBO
            let vao = gl.create_vertex_array().unwrap();
            let vbo = gl.create_buffer().unwrap();

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            
            // Tell OpenGL how to read our vertex data (2 floats per vertex)
            let pos_attrib = gl.get_attrib_location(program, "aPos").unwrap();
            gl.vertex_attrib_pointer_f32(pos_attrib, 2, glow::FLOAT, false, 8, 0);
            gl.enable_vertex_attrib_array(pos_attrib);

            (program, vao, vbo)
        };

        let colored_mesh = ColoredMesh::new(&gl);
        let mut opengl_window = Self {
            window, 
            context, 
            surface, 
            event_loop: Some(event_loop),
            gl, 
            shader_program, 
            vao, 
            vbo,
            glyph_brushes: HashMap::new(),
            colored_mesh,
            vr: None,
            vr_failed: false,
            vr_enable: false,
            eyes: None,
            clear: [0.0, 0.0, 0.0, 1.0],
        };

        let font_default_bytes = include_bytes!("font_default.ttf");
        let font_default = FontArc::try_from_slice(font_default_bytes).expect("Failed to load font_default.ttf!");
        let glyph_brush_default = GlyphBrushBuilder::using_font(font_default).build(&opengl_window.gl);
        opengl_window.glyph_brushes.insert("default".to_string(), glyph_brush_default);
        opengl_window
    }

    fn set_window_title(&mut self, title: &str) {
        self.window.set_title(title);
    }

    fn winit_window(&self) -> &WinitWindow {
        &self.window
    }

    fn take_event_loop(&mut self) -> EventLoop<()> {
        self.event_loop.take().expect("Event loop missing")
    }

    fn present(&mut self) {
        self.surface.swap_buffers(&self.context).unwrap();

        if let Some(headset) = self.vr.as_mut() {
            headset.handoff();
        }
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        self.clear = [red, green, blue, 1.0];
        unsafe {
            self.gl.depth_mask(true);
            self.gl.clear_color(red, green, blue, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }

    fn draw_colored_mesh(&mut self, vertices: &[f32], revision: u64, view: &SceneView) {
        self.colored_mesh.sync(&self.gl, vertices, revision);
        let eyes = vr::connect(&mut self.vr, &mut self.vr_failed, self.vr_enable, view);

        if let Some(eyes) = eyes {
            self.draw_headset(eyes);
        } else if self.colored_mesh.vertex_count > 0 {
            let view_proj = gl_view_proj(view);
            self.colored_mesh.draw(&self.gl, &view_proj);
        }

        unsafe {
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.disable(glow::CULL_FACE);
        }
    }

    fn set_size(&mut self, width: u32, height: u32) {
        let size = winit::dpi::PhysicalSize::new(width, height);
        let _ = self.window.request_inner_size(size);

        if let (Some(w), Some(h)) = (std::num::NonZeroU32::new(width.max(1)), std::num::NonZeroU32::new(height.max(1))) {
            self.surface.resize(&self.context, w, h);
            unsafe { self.gl.viewport(0, 0, width as i32, height as i32); }
        }
    }

    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        // Fallback to white if no color is provided
        let size = self.window.inner_size();
        
        // Two triangles that make up the rectangle (x, y coordinates)
        let vertices: [f32; 12] = [
            x, y,         // Top-left
            x + w, y,     // Top-right
            x, y + h,     // Bottom-left
            x, y + h,     // Bottom-left
            x + w, y,     // Top-right
            x + w, y + h, // Bottom-right
        ];

        unsafe {
            // Enable blending for transparency (alpha)
            self.gl.enable(glow::BLEND);
            self.gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

            self.gl.use_program(Some(self.shader_program));
            
            // Pass the screen resolution to the shader so it scales pixels properly
            if let Some(loc) = self.gl.get_uniform_location(self.shader_program, "uResolution") {
                self.gl.uniform_2_f32(Some(&loc), size.width as f32, size.height as f32);
            }
            
            // Pass the color (converted from 0-255 u8 to 0.0-1.0 f32)
            if let Some(loc) = self.gl.get_uniform_location(self.shader_program, "uColor") {
                let [r, g, b, a] = color.as_rgba_f32();
                self.gl.uniform_4_f32(Some(&loc), r, g, b, a);
            }

            // Bind our geometry buffer and push the new coordinates to the GPU
            self.gl.bind_vertex_array(Some(self.vao));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            
            // Safely cast the f32 array into raw bytes to send to OpenGL
            let vertices_u8 = core::slice::from_raw_parts(
                vertices.as_ptr() as *const u8,
                vertices.len() * std::mem::size_of::<f32>()
            );
            self.gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, vertices_u8, glow::DYNAMIC_DRAW);
            
            // Execute the draw command (6 vertices = 2 triangles)
            self.gl.draw_arrays(glow::TRIANGLES, 0, 6);
        }
    }

    fn draw_outlined_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, thickness: f32, color: Color) {
        self.draw_rectangle(x, y, w, thickness, color);
        self.draw_rectangle(x, y + h - thickness, w, thickness, color);
        self.draw_rectangle(x, y + thickness, thickness, h - 2.0 * thickness, color);
        self.draw_rectangle(x + w - thickness, y + thickness, thickness, h - 2.0 * thickness, color);
    }

    fn draw_text(&mut self, font: &str, text: &str, x: f32, y: f32, scale: f32, color: Color) {
        // Convert your 0-255 color to 0.0-1.0 floats
        let rgba = color.as_rgba_f32();

        let font_brush = if self.glyph_brushes.contains_key(font) {
            self.glyph_brushes.get_mut(font).unwrap()
        } else {
            self.glyph_brushes.get_mut("default").unwrap()
        };
        font_brush.queue(Section {
            screen_position: (x, y),
            text: vec![Text::default()
                .with_text(text)
                .with_scale(scale)
                .with_color(rgba)],
            ..Section::default()
        });
    }

    fn enable_vr(&mut self) {
        self.vr_enable = true;
    }

    fn vr_input(&self) -> VrInput {
        match self.vr.as_ref() {
            Some(headset) => headset.input(),
            None => VrInput::default(),
        }
    }

    fn render_text(&mut self) {
        let size = self.window.inner_size();
        
        unsafe {
            // glow_glyph needs blending enabled to draw smooth font edges
            self.gl.enable(glow::BLEND);
            self.gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        }

        for (_, glyph_brush) in self.glyph_brushes.iter_mut() {
            glyph_brush.draw_queued(&self.gl, size.width, size.height).expect("Failed to draw text");
        }
    }
}

impl OpenGLWindow {
    fn draw_headset(&mut self, eyes: EyeViews) {
        if !self.ensure_gl_eyes(eyes.width, eyes.height) {
            return;
        }

        let (color, frame, width, height) = {
            let Some(targets) = &self.eyes else {
                return;
            };

            (targets.color, targets.frame, targets.width, targets.height)
        };
        let mut idx = 0;

        while idx < 2 {
            unsafe {
                self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(frame[idx]));
                self.gl.viewport(0, 0, width, height);
                self.gl.depth_mask(true);
                self.gl.clear_color(self.clear[0], self.clear[1], self.clear[2], self.clear[3]);
                self.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            }

            if self.colored_mesh.vertex_count > 0 {
                let matrix = gl_view_proj(&eyes.views[idx]);
                self.colored_mesh.draw(&self.gl, &matrix);
            }

            unsafe { self.gl.flush(); }

            if let Some(headset) = self.vr.as_mut() {
                headset.submit_gl(idx, color[idx].0.get());
            }

            idx += 1;
        }

        let size = self.window.inner_size();
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.viewport(0, 0, size.width.max(1) as i32, size.height.max(1) as i32);
        }

        if self.colored_mesh.vertex_count > 0 {
            let matrix = gl_view_proj(&eyes.views[0]);
            self.colored_mesh.draw(&self.gl, &matrix);
        }
    }

    fn ensure_gl_eyes(&mut self, width: u32, height: u32) -> bool {
        let width = width.max(1) as i32;
        let height = height.max(1) as i32;

        if let Some(eyes) = &self.eyes {
            if eyes.width == width && eyes.height == height {
                return true;
            }
        }

        self.destroy_gl_eyes();
        self.eyes = GlEyes::create(&self.gl, width, height);

        self.eyes.is_some()
    }

    fn destroy_gl_eyes(&mut self) {
        let Some(eyes) = self.eyes.take() else {
            return;
        };
        let mut idx = 0;

        while idx < 2 {
            unsafe {
                self.gl.delete_framebuffer(eyes.frame[idx]);
                self.gl.delete_renderbuffer(eyes.depth[idx]);
                self.gl.delete_texture(eyes.color[idx]);
            }

            idx += 1;
        }
    }
}

impl Drop for OpenGLWindow {
    fn drop(&mut self) {
        self.destroy_gl_eyes();
        self.vr.take();
    }
}

const MESH_VERT: &str = r#"
#version 330 core
layout(location = 0) in vec3 aPos;
layout(location = 1) in vec3 aColor;
uniform mat4 uViewProj;
out vec3 vColor;
void main() {
    gl_Position = uViewProj * vec4(aPos, 1.0);
    vColor = aColor;
}
"#;

const MESH_FRAG: &str = r#"
#version 330 core
in vec3 vColor;
out vec4 FragColor;
void main() {
    FragColor = vec4(vColor, 1.0);
}
"#;

struct ColoredMesh {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    view_proj: Option<glow::UniformLocation>,
    vertex_count: i32,
    revision: u64,
    ready: bool,
}

impl ColoredMesh {
    fn new(gl: &glow::Context) -> Self {
        unsafe {
            let program = link_mesh_program(gl);
            let vao = gl.create_vertex_array().unwrap();
            let vbo = gl.create_buffer().unwrap();
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let stride = 6 * 4;
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 12);
            gl.enable_vertex_attrib_array(1);

            Self {
                program,
                vao,
                vbo,
                view_proj: gl.get_uniform_location(program, "uViewProj"),
                vertex_count: 0,
                revision: 0,
                ready: false,
            }
        }
    }

    fn sync(&mut self, gl: &glow::Context, vertices: &[f32], revision: u64) {
        if self.ready && self.revision == revision {
            return;
        }

        self.vertex_count = (vertices.len() / 6) as i32;
        self.revision = revision;
        self.ready = true;

        unsafe {
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            let bytes = std::slice::from_raw_parts(vertices.as_ptr() as *const u8, vertices.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
        }
    }

    fn draw(&self, gl: &glow::Context, view_proj: &[f32; 16]) {
        unsafe {
            gl.disable(glow::BLEND);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LESS);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);
            gl.use_program(Some(self.program));
            gl.uniform_matrix_4_f32_slice(self.view_proj.as_ref(), false, view_proj);
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, self.vertex_count);
        }
    }
}

fn gl_view_proj(view: &SceneView) -> [f32; 16] {
    vr::view_proj(view, false)
}

impl GlEyes {
    fn create(gl: &glow::Context, width: i32, height: i32) -> Option<Self> {
        let mut color = [None, None];
        let mut depth = [None, None];
        let mut frame = [None, None];
        let mut idx = 0;

        while idx < 2 {
            let texture = unsafe { gl.create_texture().ok()? };
            let render = unsafe { gl.create_renderbuffer().ok()? };
            let buffer = unsafe { gl.create_framebuffer().ok()? };
            unsafe {
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, width, height, 0, glow::RGBA, glow::UNSIGNED_BYTE, None);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(render));
                gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT24, width, height);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(buffer));
                gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(texture), 0);
                gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::RENDERBUFFER, Some(render));
                let complete = gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
                gl.bind_framebuffer(glow::FRAMEBUFFER, None);

                if !complete {
                    gl.delete_framebuffer(buffer);
                    gl.delete_renderbuffer(render);
                    gl.delete_texture(texture);

                    return None;
                }
            }
            color[idx] = Some(texture);
            depth[idx] = Some(render);
            frame[idx] = Some(buffer);
            idx += 1;
        }

        Some(Self {
            width,
            height,
            color: [color[0]?, color[1]?],
            depth: [depth[0]?, depth[1]?],
            frame: [frame[0]?, frame[1]?],
        })
    }
}

fn link_mesh_program(gl: &glow::Context) -> glow::Program {
    unsafe {
        let program = gl.create_program().unwrap();
        let vert = compile_mesh_shader(gl, glow::VERTEX_SHADER, MESH_VERT);
        let frag = compile_mesh_shader(gl, glow::FRAGMENT_SHADER, MESH_FRAG);
        gl.attach_shader(program, vert);
        gl.attach_shader(program, frag);
        gl.link_program(program);

        if !gl.get_program_link_status(program) {
            println!("[gl] voxel link {}", gl.get_program_info_log(program));
        }

        gl.delete_shader(vert);
        gl.delete_shader(frag);

        program
    }
}

fn compile_mesh_shader(gl: &glow::Context, kind: u32, source: &str) -> glow::Shader {
    unsafe {
        let shader = gl.create_shader(kind).unwrap();
        gl.shader_source(shader, source);
        gl.compile_shader(shader);

        if !gl.get_shader_compile_status(shader) {
            println!("[gl] voxel shader {}", gl.get_shader_info_log(shader));
        }

        shader
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::voxel::FlyCamera;
    use crate::world::{Block, BlockPos, VoxelWorld};

    #[test]
    fn gl_projection_shows_front_faces() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        let mesh = world.mesh();
        let view = FlyCamera::new().scene(4.0 / 3.0, 1.0);
        let view_proj = gl_view_proj(&view);
        let mut visible = 0;
        let mut front = 0;
        let mut idx = 0;

        while idx + 18 <= mesh.len() {
            let a = project(&view_proj, [mesh[idx], mesh[idx + 1], mesh[idx + 2]]);
            let b = project(&view_proj, [mesh[idx + 6], mesh[idx + 7], mesh[idx + 8]]);
            let c = project(&view_proj, [mesh[idx + 12], mesh[idx + 13], mesh[idx + 14]]);

            if a[3] > 0.0 && b[3] > 0.0 && c[3] > 0.0 {
                let ax = a[0] / a[3];
                let ay = a[1] / a[3];
                let az = a[2] / a[3];
                let bx = b[0] / b[3];
                let by = b[1] / b[3];
                let bz = b[2] / b[3];
                let cx = c[0] / c[3];
                let cy = c[1] / c[3];
                let cz = c[2] / c[3];
                let on_screen = ax.abs() < 1.5 && ay.abs() < 1.5 && bx.abs() < 1.5 && by.abs() < 1.5 && cx.abs() < 1.5 && cy.abs() < 1.5;
                let in_depth = az > -1.0 && az < 1.0 && bz > -1.0 && bz < 1.0 && cz > -1.0 && cz < 1.0;

                if on_screen && in_depth {
                    visible += 1;
                    let winding = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);

                    if winding > 0.0 {
                        front += 1;
                    }
                }
            }

            idx += 18;
        }

        assert!(visible > 0);
        assert!(front > 0);
    }

    fn project(view_proj: &[f32; 16], position: [f32; 3]) -> [f32; 4] {
        let p = [position[0], position[1], position[2], 1.0];
        let mut clip = [0.0; 4];
        let mut row = 0;

        while row < 4 {
            let mut sum = 0.0;
            let mut col = 0;

            while col < 4 {
                sum += view_proj[col * 4 + row] * p[col];
                col += 1;
            }

            clip[row] = sum;
            row += 1;
        }

        clip
    }
}