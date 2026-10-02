use crate::platform::Surface;
use crate::ui::shader;
use crate::ui::skin::SkinBatch;
use crate::ui::voxel::SceneView;
use crate::ui::vr::{self, EyeViews, Headset, VrInput};
use crate::ui::window::Window;
use crate::ui::Color;
use core_graphics_types::geometry::CGSize;
use foreign_types::ForeignType;
use glyph_brush::{
    ab_glyph::FontArc, BrushAction, BrushError, Extra, GlyphBrush, GlyphBrushBuilder, Section, Text,
};
use metal::*;
#[cfg(target_os = "macos")]
use objc::runtime::NO;
use objc::runtime::{Object, YES};
use objc::{msg_send, sel, sel_impl};
use raw_window_handle::RawWindowHandle;
#[cfg(target_os = "ios")]
use std::ffi::c_void;

#[derive(Clone, Copy)]
struct GlyphQuad {
    verts: [[f32; 8]; 6],
}

pub struct MetalWindow {
    width: u32,
    height: u32,
    scale_factor: f64,
    device: Device,
    queue: CommandQueue,
    layer: MetalLayer,
    mesh_pipeline: RenderPipelineState,
    mesh_blend: RenderPipelineState,
    color_pipeline: RenderPipelineState,
    text_pipeline: RenderPipelineState,
    skin_pipeline: RenderPipelineState,
    skin: MetalSkin,
    depth_write: DepthStencilState,
    depth_off: DepthStencilState,
    depth: Option<Texture>,
    depth_size: (u64, u64),
    mesh: Option<Buffer>,
    ranges: Vec<crate::world::SurfaceRange>,
    cpu: Vec<f32>,
    eye: [f32; 3],
    time: f32,
    far: f32,
    materials: Vec<MetalMaterial>,
    lightmaps: Vec<Texture>,
    cubemaps: Vec<Texture>,
    sky: Option<Texture>,
    wrap_sampler: SamplerState,
    clamp_sampler: SamplerState,
    white: Texture,
    flat: Texture,
    white_cube: Texture,
    scene: Texture,
    graphics_key: u64,
    mesh_vertices: u64,
    mesh_revision: u64,
    mesh_ready: bool,
    view_proj: [f32; 16],
    draw_mesh: bool,
    ui_verts: Vec<f32>,
    text_verts: Vec<f32>,
    glyphs: GlyphBrush<GlyphQuad>,
    atlas: Texture,
    atlas_size: (u32, u32),
    clear: [f64; 4],
    vr: Option<Headset>,
    vr_failed: bool,
    vr_enable: bool,
    eyes: Option<MetalEyes>,
    eye_views: Option<EyeViews>,
}

struct MetalEyes {
    width: u64,
    height: u64,
    color: [Texture; 2],
    depth: [Texture; 2],
}

impl MetalWindow {
    #[allow(unexpected_cfgs)]
    pub fn try_new(surface: &Surface) -> Result<Self, String> {
        let device = Device::system_default().ok_or("no metal device")?;
        let queue = device.new_command_queue();
        let cache = shader::Registry::for_device(&shader::id_from_u64(device.registry_id()));
        let mesh_src = crate::ui::shaders::Program::Mesh.wgsl();
        let color_src = crate::ui::shaders::Program::Color.wgsl();
        let text_src = crate::ui::shaders::Program::Text.wgsl();
        let mesh_lib = device
            .new_library_with_source(&cache.msl(&mesh_src)?, &CompileOptions::new())
            .map_err(|err| format!("shader: {err}"))?;
        let color_lib = device
            .new_library_with_source(&cache.msl(&color_src)?, &CompileOptions::new())
            .map_err(|err| format!("shader: {err}"))?;
        let text_lib = device
            .new_library_with_source(&cache.msl(&text_src)?, &CompileOptions::new())
            .map_err(|err| format!("shader: {err}"))?;
        let mesh_vert = mesh_lib
            .get_function("vs_main", None)
            .map_err(|err| format!("vs_main: {err}"))?;
        let mesh_frag = mesh_lib
            .get_function("fs_main", None)
            .map_err(|err| format!("fs_main: {err}"))?;
        let color_vert = color_lib
            .get_function("vs_main", None)
            .map_err(|err| format!("vs_main: {err}"))?;
        let color_frag = color_lib
            .get_function("fs_main", None)
            .map_err(|err| format!("fs_main: {err}"))?;
        let text_vert = text_lib
            .get_function("vs_main", None)
            .map_err(|err| format!("vs_main: {err}"))?;
        let text_frag = text_lib
            .get_function("fs_main", None)
            .map_err(|err| format!("fs_main: {err}"))?;

        let mesh_pipeline = pipeline(&device, &mesh_vert, &mesh_frag, &mesh_vertex_desc(), false)?;
        let mesh_blend = pipeline(&device, &mesh_vert, &mesh_frag, &mesh_vertex_desc(), true)?;
        let color_pipeline = pipeline(
            &device,
            &color_vert,
            &color_frag,
            &color_vertex_desc(),
            true,
        )?;
        let text_pipeline = pipeline(&device, &text_vert, &text_frag, &text_vertex_desc(), true)?;
        let skin_src = crate::ui::shaders::Program::Skinned.wgsl();
        let skin_lib = device
            .new_library_with_source(&cache.msl(&skin_src)?, &CompileOptions::new())
            .map_err(|err| format!("shader: {err}"))?;
        let skin_vert = skin_lib
            .get_function("vs_main", None)
            .map_err(|err| format!("vs_main: {err}"))?;
        let skin_frag = skin_lib
            .get_function("fs_main", None)
            .map_err(|err| format!("fs_main: {err}"))?;
        let skin_pipeline = pipeline(&device, &skin_vert, &skin_frag, &skin_vertex_desc(), false)?;

        let depth_write = depth_state(&device, MTLCompareFunction::Less, true);
        let depth_off = depth_state(&device, MTLCompareFunction::Always, false);

        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        layer.set_framebuffer_only(false);
        layer.set_presents_with_transaction(false);
        layer.set_maximum_drawable_count(3);
        #[cfg(target_os = "macos")]
        {
            layer.set_display_sync_enabled(false);
            unsafe {
                let _: () = msg_send![layer.as_ptr(), setAllowsNextDrawableTimeout: NO];
            }
        }
        attach_layer(surface, &layer)?;
        sync_layer(surface.width, surface.height, surface.scale_factor, &layer);

        let font = FontArc::try_from_slice(include_bytes!("font_default.ttf"))
            .map_err(|err| err.to_string())?;
        let glyphs = GlyphBrushBuilder::using_font(font)
            .initial_cache_size((512, 512))
            .build();
        let atlas = atlas_texture(&device, 512, 512);
        let wrap_sampler = metal_sampler(&device, MTLSamplerAddressMode::Repeat);
        let clamp_sampler = metal_sampler(&device, MTLSamplerAddressMode::ClampToEdge);
        let white = metal_image(&device, &crate::world::surface::CpuImage::white(), true);
        let flat = metal_image(
            &device,
            &crate::world::surface::CpuImage::flat_normal(),
            true,
        );
        let white_cube = metal_cube(
            &device,
            &crate::world::surface::CubeImage::solid(crate::world::surface::CpuImage::white()),
        );
        let scene = metal_image(&device, &crate::world::surface::CpuImage::white(), false);

        Ok(Self {
            width: surface.width,
            height: surface.height,
            scale_factor: surface.scale_factor,
            device,
            queue,
            layer,
            mesh_pipeline,
            mesh_blend,
            color_pipeline,
            text_pipeline,
            skin_pipeline,
            skin: MetalSkin::default(),
            depth_write,
            depth_off,
            depth: None,
            depth_size: (0, 0),
            mesh: None,
            ranges: Vec::new(),
            cpu: Vec::new(),
            eye: [0.0, 0.0, 0.0],
            time: 0.0,
            far: 1000.0,
            materials: Vec::new(),
            lightmaps: Vec::new(),
            cubemaps: Vec::new(),
            sky: None,
            wrap_sampler,
            clamp_sampler,
            white,
            flat,
            white_cube,
            scene,
            graphics_key: u64::MAX,
            mesh_vertices: 0,
            mesh_revision: 0,
            mesh_ready: false,
            view_proj: [0.0; 16],
            draw_mesh: false,
            ui_verts: Vec::new(),
            text_verts: Vec::new(),
            glyphs,
            atlas,
            atlas_size: (512, 512),
            clear: [0.0, 0.0, 0.0, 1.0],
            vr: None,
            vr_failed: false,
            vr_enable: false,
            eyes: None,
            eye_views: None,
        })
    }
}

