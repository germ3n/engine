use crate::platform::Surface;
use crate::ui::batch::Scissor;
use crate::ui::shader;
use crate::ui::skin::SkinBatch;
use crate::ui::voxel::SceneView;
use crate::ui::vr::{self, EyeViews, Headset, VrInput};
use crate::ui::window::Window;
use crate::ui::Color;
use glow::HasContext;
use glow_glyph::{ab_glyph::FontArc, GlyphBrush, GlyphBrushBuilder, Section, Text};
#[cfg(target_os = "android")]
use glutin::config::Api;
#[cfg(target_os = "android")]
use glutin::context::{ContextApi, Version};
use glutin::display::{Display, DisplayApiPreference};
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextAttributesBuilder, PossiblyCurrentContext},
    prelude::*,
    surface::{SurfaceAttributesBuilder, SwapInterval, WindowSurface},
};
use std::collections::HashMap;
use std::num::NonZeroU32;

pub struct OpenGLWindow {
    pub width: u32,
    pub height: u32,
    pub context: PossiblyCurrentContext,
    pub surface: glutin::surface::Surface<WindowSurface>,
    pub gl: glow::Context,
    pub shader_program: glow::Program,
    sprite: glow::Program,
    wrap_sampler: glow::Sampler,
    clamp_sampler: glow::Sampler,
    pub vao: glow::VertexArray,
    pub vbo: glow::Buffer,
    pub glyph_brushes: HashMap<String, GlyphBrush>,
    colored_mesh: ColoredMesh,
    skin: SkinCache,
    vr: Option<Headset>,
    vr_failed: bool,
    vr_enable: bool,
    eyes: Option<GlEyes>,
    clear: [f32; 4],
    bound: bool,
    bound_w: i32,
    bound_h: i32,
    scissor: Option<Scissor>,
    text_once: Option<crate::ui::batch::TextFrame>,
    text_atlas: Option<glow::Texture>,
}

struct GlEyes {
    width: i32,
    height: i32,
    color: [glow::NativeTexture; 2],
    depth: [glow::NativeRenderbuffer; 2],
    frame: [glow::NativeFramebuffer; 2],
}

impl OpenGLWindow {
    pub fn try_attach(surface: &Surface) -> Result<Self, String> {
        #[cfg(target_os = "android")]
        let template = ConfigTemplateBuilder::new()
            .with_depth_size(16)
            .with_api(Api::GLES2 | Api::GLES3);
        #[cfg(not(target_os = "android"))]
        let template = ConfigTemplateBuilder::new().with_depth_size(24);
        let preference = gl_preference(surface);
        let gl_display =
            unsafe { Display::new(surface.display, preference) }.map_err(|err| err.to_string())?;
        let gl_config = unsafe { gl_display.find_configs(template.build()) }
            .map_err(|err| err.to_string())?
            .reduce(|accum, config| {
                if config.num_samples() < accum.num_samples() {
                    config
                } else {
                    accum
                }
            })
            .ok_or_else(|| "no gl config".to_string())?;
        let attributes = ContextAttributesBuilder::new();
        #[cfg(target_os = "android")]
        let attributes = attributes.with_context_api(ContextApi::Gles(Some(Version::new(3, 0))));
        let context_attributes = attributes.build(Some(surface.window));
        let not_current_gl_context = unsafe {
            gl_display
                .create_context(&gl_config, &context_attributes)
                .map_err(|err| err.to_string())?
        };
        let width = surface.width.max(1);
        let height = surface.height.max(1);
        let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            surface.window,
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        );
        let gl_surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surface_attributes)
                .map_err(|err| err.to_string())?
        };
        let context = not_current_gl_context
            .make_current(&gl_surface)
            .map_err(|err| err.to_string())?;
        let _ = gl_surface.set_swap_interval(&context, SwapInterval::DontWait);
        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                let c_str = std::ffi::CString::new(s).unwrap();
                gl_display.get_proc_address(c_str.as_c_str())
            })
        };
        let cache = {
            let vendor = unsafe { gl.get_parameter_string(glow::VENDOR) };
            let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
            shader::Registry::for_device(&shader::id_from_text(&[&vendor, &renderer]))
        };
        let ui_vert = cache.glsl(
            shader::UI,
            naga::ShaderStage::Vertex,
            "ui_vert",
            shader::glsl_version(),
        )?;
        let ui_frag = cache.glsl(
            shader::UI,
            naga::ShaderStage::Fragment,
            "ui_frag",
            shader::glsl_version(),
        )?;
        let (shader_program, vao, vbo) = unsafe {
            let vs = gl.create_shader(glow::VERTEX_SHADER).unwrap();
            gl.shader_source(vs, &ui_vert);
            gl.compile_shader(vs);

            if !gl.get_shader_compile_status(vs) {
                log::warn!("[gl] ui vert {}", gl.get_shader_info_log(vs));
            }

            let fs = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
            gl.shader_source(fs, &ui_frag);
            gl.compile_shader(fs);

            if !gl.get_shader_compile_status(fs) {
                log::warn!("[gl] ui frag {}", gl.get_shader_info_log(fs));
            }

            let program = gl.create_program().unwrap();
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);

            if !gl.get_program_link_status(program) {
                log::warn!("[gl] ui link {}", gl.get_program_info_log(program));
            }

            let vao = gl.create_vertex_array().unwrap();
            let vbo = gl.create_buffer().unwrap();
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.enable_vertex_attrib_array(0);

            (program, vao, vbo)
        };
        let colored_mesh = ColoredMesh::new(&gl, &cache);
        let skin = SkinCache::new(&gl, &cache);
        let sprite = sprite_program(&gl, &cache)?;
        let wrap_sampler = gl_sampler(&gl, true, true)?;
        let clamp_sampler = gl_sampler(&gl, true, false)?;
        let mut opengl_window = Self {
            width,
            height,
            context,
            surface: gl_surface,
            gl,
            shader_program,
            sprite,
            wrap_sampler,
            clamp_sampler,
            vao,
            vbo,
            glyph_brushes: HashMap::new(),
            colored_mesh,
            skin,
            vr: None,
            vr_failed: false,
            vr_enable: false,
            eyes: None,
            clear: [0.0, 0.0, 0.0, 1.0],
            bound: false,
            bound_w: 0,
            bound_h: 0,
            scissor: None,
            text_once: None,
            text_atlas: None,
        };
        let font_default_bytes = include_bytes!("font_default.ttf");
        let font_default =
            FontArc::try_from_slice(font_default_bytes).expect("Failed to load font_default.ttf!");
        let glyph_brush_default =
            GlyphBrushBuilder::using_font(font_default).build(&opengl_window.gl);
        opengl_window
            .glyph_brushes
            .insert("default".to_string(), glyph_brush_default);

        Ok(opengl_window)
    }
}

fn gl_preference(surface: &Surface) -> DisplayApiPreference {
    #[cfg(target_os = "windows")]
    {
        return DisplayApiPreference::Wgl(Some(surface.window));
    }

    #[cfg(target_os = "macos")]
    {
        let _ = surface;

        return DisplayApiPreference::Cgl;
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = surface;

        return DisplayApiPreference::Egl;
    }

    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "ios"))
    ))]
    {
        let _ = surface;

        DisplayApiPreference::Egl
    }
}

impl Window for OpenGLWindow {
    fn attach(surface: &Surface) -> Self {
        Self::try_attach(surface).expect("opengl")
    }

    fn present(&mut self) {
        self.unbind_target();
        self.surface.swap_buffers(&self.context).unwrap();

        if let Some(headset) = self.vr.as_mut() {
            headset.handoff();
        }
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        self.scissor = None;
        self.apply_gl_scissor();
        self.unbind_target();
        self.clear = [red, green, blue, 1.0];
        unsafe {
            self.gl.depth_mask(true);
            self.gl.clear_color(red, green, blue, 1.0);
            self.gl
                .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }

    fn draw_skinned(&mut self, batch: &SkinBatch, view: &SceneView) {
        let matrix = gl_view_proj(view);
        self.skin.draw(&self.gl, batch, &matrix);
    }

    fn clear_depth(&mut self) {
        unsafe {
            self.gl.depth_mask(true);
            self.gl.clear(glow::DEPTH_BUFFER_BIT);
        }
    }

    fn draw_colored_mesh(
        &mut self,
        vertices: &[f32],
        ranges: &[crate::world::SurfaceRange],
        graphics: &crate::world::MapGraphics,
        revision: u64,
        view: &SceneView,
    ) {
        self.colored_mesh
            .sync(&self.gl, vertices, ranges, graphics, revision);
        let eyes = vr::connect(&mut self.vr, &mut self.vr_failed, self.vr_enable, view);

        if let Some(eyes) = eyes {
            self.draw_headset(eyes);
        } else if self.colored_mesh.vertex_count > 0 || graphics.sky.is_some() {
            self.colored_mesh
                .draw(&self.gl, view, self.width, self.height);
        }

        unsafe {
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.disable(glow::CULL_FACE);
        }
    }

    fn set_size(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);

        if let (Some(w), Some(h)) = (
            std::num::NonZeroU32::new(self.width),
            std::num::NonZeroU32::new(self.height),
        ) {
            self.surface.resize(&self.context, w, h);
            unsafe {
                self.gl
                    .viewport(0, 0, self.width as i32, self.height as i32);
            }
        }
    }

    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.apply_gl_scissor();
        let width = self.width;
        let height = self.height;
        let vertices: [f32; 12] = [x, y, x + w, y, x, y + h, x, y + h, x + w, y, x + w, y + h];

