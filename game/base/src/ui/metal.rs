use core_graphics_types::geometry::CGSize;
use foreign_types::ForeignType;
use glyph_brush::{ab_glyph::FontArc, BrushAction, BrushError, Extra, GlyphBrush, GlyphBrushBuilder, Section, Text};
use metal::*;
use objc::runtime::{NO, Object, YES};
use objc::{msg_send, sel, sel_impl};
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
use winit::{
    event_loop::EventLoop,
    window::{Window as WinitWindow, WindowBuilder},
};
use crate::ui::vr::{self, EyeViews, Headset, VrInput};
use crate::ui::window::Window;
use crate::ui::voxel::SceneView;
use crate::ui::Color;

const SHADERS: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct MeshIn {
    float3 position [[attribute(0)]];
    float3 color [[attribute(1)]];
};

struct MeshOut {
    float4 position [[position]];
    float3 color;
};

vertex MeshOut mesh_vert(MeshIn in [[stage_in]], constant float4x4 &view_proj [[buffer(1)]]) {
    MeshOut out;
    out.position = view_proj * float4(in.position, 1.0);
    out.color = in.color;
    return out;
}

fragment float4 mesh_frag(MeshOut in [[stage_in]]) {
    return float4(in.color, 1.0);
}

struct ColorIn {
    float2 position [[attribute(0)]];
    float4 color [[attribute(1)]];
};

struct ColorOut {
    float4 position [[position]];
    float4 color;
};

vertex ColorOut color_vert(ColorIn in [[stage_in]], constant float2 &resolution [[buffer(1)]]) {
    float2 unit = in.position / resolution;
    float2 clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    ColorOut out;
    out.position = float4(clip, 0.0, 1.0);
    out.color = in.color;
    return out;
}

fragment float4 color_frag(ColorOut in [[stage_in]]) {
    return in.color;
}

struct TextIn {
    float2 position [[attribute(0)]];
    float2 uv [[attribute(1)]];
    float4 color [[attribute(2)]];
};

struct TextOut {
    float4 position [[position]];
    float2 uv;
    float4 color;
};