impl Window for MetalWindow {
    fn attach(surface: &Surface) -> Self {
        Self::try_new(surface).expect("metal")
    }

    fn set_size(&mut self, w: u32, h: u32) {
        self.width = w.max(1);
        self.height = h.max(1);
        sync_layer(self.width, self.height, self.scale_factor, &self.layer);
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        self.clear = [red as f64, green as f64, blue as f64, 1.0];
        self.ui_verts.clear();
        self.draw_mesh = false;
    }

    fn draw_skinned(&mut self, batch: &SkinBatch, view: &SceneView) {
        self.skin.batch = batch.clone();
        self.skin.view = metal_view_proj(view);
    }

    fn draw_colored_mesh(
        &mut self,
        vertices: &[f32],
        ranges: &[crate::world::SurfaceRange],
        graphics: &crate::world::MapGraphics,
        revision: u64,
        view: &SceneView,
    ) {
        if !self.mesh_ready || self.mesh_revision != revision {
            self.mesh_vertices = (vertices.len() / crate::world::STRIDE) as u64;
            self.mesh_revision = revision;
            self.mesh_ready = true;
            self.mesh = shared_buffer(&self.device, float_bytes(vertices));
            self.ranges = ranges.to_vec();
            self.cpu = vertices.to_vec();
            self.sync_graphics(graphics);
        }

        self.view_proj = metal_view_proj(view);
        self.eye = view.eye;
        self.time = view.time;
        self.far = view.far;
        self.eye_views = vr::connect(&mut self.vr, &mut self.vr_failed, self.vr_enable, view);

        if let Some(eyes) = self.eye_views {
            self.view_proj = metal_view_proj(&eyes.views[0]);
            self.eye = eyes.views[0].eye;
            self.time = eyes.views[0].time;
            self.far = eyes.views[0].far;
        }

        self.draw_mesh = self.mesh_vertices > 0 || self.sky.is_some();
    }

    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        push_rect(&mut self.ui_verts, x, y, w, h, color.as_rgba_f32());
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

    fn draw_text(&mut self, _font: &str, text: &str, x: f32, y: f32, scale: f32, color: Color) {
        self.glyphs.queue(
            Section::default()
                .add_text(
                    Text::new(text)
                        .with_scale(scale)
                        .with_color(color.as_rgba_f32()),
                )
                .with_screen_position((x, y)),
        );
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
        for _attempt in 0..4 {
            let result = self.glyphs.process_queued(
                |rect, data| {
                    let width = rect.max[0] - rect.min[0];
                    let height = rect.max[1] - rect.min[1];

                    if width == 0 || height == 0 {
                        return;
                    }

                    self.atlas.replace_region(
                        MTLRegion::new_2d(
                            rect.min[0] as u64,
                            rect.min[1] as u64,
                            width as u64,
                            height as u64,
                        ),
                        0,
                        data.as_ptr() as *const _,
                        width as u64,
                    );
                },
                glyph_quad,
            );

            match result {
                Ok(BrushAction::Draw(quads)) => {
                    self.text_verts.clear();

                    for quad in quads {
                        for idx in 0..6 {
                            self.text_verts.extend_from_slice(&quad.verts[idx]);
                        }
                    }

                    return;
                }
                Ok(BrushAction::ReDraw) => {
                    return;
                }
                Err(BrushError::TextureTooSmall { suggested }) => {
                    self.glyphs.resize_texture(suggested.0, suggested.1);
                    self.atlas = atlas_texture(&self.device, suggested.0, suggested.1);
                    self.atlas_size = suggested;
                }
            }
        }
    }