        unsafe {
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.depth_mask(false);
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            self.gl.use_program(Some(self.shader_program));

            if let Some(loc) = self
                .gl
                .get_uniform_location(self.shader_program, "_immediates_binding_vs.resolution")
            {
                self.gl
                    .uniform_2_f32(Some(&loc), width as f32, height as f32);
            }

            if let Some(loc) = self
                .gl
                .get_uniform_location(self.shader_program, "_immediates_binding_fs.color")
            {
                let [r, g, b, a] = color.as_rgba_f32();
                self.gl.uniform_4_f32(Some(&loc), r, g, b, a);
            }

            self.gl.bind_vertex_array(Some(self.vao));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            let vertices_u8 = core::slice::from_raw_parts(
                vertices.as_ptr() as *const u8,
                vertices.len() * std::mem::size_of::<f32>(),
            );
            self.gl
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, vertices_u8, glow::DYNAMIC_DRAW);
            self.gl.draw_arrays(glow::TRIANGLES, 0, 6);
        }
    }

    fn draw_outlined_rectangle(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        thickness: f32,
        color: Color,
    ) {
        self.draw_rectangle(x, y, w, thickness, color);
        self.draw_rectangle(x, y + h - thickness, w, thickness, color);
        self.draw_rectangle(x, y + thickness, thickness, h - 2.0 * thickness, color);
        self.draw_rectangle(
            x + w - thickness,
            y + thickness,
            thickness,
            h - 2.0 * thickness,
            color,
        );
    }

    fn draw_text(&mut self, font: &str, text: &str, x: f32, y: f32, scale: f32, color: Color) {
        if self.scissor.is_some() {
            crate::ui::gfx::BackendGpu::draw_text_user(
                self,
                text,
                x,
                y,
                scale,
                color.as_rgba_f32(),
                None,
                None,
                None,
            );

            return;
        }

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

    fn set_scissor(&mut self, rect: Option<[f32; 4]>) {
        self.scissor = rect.map(|rect| Scissor::from_rect(rect[0], rect[1], rect[2], rect[3]));
    }

    fn render_text(&mut self) {
        self.scissor = None;
        self.apply_gl_scissor();
        unsafe {
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.depth_mask(false);
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        }

        for (_, glyph_brush) in self.glyph_brushes.iter_mut() {
            glyph_brush
                .draw_queued(&self.gl, self.width, self.height)
                .expect("Failed to draw text");
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
                self.gl
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(frame[idx]));
                self.gl.viewport(0, 0, width, height);
                self.gl.depth_mask(true);
                self.gl
                    .clear_color(self.clear[0], self.clear[1], self.clear[2], self.clear[3]);
                self.gl
                    .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            }

            if self.colored_mesh.vertex_count > 0 || self.colored_mesh.has_sky() {
                self.colored_mesh
                    .draw(&self.gl, &eyes.views[idx], width as u32, height as u32);
            }

            unsafe {
                self.gl.flush();
            }

            if let Some(headset) = self.vr.as_mut() {
                headset.submit_gl(idx, color[idx].0.get());
            }

            idx += 1;
        }

        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl
                .viewport(0, 0, self.width.max(1) as i32, self.height.max(1) as i32);
        }

        if self.colored_mesh.vertex_count > 0 || self.colored_mesh.has_sky() {
            self.colored_mesh
                .draw(&self.gl, &eyes.views[0], self.width, self.height);
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

struct ColoredMesh {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    sky_vao: glow::VertexArray,
    sky_vbo: glow::Buffer,
    ubo: glow::Buffer,
    gpu_stride: i32,
    gpu_slot: i32,
    shaded_slot: i32,
    unlit_slot: i32,
    view_vs: [Option<glow::UniformLocation>; 3],
    view_fs: [Option<glow::UniformLocation>; 3],
    samplers: Vec<(glow::UniformLocation, i32)>,
    white: glow::Texture,
    flat: glow::Texture,
    cube: glow::Texture,
    scene: glow::Texture,
    scene_size: (i32, i32),
    materials: Vec<GlMaterial>,
    material_lookup: HashMap<String, glow::Texture>,
    lightmaps: [glow::Texture; 4],
    cubemaps: Vec<glow::Texture>,
    sky: Option<glow::Texture>,
    ranges: Vec<crate::world::SurfaceRange>,
    cpu: Vec<f32>,
    order: Vec<usize>,
    order_eye: [f32; 3],
    bound: [Option<glow::Texture>; 13],
    pass_key: u8,
    copy_water: bool,
    sky_eye: [f32; 3],
    sky_ready: bool,
    graphics_key: u64,
    vertex_count: i32,
    revision: u64,
    ready: bool,
    using_slow: bool,
    batch_ok: bool,
    fast_ok: bool,
    fast_current: bool,
    batch_program: glow::Program,
    fast_program: glow::Program,
    batch_view_vs: [Option<glow::UniformLocation>; 3],
    batch_view_fs: [Option<glow::UniformLocation>; 3],
    fast_view_vs: [Option<glow::UniformLocation>; 3],
    fast_view_fs: [Option<glow::UniformLocation>; 3],
    batch_vao: glow::VertexArray,
    batch_vbo: glow::Buffer,
    batch_first: [i32; 3],
    batch_count: [i32; 3],
    fast_count: i32,
    fast_chunks: Vec<MeshChunk>,
    opaque_chunks: Vec<MeshChunk>,
    base_array: [glow::Texture; 2],
    bump_array: [glow::Texture; 2],
    detail_array: [glow::Texture; 2],
    base2_array: [glow::Texture; 2],
    env_array: glow::Texture,
    params_tex: glow::Texture,
}

struct MeshChunk {
    first: i32,
    count: i32,
    min: [f32; 3],
    max: [f32; 3],
}

struct GlMaterial {
    gpu: crate::world::surface::MaterialGpu,
    base: glow::Texture,
    base2: glow::Texture,
    bump: glow::Texture,
    bump2: glow::Texture,
    detail: glow::Texture,
    blend: glow::Texture,
    mask: glow::Texture,
}

impl ColoredMesh {
    fn new(gl: &glow::Context, cache: &shader::Registry) -> Self {
        unsafe {
            let program = link_mesh_program(gl, cache, "fs_main");
            let batch_program = link_mesh_program(gl, cache, "fs_batch");
            let fast_program = link_mesh_program(gl, cache, "fs_fast");
            let batch_ok = gl.get_program_link_status(batch_program);
            let fast_ok = gl.get_program_link_status(fast_program);

            if !batch_ok {
                log::warn!("[gl] batch link {}", gl.get_program_info_log(batch_program));
            }

            if !fast_ok {
                log::warn!("[gl] fast link {}", gl.get_program_info_log(fast_program));
            }
            let vao = gl.create_vertex_array().unwrap();
            let vbo = gl.create_buffer().unwrap();
            let sky_vao = gl.create_vertex_array().unwrap();
            let sky_vbo = gl.create_buffer().unwrap();
            let ubo = gl.create_buffer().unwrap();
            let align = gl
                .get_parameter_i32(glow::UNIFORM_BUFFER_OFFSET_ALIGNMENT)
                .max(1);
            let gpu_stride = gpu_stride(align);
            let packed = pack_gpus(
                &[
                    crate::world::surface::MaterialGpu::shaded(),
                    crate::world::surface::MaterialGpu::unlit(),
                ],
                gpu_stride as usize,
            );
            gl.bind_buffer(glow::UNIFORM_BUFFER, Some(ubo));
            gl.buffer_data_u8_slice(glow::UNIFORM_BUFFER, &packed, glow::DYNAMIC_DRAW);
            bind_mesh_attribs(gl, vao, vbo);
            bind_mesh_attribs(gl, sky_vao, sky_vbo);

            gl.use_program(Some(program));
            let names = [
                ("_group_0_binding_2_fs", 0),
                ("_group_0_binding_3_fs", 1),
                ("_group_0_binding_4_fs", 2),
                ("_group_0_binding_5_fs", 3),
                ("_group_0_binding_6_fs", 4),
                ("_group_0_binding_7_fs", 5),
                ("_group_0_binding_8_fs", 6),
                ("_group_0_binding_9_fs", 7),
                ("_group_1_binding_0_fs", 8),
                ("_group_1_binding_1_fs", 9),
                ("_group_1_binding_2_fs", 10),
                ("_group_1_binding_3_fs", 11),
                ("_group_1_binding_4_fs", 12),
            ];
            let mut samplers = Vec::new();
            let mut idx = 0;

            while idx < names.len() {
                if let Some(location) = gl.get_uniform_location(program, names[idx].0) {
                    gl.uniform_1_i32(Some(&location), names[idx].1);
                    samplers.push((location, names[idx].1));
                }

                idx += 1;
            }

            let block = gl.get_uniform_block_index(program, "MaterialGpu_block_0Fragment");

            if let Some(index) = block {
                gl.uniform_block_binding(program, index, 0);
            }
            let white = gl_color(gl, &crate::world::surface::CpuImage::white(), true);
            let flat = gl_color(gl, &crate::world::surface::CpuImage::flat_normal(), true);
            let cube = gl_cube(
                gl,
                &crate::world::surface::CubeImage::solid(crate::world::surface::CpuImage::white()),
            );
            let scene = gl.create_texture().unwrap();
            let batch_vao = gl.create_vertex_array().unwrap();
            let batch_vbo = gl.create_buffer().unwrap();
            bind_mesh_attribs(gl, batch_vao, batch_vbo);
            gl.use_program(Some(fast_program));
            assign_samplers(
                gl,
                fast_program,
                &[
                    ("_group_2_binding_0_fs", 0),
                    ("_group_2_binding_1_fs", 1),
                    ("_group_2_binding_4_fs", 4),
                    ("_group_1_binding_0_fs", 5),
                ],
            );
            gl.use_program(Some(batch_program));
            assign_samplers(
                gl,
                batch_program,
                &[
                    ("_group_2_binding_0_fs", 0),
                    ("_group_2_binding_1_fs", 1),
                    ("_group_2_binding_2_fs", 2),
                    ("_group_2_binding_3_fs", 3),
                    ("_group_2_binding_4_fs", 4),
                    ("_group_2_binding_5_fs", 9),
                    ("_group_2_binding_6_fs", 10),
                    ("_group_2_binding_7_fs", 11),
                    ("_group_2_binding_8_fs", 12),
                    ("_group_2_binding_9_fs", 13),
                    ("_group_1_binding_0_fs", 5),
                    ("_group_1_binding_1_fs", 6),
                    ("_group_1_binding_2_fs", 7),
                    ("_group_1_binding_3_fs", 8),
                ],
            );
            let white_px = [255u8, 255, 255, 255];
            let flat_px = [128u8, 128, 255, 255];
            let base_array = [solid_array(gl, &white_px), solid_array(gl, &white_px)];
            let bump_array = [solid_array(gl, &flat_px), solid_array(gl, &flat_px)];
            let detail_array = [solid_array(gl, &white_px), solid_array(gl, &white_px)];
            let base2_array = [solid_array(gl, &white_px), solid_array(gl, &white_px)];
            let env_array = solid_cube_array(gl);
            let params_tex = gl.create_texture().unwrap();

            Self {
                program,
                vao,
                vbo,
                sky_vao,
                sky_vbo,
                ubo,
                gpu_stride,
                gpu_slot: -1,
                shaded_slot: 0,
                unlit_slot: 1,
                view_vs: view_locations(gl, program, "vs"),
                view_fs: view_locations(gl, program, "fs"),
                samplers,
                white,
                flat,
                cube,
                scene,
                scene_size: (0, 0),
                materials: Vec::new(),
                material_lookup: HashMap::new(),
                lightmaps: [white, white, white, white],
                cubemaps: Vec::new(),
                sky: None,
                ranges: Vec::new(),
                cpu: Vec::new(),
                order: Vec::new(),
                order_eye: [0.0, 0.0, 0.0],
                bound: [None; 13],
                pass_key: 255,
                copy_water: false,
                sky_eye: [0.0, 0.0, 0.0],
                sky_ready: false,
                graphics_key: u64::MAX,
                vertex_count: 0,
                revision: 0,
                ready: false,
                using_slow: true,
                batch_ok,
                fast_ok,
                fast_current: false,
                batch_program,
                fast_program,
                batch_view_vs: view_locations(gl, batch_program, "vs"),
                batch_view_fs: view_locations(gl, batch_program, "fs"),
                fast_view_vs: view_locations(gl, fast_program, "vs"),
                fast_view_fs: view_locations(gl, fast_program, "fs"),
                batch_vao,
                batch_vbo,
                batch_first: [0; 3],
                batch_count: [0; 3],
                fast_count: 0,
                fast_chunks: Vec::new(),
                opaque_chunks: Vec::new(),
                base_array,
                bump_array,
                detail_array,
                base2_array,
                env_array,
                params_tex,
            }
        }
    }

    fn has_sky(&self) -> bool {
        self.sky.is_some()
    }

    fn sync(
        &mut self,
        gl: &glow::Context,
        vertices: &[f32],
        ranges: &[crate::world::SurfaceRange],
        graphics: &crate::world::MapGraphics,
        revision: u64,
    ) {
        let mesh_changed = !self.ready || self.revision != revision;

        if mesh_changed {
            self.vertex_count = (vertices.len() / crate::world::STRIDE) as i32;
            self.revision = revision;
            self.ready = true;
            self.ranges = ranges.to_vec();
            self.cpu = vertices.to_vec();
            self.order.clear();

            unsafe {
                gl.bind_vertex_array(Some(self.vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
                let bytes =
                    std::slice::from_raw_parts(vertices.as_ptr() as *const u8, vertices.len() * 4);
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            }
        }

        let key = graphics_key(graphics);
        let graphics_changed = self.graphics_key != key;

        if graphics_changed {
            self.graphics_key = key;
            self.bound = [None; 13];
            self.upload_graphics(gl, graphics);
        }

        if mesh_changed || graphics_changed {
            self.rebuild_batch(gl);
        }
    }

    fn upload_graphics(&mut self, gl: &glow::Context, graphics: &crate::world::MapGraphics) {
        self.clear_graphics(gl);
        let mut materials = Vec::with_capacity(graphics.materials.len());
        let mut idx = 0;

        while idx < graphics.materials.len() {
            let material = &graphics.materials[idx];
            materials.push(GlMaterial {
                gpu: material.gpu,
                base: gl_color(gl, &material.base, true),
                base2: gl_color(gl, &material.base2, true),
                bump: gl_color(gl, &material.bump, true),
                bump2: gl_color(gl, &material.bump2, true),
                detail: gl_color(gl, &material.detail, true),
                blend: gl_color(gl, &material.blend, true),
                mask: gl_color(gl, &material.mask, true),
            });
            idx += 1;
        }

        self.materials = materials;
        self.material_lookup.clear();
        idx = 0;

        while idx < graphics.material_names.len() && idx < self.materials.len() {
            let name = &graphics.material_names[idx];

            if !name.is_empty() {
                self.material_lookup
                    .insert(name.clone(), self.materials[idx].base);
            }

            idx += 1;
        }

        idx = 0;

        while idx < 4 {
            self.lightmaps[idx] = gl_color(gl, &graphics.lightmaps[idx], false);
            idx += 1;
        }

        idx = 0;

        while idx < graphics.cubemaps.len() {
            self.cubemaps.push(gl_cube(gl, &graphics.cubemaps[idx]));
            idx += 1;
        }

        if let Some(sky) = &graphics.sky {
            self.sky = Some(gl_cube(gl, sky));
        }

        self.copy_water = false;
        idx = 0;

        while idx < self.materials.len() {
            if self.materials[idx].gpu.params[0] == crate::world::surface::MODE_WATER {
                self.copy_water = true;

                break;
            }

            idx += 1;
        }

        self.sky_ready = false;
        self.pack_arrays(gl, graphics);
        self.pack_gpu(gl);
    }

    fn pack_gpu(&mut self, gl: &glow::Context) {
        let mut gpus = Vec::with_capacity(self.materials.len() + 2);
        let mut idx = 0;

        while idx < self.materials.len() {
            gpus.push(self.materials[idx].gpu);
            idx += 1;
        }

        self.shaded_slot = gpus.len() as i32;
        gpus.push(crate::world::surface::MaterialGpu::shaded());
        self.unlit_slot = gpus.len() as i32;
        gpus.push(crate::world::surface::MaterialGpu::unlit());
        let bytes = pack_gpus(&gpus, self.gpu_stride as usize);
        unsafe {
            gl.bind_buffer(glow::UNIFORM_BUFFER, Some(self.ubo));
            gl.buffer_data_u8_slice(glow::UNIFORM_BUFFER, &bytes, glow::DYNAMIC_DRAW);
        }
        self.gpu_slot = -1;
    }

    fn pack_arrays(&mut self, gl: &glow::Context, graphics: &crate::world::MapGraphics) {
        unsafe {
            gl.delete_texture(self.base_array[0]);
            gl.delete_texture(self.base_array[1]);
            gl.delete_texture(self.bump_array[0]);
            gl.delete_texture(self.bump_array[1]);
            gl.delete_texture(self.detail_array[0]);
            gl.delete_texture(self.detail_array[1]);
            gl.delete_texture(self.base2_array[0]);
            gl.delete_texture(self.base2_array[1]);
            gl.delete_texture(self.env_array);
            gl.delete_texture(self.params_tex);
        }
        let mut bases = Vec::with_capacity(graphics.materials.len());
        let mut bumps = Vec::with_capacity(graphics.materials.len());
        let mut details = Vec::with_capacity(graphics.materials.len());
        let mut base2s = Vec::with_capacity(graphics.materials.len());
        let mut idx = 0;

        while idx < graphics.materials.len() {
            bases.push(&graphics.materials[idx].base);
            bumps.push(&graphics.materials[idx].bump);
            details.push(&graphics.materials[idx].detail);
            base2s.push(&graphics.materials[idx].base2);
            idx += 1;
        }

        let white = crate::world::surface::CpuImage::white();
        let flat = crate::world::surface::CpuImage::flat_normal();
        let (base0, base1, base_picks) = pack_role(gl, &bases, &white);
        let (bump0, bump1, bump_picks) = pack_role(gl, &bumps, &flat);
        let (detail0, detail1, detail_picks) = pack_role(gl, &details, &white);
        let (base20, base21, base2_picks) = pack_role(gl, &base2s, &white);
        self.base_array = [base0, base1];
        self.bump_array = [bump0, bump1];
        self.detail_array = [detail0, detail1];
        self.base2_array = [base20, base21];
        self.env_array = upload_env(gl, &graphics.cubemaps);
        self.params_tex = params_texture(
            gl,
            graphics,
            &base_picks,
            &bump_picks,
            &detail_picks,
            &base2_picks,
        );
    }

    fn rebuild_batch(&mut self, gl: &glow::Context) {
        let mut fast = Vec::new();
        let mut opaque = Vec::new();
        let mut alpha = Vec::new();
        let mut decal = Vec::new();
        let mut idx = 0;

        while idx < self.ranges.len() {
            let range = self.ranges[idx];
            let Some(slot) = batch_slot(range.pass) else {
                idx += 1;

                continue;
            };

            if !self.is_batchable(range.material) {
                idx += 1;

                continue;
            }

            let start = range.first as usize * crate::world::STRIDE;
            let end = start + range.count as usize * crate::world::STRIDE;

            if end <= self.cpu.len() {
                let verts = &self.cpu[start..end];

                if slot == 0 && self.is_fast(range.material) {
                    fast.extend_from_slice(verts);
                } else if slot == 0 {
                    opaque.extend_from_slice(verts);
                } else if slot == 1 {
                    alpha.extend_from_slice(verts);
                } else {
                    decal.extend_from_slice(verts);
                }
            }

            idx += 1;
        }

        let (fast, fast_chunks) = grid_mesh(&fast);
        let (opaque, mut opaque_chunks) = grid_mesh(&opaque);
        let mut all = Vec::with_capacity(fast.len() + opaque.len() + alpha.len() + decal.len());
        self.fast_count = (fast.len() / crate::world::STRIDE) as i32;
        self.fast_chunks = fast_chunks;
        all.extend_from_slice(&fast);
        self.batch_first[0] = (all.len() / crate::world::STRIDE) as i32;
        self.batch_count[0] = (opaque.len() / crate::world::STRIDE) as i32;
        let mut chunk = 0;

        while chunk < opaque_chunks.len() {
            opaque_chunks[chunk].first += self.fast_count;
            chunk += 1;
        }

        self.opaque_chunks = opaque_chunks;
        all.extend_from_slice(&opaque);
        self.batch_first[1] = (all.len() / crate::world::STRIDE) as i32;
        self.batch_count[1] = (alpha.len() / crate::world::STRIDE) as i32;
        all.extend_from_slice(&alpha);
        self.batch_first[2] = (all.len() / crate::world::STRIDE) as i32;
        self.batch_count[2] = (decal.len() / crate::world::STRIDE) as i32;
        all.extend_from_slice(&decal);
        unsafe {
            gl.bind_vertex_array(Some(self.batch_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.batch_vbo));
            let bytes = std::slice::from_raw_parts(all.as_ptr() as *const u8, all.len() * 4);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
        }
        log::info!(
            "[gl] batched {} fast in {} chunks, {} opaque in {} chunks, {} alpha, {} decals",
            self.fast_count,
            self.fast_chunks.len(),
            self.batch_count[0],
            self.opaque_chunks.len(),
            self.batch_count[1],
            self.batch_count[2]
        );
    }

    fn is_batchable(&self, material: u16) -> bool {
        let Some(item) = self.materials.get(material as usize) else {
            return false;
        };
        let mode = item.gpu.params[0];

        if mode != crate::world::surface::MODE_LIGHT
            && mode != crate::world::surface::MODE_UNLIT
            && mode != crate::world::surface::MODE_BLEND
        {
            return false;
        }

        let flags = item.gpu.detail[3].to_bits();

        if !self.batch_ok {
            return false;
        }

        let blocked = crate::world::surface::FLAG_BUMP2 | crate::world::surface::FLAG_BLENDMOD;

        flags & blocked == 0
    }

    fn is_fast(&self, material: u16) -> bool {
        if !self.fast_ok || !self.is_batchable(material) {
            return false;
        }

        let Some(item) = self.materials.get(material as usize) else {
            return false;
        };
        let mode = item.gpu.params[0];

        if mode != crate::world::surface::MODE_LIGHT && mode != crate::world::surface::MODE_UNLIT {
            return false;
        }

        let flags = item.gpu.detail[3].to_bits();
        let heavy = crate::world::surface::FLAG_BUMP
            | crate::world::surface::FLAG_SSBUMP
            | crate::world::surface::FLAG_DETAIL
            | crate::world::surface::FLAG_ENV
            | crate::world::surface::FLAG_BASE2
            | crate::world::surface::FLAG_SELF
            | crate::world::surface::FLAG_ALPHA
            | crate::world::surface::FLAG_PHONG
            | crate::world::surface::FLAG_MASK;

        flags & heavy == 0
    }

    fn use_slow(&mut self, gl: &glow::Context) {
        if self.using_slow {
            return;
        }

        self.using_slow = true;
        self.fast_current = false;
        unsafe {
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
        }
    }

    fn draw_batch(
        &mut self,
        gl: &glow::Context,
        slot: usize,
        matrix: &[f32; 16],
        view: &SceneView,
        width: u32,
        height: u32,
        planes: &[[f32; 4]; 6],
    ) {
        if self.batch_count[slot] <= 0 {
            return;
        }

        if slot == 0 && !any_visible(&self.opaque_chunks, planes) {
            return;
        }

        self.using_slow = false;
        self.fast_current = false;
        let pass = if slot == 0 {
            crate::world::surface::PASS_OPAQUE
        } else if slot == 1 {
            crate::world::surface::PASS_ALPHA
        } else {
            crate::world::surface::PASS_DECAL
        };
        unsafe {
            gl.use_program(Some(self.batch_program));
            gl.bind_vertex_array(Some(self.batch_vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.base_array[0]));
            gl.active_texture(glow::TEXTURE0 + 1);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.base_array[1]));
            gl.active_texture(glow::TEXTURE0 + 2);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.bump_array[0]));
            gl.active_texture(glow::TEXTURE0 + 3);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.bump_array[1]));
            gl.active_texture(glow::TEXTURE0 + 4);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.params_tex));
            gl.active_texture(glow::TEXTURE0 + 9);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.detail_array[0]));
            gl.active_texture(glow::TEXTURE0 + 10);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.detail_array[1]));
            gl.active_texture(glow::TEXTURE0 + 11);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.base2_array[0]));
            gl.active_texture(glow::TEXTURE0 + 12);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.base2_array[1]));
            gl.active_texture(glow::TEXTURE0 + 13);
            gl.bind_texture(glow::TEXTURE_CUBE_MAP_ARRAY, Some(self.env_array));
            gl.active_texture(glow::TEXTURE0 + 5);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.lightmaps[0]));
            gl.active_texture(glow::TEXTURE0 + 6);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.lightmaps[1]));
            gl.active_texture(glow::TEXTURE0 + 7);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.lightmaps[2]));
            gl.active_texture(glow::TEXTURE0 + 8);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.lightmaps[3]));
        }
        self.bound = [None; 13];
        self.bind_view(gl, matrix, view, width, height, 0.0);
        self.apply_pass(gl, pass, crate::world::surface::MODE_LIGHT);

        if slot == 0 {
            draw_visible(gl, &self.opaque_chunks, planes);

            return;
        }

        unsafe {
            gl.draw_arrays(
                glow::TRIANGLES,
                self.batch_first[slot],
                self.batch_count[slot],
            );
        }
    }

    fn draw_fast(
        &mut self,
        gl: &glow::Context,
        matrix: &[f32; 16],
        view: &SceneView,
        width: u32,
        height: u32,
        planes: &[[f32; 4]; 6],
    ) {
        if self.fast_count <= 0 || !any_visible(&self.fast_chunks, planes) {
            return;
        }

        self.using_slow = false;
        self.fast_current = true;
        unsafe {
            gl.use_program(Some(self.fast_program));
            gl.bind_vertex_array(Some(self.batch_vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.base_array[0]));
            gl.active_texture(glow::TEXTURE0 + 1);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.base_array[1]));
            gl.active_texture(glow::TEXTURE0 + 4);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.params_tex));
            gl.active_texture(glow::TEXTURE0 + 5);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.lightmaps[0]));
        }
        self.bound = [None; 13];
        self.bind_view(gl, matrix, view, width, height, 0.0);
        self.apply_pass(
            gl,
            crate::world::surface::PASS_OPAQUE,
            crate::world::surface::MODE_LIGHT,
        );
        draw_visible(gl, &self.fast_chunks, planes);
    }

    fn clear_graphics(&mut self, gl: &glow::Context) {
        let mut idx = 0;

        while idx < self.materials.len() {
            let material = &self.materials[idx];
            unsafe {
                gl.delete_texture(material.base);
                gl.delete_texture(material.base2);
                gl.delete_texture(material.bump);
                gl.delete_texture(material.bump2);
                gl.delete_texture(material.detail);
                gl.delete_texture(material.blend);
                gl.delete_texture(material.mask);
            }
            idx += 1;
        }

        self.materials.clear();
        self.material_lookup.clear();
        idx = 0;

        while idx < 4 {
            if self.lightmaps[idx] != self.white {
                unsafe { gl.delete_texture(self.lightmaps[idx]) };
            }

            self.lightmaps[idx] = self.white;
            idx += 1;
        }

        idx = 0;

        while idx < self.cubemaps.len() {
            unsafe { gl.delete_texture(self.cubemaps[idx]) };
            idx += 1;
        }

        self.cubemaps.clear();

        if let Some(sky) = self.sky.take() {
            unsafe { gl.delete_texture(sky) };
        }
    }

    fn draw(&mut self, gl: &glow::Context, view: &SceneView, width: u32, height: u32) {
        let matrix = gl_view_proj(view);
        self.using_slow = false;
        self.use_slow(gl);
        unsafe {
            gl.enable(glow::DEPTH_TEST);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);
        }
        self.bind_view(gl, &matrix, view, width, height, 0.0);

        if let Some(sky) = self.sky {
            self.draw_sky(gl, sky, &matrix, view, width, height);
            self.bind_view(gl, &matrix, view, width, height, 0.0);
        }

        self.ensure_order(view.eye);
        let planes = frustum_planes(&matrix);
        self.pass_key = 255;
        let mut copied = false;
        let mut seen = [false; 3];
        let mut idx = 0;

        while idx < self.order.len() {
            let range = self.ranges[self.order[idx]];

            if let Some(slot) = batch_slot(range.pass) {
                if !seen[slot] {
                    if slot == 0 {
                        self.draw_fast(gl, &matrix, view, width, height, &planes);
                    }

                    self.draw_batch(gl, slot, &matrix, view, width, height, &planes);
                    seen[slot] = true;
                }

                if self.is_batchable(range.material) {
                    idx += 1;

                    continue;
                }
            }

            if self.copy_water && !copied && range.pass >= crate::world::surface::PASS_BLEND {
                self.use_slow(gl);
                self.copy_scene(gl, width, height);
                copied = true;
            }

            self.draw_range(gl, &range);
            idx += 1;
        }

        unsafe {
            gl.disable(glow::POLYGON_OFFSET_FILL);
            gl.disable(glow::BLEND);
            gl.depth_mask(true);
        }
    }

    fn draw_sky(
        &mut self,
        gl: &glow::Context,
        sky: glow::Texture,
        matrix: &[f32; 16],
        view: &SceneView,
        width: u32,
        height: u32,
    ) {
        self.bind_view(gl, matrix, view, width, height, 1.0);
        self.bind_material(gl, self.unlit_slot);
        self.bind_slots(
            gl,
            [
                self.white,
                self.white,
                self.flat,
                self.flat,
                self.white,
                self.white,
                self.white,
                sky,
                self.lightmaps[0],
                self.lightmaps[1],
                self.lightmaps[2],
                self.lightmaps[3],
                self.scene,
            ],
        );

        if !self.sky_ready || sky_moved(self.sky_eye, view.eye) {
            let verts = crate::world::surface::sky_vertices(view.eye, view.far * 0.25);
            unsafe {
                gl.bind_vertex_array(Some(self.sky_vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.sky_vbo));
                let bytes =
                    std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 4);
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            }
            self.sky_eye = view.eye;
            self.sky_ready = true;
        }

        unsafe {
            gl.disable(glow::CULL_FACE);
            gl.depth_func(glow::ALWAYS);
            gl.depth_mask(true);
            gl.disable(glow::BLEND);
            gl.bind_vertex_array(Some(self.sky_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 36);
            gl.bind_vertex_array(Some(self.vao));
            gl.depth_func(glow::LESS);
            gl.enable(glow::CULL_FACE);
        }
    }

    fn ensure_order(&mut self, eye: [f32; 3]) {
        if self.order.len() == self.ranges.len() && !sky_moved(self.order_eye, eye) {
            return;
        }

        self.order = crate::world::surface::ordered_ranges(&self.cpu, &self.ranges, eye);
        self.order_eye = eye;
    }

    fn draw_range(&mut self, gl: &glow::Context, range: &crate::world::SurfaceRange) {
        self.use_slow(gl);
        let material = self.materials.get(range.material as usize);
        let gpu = material
            .map(|item| item.gpu)
            .unwrap_or_else(crate::world::surface::MaterialGpu::shaded);
        let base = material.map(|item| item.base).unwrap_or(self.white);
        let base2 = material.map(|item| item.base2).unwrap_or(self.white);
        let bump = material.map(|item| item.bump).unwrap_or(self.flat);
        let bump2 = material.map(|item| item.bump2).unwrap_or(self.flat);
        let detail = material.map(|item| item.detail).unwrap_or(self.white);
        let blend = material.map(|item| item.blend).unwrap_or(self.white);
        let mask = material.map(|item| item.mask).unwrap_or(self.white);
        let env = self
            .cubemaps
            .get(range.cubemap as usize)
            .copied()
            .unwrap_or(self.cube);
        let slot = if (range.material as usize) < self.materials.len() {
            range.material as i32
        } else {
            self.shaded_slot
        };
        self.bind_material(gl, slot);
        self.bind_slots(
            gl,
            [
                base,
                base2,
                bump,
                bump2,
                detail,
                blend,
                mask,
                env,
                self.lightmaps[0],
                self.lightmaps[1],
                self.lightmaps[2],
                self.lightmaps[3],
                self.scene,
            ],
        );
        self.apply_pass(gl, range.pass, gpu.params[0]);
        unsafe {
            gl.draw_arrays(glow::TRIANGLES, range.first as i32, range.count as i32);
        }
    }

    fn apply_pass(&mut self, gl: &glow::Context, pass: u8, mode: f32) {
        let key = pass_key(pass, mode);

        if self.pass_key == key {
            return;
        }

        self.pass_key = key;
        unsafe {
            gl.disable(glow::POLYGON_OFFSET_FILL);
            gl.depth_func(glow::LESS);
            gl.depth_mask(true);
            gl.disable(glow::BLEND);

            if pass == crate::world::surface::PASS_DECAL {
                gl.enable(glow::POLYGON_OFFSET_FILL);
                gl.polygon_offset(-1.0, -2.0);
                gl.depth_func(glow::LEQUAL);
                gl.depth_mask(false);
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            }

            if pass == crate::world::surface::PASS_BLEND {
                gl.depth_mask(false);
                gl.enable(glow::BLEND);

                if mode == crate::world::surface::MODE_ADD {
                    gl.blend_func(glow::ONE, glow::ONE);
                } else if mode == crate::world::surface::MODE_MODULATE {
                    gl.blend_func(glow::DST_COLOR, glow::ZERO);
                } else {
                    gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                }
            }
        }
    }

    fn bind_slots(&mut self, gl: &glow::Context, slots: [glow::Texture; 13]) {
        let mut idx = 0;

        while idx < slots.len() {
            if self.bound[idx] != Some(slots[idx]) {
                unsafe {
                    gl.active_texture(glow::TEXTURE0 + idx as u32);
                    let target = if idx == 7 {
                        glow::TEXTURE_CUBE_MAP
                    } else {
                        glow::TEXTURE_2D
                    };
                    gl.bind_texture(target, Some(slots[idx]));
                }
                self.bound[idx] = Some(slots[idx]);
            }

            idx += 1;
        }
    }

    fn bind_material(&mut self, gl: &glow::Context, slot: i32) {
        if self.gpu_slot == slot {
            return;
        }

        self.gpu_slot = slot;
        unsafe {
            gl.bind_buffer_range(
                glow::UNIFORM_BUFFER,
                0,
                Some(self.ubo),
                slot * self.gpu_stride,
                self.gpu_stride,
            );
        }
    }

    fn bind_view(
        &self,
        gl: &glow::Context,
        matrix: &[f32; 16],
        view: &SceneView,
        width: u32,
        height: u32,
        sky: f32,
    ) {
        let (vs, fs) = if self.fast_current {
            (&self.fast_view_vs, &self.fast_view_fs)
        } else if self.using_slow {
            (&self.view_vs, &self.view_fs)
        } else {
            (&self.batch_view_vs, &self.batch_view_fs)
        };
        unsafe {
            gl.uniform_matrix_4_f32_slice(vs[0].as_ref(), false, matrix);
            gl.uniform_4_f32(
                vs[1].as_ref(),
                view.eye[0],
                view.eye[1],
                view.eye[2],
                view.time,
            );
            gl.uniform_4_f32(vs[2].as_ref(), width as f32, height as f32, sky, 0.0);
            gl.uniform_matrix_4_f32_slice(fs[0].as_ref(), false, matrix);
            gl.uniform_4_f32(
                fs[1].as_ref(),
                view.eye[0],
                view.eye[1],
                view.eye[2],
                view.time,
            );
            gl.uniform_4_f32(fs[2].as_ref(), width as f32, height as f32, sky, 0.0);
        }
    }

    fn copy_scene(&mut self, gl: &glow::Context, width: u32, height: u32) {
        let width = width.max(1) as i32;
        let height = height.max(1) as i32;
        unsafe {
            gl.active_texture(glow::TEXTURE0 + 12);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.scene));
            self.bound[12] = Some(self.scene);

            if self.scene_size != (width, height) {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA as i32,
                    width,
                    height,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    None,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
                self.scene_size = (width, height);
            }

            gl.copy_tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, 0, 0, width, height);
        }
    }
}