vertex TextOut text_vert(TextIn in [[stage_in]], constant float2 &resolution [[buffer(1)]]) {
    float2 unit = in.position / resolution;
    float2 clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    TextOut out;
    out.position = float4(clip, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

fragment float4 text_frag(TextOut in [[stage_in]], texture2d<float> atlas [[texture(0)]]) {
    constexpr sampler samp(filter::linear);
    float coverage = atlas.sample(samp, in.uv).r;
    return float4(in.color.rgb, in.color.a * coverage);
}
"#;

#[derive(Clone, Copy)]
struct GlyphQuad {
    verts: [[f32; 8]; 6],
}

pub struct MetalWindow {
    window: WinitWindow,
    event_loop: Option<EventLoop<()>>,
    device: Device,
    queue: CommandQueue,
    layer: MetalLayer,
    mesh_pipeline: RenderPipelineState,
    color_pipeline: RenderPipelineState,
    text_pipeline: RenderPipelineState,
    depth_write: DepthStencilState,
    depth_off: DepthStencilState,
    depth: Option<Texture>,
    depth_size: (u64, u64),
    mesh: Option<Buffer>,
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
    pub fn try_new() -> Result<Self, String> {
        let device = Device::system_default().ok_or("no metal device")?;
        let queue = device.new_command_queue();
        let library = device
            .new_library_with_source(SHADERS, &CompileOptions::new())
            .map_err(|err| format!("shader: {err}"))?;
        let mesh_vert = library.get_function("mesh_vert", None).map_err(|err| format!("mesh_vert: {err}"))?;
        let mesh_frag = library.get_function("mesh_frag", None).map_err(|err| format!("mesh_frag: {err}"))?;
        let color_vert = library.get_function("color_vert", None).map_err(|err| format!("color_vert: {err}"))?;
        let color_frag = library.get_function("color_frag", None).map_err(|err| format!("color_frag: {err}"))?;
        let text_vert = library.get_function("text_vert", None).map_err(|err| format!("text_vert: {err}"))?;
        let text_frag = library.get_function("text_frag", None).map_err(|err| format!("text_frag: {err}"))?;

        let mesh_pipeline = pipeline(
            &device,
            &mesh_vert,
            &mesh_frag,
            &mesh_vertex_desc(),
            false,
        )?;
        let color_pipeline = pipeline(
            &device,
            &color_vert,
            &color_frag,
            &color_vertex_desc(),
            true,
        )?;
        let text_pipeline = pipeline(
            &device,
            &text_vert,
            &text_frag,
            &text_vertex_desc(),
            true,
        )?;

        let depth_write = depth_state(&device, MTLCompareFunction::Less, true);
        let depth_off = depth_state(&device, MTLCompareFunction::Always, false);

        let event_loop = EventLoop::new().map_err(|err| err.to_string())?;
        let window = WindowBuilder::new()
            .with_title("Starting...")
            .build(&event_loop)
            .map_err(|err| err.to_string())?;

        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        layer.set_framebuffer_only(true);
        layer.set_presents_with_transaction(false);
        layer.set_display_sync_enabled(false);
        layer.set_maximum_drawable_count(3);
        unsafe {
            let _: () = msg_send![layer.as_ptr(), setAllowsNextDrawableTimeout: NO];
        }
        attach_layer(&window, &layer)?;
        resize_layer(&window, &layer);

        let font = FontArc::try_from_slice(include_bytes!("font_default.ttf")).map_err(|err| err.to_string())?;
        let glyphs = GlyphBrushBuilder::using_font(font)
            .initial_cache_size((512, 512))
            .build();
        let atlas = atlas_texture(&device, 512, 512);

        Ok(Self {
            window,
            event_loop: Some(event_loop),
            device,
            queue,
            layer,
            mesh_pipeline,
            color_pipeline,
            text_pipeline,
            depth_write,
            depth_off,
            depth: None,
            depth_size: (0, 0),
            mesh: None,
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
    fn create_window() -> Self {
        Self::try_new().expect("metal")
    }

    fn set_window_title(&mut self, title: &str) {
        self.window.set_title(title);
    }

    fn set_size(&mut self, w: u32, h: u32) {
        let size = winit::dpi::PhysicalSize::new(w, h);
        let _ = self.window.request_inner_size(size);
        resize_layer(&self.window, &self.layer);
    }

    fn winit_window(&self) -> &WinitWindow {
        &self.window
    }

    fn take_event_loop(&mut self) -> EventLoop<()> {
        self.event_loop.take().expect("Event loop missing")
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        self.clear = [red as f64, green as f64, blue as f64, 1.0];
        self.ui_verts.clear();
        self.draw_mesh = false;
    }

    fn draw_colored_mesh(&mut self, vertices: &[f32], revision: u64, view: &SceneView) {
        if !self.mesh_ready || self.mesh_revision != revision {
            self.mesh_vertices = (vertices.len() / 6) as u64;
            self.mesh_revision = revision;
            self.mesh_ready = true;
            self.mesh = shared_buffer(&self.device, float_bytes(vertices));
        }

        self.view_proj = metal_view_proj(view);
        self.eye_views = vr::connect(&mut self.vr, &mut self.vr_failed, self.vr_enable, view);

        if let Some(eyes) = self.eye_views {
            self.view_proj = metal_view_proj(&eyes.views[0]);
        }

        self.draw_mesh = self.mesh_vertices > 0;
    }

    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        push_rect(&mut self.ui_verts, x, y, w, h, color.as_rgba_f32());
    }

    fn draw_outlined_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, thickness: f32, color: Color) {
        self.draw_rectangle(x, y, w, thickness, color);
        self.draw_rectangle(x, y + h - thickness, w, thickness, color);
        self.draw_rectangle(x, y + thickness, thickness, h - 2.0 * thickness, color);
        self.draw_rectangle(x + w - thickness, y + thickness, thickness, h - 2.0 * thickness, color);
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
                        MTLRegion::new_2d(rect.min[0] as u64, rect.min[1] as u64, width as u64, height as u64),
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
        sync_layer(&self.window, &self.layer);
        let drawable_size = self.layer.drawable_size();
        let width = drawable_size.width as u64;
        let height = drawable_size.height as u64;

        if width == 0 || height == 0 {
            return;
        }

        self.render_headset();
        self.ensure_depth(width, height);
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
            color.set_clear_color(MTLClearColor::new(self.clear[0], self.clear[1], self.clear[2], self.clear[3]));

            let depth_attachment = pass.depth_attachment().unwrap();
            depth_attachment.set_texture(Some(depth));
            depth_attachment.set_load_action(MTLLoadAction::Clear);
            depth_attachment.set_store_action(MTLStoreAction::DontCare);
            depth_attachment.set_clear_depth(1.0);
            pass.set_depth_attachment(Some(depth_attachment));

            let command = self.queue.new_command_buffer();
            let encoder = command.new_render_command_encoder(pass);
            let resolution = [width as f32, height as f32];

            if self.draw_mesh {
                if let Some(mesh) = self.mesh.as_ref() {
                    encoder.set_render_pipeline_state(&self.mesh_pipeline);
                    encoder.set_depth_stencil_state(&self.depth_write);
                    encoder.set_cull_mode(MTLCullMode::Back);
                    encoder.set_front_facing_winding(MTLWinding::CounterClockwise);
                    encoder.set_vertex_buffer(0, Some(mesh), 0);
                    encoder.set_vertex_bytes(1, std::mem::size_of::<[f32; 16]>() as u64, self.view_proj.as_ptr() as *const _);
                    encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, self.mesh_vertices);
                }
            }

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

            ([eyes.color[0].clone(), eyes.color[1].clone()], [eyes.depth[0].clone(), eyes.depth[1].clone()], eyes.width, eyes.height)
        };
        let command = self.queue.new_command_buffer();
        objc::rc::autoreleasepool(|| {
            let mut idx = 0;

            while idx < 2 {
                let matrix = metal_view_proj(&frame.views[idx]);
                let pass = RenderPassDescriptor::new();
                let attachment = pass.color_attachments().object_at(0).unwrap();
                attachment.set_texture(Some(&color[idx]));
                attachment.set_load_action(MTLLoadAction::Clear);
                attachment.set_store_action(MTLStoreAction::Store);
                attachment.set_clear_color(MTLClearColor::new(self.clear[0], self.clear[1], self.clear[2], self.clear[3]));
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
                    if let Some(mesh) = self.mesh.as_ref() {
                        encoder.set_render_pipeline_state(&self.mesh_pipeline);
                        encoder.set_depth_stencil_state(&self.depth_write);
                        encoder.set_cull_mode(MTLCullMode::Back);
                        encoder.set_front_facing_winding(MTLWinding::CounterClockwise);
                        encoder.set_vertex_buffer(0, Some(mesh), 0);
                        encoder.set_vertex_bytes(1, std::mem::size_of::<[f32; 16]>() as u64, matrix.as_ptr() as *const _);
                        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, self.mesh_vertices);
                    }
                }

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
            color: [eye_color(&self.device, width, height), eye_color(&self.device, width, height)],
            depth: [eye_depth(&self.device, width, height), eye_depth(&self.device, width, height)],
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
fn attach_layer(window: &WinitWindow, layer: &MetalLayer) -> Result<(), String> {
    let handle = window.raw_window_handle();
    let RawWindowHandle::AppKit(appkit) = handle else {
        return Err("window is not appkit".to_string());
    };

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

fn resize_layer(window: &WinitWindow, layer: &MetalLayer) {
    sync_layer(window, layer);
}

fn sync_layer(window: &WinitWindow, layer: &MetalLayer) {
    let size = window.inner_size();
    let scale = window.scale_factor();
    let drawable = layer.drawable_size();
    let same_size = drawable.width == size.width as f64 && drawable.height == size.height as f64;
    let same_scale = layer.contents_scale() == scale;

    if same_size && same_scale {
        return;
    }

    layer.set_contents_scale(scale);
    layer.set_drawable_size(CGSize::new(size.width as f64, size.height as f64));
}

fn bind_bytes(
    encoder: &RenderCommandEncoderRef,
    device: &DeviceRef,
    pipeline: &RenderPipelineStateRef,
    depth: &DepthStencilStateRef,
    bytes: &[u8],
    vertices: usize,
    resolution: &[f32; 2],
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
    encoder.set_vertex_bytes(1, std::mem::size_of::<[f32; 2]>() as u64, resolution.as_ptr() as *const _);

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

fn mesh_vertex_desc() -> &'static VertexDescriptorRef {
    let desc = VertexDescriptor::new();
    set_attr(desc, 0, MTLVertexFormat::Float3, 0);
    set_attr(desc, 1, MTLVertexFormat::Float3, 12);
    set_layout(desc, 24);

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
        [vertex.pixel_coords.min.x, vertex.pixel_coords.min.y, vertex.tex_coords.min.x, vertex.tex_coords.min.y],
        [vertex.pixel_coords.max.x, vertex.pixel_coords.min.y, vertex.tex_coords.max.x, vertex.tex_coords.min.y],
        [vertex.pixel_coords.min.x, vertex.pixel_coords.max.y, vertex.tex_coords.min.x, vertex.tex_coords.max.y],
        [vertex.pixel_coords.min.x, vertex.pixel_coords.max.y, vertex.tex_coords.min.x, vertex.tex_coords.max.y],
        [vertex.pixel_coords.max.x, vertex.pixel_coords.min.y, vertex.tex_coords.max.x, vertex.tex_coords.min.y],
        [vertex.pixel_coords.max.x, vertex.pixel_coords.max.y, vertex.tex_coords.max.x, vertex.tex_coords.max.y],
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
    #[cfg(target_arch = "aarch64")]
    desc.set_storage_mode(MTLStorageMode::Shared);
    #[cfg(not(target_arch = "aarch64"))]
    desc.set_storage_mode(MTLStorageMode::Managed);

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