    fn present(&mut self) {
        sync_layer(self.width, self.height, self.scale_factor, &self.layer);
        let drawable_size = self.layer.drawable_size();
        let width = drawable_size.width as u64;
        let height = drawable_size.height as u64;

        if width == 0 || height == 0 {
            return;
        }

        self.render_headset();
        self.ensure_depth(width, height);
        self.ensure_scene(width, height);
        objc::rc::autoreleasepool(|| {
            let Some(drawable) = self.layer.next_drawable() else {
                return;
            };

            let color_texture = drawable.texture();
            let Some(depth) = self.depth.as_ref() else {
                return;
            };

            let pass = RenderPassDescriptor::new();
            let color = pass.color_attachments().object_at(0).unwrap();
            color.set_texture(Some(color_texture));
            color.set_load_action(MTLLoadAction::Clear);
            color.set_store_action(MTLStoreAction::Store);
            color.set_clear_color(MTLClearColor::new(
                self.clear[0],
                self.clear[1],
                self.clear[2],
                self.clear[3],
            ));

            let depth_attachment = pass.depth_attachment().unwrap();
            depth_attachment.set_texture(Some(depth));
            depth_attachment.set_load_action(MTLLoadAction::Clear);
            depth_attachment.set_store_action(MTLStoreAction::Store);
            depth_attachment.set_clear_depth(1.0);
            pass.set_depth_attachment(Some(depth_attachment));

            let command = self.queue.new_command_buffer();
            let encoder = command.new_render_command_encoder(pass);
            let resolution = [width as f32, height as f32, 0.0, 0.0];

            if self.draw_mesh {
                self.encode_mesh(&encoder, width as f32, height as f32, false);
            }

            encoder.end_encoding();
            self.blit_scene(&command, color_texture, width, height);
            let load = RenderPassDescriptor::new();
            let load_color = load.color_attachments().object_at(0).unwrap();
            load_color.set_texture(Some(color_texture));
            load_color.set_load_action(MTLLoadAction::Load);
            load_color.set_store_action(MTLStoreAction::Store);
            let load_depth = load.depth_attachment().unwrap();
            load_depth.set_texture(Some(depth));
            load_depth.set_load_action(MTLLoadAction::Load);
            load_depth.set_store_action(MTLStoreAction::DontCare);
            load.set_depth_attachment(Some(load_depth));
            let encoder = command.new_render_command_encoder(load);

            if self.draw_mesh {
                self.encode_mesh(&encoder, width as f32, height as f32, true);
            }

            encode_skin(
                &mut self.skin,
                &self.device,
                &encoder,
                &self.skin_pipeline,
                &self.depth_write,
                &self.view_proj,
            );
            bind_bytes(
                encoder,
                &self.device,
                &self.color_pipeline,
                &self.depth_off,
                float_bytes(&self.ui_verts),
                self.ui_verts.len() / 6,
                &resolution,
                None,
            );
            bind_bytes(
                encoder,
                &self.device,
                &self.text_pipeline,
                &self.depth_off,
                float_bytes(&self.text_verts),
                self.text_verts.len() / 8,
                &resolution,
                Some(&self.atlas),
            );

            encoder.end_encoding();
            command.present_drawable(drawable);
            command.commit();
        });

        if let Some(headset) = self.vr.as_mut() {
            headset.handoff();
        }
    }
}

impl MetalWindow {
    fn ensure_depth(&mut self, width: u64, height: u64) {
        if self.depth_size == (width, height) && self.depth.is_some() {
            return;
        }

        let desc = TextureDescriptor::new();
        desc.set_texture_type(MTLTextureType::D2);
        desc.set_pixel_format(MTLPixelFormat::Depth32Float);
        desc.set_width(width);
        desc.set_height(height);
        desc.set_usage(MTLTextureUsage::RenderTarget);
        desc.set_storage_mode(MTLStorageMode::Private);
        self.depth = Some(self.device.new_texture(&desc));
        self.depth_size = (width, height);
    }

    fn render_headset(&mut self) {
        let Some(frame) = self.eye_views else {
            return;
        };

        if !self.ensure_metal_eyes(frame.width as u64, frame.height as u64) {
            return;
        }

        let (color, depth, width, height) = {
            let Some(eyes) = &self.eyes else {
                return;
            };

            (
                [eyes.color[0].clone(), eyes.color[1].clone()],
                [eyes.depth[0].clone(), eyes.depth[1].clone()],
                eyes.width,
                eyes.height,
            )
        };
        let queue = self.queue.clone();
        let command = queue.new_command_buffer();
        objc::rc::autoreleasepool(|| {
            let mut idx = 0;

            while idx < 2 {
                let matrix = metal_view_proj(&frame.views[idx]);
                let pass = RenderPassDescriptor::new();
                let attachment = pass.color_attachments().object_at(0).unwrap();
                attachment.set_texture(Some(&color[idx]));
                attachment.set_load_action(MTLLoadAction::Clear);
                attachment.set_store_action(MTLStoreAction::Store);
                attachment.set_clear_color(MTLClearColor::new(
                    self.clear[0],
                    self.clear[1],
                    self.clear[2],
                    self.clear[3],
                ));
                let depth_attachment = pass.depth_attachment().unwrap();
                depth_attachment.set_texture(Some(&depth[idx]));
                depth_attachment.set_load_action(MTLLoadAction::Clear);
                depth_attachment.set_store_action(MTLStoreAction::DontCare);
                depth_attachment.set_clear_depth(1.0);
                pass.set_depth_attachment(Some(depth_attachment));
                let encoder = command.new_render_command_encoder(pass);
                encoder.set_viewport(MTLViewport {
                    originX: 0.0,
                    originY: 0.0,
                    width: width as f64,
                    height: height as f64,
                    znear: 0.0,
                    zfar: 1.0,
                });

                if self.draw_mesh {
                    self.encode_mesh(&encoder, width as f32, height as f32, false);
                    self.encode_mesh(&encoder, width as f32, height as f32, true);
                }

                encode_skin(
                    &mut self.skin,
                    &self.device,
                    &encoder,
                    &self.skin_pipeline,
                    &self.depth_write,
                    &matrix,
                );
                encoder.end_encoding();
                idx += 1;
            }
        });

        command.commit();
        command.wait_until_completed();

        if let Some(headset) = self.vr.as_mut() {
            headset.submit_metal(0, color[0].as_ptr() as *mut std::ffi::c_void);
            headset.submit_metal(1, color[1].as_ptr() as *mut std::ffi::c_void);
        }
    }