fn bind_mesh_attribs(gl: &glow::Context, vao: glow::VertexArray, vbo: glow::Buffer) {
    unsafe {
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let stride = (crate::world::STRIDE * 4) as i32;
        let attrs = [
            (0, 3, 0),
            (1, 3, 12),
            (2, 4, 24),
            (3, 2, 40),
            (4, 2, 48),
            (5, 3, 56),
            (6, 1, 68),
            (7, 1, 72),
        ];
        let mut idx = 0;

        while idx < attrs.len() {
            let (location, size, offset) = attrs[idx];
            gl.vertex_attrib_pointer_f32(location, size, glow::FLOAT, false, stride, offset);
            gl.enable_vertex_attrib_array(location);
            idx += 1;
        }
    }
}

fn pass_key(pass: u8, mode: f32) -> u8 {
    if pass == crate::world::surface::PASS_DECAL {
        return 3;
    }

    if pass == crate::world::surface::PASS_BLEND {
        if mode == crate::world::surface::MODE_ADD {
            return 10;
        }

        if mode == crate::world::surface::MODE_MODULATE {
            return 11;
        }

        return 12;
    }

    1
}

fn sky_moved(from: [f32; 3], to: [f32; 3]) -> bool {
    let dx = from[0] - to[0];
    let dy = from[1] - to[1];
    let dz = from[2] - to[2];

    dx * dx + dy * dy + dz * dz > 64.0
}

