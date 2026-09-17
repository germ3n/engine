use crate::ui::window::Window;
use crate::ui::Color;
use glow::HasContext; // Exposes OpenGL methods
use glutin::{
    config::{ConfigTemplateBuilder, GlConfig},
    context::{ContextAttributesBuilder, PossiblyCurrentContext},
    display::GetGlDisplay,
    prelude::*,
    surface::{SurfaceAttributesBuilder, WindowSurface},
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
}

impl Window for OpenGLWindow {
    fn create_window() -> Self {
        let event_loop = EventLoop::new().unwrap();
        let window_builder = WindowBuilder::new().with_title("Starting...");

        let template = ConfigTemplateBuilder::new();
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

        let mut opengl_window = Self {
            window, 
            context, 
            surface, 
            event_loop: Some(event_loop),
            gl, 
            shader_program, 
            vao, 
            vbo,
            glyph_brushes: HashMap::new()
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

    fn render_text(&mut self) {
        let size = self.window.inner_size();
        
        unsafe {
            // glow_glyph needs blending enabled to draw smooth font edges
            self.gl.enable(glow::BLEND);
            self.gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        }

        for (font, glyph_brush) in self.glyph_brushes.iter_mut() {
            glyph_brush.draw_queued(&self.gl, size.width, size.height).expect("Failed to draw text");
        }
    }
}