    fn ensure_metal_eyes(&mut self, width: u64, height: u64) -> bool {
        let width = width.max(1);
        let height = height.max(1);

        if let Some(eyes) = &self.eyes {
            if eyes.width == width && eyes.height == height {
                return true;
            }
        }

        self.eyes = Some(MetalEyes {
            width,
            height,
            color: [
                eye_color(&self.device, width, height),
                eye_color(&self.device, width, height),
            ],
            depth: [
                eye_depth(&self.device, width, height),
                eye_depth(&self.device, width, height),
            ],
        });

        true
    }
}

impl Drop for MetalWindow {
    fn drop(&mut self) {
        self.vr.take();
    }
}

#[allow(unexpected_cfgs)]
fn attach_layer(surface: &Surface, layer: &MetalLayer) -> Result<(), String> {
    match surface.window {
        RawWindowHandle::AppKit(appkit) => {
            if appkit.ns_view.is_null() {
                return Err("missing ns view".to_string());
            }

            unsafe {
                let view = appkit.ns_view as *mut Object;
                let _: () = msg_send![view, setWantsLayer: YES];
                let _: () = msg_send![view, setLayer: layer.as_ptr()];
            }

            Ok(())
        }
        #[cfg(target_os = "ios")]
        RawWindowHandle::UiKit(uikit) => {
            if uikit.ui_view.is_null() {
                return Err("missing ui view".to_string());
            }

            let scale = surface.scale_factor.max(1.0);
            unsafe {
                engine_attach_metal_layer(
                    uikit.ui_view,
                    layer.as_ptr() as *mut c_void,
                    surface.width as f64 / scale,
                    surface.height as f64 / scale,
                );
            }

            Ok(())
        }
        _ => Err("window is not appkit".to_string()),
    }
}

#[cfg(target_os = "ios")]
extern "C" {
    fn engine_attach_metal_layer(view: *mut c_void, layer: *mut c_void, width: f64, height: f64);
    fn engine_resize_metal_layer(layer: *mut c_void, width: f64, height: f64);
}

fn sync_layer(width: u32, height: u32, scale: f64, layer: &MetalLayer) {
    let drawable = layer.drawable_size();
    let same_size = drawable.width == width as f64 && drawable.height == height as f64;
    let same_scale = layer.contents_scale() == scale;

    if same_size && same_scale {
        return;
    }

    layer.set_contents_scale(scale);
    layer.set_drawable_size(CGSize::new(width as f64, height as f64));
    #[cfg(target_os = "ios")]
    unsafe {
        engine_resize_metal_layer(
            layer.as_ptr() as *mut c_void,
            width as f64 / scale.max(1.0),
            height as f64 / scale.max(1.0),
        );
    }
}

fn bind_bytes(
    encoder: &RenderCommandEncoderRef,
    device: &DeviceRef,
    pipeline: &RenderPipelineStateRef,
    depth: &DepthStencilStateRef,
    bytes: &[u8],
    vertices: usize,
    resolution: &[f32; 4],
    texture: Option<&TextureRef>,
) {
    if bytes.is_empty() || vertices == 0 {
        return;
    }

    encoder.set_render_pipeline_state(pipeline);
    encoder.set_depth_stencil_state(depth);
    encoder.set_cull_mode(MTLCullMode::None);
    let owned = if bytes.len() <= 4096 {
        encoder.set_vertex_bytes(0, bytes.len() as u64, bytes.as_ptr() as *const _);
        None
    } else {
        let buffer = shared_buffer(device, bytes);
        encoder.set_vertex_buffer(0, buffer.as_deref(), 0);
        buffer
    };
    encoder.set_vertex_bytes(
        1,
        std::mem::size_of::<[f32; 4]>() as u64,
        resolution.as_ptr() as *const _,
    );

    if let Some(texture) = texture {
        encoder.set_fragment_texture(0, Some(texture));
    }

    encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, vertices as u64);
    drop(owned);
}

fn pipeline(
    device: &Device,
    vertex: &Function,
    fragment: &Function,
    vertex_desc: &VertexDescriptorRef,
    blend: bool,
) -> Result<RenderPipelineState, String> {
    let desc = RenderPipelineDescriptor::new();
    desc.set_vertex_function(Some(vertex));
    desc.set_fragment_function(Some(fragment));
    desc.set_vertex_descriptor(Some(vertex_desc));
    desc.set_depth_attachment_pixel_format(MTLPixelFormat::Depth32Float);
    let color = desc.color_attachments().object_at(0).unwrap();
    color.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    color.set_blending_enabled(blend);

    if blend {
        color.set_rgb_blend_operation(MTLBlendOperation::Add);
        color.set_alpha_blend_operation(MTLBlendOperation::Add);
        color.set_source_rgb_blend_factor(MTLBlendFactor::SourceAlpha);
        color.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        color.set_source_alpha_blend_factor(MTLBlendFactor::One);
        color.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
    }

    device.new_render_pipeline_state(&desc)
}

#[derive(Default)]
struct MetalSkin {
    batch: SkinBatch,
    view: [f32; 16],
    meshes: std::collections::HashMap<u64, MetalSkinMesh>,
}

struct MetalSkinMesh {
    vertices: Buffer,
    indices: Buffer,
    instances: Buffer,
    albedo: Texture,
    palette: Texture,
    index_count: u64,
    vertex_ptr: usize,
    palette_size: (u64, u64),
}