fn view_locations(
    gl: &glow::Context,
    program: glow::Program,
    stage: &str,
) -> [Option<glow::UniformLocation>; 3] {
    unsafe {
        [
            gl.get_uniform_location(program, &format!("_immediates_binding_{stage}.view_proj")),
            gl.get_uniform_location(program, &format!("_immediates_binding_{stage}.eye_time")),
            gl.get_uniform_location(program, &format!("_immediates_binding_{stage}.screen")),
        ]
    }
}

fn graphics_key(graphics: &crate::world::MapGraphics) -> u64 {
    let mut key = graphics.materials.len() as u64;
    key = key
        .wrapping_mul(131)
        .wrapping_add(graphics.lightmaps[0].width as u64);
    key = key
        .wrapping_mul(131)
        .wrapping_add(graphics.lightmaps[0].bytes.len() as u64);
    key = key
        .wrapping_mul(131)
        .wrapping_add(graphics.cubemaps.len() as u64);
    key = key
        .wrapping_mul(131)
        .wrapping_add(u64::from(graphics.sky.is_some()));
    let sample = graphics.lightmaps[0].bytes.len().min(64);
    let mut idx = 0;

    while idx + 4 <= sample {
        let chunk = u32::from_le_bytes([
            graphics.lightmaps[0].bytes[idx],
            graphics.lightmaps[0].bytes[idx + 1],
            graphics.lightmaps[0].bytes[idx + 2],
            graphics.lightmaps[0].bytes[idx + 3],
        ]);
        key = key.wrapping_mul(131).wrapping_add(chunk as u64);
        idx += 4;
    }

    key
}

fn assign_samplers(gl: &glow::Context, program: glow::Program, names: &[(&str, i32)]) {
    let mut idx = 0;

    while idx < names.len() {
        unsafe {
            if let Some(location) = gl.get_uniform_location(program, names[idx].0) {
                gl.uniform_1_i32(Some(&location), names[idx].1);
            }
        }
        idx += 1;
    }
}

fn solid_array(gl: &glow::Context, rgba: &[u8; 4]) -> glow::Texture {
    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(texture));
        gl.tex_image_3d(
            glow::TEXTURE_2D_ARRAY,
            0,
            glow::RGBA as i32,
            1,
            1,
            1,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(rgba),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_MIN_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_MAG_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAX_LEVEL, 0);
        texture
    }
}

fn solid_cube_array(gl: &glow::Context) -> glow::Texture {
    let face = [0u8, 0, 0, 255];
    let mut bytes = Vec::new();
    let mut idx = 0;

    while idx < 6 {
        bytes.extend_from_slice(&face);
        idx += 1;
    }

    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_CUBE_MAP_ARRAY, Some(texture));
        gl.tex_image_3d(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            0,
            glow::RGBA as i32,
            1,
            1,
            6,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(&bytes),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            glow::TEXTURE_WRAP_R,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(glow::TEXTURE_CUBE_MAP_ARRAY, glow::TEXTURE_MAX_LEVEL, 0);
        texture
    }
}

fn upload_env(gl: &glow::Context, cubes: &[crate::world::surface::CubeImage]) -> glow::Texture {
    let size = 128u32;
    let layers = cubes.len() + 1;
    let face_bytes = (size as usize) * (size as usize) * 4;
    let mut bytes = vec![0u8; face_bytes * 6 * layers];
    let mut idx = 0;

    while idx < cubes.len() {
        let mut face = 0;

        while face < 6 {
            let image = &cubes[idx].faces[face];
            let rgba = crate::world::image_rgba(image);
            let scaled = scale_rgba(&rgba, image.width.max(1), image.height.max(1), size, size);
            let dst = ((idx + 1) * 6 + face) * face_bytes;

            if scaled.len() == face_bytes {
                bytes[dst..dst + face_bytes].copy_from_slice(&scaled);
            }

            face += 1;
        }

        idx += 1;
    }

    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_CUBE_MAP_ARRAY, Some(texture));
        gl.tex_image_3d(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            0,
            glow::RGBA as i32,
            size as i32,
            size as i32,
            (layers * 6) as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(&bytes),
        );

        if cubes.is_empty() {
            gl.tex_parameter_i32(
                glow::TEXTURE_CUBE_MAP_ARRAY,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(glow::TEXTURE_CUBE_MAP_ARRAY, glow::TEXTURE_MAX_LEVEL, 0);
        } else {
            gl.generate_mipmap(glow::TEXTURE_CUBE_MAP_ARRAY);
            gl.tex_parameter_i32(
                glow::TEXTURE_CUBE_MAP_ARRAY,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR_MIPMAP_LINEAR as i32,
            );
        }

        gl.tex_parameter_i32(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_CUBE_MAP_ARRAY,
            glow::TEXTURE_WRAP_R,
            glow::CLAMP_TO_EDGE as i32,
        );
        texture
    }
}

fn batch_slot(pass: u8) -> Option<usize> {
    if pass == crate::world::surface::PASS_OPAQUE {
        return Some(0);
    }

    if pass == crate::world::surface::PASS_ALPHA {
        return Some(1);
    }

    if pass == crate::world::surface::PASS_DECAL {
        return Some(2);
    }

    None
}

fn pack_role(
    gl: &glow::Context,
    images: &[&crate::world::surface::CpuImage],
    fallback: &crate::world::surface::CpuImage,
) -> (glow::Texture, glow::Texture, Vec<[u32; 2]>) {
    let mut tally: Vec<(crate::world::surface::PixelFormat, u32, u32, usize)> = Vec::new();
    let mut idx = 0;

    while idx < images.len() {
        let image = images[idx];

        if image.width >= 32 && image.height >= 32 && !image.bytes.is_empty() {
            let mut found = false;
            let mut slot = 0;

            while slot < tally.len() {
                if tally[slot].0 == image.format
                    && tally[slot].1 == image.width
                    && tally[slot].2 == image.height
                {
                    tally[slot].3 += 1;
                    found = true;

                    break;
                }

                slot += 1;
            }

            if !found {
                tally.push((image.format, image.width, image.height, 1));
            }
        }

        idx += 1;
    }

    let mut best = 0usize;
    idx = 1;

    while idx < tally.len() {
        if tally[idx].3 > tally[best].3 {
            best = idx;
        }

        idx += 1;
    }

    let native = !tally.is_empty();
    let native_format = if native {
        tally[best].0
    } else {
        fallback.format
    };
    let native_w = if native { tally[best].1 } else { 1 };
    let native_h = if native { tally[best].2 } else { 1 };
    let scale_w = if native { native_w.min(512) } else { 256 };
    let scale_h = if native { native_h.min(512) } else { 256 };
    let mut native_layers = Vec::new();
    let mut native_index: HashMap<u64, u32> = HashMap::new();
    let mut scaled_layers = Vec::new();
    let mut scaled_index: HashMap<u64, u32> = HashMap::new();
    scaled_layers.push(scaled_image(fallback, scale_w, scale_h));
    scaled_index.insert(image_hash(fallback), 0);
    let mut picks = Vec::with_capacity(images.len());
    idx = 0;

    while idx < images.len() {
        let image = images[idx];
        let hash = image_hash(image);

        if native
            && image.format == native_format
            && image.width == native_w
            && image.height == native_h
            && !image.bytes.is_empty()
        {
            let layer = if let Some(layer) = native_index.get(&hash) {
                *layer
            } else {
                let layer = native_layers.len() as u32;
                native_index.insert(hash, layer);
                native_layers.push((*image).clone());
                layer
            };
            picks.push([0, layer]);
        } else {
            let layer = if let Some(layer) = scaled_index.get(&hash) {
                *layer
            } else {
                let layer = scaled_layers.len() as u32;
                scaled_index.insert(hash, layer);
                scaled_layers.push(scaled_image(image, scale_w, scale_h));
                layer
            };
            picks.push([1, layer]);
        }

        idx += 1;
    }

    if native_layers.is_empty() {
        native_layers.push(fallback.clone());
    }

    let native_tex = upload_array(gl, &native_layers);
    let scaled_tex = upload_array(gl, &scaled_layers);

    (native_tex, scaled_tex, picks)
}