fn encode_skin(
    skin: &mut MetalSkin,
    device: &Device,
    encoder: &RenderCommandEncoderRef,
    pipeline: &RenderPipelineState,
    depth: &DepthStencilState,
    matrix: &[f32; 16],
) {
    if skin.batch.groups.is_empty() {
        return;
    }

    encoder.set_render_pipeline_state(pipeline);
    encoder.set_depth_stencil_state(depth);
    encoder.set_cull_mode(MTLCullMode::Back);
    encoder.set_front_facing_winding(MTLWinding::CounterClockwise);
    let mut idx = 0;

    while idx < skin.batch.groups.len() {
        let group = &skin.batch.groups[idx];
        let key = group.key;
        let vertex_ptr = group.vertices.as_ptr() as usize;
        let stale = skin
            .meshes
            .get(&key)
            .map(|mesh| mesh.vertex_ptr != vertex_ptr)
            .unwrap_or(true);

        if stale {
            skin.meshes.insert(key, upload_skin_mesh(device, group));
        }

        let Some(mesh) = skin.meshes.get_mut(&key) else {
            idx += 1;

            continue;
        };
        let instances = match shared_buffer(device, float_bytes(group.instances.as_ref())) {
            Some(buffer) => buffer,
            None => {
                idx += 1;

                continue;
            }
        };
        mesh.instances = instances;

        if mesh.palette_size != (group.palette_w as u64, group.palette_h as u64) {
            mesh.palette = float_texture(device, group.palette_w, group.palette_h.max(1));
            mesh.palette_size = (group.palette_w as u64, group.palette_h as u64);
        }

        if group.palette_w > 0 && group.palette_h > 0 {
            mesh.palette.replace_region(
                MTLRegion::new_2d(0, 0, group.palette_w as u64, group.palette_h as u64),
                0,
                group.palette.as_ptr() as *const _,
                (group.palette_w as u64) * 16,
            );
        }

        let uniforms = skin_uniforms(matrix, group.bones);
        encoder.set_vertex_buffer(0, Some(&mesh.vertices), 0);
        encoder.set_vertex_buffer(1, Some(&mesh.instances), 0);
        encoder.set_vertex_bytes(
            2,
            std::mem::size_of_val(&uniforms) as u64,
            &uniforms as *const _ as *const _,
        );
        encoder.set_fragment_texture(0, Some(&mesh.albedo));
        encoder.set_vertex_texture(1, Some(&mesh.palette));
        encoder.draw_indexed_primitives_instanced(
            MTLPrimitiveType::Triangle,
            mesh.index_count,
            MTLIndexType::UInt32,
            &mesh.indices,
            0,
            group.palette_h as u64,
        );
        idx += 1;
    }
}

fn upload_skin_mesh(device: &Device, group: &crate::ui::skin::SkinGroup) -> MetalSkinMesh {
    let vertices = shared_buffer(device, float_bytes(group.vertices.as_ref()))
        .unwrap_or_else(|| device.new_buffer(4, MTLResourceOptions::StorageModeShared));
    let indices = device.new_buffer_with_data(
        group.indices.as_ptr() as *const _,
        (group.indices.len() * 4) as u64,
        MTLResourceOptions::StorageModeShared,
    );
    let albedo = rgba_texture(
        device,
        group.albedo_w,
        group.albedo_h,
        group.albedo.as_ref(),
    );
    let palette = float_texture(device, group.palette_w.max(1), group.palette_h.max(1));
    let instances = device.new_buffer(4, MTLResourceOptions::StorageModeShared);

    MetalSkinMesh {
        vertices,
        indices,
        instances,
        albedo,
        palette,
        index_count: group.indices.len() as u64,
        vertex_ptr: group.vertices.as_ptr() as usize,
        palette_size: (0, 0),
    }
}

fn skin_uniforms(matrix: &[f32; 16], bones: u32) -> [u32; 20] {
    let mut words = [0u32; 20];
    let mut idx = 0;

    while idx < 16 {
        words[idx] = matrix[idx].to_bits();
        idx += 1;
    }

    words[16] = bones;

    words
}

fn rgba_texture(device: &Device, width: u32, height: u32, pixels: &[u8]) -> Texture {
    let texture = color_texture(
        device,
        width.max(1),
        height.max(1),
        MTLPixelFormat::RGBA8Unorm,
    );

    if width > 0 && height > 0 && pixels.len() >= (width as usize) * (height as usize) * 4 {
        texture.replace_region(
            MTLRegion::new_2d(0, 0, width as u64, height as u64),
            0,
            pixels.as_ptr() as *const _,
            (width as u64) * 4,
        );
    }

    texture
}

fn float_texture(device: &Device, width: u32, height: u32) -> Texture {
    color_texture(
        device,
        width.max(1),
        height.max(1),
        MTLPixelFormat::RGBA32Float,
    )
}

fn color_texture(device: &Device, width: u32, height: u32, format: MTLPixelFormat) -> Texture {
    let desc = TextureDescriptor::new();
    desc.set_texture_type(MTLTextureType::D2);
    desc.set_pixel_format(format);
    desc.set_width(width as u64);
    desc.set_height(height as u64);
    desc.set_usage(MTLTextureUsage::ShaderRead);
    desc.set_storage_mode(MTLStorageMode::Shared);

    device.new_texture(&desc)
}

fn skin_vertex_desc() -> &'static VertexDescriptorRef {
    let desc = VertexDescriptor::new();
    set_skin_attr(desc, 0, MTLVertexFormat::Float3, 0, 0);
    set_skin_attr(desc, 1, MTLVertexFormat::Float3, 12, 0);
    set_skin_attr(desc, 2, MTLVertexFormat::Float2, 24, 0);
    set_skin_attr(desc, 3, MTLVertexFormat::Float4, 32, 0);
    set_skin_attr(desc, 4, MTLVertexFormat::Float4, 48, 0);
    set_skin_layout(desc, 0, 64, MTLVertexStepFunction::PerVertex);
    set_skin_attr(desc, 5, MTLVertexFormat::Float4, 0, 1);
    set_skin_attr(desc, 6, MTLVertexFormat::Float4, 16, 1);
    set_skin_attr(desc, 7, MTLVertexFormat::Float4, 32, 1);
    set_skin_attr(desc, 8, MTLVertexFormat::Float4, 48, 1);
    set_skin_layout(desc, 1, 64, MTLVertexStepFunction::PerInstance);

    desc
}

fn set_skin_attr(
    desc: &VertexDescriptorRef,
    index: u64,
    format: MTLVertexFormat,
    offset: u64,
    buffer: u64,
) {
    let attr = desc.attributes().object_at(index).unwrap();
    attr.set_format(format);
    attr.set_offset(offset);
    attr.set_buffer_index(buffer);
}

fn set_skin_layout(
    desc: &VertexDescriptorRef,
    index: u64,
    stride: u64,
    step: MTLVertexStepFunction,
) {
    let layout = desc.layouts().object_at(index).unwrap();
    layout.set_stride(stride);
    layout.set_step_function(step);
}

struct MetalMaterial {
    gpu: crate::world::surface::MaterialGpu,
    base: Texture,
    base2: Texture,
    bump: Texture,
    bump2: Texture,
    detail: Texture,
    blend: Texture,
    mask: Texture,
}

impl MetalWindow {
    fn sync_graphics(&mut self, graphics: &crate::world::MapGraphics) {
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

        if self.graphics_key == key {
            return;
        }

        self.graphics_key = key;
        self.materials.clear();
        let mut idx = 0;

        while idx < graphics.materials.len() {
            let material = &graphics.materials[idx];
            self.materials.push(MetalMaterial {
                gpu: material.gpu,
                base: metal_image(&self.device, &material.base, true),
                base2: metal_image(&self.device, &material.base2, true),
                bump: metal_image(&self.device, &material.bump, true),
                bump2: metal_image(&self.device, &material.bump2, true),
                detail: metal_image(&self.device, &material.detail, true),
                blend: metal_image(&self.device, &material.blend, true),
                mask: metal_image(&self.device, &material.mask, true),
            });
            idx += 1;
        }

        self.lightmaps.clear();
        idx = 0;

        while idx < 4 {
            self.lightmaps
                .push(metal_image(&self.device, &graphics.lightmaps[idx], false));
            idx += 1;
        }

        self.cubemaps.clear();
        idx = 0;

        while idx < graphics.cubemaps.len() {
            self.cubemaps
                .push(metal_cube(&self.device, &graphics.cubemaps[idx]));
            idx += 1;
        }

        self.sky = graphics
            .sky
            .as_ref()
            .map(|sky| metal_cube(&self.device, sky));
    }

    fn encode_mesh(
        &self,
        encoder: &RenderCommandEncoderRef,
        width: f32,
        height: f32,
        translucent: bool,
    ) {
        let Some(mesh) = self.mesh.as_ref() else {
            return;
        };
        encoder.set_cull_mode(MTLCullMode::Back);
        encoder.set_front_facing_winding(MTLWinding::CounterClockwise);
        encoder.set_vertex_buffer(0, Some(mesh), 0);
        encoder.set_fragment_sampler_state(0, Some(&self.wrap_sampler));
        encoder.set_fragment_sampler_state(1, Some(&self.clamp_sampler));
        let mut view = crate::world::surface::view_constants(
            self.view_proj,
            self.eye,
            self.time,
            [width, height, 0.0, 0.0],
        );

        if !translucent {
            if let Some(sky) = &self.sky {
                view[22] = 1.0;
                encoder.set_render_pipeline_state(&self.mesh_pipeline);
                encoder.set_depth_stencil_state(&self.depth_off);
                encoder.set_cull_mode(MTLCullMode::None);
                let verts = crate::world::surface::sky_vertices(self.eye, self.far * 0.25);
                let buffer = shared_buffer(&self.device, float_bytes(&verts));
                encoder.set_vertex_buffer(0, buffer.as_deref(), 0);
                encoder.set_vertex_bytes(1, 96, view.as_ptr() as *const _);
                encoder.set_fragment_bytes(1, 96, view.as_ptr() as *const _);
                let gpu = crate::world::surface::MaterialGpu::unlit();
                encoder.set_fragment_bytes(2, 96, gpu_ptr(&gpu));
                bind_metal_slots(
                    encoder,
                    &self.white,
                    &self.white,
                    &self.flat,
                    &self.flat,
                    &self.white,
                    &self.white,
                    &self.white,
                    sky,
                    self.lightmaps.get(0).unwrap_or(&self.white),
                    self.lightmaps.get(1).unwrap_or(&self.white),
                    self.lightmaps.get(2).unwrap_or(&self.white),
                    self.lightmaps.get(3).unwrap_or(&self.white),
                    &self.scene,
                );
                encoder.draw_primitives(
                    MTLPrimitiveType::Triangle,
                    0,
                    (verts.len() / crate::world::STRIDE) as u64,
                );
                encoder.set_vertex_buffer(0, Some(mesh), 0);
                encoder.set_cull_mode(MTLCullMode::Back);
                view[22] = 0.0;
            }
        }

        let order = crate::world::surface::ordered_ranges(&self.cpu, &self.ranges, self.eye);
        let mut idx = 0;

        while idx < order.len() {
            let range = self.ranges[order[idx]];
            let blend = range.pass >= crate::world::surface::PASS_BLEND;

            if blend != translucent {
                idx += 1;

                continue;
            }

            encoder.set_vertex_bytes(1, 96, view.as_ptr() as *const _);
            encoder.set_fragment_bytes(1, 96, view.as_ptr() as *const _);
            let material = self.materials.get(range.material as usize);
            let gpu = material
                .map(|item| item.gpu)
                .unwrap_or_else(crate::world::surface::MaterialGpu::shaded);
            encoder.set_fragment_bytes(2, 96, gpu_ptr(&gpu));
            let env = self
                .cubemaps
                .get(range.cubemap as usize)
                .unwrap_or(&self.white_cube);
            bind_metal_slots(
                encoder,
                material.map(|item| &item.base).unwrap_or(&self.white),
                material.map(|item| &item.base2).unwrap_or(&self.white),
                material.map(|item| &item.bump).unwrap_or(&self.flat),
                material.map(|item| &item.bump2).unwrap_or(&self.flat),
                material.map(|item| &item.detail).unwrap_or(&self.white),
                material.map(|item| &item.blend).unwrap_or(&self.white),
                material.map(|item| &item.mask).unwrap_or(&self.white),
                env,
                self.lightmaps.get(0).unwrap_or(&self.white),
                self.lightmaps.get(1).unwrap_or(&self.white),
                self.lightmaps.get(2).unwrap_or(&self.white),
                self.lightmaps.get(3).unwrap_or(&self.white),
                &self.scene,
            );

            if range.pass == crate::world::surface::PASS_DECAL || blend {
                encoder.set_render_pipeline_state(&self.mesh_blend);
                encoder.set_depth_stencil_state(&self.depth_off);
            } else {
                encoder.set_render_pipeline_state(&self.mesh_pipeline);
                encoder.set_depth_stencil_state(&self.depth_write);
            }

            encoder.draw_primitives(
                MTLPrimitiveType::Triangle,
                range.first as u64,
                range.count as u64,
            );
            idx += 1;
        }
    }