fn params_texture(
    gl: &glow::Context,
    graphics: &crate::world::MapGraphics,
    base_picks: &[[u32; 2]],
    bump_picks: &[[u32; 2]],
    detail_picks: &[[u32; 2]],
    base2_picks: &[[u32; 2]],
) -> glow::Texture {
    let rows = graphics.materials.len().max(1);
    let mut pixels = vec![0f32; rows * 8 * 4];
    let mut idx = 0;

    while idx < graphics.materials.len() {
        let gpu = graphics.materials[idx].gpu;
        let base = base_picks.get(idx).copied().unwrap_or([1, 0]);
        let bump = bump_picks.get(idx).copied().unwrap_or([1, 0]);
        let detail = detail_picks.get(idx).copied().unwrap_or([1, 0]);
        let base2 = base2_picks.get(idx).copied().unwrap_or([1, 0]);
        let fields = [
            gpu.tint,
            gpu.params,
            gpu.detail,
            gpu.env,
            gpu.extra,
            gpu.fog,
            [
                base[0] as f32,
                base[1] as f32,
                bump[0] as f32,
                bump[1] as f32,
            ],
            [
                detail[0] as f32,
                detail[1] as f32,
                base2[0] as f32,
                base2[1] as f32,
            ],
        ];
        let mut column = 0;

        while column < fields.len() {
            let dst = (idx * 8 + column) * 4;
            pixels[dst] = fields[column][0];
            pixels[dst + 1] = fields[column][1];
            pixels[dst + 2] = fields[column][2];
            pixels[dst + 3] = fields[column][3];
            column += 1;
        }

        idx += 1;
    }

    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        let bytes = std::slice::from_raw_parts(pixels.as_ptr() as *const u8, pixels.len() * 4);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA32F as i32,
            8,
            rows as i32,
            0,
            glow::RGBA,
            glow::FLOAT,
            Some(bytes),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAX_LEVEL, 0);
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        texture
    }
}

fn scaled_image(
    image: &crate::world::surface::CpuImage,
    width: u32,
    height: u32,
) -> crate::world::surface::CpuImage {
    let rgba = crate::world::image_rgba(image);
    let bytes = if image.width == width
        && image.height == height
        && matches!(image.format, crate::world::surface::PixelFormat::Rgba8)
    {
        rgba
    } else {
        scale_rgba(
            &rgba,
            image.width.max(1),
            image.height.max(1),
            width,
            height,
        )
    };

    crate::world::surface::CpuImage {
        width,
        height,
        format: crate::world::surface::PixelFormat::Rgba8,
        bytes,
        mips: Vec::new(),
    }
}

fn scale_rgba(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![255u8; (dw as usize) * (dh as usize) * 4];
    let mut y = 0;

    while y < dh {
        let sy = (y * sh / dh.max(1)).min(sh.saturating_sub(1));
        let mut x = 0;

        while x < dw {
            let sx = (x * sw / dw.max(1)).min(sw.saturating_sub(1));
            let s = ((sy * sw + sx) * 4) as usize;
            let d = ((y * dw + x) * 4) as usize;

            if s + 4 <= src.len() && d + 4 <= out.len() {
                out[d..d + 4].copy_from_slice(&src[s..s + 4]);
            }

            x += 1;
        }

        y += 1;
    }

    out
}

fn image_hash(image: &crate::world::surface::CpuImage) -> u64 {
    let mut key = 0xcbf29ce484222325u64;
    key ^= image.width as u64;
    key = key.wrapping_mul(0x100000001b3);
    key ^= image.height as u64;
    key = key.wrapping_mul(0x100000001b3);
    key ^= image.bytes.len() as u64;
    key = key.wrapping_mul(0x100000001b3);
    let mut idx = 0;

    while idx < image.bytes.len() {
        key ^= image.bytes[idx] as u64;
        key = key.wrapping_mul(0x100000001b3);
        idx += 1;
    }

    key
}

fn upload_array(gl: &glow::Context, layers: &[crate::world::surface::CpuImage]) -> glow::Texture {
    let image = &layers[0];
    let depth = layers.len() as i32;
    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(texture));
        let compressed = compressed_format(image.format);

        if compressed == 0 {
            let mut bytes = Vec::new();
            let mut idx = 0;

            let need = (image.width as usize) * (image.height as usize) * 4;

            while idx < layers.len() {
                let source = &layers[idx].bytes;

                if source.len() < need {
                    bytes.extend_from_slice(&vec![255u8; need]);
                } else {
                    bytes.extend_from_slice(&source[..need]);
                }

                idx += 1;
            }

            gl.tex_image_3d(
                glow::TEXTURE_2D_ARRAY,
                0,
                glow::RGBA as i32,
                image.width as i32,
                image.height as i32,
                depth,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(&bytes),
            );

            if image.width > 1 && image.height > 1 {
                gl.generate_mipmap(glow::TEXTURE_2D_ARRAY);
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D_ARRAY,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR_MIPMAP_LINEAR as i32,
                );
            } else {
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D_ARRAY,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAX_LEVEL, 0);
            }
        } else {
            let mut level = 0i32;
            let mut width = image.width;
            let mut height = image.height;

            loop {
                let mut bytes = Vec::new();
                let mut idx = 0;
                let mut complete = true;

                while idx < layers.len() {
                    let source = if level == 0 {
                        layers[idx].bytes.as_slice()
                    } else if (level as usize) <= layers[idx].mips.len() {
                        layers[idx].mips[level as usize - 1].as_slice()
                    } else {
                        &[]
                    };

                    if source.len() != compressed_len(compressed, width, height) {
                        complete = false;

                        break;
                    }

                    bytes.extend_from_slice(source);
                    idx += 1;
                }

                if !complete || bytes.is_empty() {
                    break;
                }

                gl.compressed_tex_image_3d(
                    glow::TEXTURE_2D_ARRAY,
                    level,
                    compressed as i32,
                    width as i32,
                    height as i32,
                    depth,
                    0,
                    bytes.len() as i32,
                    &bytes,
                );
                level += 1;
                width = (width / 2).max(1);
                height = (height / 2).max(1);

                if level > 16 || (width == 1 && height == 1) {
                    break;
                }
            }

            if level > 1 {
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D_ARRAY,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR_MIPMAP_LINEAR as i32,
                );
                gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAX_LEVEL, level - 1);
            } else {
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D_ARRAY,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, glow::TEXTURE_MAX_LEVEL, 0);
            }
        }

        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_WRAP_S,
            glow::REPEAT as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_WRAP_T,
            glow::REPEAT as i32,
        );
        texture
    }
}

fn compressed_format(format: crate::world::surface::PixelFormat) -> u32 {
    match format {
        crate::world::surface::PixelFormat::Bc1 => 0x83F1,
        crate::world::surface::PixelFormat::Bc2 => 0x83F2,
        crate::world::surface::PixelFormat::Bc3 => 0x83F3,
        crate::world::surface::PixelFormat::Bc5 => 0x8DBD,
        crate::world::surface::PixelFormat::Bc7 => 0x8E8C,
        crate::world::surface::PixelFormat::Rgba8 | crate::world::surface::PixelFormat::Rgba16f => {
            0
        }
    }
}

fn compressed_len(format: u32, width: u32, height: u32) -> usize {
    let blocks_x = ((width + 3) / 4) as usize;
    let blocks_y = ((height + 3) / 4) as usize;
    let block = if format == 0x83F1 || format == 0x8DBD {
        8
    } else {
        16
    };

    blocks_x * blocks_y * block
}

fn gpu_stride(align: i32) -> i32 {
    let align = align.max(1);
    let mut stride = 96;

    if stride % align != 0 {
        stride += align - (stride % align);
    }

    stride
}

fn pack_gpus(gpus: &[crate::world::surface::MaterialGpu], stride: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; stride.saturating_mul(gpus.len())];
    let mut idx = 0;

    while idx < gpus.len() {
        let packed = gpu_bytes(&gpus[idx]);
        let start = idx * stride;
        bytes[start..start + packed.len()].copy_from_slice(&packed);
        idx += 1;
    }

    bytes
}

fn gpu_bytes(gpu: &crate::world::surface::MaterialGpu) -> [u8; 96] {
    let mut bytes = [0u8; 96];
    let fields = [
        gpu.tint, gpu.params, gpu.detail, gpu.env, gpu.extra, gpu.fog,
    ];
    let mut idx = 0;

    while idx < fields.len() {
        let chunk = idx * 16;
        let raw = fields[idx].map(|value| value.to_le_bytes());
        let mut part = 0;

        while part < 4 {
            bytes[chunk + part * 4..chunk + part * 4 + 4].copy_from_slice(&raw[part]);
            part += 1;
        }

        idx += 1;
    }

    bytes
}

fn gl_color(
    gl: &glow::Context,
    image: &crate::world::surface::CpuImage,
    repeat: bool,
) -> glow::Texture {
    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        upload_2d(gl, glow::TEXTURE_2D, image);
        texture_sampling(
            gl,
            glow::TEXTURE_2D,
            repeat,
            1 + image.mips.len() as i32,
            false,
        );

        texture
    }
}

fn gl_cube(gl: &glow::Context, image: &crate::world::surface::CubeImage) -> glow::Texture {
    unsafe {
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_CUBE_MAP, Some(texture));
        let mut levels = image.faces[0].mips.len();
        let mut idx = 1;

        while idx < 6 {
            if image.faces[idx].mips.len() < levels {
                levels = image.faces[idx].mips.len();
            }

            idx += 1;
        }

        idx = 0;

        while idx < 6 {
            upload_2d(
                gl,
                glow::TEXTURE_CUBE_MAP_POSITIVE_X + idx as u32,
                &image.faces[idx],
            );
            idx += 1;
        }

        texture_sampling(gl, glow::TEXTURE_CUBE_MAP, false, 1 + levels as i32, true);

        texture
    }
}

fn texture_sampling(gl: &glow::Context, target: u32, repeat: bool, levels: i32, cube: bool) {
    let wrap = if repeat {
        glow::REPEAT
    } else {
        glow::CLAMP_TO_EDGE
    } as i32;
    unsafe {
        gl.tex_parameter_i32(target, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(target, glow::TEXTURE_WRAP_S, wrap);
        gl.tex_parameter_i32(target, glow::TEXTURE_WRAP_T, wrap);

        if cube {
            gl.tex_parameter_i32(target, glow::TEXTURE_WRAP_R, glow::CLAMP_TO_EDGE as i32);
        }

        if levels > 1 {
            gl.tex_parameter_i32(
                target,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR_MIPMAP_LINEAR as i32,
            );
            gl.tex_parameter_i32(target, glow::TEXTURE_MAX_LEVEL, levels - 1);

            return;
        }

        gl.tex_parameter_i32(target, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(target, glow::TEXTURE_MAX_LEVEL, 0);
    }
}

fn upload_2d(gl: &glow::Context, target: u32, image: &crate::world::surface::CpuImage) {
    upload_level(
        gl,
        target,
        0,
        image.width,
        image.height,
        image.format,
        &image.bytes,
    );
    let mut idx = 0;

    while idx < image.mips.len() {
        let level = (idx + 1) as u32;
        let width = (image.width >> level).max(1);
        let height = (image.height >> level).max(1);
        upload_level(
            gl,
            target,
            level as i32,
            width,
            height,
            image.format,
            &image.mips[idx],
        );
        idx += 1;
    }
}

fn upload_level(
    gl: &glow::Context,
    target: u32,
    level: i32,
    width: u32,
    height: u32,
    format: crate::world::surface::PixelFormat,
    bytes: &[u8],
) {
    let compressed = match format {
        crate::world::surface::PixelFormat::Bc1 => 0x83F1,
        crate::world::surface::PixelFormat::Bc2 => 0x83F2,
        crate::world::surface::PixelFormat::Bc3 => 0x83F3,
        crate::world::surface::PixelFormat::Bc5 => 0x8DBD,
        crate::world::surface::PixelFormat::Bc7 => 0x8E8C,
        crate::world::surface::PixelFormat::Rgba8 | crate::world::surface::PixelFormat::Rgba16f => {
            0
        }
    };
    unsafe {
        if compressed == 0 {
            gl.tex_image_2d(
                target,
                level,
                glow::RGBA as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(bytes),
            );

            return;
        }

        gl.compressed_tex_image_2d(
            target,
            level,
            compressed as i32,
            width as i32,
            height as i32,
            0,
            bytes.len() as i32,
            bytes,
        );
    }
}

struct SkinGpu {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ibo: glow::Buffer,
    instances: glow::Buffer,
    albedo: glow::Texture,
    palette: glow::Texture,
    index_count: i32,
    vertices: usize,
    albedo_size: (u32, u32),
    palette_size: (i32, i32),
}

struct SkinCache {
    program: glow::Program,
    view_proj: Option<glow::UniformLocation>,
    bones: Option<glow::UniformLocation>,
    palette_loc: Option<glow::UniformLocation>,
    albedo_loc: Option<glow::UniformLocation>,
    meshes: HashMap<u64, SkinGpu>,
}

impl SkinCache {
    fn new(gl: &glow::Context, cache: &shader::Registry) -> Self {
        let program = link_skinned_program(gl, cache);

        unsafe {
            Self {
                view_proj: gl.get_uniform_location(program, "_immediates_binding_vs.view_proj"),
                bones: gl.get_uniform_location(program, "_immediates_binding_vs.bones"),
                palette_loc: gl.get_uniform_location(program, "_group_0_binding_2_vs"),
                albedo_loc: gl.get_uniform_location(program, "_group_0_binding_0_fs"),
                program,
                meshes: HashMap::new(),
            }
        }
    }

    fn draw(&mut self, gl: &glow::Context, batch: &SkinBatch, view_proj: &[f32; 16]) {
        if batch.groups.is_empty() {
            return;
        }

        unsafe {
            gl.disable(glow::BLEND);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_mask(true);
            gl.depth_func(glow::LESS);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);
            gl.use_program(Some(self.program));
            gl.uniform_matrix_4_f32_slice(self.view_proj.as_ref(), false, view_proj);
        }

        let mut idx = 0;

        while idx < batch.groups.len() {
            let group = &batch.groups[idx];
            self.sync(gl, group);
            let gpu = &self.meshes[&group.key];

            unsafe {
                gl.uniform_1_u32(self.bones.as_ref(), group.bones);
                gl.uniform_1_i32(self.palette_loc.as_ref(), 1);
                gl.uniform_1_i32(self.albedo_loc.as_ref(), 0);
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(gpu.albedo));
                gl.active_texture(glow::TEXTURE1);
                gl.bind_texture(glow::TEXTURE_2D, Some(gpu.palette));
                gl.bind_vertex_array(Some(gpu.vao));
                gl.draw_elements_instanced(
                    glow::TRIANGLES,
                    gpu.index_count,
                    glow::UNSIGNED_INT,
                    0,
                    group.palette_h as i32,
                );
                gl.bind_vertex_array(None);
            }

            idx += 1;
        }

        unsafe {
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.depth_mask(false);
        }
    }

    fn sync(&mut self, gl: &glow::Context, group: &crate::ui::skin::SkinGroup) {
        let vertices = group.vertices.as_ptr() as usize;
        let fresh = match self.meshes.get(&group.key) {
            Some(gpu) => gpu.vertices != vertices,
            None => true,
        };

        if fresh {
            if let Some(old) = self.meshes.remove(&group.key) {
                drop_skin(gl, old);
            }

            self.meshes.insert(group.key, upload_skin(gl, group));
        }

        let Some(gpu) = self.meshes.get_mut(&group.key) else {
            return;
        };

        unsafe {
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(gpu.instances));
            let bytes = std::slice::from_raw_parts(
                group.instances.as_ptr() as *const u8,
                group.instances.len() * 4,
            );
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            let width = group.palette_w as i32;
            let height = group.palette_h as i32;

            if gpu.palette_size != (width, height) {
                gl.bind_texture(glow::TEXTURE_2D, Some(gpu.palette));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA32F as i32,
                    width,
                    height,
                    0,
                    glow::RGBA,
                    glow::FLOAT,
                    Some(std::slice::from_raw_parts(
                        group.palette.as_ptr() as *const u8,
                        group.palette.len() * 4,
                    )),
                );
                gpu.palette_size = (width, height);
            } else {
                gl.bind_texture(glow::TEXTURE_2D, Some(gpu.palette));
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    width,
                    height,
                    glow::RGBA,
                    glow::FLOAT,
                    glow::PixelUnpackData::Slice(std::slice::from_raw_parts(
                        group.palette.as_ptr() as *const u8,
                        group.palette.len() * 4,
                    )),
                );
            }
        }
    }
}

fn upload_skin(gl: &glow::Context, group: &crate::ui::skin::SkinGroup) -> SkinGpu {
    unsafe {
        let vao = gl.create_vertex_array().unwrap();
        let vbo = gl.create_buffer().unwrap();
        let ibo = gl.create_buffer().unwrap();
        let instances = gl.create_buffer().unwrap();
        let albedo = gl.create_texture().unwrap();
        let palette = gl.create_texture().unwrap();
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let vertex_bytes = std::slice::from_raw_parts(
            group.vertices.as_ptr() as *const u8,
            group.vertices.len() * 4,
        );
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, vertex_bytes, glow::STATIC_DRAW);
        let stride = 64;
        attrib_f32(gl, 0, 3, stride, 0);
        attrib_f32(gl, 1, 3, stride, 12);
        attrib_f32(gl, 2, 2, stride, 24);
        attrib_f32(gl, 3, 4, stride, 32);
        attrib_f32(gl, 4, 4, stride, 48);
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(instances));
        let mut attr = 5;

        while attr < 9 {
            let offset = (attr - 5) * 16;
            attrib_f32(gl, attr, 4, 64, offset as i32);
            gl.vertex_attrib_divisor(attr, 1);
            attr += 1;
        }

        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
        let index_bytes = std::slice::from_raw_parts(
            group.indices.as_ptr() as *const u8,
            group.indices.len() * 4,
        );
        gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, index_bytes, glow::STATIC_DRAW);
        gl.bind_texture(glow::TEXTURE_2D, Some(albedo));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            group.albedo_w as i32,
            group.albedo_h as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(group.albedo.as_ref()),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
        gl.bind_texture(glow::TEXTURE_2D, Some(palette));
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.bind_vertex_array(None);

        SkinGpu {
            vao,
            vbo,
            ibo,
            instances,
            albedo,
            palette,
            index_count: group.indices.len() as i32,
            vertices: group.vertices.as_ptr() as usize,
            albedo_size: (group.albedo_w, group.albedo_h),
            palette_size: (0, 0),
        }
    }
}

fn attrib_f32(gl: &glow::Context, index: u32, size: i32, stride: i32, offset: i32) {
    unsafe {
        gl.vertex_attrib_pointer_f32(index, size, glow::FLOAT, false, stride, offset);
        gl.enable_vertex_attrib_array(index);
    }
}

fn drop_skin(gl: &glow::Context, gpu: SkinGpu) {
    unsafe {
        gl.delete_vertex_array(gpu.vao);
        gl.delete_buffer(gpu.vbo);
        gl.delete_buffer(gpu.ibo);
        gl.delete_buffer(gpu.instances);
        gl.delete_texture(gpu.albedo);
        gl.delete_texture(gpu.palette);
    }
}

fn link_skinned_program(gl: &glow::Context, cache: &shader::Registry) -> glow::Program {
    let source = crate::ui::shaders::Program::Skinned.wgsl();
    let vert = cache
        .glsl(
            &source,
            naga::ShaderStage::Vertex,
            "vs_main",
            shader::glsl_version(),
        )
        .expect("skin vert");
    let frag = cache
        .glsl(
            &source,
            naga::ShaderStage::Fragment,
            "fs_main",
            shader::glsl_version(),
        )
        .expect("skin frag");

    unsafe {
        let program = gl.create_program().unwrap();
        let vs = compile_mesh_shader(gl, glow::VERTEX_SHADER, &vert);
        let fs = compile_mesh_shader(gl, glow::FRAGMENT_SHADER, &frag);
        gl.attach_shader(program, vs);
        gl.attach_shader(program, fs);
        gl.link_program(program);

        if !gl.get_program_link_status(program) {
            log::warn!("[gl] skin link {}", gl.get_program_info_log(program));
        }

        gl.delete_shader(vs);
        gl.delete_shader(fs);

        program
    }
}

fn grid_cell(x: f32, y: f32, z: f32) -> (i32, i32, i32) {
    const CELL: f32 = 2048.0;

    (
        (x / CELL).floor() as i32,
        (y / CELL).floor() as i32,
        (z / CELL).floor() as i32,
    )
}

fn grid_mesh(src: &[f32]) -> (Vec<f32>, Vec<MeshChunk>) {
    let stride = crate::world::STRIDE;
    let tri = stride * 3;
    let mut buckets: HashMap<(i32, i32, i32), Vec<f32>> = HashMap::new();
    let mut idx = 0;

    while idx + tri <= src.len() {
        let ax = src[idx];
        let ay = src[idx + 1];
        let az = src[idx + 2];
        let bx = src[idx + stride];
        let by = src[idx + stride + 1];
        let bz = src[idx + stride + 2];
        let cx = src[idx + stride * 2];
        let cy = src[idx + stride * 2 + 1];
        let cz = src[idx + stride * 2 + 2];
        let key = grid_cell(
            (ax + bx + cx) / 3.0,
            (ay + by + cy) / 3.0,
            (az + bz + cz) / 3.0,
        );
        buckets
            .entry(key)
            .or_default()
            .extend_from_slice(&src[idx..idx + tri]);
        idx += tri;
    }

    let mut verts = Vec::with_capacity(src.len());
    let mut chunks = Vec::with_capacity(buckets.len());

    for bucket in buckets.into_values() {
        let first = (verts.len() / stride) as i32;
        let count = (bucket.len() / stride) as i32;
        let mut min = [f32::MAX; 3];
        let mut max = [-f32::MAX; 3];
        let mut vert = 0;

        while vert + 2 < bucket.len() {
            min[0] = min[0].min(bucket[vert]);
            min[1] = min[1].min(bucket[vert + 1]);
            min[2] = min[2].min(bucket[vert + 2]);
            max[0] = max[0].max(bucket[vert]);
            max[1] = max[1].max(bucket[vert + 1]);
            max[2] = max[2].max(bucket[vert + 2]);
            vert += stride;
        }

        verts.extend_from_slice(&bucket);
        chunks.push(MeshChunk {
            first,
            count,
            min,
            max,
        });
    }

    (verts, chunks)
}

fn frustum_planes(matrix: &[f32; 16]) -> [[f32; 4]; 6] {
    let rows = [0usize, 0, 1, 1, 2, 2];
    let signs = [1.0f32, -1.0, 1.0, -1.0, 1.0, -1.0];
    let mut planes = [[0.0; 4]; 6];
    let mut idx = 0;

    while idx < 6 {
        let row = rows[idx];
        let sign = signs[idx];
        let mut axis = 0;

        while axis < 4 {
            planes[idx][axis] = matrix[axis * 4 + 3] + sign * matrix[axis * 4 + row];
            axis += 1;
        }

        let len = (planes[idx][0] * planes[idx][0]
            + planes[idx][1] * planes[idx][1]
            + planes[idx][2] * planes[idx][2])
            .sqrt();

        if len > 1e-8 {
            planes[idx][0] /= len;
            planes[idx][1] /= len;
            planes[idx][2] /= len;
            planes[idx][3] /= len;
        }

        idx += 1;
    }

    planes
}

fn chunk_visible(min: [f32; 3], max: [f32; 3], planes: &[[f32; 4]; 6]) -> bool {
    let mut idx = 0;

    while idx < 6 {
        let plane = planes[idx];
        let x = if plane[0] >= 0.0 { max[0] } else { min[0] };
        let y = if plane[1] >= 0.0 { max[1] } else { min[1] };
        let z = if plane[2] >= 0.0 { max[2] } else { min[2] };

        if x * plane[0] + y * plane[1] + z * plane[2] + plane[3] < 0.0 {
            return false;
        }

        idx += 1;
    }

    true
}

fn any_visible(chunks: &[MeshChunk], planes: &[[f32; 4]; 6]) -> bool {
    let mut idx = 0;

    while idx < chunks.len() {
        if chunk_visible(chunks[idx].min, chunks[idx].max, planes) {
            return true;
        }

        idx += 1;
    }

    false
}

fn draw_visible(gl: &glow::Context, chunks: &[MeshChunk], planes: &[[f32; 4]; 6]) {
    let mut idx = 0;

    while idx < chunks.len() {
        let chunk = &chunks[idx];

        if chunk_visible(chunk.min, chunk.max, planes) {
            unsafe {
                gl.draw_arrays(glow::TRIANGLES, chunk.first, chunk.count);
            }
        }

        idx += 1;
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
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    width,
                    height,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    None,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(render));
                gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT24, width, height);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(buffer));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                gl.framebuffer_renderbuffer(
                    glow::FRAMEBUFFER,
                    glow::DEPTH_ATTACHMENT,
                    glow::RENDERBUFFER,
                    Some(render),
                );
                let complete =
                    gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
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

fn link_mesh_program(gl: &glow::Context, cache: &shader::Registry, frag: &str) -> glow::Program {
    let mesh_src = crate::ui::shaders::Program::Mesh.wgsl();
    let mesh_vert = cache
        .glsl(
            &mesh_src,
            naga::ShaderStage::Vertex,
            "vs_main",
            shader::glsl_version(),
        )
        .expect("mesh vert");
    let mesh_frag = cache
        .glsl(
            &mesh_src,
            naga::ShaderStage::Fragment,
            frag,
            shader::glsl_version(),
        )
        .expect("mesh frag");

    unsafe {
        let program = gl.create_program().unwrap();
        let vert = compile_mesh_shader(gl, glow::VERTEX_SHADER, &mesh_vert);
        let frag = compile_mesh_shader(gl, glow::FRAGMENT_SHADER, &mesh_frag);
        gl.attach_shader(program, vert);
        gl.attach_shader(program, frag);
        gl.link_program(program);

        if !gl.get_program_link_status(program) {
            log::warn!("[gl] voxel link {}", gl.get_program_info_log(program));
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
            log::warn!("[gl] voxel shader {}", gl.get_shader_info_log(shader));
        }

        shader
    }
}

fn sprite_program(gl: &glow::Context, cache: &shader::Registry) -> Result<glow::Program, String> {
    let source = crate::ui::shaders::Program::Text.wgsl();
    let shader = compile_wgsl(gl, cache, &source)?;
    let program = link_wgsl(gl, &shader)?;
    unsafe {
        gl.delete_shader(shader.vs);
        gl.delete_shader(shader.fs);
    }

    Ok(program)
}

fn gl_sampler(gl: &glow::Context, linear: bool, repeat: bool) -> Result<glow::Sampler, String> {
    let sampler = unsafe { gl.create_sampler().map_err(|err| err.to_string())? };
    let filter = if linear { glow::LINEAR } else { glow::NEAREST } as i32;
    let wrap = if repeat {
        glow::REPEAT
    } else {
        glow::CLAMP_TO_EDGE
    } as i32;
    unsafe {
        gl.sampler_parameter_i32(sampler, glow::TEXTURE_MIN_FILTER, filter);
        gl.sampler_parameter_i32(sampler, glow::TEXTURE_MAG_FILTER, filter);
        gl.sampler_parameter_i32(sampler, glow::TEXTURE_WRAP_S, wrap);
        gl.sampler_parameter_i32(sampler, glow::TEXTURE_WRAP_T, wrap);
    }

    Ok(sampler)
}