    fn ensure_scene(&mut self, width: u64, height: u64) {
        if self.scene.width() == width && self.scene.height() == height {
            return;
        }

        let image = crate::world::surface::CpuImage::solid(
            width as u32,
            height as u32,
            [255, 255, 255, 255],
        );
        self.scene = metal_image(&self.device, &image, false);
    }

    fn blit_scene(&self, command: &CommandBufferRef, color: &TextureRef, width: u64, height: u64) {
        if self.scene.width() != width || self.scene.height() != height {
            return;
        }

        let blit = command.new_blit_command_encoder();
        blit.copy_from_texture(
            color,
            0,
            0,
            MTLOrigin { x: 0, y: 0, z: 0 },
            MTLSize {
                width,
                height,
                depth: 1,
            },
            &self.scene,
            0,
            0,
            MTLOrigin { x: 0, y: 0, z: 0 },
        );
        blit.end_encoding();
    }
}

fn gpu_ptr(gpu: &crate::world::surface::MaterialGpu) -> *const std::ffi::c_void {
    gpu as *const crate::world::surface::MaterialGpu as *const std::ffi::c_void
}

fn bind_metal_slots(
    encoder: &RenderCommandEncoderRef,
    base: &TextureRef,
    base2: &TextureRef,
    bump: &TextureRef,
    bump2: &TextureRef,
    detail: &TextureRef,
    blend: &TextureRef,
    mask: &TextureRef,
    env: &TextureRef,
    light0: &TextureRef,
    light1: &TextureRef,
    light2: &TextureRef,
    light3: &TextureRef,
    scene: &TextureRef,
) {
    let slots: [&TextureRef; 13] = [
        base, base2, bump, bump2, detail, blend, mask, env, light0, light1, light2, light3, scene,
    ];
    let mut idx = 0;

    while idx < slots.len() {
        encoder.set_fragment_texture(idx as u64, Some(slots[idx]));
        idx += 1;
    }
}

fn metal_sampler(device: &Device, address: MTLSamplerAddressMode) -> SamplerState {
    let desc = SamplerDescriptor::new();
    desc.set_min_filter(MTLSamplerMinMagFilter::Linear);
    desc.set_mag_filter(MTLSamplerMinMagFilter::Linear);
    desc.set_address_mode_s(address);
    desc.set_address_mode_t(address);
    desc.set_address_mode_r(address);

    device.new_sampler(&desc)
}

fn metal_image(device: &Device, image: &crate::world::surface::CpuImage, repeat: bool) -> Texture {
    let desc = TextureDescriptor::new();
    desc.set_texture_type(MTLTextureType::D2);
    desc.set_pixel_format(metal_format(image.format));
    desc.set_width(image.width as u64);
    desc.set_height(image.height as u64);
    desc.set_usage(MTLTextureUsage::ShaderRead);
    desc.set_storage_mode(MTLStorageMode::Shared);
    let texture = device.new_texture(&desc);
    texture.replace_region(
        MTLRegion::new_2d(0, 0, image.width as u64, image.height as u64),
        0,
        image.bytes.as_ptr() as *const _,
        metal_pitch(image),
    );
    let _ = repeat;

    texture
}

fn metal_cube(device: &Device, image: &crate::world::surface::CubeImage) -> Texture {
    let face = &image.faces[0];
    let desc = TextureDescriptor::new();
    desc.set_texture_type(MTLTextureType::Cube);
    desc.set_pixel_format(metal_format(face.format));
    desc.set_width(face.width as u64);
    desc.set_height(face.height as u64);
    desc.set_usage(MTLTextureUsage::ShaderRead);
    desc.set_storage_mode(MTLStorageMode::Shared);
    let texture = device.new_texture(&desc);
    let mut idx = 0;

    while idx < 6 {
        let face = &image.faces[idx];
        texture.replace_region_in_slice(
            MTLRegion::new_2d(0, 0, face.width as u64, face.height as u64),
            0,
            idx as u64,
            face.bytes.as_ptr() as *const _,
            metal_pitch(face),
            metal_pitch(face) * face.height as u64,
        );
        idx += 1;
    }

    texture
}

fn metal_format(format: crate::world::surface::PixelFormat) -> MTLPixelFormat {
    match format {
        crate::world::surface::PixelFormat::Bc1 => MTLPixelFormat::BC1_RGBA,
        crate::world::surface::PixelFormat::Bc2 => MTLPixelFormat::BC2_RGBA,
        crate::world::surface::PixelFormat::Bc3 => MTLPixelFormat::BC3_RGBA,
        crate::world::surface::PixelFormat::Bc5 => MTLPixelFormat::BC5_RGUnorm,
        crate::world::surface::PixelFormat::Bc7 => MTLPixelFormat::BC7_RGBAUnorm,
        crate::world::surface::PixelFormat::Rgba8 | crate::world::surface::PixelFormat::Rgba16f => {
            MTLPixelFormat::RGBA8Unorm
        }
    }
}

fn metal_pitch(image: &crate::world::surface::CpuImage) -> u64 {
    match image.format {
        crate::world::surface::PixelFormat::Bc1 | crate::world::surface::PixelFormat::Bc5 => {
            (image.width.max(4) / 4) as u64 * 8
        }
        crate::world::surface::PixelFormat::Bc2
        | crate::world::surface::PixelFormat::Bc3
        | crate::world::surface::PixelFormat::Bc7 => (image.width.max(4) / 4) as u64 * 16,
        crate::world::surface::PixelFormat::Rgba8 | crate::world::surface::PixelFormat::Rgba16f => {
            image.width as u64 * 4
        }
    }
}