fn gl_cache(gl: &glow::Context) -> shader::Registry {
    let vendor = unsafe { gl.get_parameter_string(glow::VENDOR) };
    let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };

    shader::Registry::for_device(&shader::id_from_text(&[&vendor, &renderer]))
}

fn compile_wgsl(
    gl: &glow::Context,
    cache: &shader::Registry,
    source: &str,
) -> Result<crate::ui::gfx::GlShader, String> {
    let version = shader::glsl_version();
    let vs_src = cache.glsl(source, naga::ShaderStage::Vertex, "vs_main", version)?;
    let fs_src = cache.glsl(source, naga::ShaderStage::Fragment, "fs_main", version)?;
    let vs = compile_stage(gl, glow::VERTEX_SHADER, &vs_src)?;
    let fs = compile_stage(gl, glow::FRAGMENT_SHADER, &fs_src)?;

    Ok(crate::ui::gfx::GlShader { vs, fs })
}

fn compile_stage(gl: &glow::Context, kind: u32, source: &str) -> Result<glow::Shader, String> {
    unsafe {
        let shader = gl.create_shader(kind).map_err(|err| err.to_string())?;
        gl.shader_source(shader, source);
        gl.compile_shader(shader);

        if !gl.get_shader_compile_status(shader) {
            let log = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);

            return Err(log);
        }

        Ok(shader)
    }
}

fn link_wgsl(
    gl: &glow::Context,
    shader: &crate::ui::gfx::GlShader,
) -> Result<glow::Program, String> {
    unsafe {
        let program = gl.create_program().map_err(|err| err.to_string())?;
        gl.attach_shader(program, shader.vs);
        gl.attach_shader(program, shader.fs);
        gl.link_program(program);

        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);

            return Err(log);
        }

        Ok(program)
    }
}

fn rgba_texture(
    gl: &glow::Context,
    width: i32,
    height: i32,
    pixels: &[u8],
) -> Result<glow::Texture, String> {
    unsafe {
        let texture = gl.create_texture().map_err(|err| err.to_string())?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            width,
            height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(pixels),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.bind_texture(glow::TEXTURE_2D, None);

        Ok(texture)
    }
}

fn mesh_buffer(
    gl: &glow::Context,
    verts: &[f32],
    screen: bool,
) -> Result<crate::ui::gfx::GlMesh, String> {
    let stride_floats = if screen {
        crate::ui::gfx::SCREEN_FLOATS
    } else {
        crate::ui::gfx::MESH_FLOATS
    };
    unsafe {
        let vao = gl.create_vertex_array().map_err(|err| err.to_string())?;
        let vbo = gl.create_buffer().map_err(|err| err.to_string())?;
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let bytes = std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 4);
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
        let stride = (stride_floats * 4) as i32;

        if screen {
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 8);
            gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, 16);
            gl.enable_vertex_attrib_array(2);
        } else {
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 12);
            gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, 20);
            gl.enable_vertex_attrib_array(2);
        }

        gl.enable_vertex_attrib_array(0);
        gl.enable_vertex_attrib_array(1);
        gl.bind_vertex_array(None);

        Ok(crate::ui::gfx::GlMesh {
            vao,
            vbo,
            floats: verts.len() as i32,
            screen,
        })
    }
}

fn bind_user(
    gl: &glow::Context,
    program: glow::Program,
    screen: bool,
    width: f32,
    height: f32,
    view: Option<&crate::ui::voxel::SceneView>,
) {
    unsafe {
        gl.use_program(Some(program));

        if screen {
            if let Some(loc) = gl.get_uniform_location(program, "_immediates_binding_vs.resolution")
            {
                gl.uniform_4_f32(Some(&loc), width, height, 0.0, 0.0);
            }

            return;
        }

        let Some(view) = view else {
            return;
        };
        let matrix = gl_view_proj(view);

        if let Some(loc) = gl.get_uniform_location(program, "_immediates_binding_vs.view_proj") {
            gl.uniform_matrix_4_f32_slice(Some(&loc), false, &matrix);
        }
    }
}

impl OpenGLWindow {
    fn apply_gl_scissor(&self) {
        let (width, height) = self.pixel_size();
        let height = height as i32;
        unsafe {
            match self.scissor {
                Some(scissor) => {
                    let scissor = scissor.clamp(width as i32, height);
                    self.gl.enable(glow::SCISSOR_TEST);
                    self.gl.scissor(
                        scissor.x,
                        scissor.gl_y(height),
                        scissor.w.max(0),
                        scissor.h.max(0),
                    );
                }
                None => self.gl.disable(glow::SCISSOR_TEST),
            }
        }
    }

    fn pixel_size(&self) -> (f32, f32) {
        if self.bound {
            return (self.bound_w as f32, self.bound_h as f32);
        }

        (self.width as f32, self.height as f32)
    }

    fn unbind_target(&mut self) {
        if !self.bound {
            return;
        }

        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl
                .viewport(0, 0, self.width as i32, self.height as i32);
        }
        self.bound = false;
    }

    fn draw_screen_verts(
        &mut self,
        verts: &[f32],
        program: glow::Program,
        texture: Option<glow::Texture>,
        sampler: Option<glow::Sampler>,
    ) {
        if verts.len() < crate::ui::gfx::SCREEN_FLOATS {
            return;
        }

        self.apply_gl_scissor();
        let (width, height) = self.pixel_size();
        unsafe {
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.depth_mask(false);
            self.gl.disable(glow::CULL_FACE);
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            bind_user(&self.gl, program, true, width, height, None);
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(glow::TEXTURE_2D, texture);
            self.gl.bind_sampler(0, sampler);
            self.gl.bind_vertex_array(Some(self.vao));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            let bytes = std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 4);
            self.gl
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            self.gl
                .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 32, 0);
            self.gl
                .vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 32, 8);
            self.gl
                .vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, 32, 16);
            self.gl.enable_vertex_attrib_array(0);
            self.gl.enable_vertex_attrib_array(1);
            self.gl.enable_vertex_attrib_array(2);
            self.gl.draw_arrays(
                glow::TRIANGLES,
                0,
                (verts.len() / crate::ui::gfx::SCREEN_FLOATS) as i32,
            );
            self.gl.disable_vertex_attrib_array(1);
            self.gl.disable_vertex_attrib_array(2);
            self.gl
                .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
        }
    }

    fn text_texture(&mut self) -> Option<glow::Texture> {
        let (pixels, width, height, dirty) = {
            let frame = self.text_once.as_ref()?;
            let (pixels, width, height, dirty) = frame.captured_atlas();

            (pixels.to_vec(), width, height, dirty)
        };

        if !dirty && self.text_atlas.is_some() {
            return self.text_atlas;
        }

        let mut rgba = Vec::with_capacity(pixels.len() * 4);
        let mut idx = 0;

        while idx < pixels.len() {
            let coverage = pixels[idx];
            rgba.extend_from_slice(&[coverage, coverage, coverage, coverage]);
            idx += 1;
        }

        let name = rgba_texture(&self.gl, width as i32, height as i32, &rgba).ok()?;
        if let Some(old) = self.text_atlas.replace(name) {
            unsafe { self.gl.delete_texture(old) }
        }
        self.text_once.as_mut()?.clear_captured_dirty();

        self.text_atlas
    }
}

impl crate::ui::gfx::BackendGpu for OpenGLWindow {
    fn make_shader(&mut self, wgsl: &str) -> Result<crate::ui::gfx::Shader, String> {
        let cache = gl_cache(&self.gl);

        Ok(crate::ui::gfx::Shader::opengl(compile_wgsl(
            &self.gl, &cache, wgsl,
        )?))
    }

    fn make_texture(
        &mut self,
        image: &crate::world::surface::CpuImage,
    ) -> Result<crate::ui::gfx::Texture, String> {
        let pixels = crate::world::image_rgba(image);
        let name = rgba_texture(&self.gl, image.width as i32, image.height as i32, &pixels)?;

        Ok(crate::ui::gfx::Texture::opengl(crate::ui::gfx::GlTexture {
            name,
        }))
    }