fn mesh_vertex_desc() -> &'static VertexDescriptorRef {
    let desc = VertexDescriptor::new();
    set_attr(desc, 0, MTLVertexFormat::Float3, 0);
    set_attr(desc, 1, MTLVertexFormat::Float3, 12);
    set_attr(desc, 2, MTLVertexFormat::Float4, 24);
    set_attr(desc, 3, MTLVertexFormat::Float2, 40);
    set_attr(desc, 4, MTLVertexFormat::Float2, 48);
    set_attr(desc, 5, MTLVertexFormat::Float3, 56);
    set_attr(desc, 6, MTLVertexFormat::Float, 68);
    set_attr(desc, 7, MTLVertexFormat::Float, 72);
    set_layout(desc, (crate::world::STRIDE * 4) as u64);

    desc
}

fn color_vertex_desc() -> &'static VertexDescriptorRef {
    let desc = VertexDescriptor::new();
    set_attr(desc, 0, MTLVertexFormat::Float2, 0);
    set_attr(desc, 1, MTLVertexFormat::Float4, 8);
    set_layout(desc, 24);

    desc
}

fn text_vertex_desc() -> &'static VertexDescriptorRef {
    let desc = VertexDescriptor::new();
    set_attr(desc, 0, MTLVertexFormat::Float2, 0);
    set_attr(desc, 1, MTLVertexFormat::Float2, 8);
    set_attr(desc, 2, MTLVertexFormat::Float4, 16);
    set_layout(desc, 32);

    desc
}

fn set_attr(desc: &VertexDescriptorRef, index: u64, format: MTLVertexFormat, offset: u64) {
    let attr = desc.attributes().object_at(index).unwrap();
    attr.set_format(format);
    attr.set_offset(offset);
    attr.set_buffer_index(0);
}

fn set_layout(desc: &VertexDescriptorRef, stride: u64) {
    let layout = desc.layouts().object_at(0).unwrap();
    layout.set_stride(stride);
    layout.set_step_function(MTLVertexStepFunction::PerVertex);
}

fn depth_state(device: &Device, compare: MTLCompareFunction, write: bool) -> DepthStencilState {
    let desc = DepthStencilDescriptor::new();
    desc.set_depth_compare_function(compare);
    desc.set_depth_write_enabled(write);

    device.new_depth_stencil_state(&desc)
}

fn atlas_texture(device: &Device, width: u32, height: u32) -> Texture {
    let desc = TextureDescriptor::new();
    desc.set_texture_type(MTLTextureType::D2);
    desc.set_pixel_format(MTLPixelFormat::R8Unorm);
    desc.set_width(width as u64);
    desc.set_height(height as u64);
    desc.set_usage(MTLTextureUsage::ShaderRead);
    desc.set_storage_mode(MTLStorageMode::Shared);

    device.new_texture(&desc)
}

fn shared_buffer(device: &DeviceRef, bytes: &[u8]) -> Option<Buffer> {
    if bytes.is_empty() {
        return None;
    }

    Some(device.new_buffer_with_data(
        bytes.as_ptr() as *const _,
        bytes.len() as u64,
        MTLResourceOptions::StorageModeShared,
    ))
}

fn float_bytes(values: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr() as *const u8, values.len() * 4) }
}

fn push_rect(verts: &mut Vec<f32>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    let corners = [
        [x, y],
        [x + w, y],
        [x, y + h],
        [x, y + h],
        [x + w, y],
        [x + w, y + h],
    ];

    for corner in corners {
        verts.push(corner[0]);
        verts.push(corner[1]);
        verts.extend_from_slice(&color);
    }
}

fn glyph_quad(vertex: glyph_brush::GlyphVertex<Extra>) -> GlyphQuad {
    let color = vertex.extra.color;
    let positions = [
        [
            vertex.pixel_coords.min.x,
            vertex.pixel_coords.min.y,
            vertex.tex_coords.min.x,
            vertex.tex_coords.min.y,
        ],
        [
            vertex.pixel_coords.max.x,
            vertex.pixel_coords.min.y,
            vertex.tex_coords.max.x,
            vertex.tex_coords.min.y,
        ],
        [
            vertex.pixel_coords.min.x,
            vertex.pixel_coords.max.y,
            vertex.tex_coords.min.x,
            vertex.tex_coords.max.y,
        ],
        [
            vertex.pixel_coords.min.x,
            vertex.pixel_coords.max.y,
            vertex.tex_coords.min.x,
            vertex.tex_coords.max.y,
        ],
        [
            vertex.pixel_coords.max.x,
            vertex.pixel_coords.min.y,
            vertex.tex_coords.max.x,
            vertex.tex_coords.min.y,
        ],
        [
            vertex.pixel_coords.max.x,
            vertex.pixel_coords.max.y,
            vertex.tex_coords.max.x,
            vertex.tex_coords.max.y,
        ],
    ];
    let mut verts = [[0.0; 8]; 6];

    for idx in 0..6 {
        verts[idx] = [
            positions[idx][0],
            positions[idx][1],
            positions[idx][2],
            positions[idx][3],
            color[0],
            color[1],
            color[2],
            color[3],
        ];
    }

    GlyphQuad { verts }
}

fn metal_view_proj(view: &SceneView) -> [f32; 16] {
    vr::view_proj(view, true)
}

fn eye_color(device: &Device, width: u64, height: u64) -> Texture {
    let desc = TextureDescriptor::new();
    desc.set_texture_type(MTLTextureType::D2);
    desc.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    desc.set_width(width);
    desc.set_height(height);
    desc.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
    desc.set_storage_mode(MTLStorageMode::Managed);
    #[cfg(not(all(target_os = "macos", not(target_arch = "aarch64"))))]
    desc.set_storage_mode(MTLStorageMode::Shared);

    device.new_texture(&desc)
}

fn eye_depth(device: &Device, width: u64, height: u64) -> Texture {
    let desc = TextureDescriptor::new();
    desc.set_texture_type(MTLTextureType::D2);
    desc.set_pixel_format(MTLPixelFormat::Depth32Float);
    desc.set_width(width);
    desc.set_height(height);
    desc.set_usage(MTLTextureUsage::RenderTarget);
    desc.set_storage_mode(MTLStorageMode::Private);

    device.new_texture(&desc)
}