    fn make_target(&mut self, width: u32, height: u32) -> Result<crate::ui::gfx::Target, String> {
        let width = width.max(1) as i32;
        let height = height.max(1) as i32;
        let empty = vec![0u8; (width as usize) * (height as usize) * 4];
        let color = rgba_texture(&self.gl, width, height, &empty)?;
        unsafe {
            let depth = self
                .gl
                .create_renderbuffer()
                .map_err(|err| err.to_string())?;
            let frame = self
                .gl
                .create_framebuffer()
                .map_err(|err| err.to_string())?;
            self.gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
            self.gl.renderbuffer_storage(
                glow::RENDERBUFFER,
                glow::DEPTH_COMPONENT24,
                width,
                height,
            );
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(frame));
            self.gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(color),
                0,
            );
            self.gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(depth),
            );
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);

            Ok(crate::ui::gfx::Target::opengl(crate::ui::gfx::GlTarget {
                frame,
                color,
                depth,
                width,
                height,
            }))
        }
    }

    fn make_buffer(&mut self, bytes: &[u8]) -> Result<crate::ui::gfx::Buffer, String> {
        unsafe {
            let name = self.gl.create_buffer().map_err(|err| err.to_string())?;
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(name));
            self.gl
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);

            Ok(crate::ui::gfx::Buffer::opengl(crate::ui::gfx::GlBuffer {
                name,
                bytes: bytes.len() as u32,
            }))
        }
    }

    fn make_sampler(
        &mut self,
        linear: bool,
        repeat: bool,
    ) -> Result<crate::ui::gfx::Sampler, String> {
        Ok(crate::ui::gfx::Sampler::opengl(crate::ui::gfx::GlSampler {
            name: gl_sampler(&self.gl, linear, repeat)?,
        }))
    }

    fn make_pipeline(
        &mut self,
        shader: &crate::ui::gfx::Shader,
        screen: bool,
    ) -> Result<crate::ui::gfx::Pipeline, String> {
        let shader = shader.as_opengl().ok_or_else(|| "shader".to_string())?;
        let program = link_wgsl(&self.gl, shader)?;
        let stride = if screen {
            crate::ui::gfx::SCREEN_FLOATS as u8
        } else {
            crate::ui::gfx::MESH_FLOATS as u8
        };

        Ok(crate::ui::gfx::Pipeline::opengl(
            crate::ui::gfx::GlPipeline {
                program,
                stride,
                depth: !screen,
            },
        ))
    }

    fn make_mesh(&mut self, verts: &[f32], screen: bool) -> Result<crate::ui::gfx::Mesh, String> {
        Ok(crate::ui::gfx::Mesh::opengl(mesh_buffer(
            &self.gl, verts, screen,
        )?))
    }

    fn destroy_shader(&mut self, shader: crate::ui::gfx::Shader) {
        let Some(shader) = shader.into_opengl() else {
            return;
        };
        unsafe {
            self.gl.delete_shader(shader.vs);
            self.gl.delete_shader(shader.fs);
        }
    }

    fn destroy_texture(&mut self, texture: crate::ui::gfx::Texture) {
        let Some(texture) = texture.into_opengl() else {
            return;
        };
        unsafe { self.gl.delete_texture(texture.name) }
    }

    fn destroy_buffer(&mut self, buffer: crate::ui::gfx::Buffer) {
        let Some(buffer) = buffer.into_opengl() else {
            return;
        };
        unsafe { self.gl.delete_buffer(buffer.name) }
    }

    fn destroy_sampler(&mut self, sampler: crate::ui::gfx::Sampler) {
        let Some(sampler) = sampler.into_opengl() else {
            return;
        };
        unsafe { self.gl.delete_sampler(sampler.name) }
    }

    fn destroy_pipeline(&mut self, pipeline: crate::ui::gfx::Pipeline) {
        let Some(pipeline) = pipeline.into_opengl() else {
            return;
        };
        unsafe { self.gl.delete_program(pipeline.program) }
    }

    fn destroy_target(&mut self, target: crate::ui::gfx::Target) {
        let Some(target) = target.into_opengl() else {
            return;
        };
        unsafe {
            self.gl.delete_framebuffer(target.frame);
            self.gl.delete_texture(target.color);
            self.gl.delete_renderbuffer(target.depth);
        }
    }

    fn destroy_mesh(&mut self, mesh: crate::ui::gfx::Mesh) {
        let Some(mesh) = mesh.into_opengl() else {
            return;
        };
        unsafe {
            self.gl.delete_vertex_array(mesh.vao);
            self.gl.delete_buffer(mesh.vbo);
        }
    }

    fn draw_mesh(
        &mut self,
        mesh: &crate::ui::gfx::Mesh,
        pipeline: &crate::ui::gfx::Pipeline,
        texture: Option<&crate::ui::gfx::Texture>,
        sampler: Option<&crate::ui::gfx::Sampler>,
        view: &crate::ui::voxel::SceneView,
    ) {
        let (Some(mesh), Some(pipeline)) = (mesh.as_opengl(), pipeline.as_opengl()) else {
            return;
        };
        self.apply_gl_scissor();
        let stride = pipeline.stride.max(1) as i32;
        unsafe {
            self.gl.disable(glow::CULL_FACE);
            if pipeline.depth {
                self.gl.enable(glow::DEPTH_TEST);
                self.gl.depth_mask(true);
            } else {
                self.gl.disable(glow::DEPTH_TEST);
                self.gl.depth_mask(false);
            }
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            let (width, height) = self.pixel_size();
            bind_user(
                &self.gl,
                pipeline.program,
                mesh.screen,
                width,
                height,
                Some(view),
            );
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(
                glow::TEXTURE_2D,
                texture
                    .and_then(|item| item.as_opengl())
                    .map(|item| item.name),
            );
            self.gl.bind_sampler(
                0,
                sampler
                    .and_then(|item| item.as_opengl())
                    .map(|item| item.name),
            );
            self.gl.bind_vertex_array(Some(mesh.vao));
            self.gl
                .draw_arrays(glow::TRIANGLES, 0, mesh.floats / stride);
            self.gl.bind_vertex_array(None);
        }
    }

    fn draw_sprite(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        texture: Option<&crate::ui::gfx::Texture>,
        pipeline: Option<&crate::ui::gfx::Pipeline>,
        sampler: Option<&crate::ui::gfx::Sampler>,
    ) {
        let program = pipeline
            .and_then(|item| item.as_opengl())
            .map(|item| item.program)
            .unwrap_or(self.sprite);
        let verts = crate::ui::gfx::screen_quad(x, y, w, h, color);
        self.apply_gl_scissor();
        unsafe {
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.depth_mask(false);
            self.gl.disable(glow::CULL_FACE);
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            let (width, height) = self.pixel_size();
            bind_user(&self.gl, program, true, width, height, None);
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(
                glow::TEXTURE_2D,
                texture
                    .and_then(|item| item.as_opengl())
                    .map(|item| item.name)
                    .or(Some(self.colored_mesh.white)),
            );
            self.gl.bind_sampler(
                0,
                sampler
                    .and_then(|item| item.as_opengl())
                    .map(|item| item.name)
                    .or(Some(self.clamp_sampler)),
            );
            self.gl.bind_vertex_array(Some(self.vao));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            let bytes = std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 4);
            self.gl
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            self.gl
                .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 32, 0);
            self.gl
                .vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 32, 8);
            self.gl
                .vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, 32, 16);
            self.gl.enable_vertex_attrib_array(0);
            self.gl.enable_vertex_attrib_array(1);
            self.gl.enable_vertex_attrib_array(2);
            self.gl.draw_arrays(glow::TRIANGLES, 0, 6);
            self.gl.disable_vertex_attrib_array(1);
            self.gl.disable_vertex_attrib_array(2);
            self.gl
                .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
        }
    }

    fn draw_screen(
        &mut self,
        verts: &[f32],
        texture: Option<&crate::ui::gfx::Texture>,
        sampler: Option<&crate::ui::gfx::Sampler>,
    ) {
        let texture = texture
            .and_then(|item| item.as_opengl())
            .map(|item| item.name)
            .or(Some(self.colored_mesh.white));
        let sampler = sampler
            .and_then(|item| item.as_opengl())
            .map(|item| item.name)
            .or(Some(self.clamp_sampler));
        self.draw_screen_verts(verts, self.sprite, texture, sampler);
    }

    fn builtin_shader(&mut self, index: u32) -> Option<crate::ui::gfx::Shader> {
        let source = match index {
            crate::ui::gfx::IDX_MESH => crate::ui::shaders::Program::Mesh.wgsl(),
            crate::ui::gfx::IDX_COLOR => crate::ui::shaders::Program::Color.wgsl(),
            crate::ui::gfx::IDX_TEXT => crate::ui::shaders::Program::Text.wgsl(),
            crate::ui::gfx::IDX_SKINNED => crate::ui::shaders::Program::Skinned.wgsl(),
            _ => return None,
        };
        let cache = gl_cache(&self.gl);

        compile_wgsl(&self.gl, &cache, &source)
            .ok()
            .map(crate::ui::gfx::Shader::opengl)
    }

    fn builtin_pipeline(&mut self, index: u32) -> Option<crate::ui::gfx::Pipeline> {
        let (program, stride, depth) = match index {
            crate::ui::gfx::IDX_MESH => (self.colored_mesh.program, 0, true),
            crate::ui::gfx::IDX_COLOR => (self.shader_program, 6, false),
            crate::ui::gfx::IDX_TEXT => (self.sprite, crate::ui::gfx::SCREEN_FLOATS as u8, false),
            crate::ui::gfx::IDX_SKINNED => (self.skin.program, 0, true),
            _ => return None,
        };

        Some(crate::ui::gfx::Pipeline::opengl(
            crate::ui::gfx::GlPipeline {
                program,
                stride,
                depth,
            },
        ))
    }

    fn builtin_texture(&mut self, index: u32) -> Option<crate::ui::gfx::Texture> {
        let name = match index {
            crate::ui::gfx::IDX_WHITE => self.colored_mesh.white,
            crate::ui::gfx::IDX_FLAT => self.colored_mesh.flat,
            _ => return None,
        };

        Some(crate::ui::gfx::Texture::opengl(crate::ui::gfx::GlTexture {
            name,
        }))
    }

    fn builtin_sampler(&mut self, index: u32) -> Option<crate::ui::gfx::Sampler> {
        let name = match index {
            crate::ui::gfx::IDX_WRAP => self.wrap_sampler,
            crate::ui::gfx::IDX_CLAMP => self.clamp_sampler,
            _ => return None,
        };

        Some(crate::ui::gfx::Sampler::opengl(crate::ui::gfx::GlSampler {
            name,
        }))
    }

    fn material_alias(&self, name: &str) -> Option<crate::ui::gfx::Texture> {
        self.colored_mesh
            .material_lookup
            .get(name)
            .copied()
            .map(|name| crate::ui::gfx::Texture::opengl(crate::ui::gfx::GlTexture { name }))
    }

    fn target_color(&self, target: &crate::ui::gfx::Target) -> Option<crate::ui::gfx::Texture> {
        target.as_opengl().map(|target| {
            crate::ui::gfx::Texture::opengl(crate::ui::gfx::GlTexture { name: target.color })
        })
    }

    fn draw_buffer(
        &mut self,
        buffer: &crate::ui::gfx::Buffer,
        pipeline: &crate::ui::gfx::Pipeline,
        texture: Option<&crate::ui::gfx::Texture>,
        sampler: Option<&crate::ui::gfx::Sampler>,
        view: &crate::ui::voxel::SceneView,
    ) {
        let (Some(buffer), Some(pipeline)) = (buffer.as_opengl(), pipeline.as_opengl()) else {
            return;
        };
        self.apply_gl_scissor();
        let stride = pipeline.stride.max(1) as i32;
        let screen = stride == crate::ui::gfx::SCREEN_FLOATS as i32;
        let (width, height) = self.pixel_size();
        unsafe {
            self.gl.disable(glow::CULL_FACE);
            if pipeline.depth {
                self.gl.enable(glow::DEPTH_TEST);
                self.gl.depth_mask(true);
            } else {
                self.gl.disable(glow::DEPTH_TEST);
                self.gl.depth_mask(false);
            }
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            bind_user(
                &self.gl,
                pipeline.program,
                screen,
                width,
                height,
                Some(view),
            );
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(
                glow::TEXTURE_2D,
                texture
                    .and_then(|item| item.as_opengl())
                    .map(|item| item.name),
            );
            self.gl.bind_sampler(
                0,
                sampler
                    .and_then(|item| item.as_opengl())
                    .map(|item| item.name),
            );
            self.gl.bind_vertex_array(Some(self.vao));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer.name));
            let byte_stride = stride * 4;

            if screen {
                self.gl
                    .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, byte_stride, 0);
                self.gl
                    .vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, byte_stride, 8);
                self.gl
                    .vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, byte_stride, 16);
            } else {
                self.gl
                    .vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, byte_stride, 0);
                self.gl
                    .vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, byte_stride, 12);
                self.gl
                    .vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, byte_stride, 20);
            }

            self.gl.enable_vertex_attrib_array(0);
            self.gl.enable_vertex_attrib_array(1);
            self.gl.enable_vertex_attrib_array(2);
            self.gl
                .draw_arrays(glow::TRIANGLES, 0, buffer.bytes as i32 / byte_stride.max(1));
            self.gl.disable_vertex_attrib_array(1);
            self.gl.disable_vertex_attrib_array(2);
            self.gl
                .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            self.gl.bind_vertex_array(None);
        }
    }

    fn draw_text_user(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        texture: Option<&crate::ui::gfx::Texture>,
        pipeline: Option<&crate::ui::gfx::Pipeline>,
        sampler: Option<&crate::ui::gfx::Sampler>,
    ) {
        if self.text_once.is_none() {
            self.text_once = crate::ui::batch::TextFrame::new().ok();
        }

        let Some(frame) = self.text_once.as_mut() else {
            return;
        };
        let verts = frame.capture(text, x, y, scale, color);
        let program = pipeline
            .and_then(|item| item.as_opengl())
            .map(|item| item.program)
            .unwrap_or(self.sprite);
        let atlas = if texture.is_some() {
            None
        } else {
            self.text_texture()
        };
        let texture = texture
            .and_then(|item| item.as_opengl())
            .map(|item| item.name)
            .or(atlas);
        let sampler = sampler
            .and_then(|item| item.as_opengl())
            .map(|item| item.name)
            .or(Some(self.clamp_sampler));
        self.draw_screen_verts(&verts, program, texture, sampler);
    }

    fn set_target(&mut self, target: Option<&crate::ui::gfx::Target>) {
        match target.and_then(|item| item.as_opengl()) {
            Some(target) => {
                unsafe {
                    self.gl
                        .bind_framebuffer(glow::FRAMEBUFFER, Some(target.frame));
                    self.gl.viewport(0, 0, target.width, target.height);
                }
                self.bound = true;
                self.bound_w = target.width;
                self.bound_h = target.height;
            }
            None => self.unbind_target(),
        }
    }

    fn target_bound(&self) -> bool {
        self.bound
    }

    fn update_buffer(
        &mut self,
        buffer: crate::ui::gfx::Buffer,
        bytes: &[u8],
    ) -> crate::ui::gfx::Buffer {
        let Some(buffer) = buffer.into_opengl() else {
            return self.make_buffer(bytes).unwrap_or_else(|_| {
                crate::ui::gfx::Buffer::opengl(crate::ui::gfx::GlBuffer {
                    name: unsafe { self.gl.create_buffer().unwrap() },
                    bytes: 0,
                })
            });
        };
        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer.name));
            self.gl
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }

        crate::ui::gfx::Buffer::opengl(crate::ui::gfx::GlBuffer {
            name: buffer.name,
            bytes: bytes.len() as u32,
        })
    }

    fn update_mesh(&mut self, mesh: crate::ui::gfx::Mesh, verts: &[f32]) -> crate::ui::gfx::Mesh {
        let Some(mesh) = mesh.into_opengl() else {
            return self
                .make_mesh(verts, false)
                .ok()
                .and_then(|item| item.into_opengl())
                .map(crate::ui::gfx::Mesh::opengl)
                .unwrap_or_else(|| {
                    crate::ui::gfx::Mesh::opengl(crate::ui::gfx::GlMesh {
                        vao: unsafe { self.gl.create_vertex_array().unwrap() },
                        vbo: unsafe { self.gl.create_buffer().unwrap() },
                        floats: 0,
                        screen: false,
                    })
                });
        };
        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(mesh.vbo));
            let bytes = std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * 4);
            self.gl
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }

        crate::ui::gfx::Mesh::opengl(crate::ui::gfx::GlMesh {
            floats: verts.len() as i32,
            ..mesh
        })
    }

    fn update_texture(
        &mut self,
        texture: crate::ui::gfx::Texture,
        image: &crate::world::surface::CpuImage,
    ) -> crate::ui::gfx::Texture {
        let Some(texture) = texture.into_opengl() else {
            return self.make_texture(image).ok().unwrap_or_else(|| {
                crate::ui::gfx::Texture::opengl(crate::ui::gfx::GlTexture {
                    name: self.colored_mesh.white,
                })
            });
        };
        let pixels = crate::world::image_rgba(image);
        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture.name));
            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                image.width as i32,
                image.height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(&pixels),
            );
            self.gl.bind_texture(glow::TEXTURE_2D, None);
        }

        crate::ui::gfx::Texture::opengl(texture)
    }

    fn resize_target(
        &mut self,
        target: crate::ui::gfx::Target,
        width: u32,
        height: u32,
    ) -> Result<crate::ui::gfx::Target, String> {
        self.destroy_target(target);

        self.make_target(width, height)
    }

    fn before_destroy(&mut self) {
        let _ = self.context.make_current(&self.surface);
        unsafe {
            self.gl.delete_program(self.sprite);
            self.gl.delete_sampler(self.wrap_sampler);
            self.gl.delete_sampler(self.clamp_sampler);

            if let Some(texture) = self.text_atlas {
                self.gl.delete_texture(texture);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::voxel::FlyCamera;
    use crate::world::{Block, BlockPos, VoxelWorld};

    #[test]
    fn frustum_drops_a_box_behind_the_camera() {
        let view = FlyCamera::new().scene(16.0 / 9.0, 1.0);
        let planes = frustum_planes(&gl_view_proj(&view));

        assert!(chunk_visible([0.0, 40.0, 8.0], [32.0, 80.0, 24.0], &planes));
        assert!(!chunk_visible(
            [0.0, -4000.0, 8.0],
            [32.0, -2000.0, 24.0],
            &planes
        ));
    }

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

        while idx + crate::world::STRIDE <= mesh.len() {
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
                let on_screen = ax.abs() < 1.5
                    && ay.abs() < 1.5
                    && bx.abs() < 1.5
                    && by.abs() < 1.5
                    && cx.abs() < 1.5
                    && cy.abs() < 1.5;
                let in_depth =
                    az > -1.0 && az < 1.0 && bz > -1.0 && bz < 1.0 && cz > -1.0 && cz < 1.0;

                if on_screen && in_depth {
                    visible += 1;
                    let winding = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);

                    if winding > 0.0 {
                        front += 1;
                    }
                }
            }

            idx += crate::world::STRIDE;
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
