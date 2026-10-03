use crate::platform::Surface;
use crate::ui::gfx::{self, BackendGpu, Store};
#[cfg(not(target_os = "ios"))]
use crate::ui::opengl::OpenGLWindow;
use crate::ui::skin::SkinBatch;
use crate::ui::voxel::SceneView;
#[cfg(not(target_os = "ios"))]
use crate::ui::vulkan::VulkanWindow;
use crate::ui::window::Window;
use crate::ui::Color;
use crate::world::surface::{CpuImage, PixelFormat};

pub struct GfxWindow {
    gpu: Store,
    backend: Backend,
}

enum Backend {
    #[cfg(not(target_os = "ios"))]
    OpenGL(OpenGLWindow),
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    Metal(crate::ui::metal::MetalWindow),
    #[cfg(windows)]
    D3D12(crate::ui::d3d::D3D12Window),
    #[cfg(windows)]
    D3D11(crate::ui::d3d::D3D11Window),
    #[cfg(not(target_os = "ios"))]
    Vulkan(VulkanWindow),
}

fn host(backend: Backend) -> GfxWindow {
    let mut window = GfxWindow {
        gpu: Store::new(),
        backend,
    };
    window.seed();

    window
}

pub fn create(surface: &Surface) -> GfxWindow {
    pick(surface, true)
}

pub fn create_tool(surface: &Surface) -> GfxWindow {
    pick(surface, std::env::var("ENGINE_GFX").is_ok())
}

#[allow(unused_variables)]
fn pick(surface: &Surface, allow_d3d12: bool) -> GfxWindow {
    #[cfg(target_os = "ios")]
    {
        log::info!("[gfx] metal");

        return host(Backend::Metal(
            crate::ui::metal::MetalWindow::try_new(surface).expect("metal"),
        ));
    }

    #[cfg(target_os = "macos")]
    if chosen("metal") {
        match crate::ui::metal::MetalWindow::try_new(surface) {
            Ok(window) => {
                log::info!("[gfx] metal");

                return host(Backend::Metal(window));
            }
            Err(err) => {
                log::warn!("[gfx] metal failed: {err}");
            }
        }
    }

    #[cfg(windows)]
    if allow_d3d12 && chosen("d3d12") {
        match crate::ui::d3d::D3D12Window::try_new(surface) {
            Ok(window) => {
                log::info!("[gfx] d3d12");

                return host(Backend::D3D12(window));
            }
            Err(err) => {
                log::warn!("[gfx] d3d12 failed: {err}");
            }
        }
    }

    #[cfg(windows)]
    if chosen("d3d11") {
        match crate::ui::d3d::D3D11Window::try_new(surface) {
            Ok(window) => {
                log::info!("[gfx] d3d11");

                return host(Backend::D3D11(window));
            }
            Err(err) => {
                log::warn!("[gfx] d3d11 failed: {err}");
            }
        }
    }

    #[cfg(not(target_os = "ios"))]
    if chosen("vulkan") {
        match VulkanWindow::try_new(surface) {
            Ok(window) => {
                log::info!("[gfx] vulkan");

                return host(Backend::Vulkan(window));
            }
            Err(err) => {
                log::warn!("[gfx] vulkan failed: {err}");
            }
        }
    }

    #[cfg(not(target_os = "ios"))]
    {
        log::info!("[gfx] opengl");

        return host(Backend::OpenGL(OpenGLWindow::attach(surface)));
    }
}

#[cfg(target_os = "android")]
pub fn android_window(surface: &Surface) -> GfxWindow {
    log::info!("[gfx] opengl es");
    let mut window = OpenGLWindow::attach(surface);
    window.enable_vr();

    host(Backend::OpenGL(window))
}

fn chosen(name: &str) -> bool {
    match std::env::var("ENGINE_GFX") {
        Ok(value) => value.eq_ignore_ascii_case(name),
        Err(_) => true,
    }
}

macro_rules! each_window {
    ($self:ident, |$window:ident| $body:expr) => {
        match &mut $self.backend {
            #[cfg(not(target_os = "ios"))]
            Backend::OpenGL($window) => $body,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            Backend::Metal($window) => $body,
            #[cfg(windows)]
            Backend::D3D12($window) => $body,
            #[cfg(windows)]
            Backend::D3D11($window) => $body,
            #[cfg(not(target_os = "ios"))]
            Backend::Vulkan($window) => $body,
        }
    };
}

impl Window for GfxWindow {
    fn attach(surface: &Surface) -> Self {
        create(surface)
    }

    fn set_size(&mut self, w: u32, h: u32) {
        each_window!(self, |window| window.set_size(w, h))
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        each_window!(self, |window| window.begin_frame(red, green, blue))
    }

    fn draw_colored_mesh(
        &mut self,
        vertices: &[f32],
        ranges: &[crate::world::SurfaceRange],
        graphics: &crate::world::MapGraphics,
        revision: u64,
        view: &SceneView,
    ) {
        each_window!(self, |window| window
            .draw_colored_mesh(vertices, ranges, graphics, revision, view))
    }

    fn draw_skinned(&mut self, batch: &SkinBatch, view: &SceneView) {
        each_window!(self, |window| window.draw_skinned(batch, view))
    }

    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        each_window!(self, |window| window.draw_rectangle(x, y, w, h, color))
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
        each_window!(self, |window| window
            .draw_outlined_rectangle(x, y, w, h, thickness, color))
    }

    fn draw_text(&mut self, font: &str, text: &str, x: f32, y: f32, scale: f32, color: Color) {
        each_window!(self, |window| window
            .draw_text(font, text, x, y, scale, color))
    }

    fn set_scissor(&mut self, rect: Option<[f32; 4]>) {
        each_window!(self, |window| window.set_scissor(rect))
    }

    fn render_text(&mut self) {
        each_window!(self, |window| window.render_text())
    }

    fn present(&mut self) {
        each_window!(self, |window| window.present())
    }

    fn enable_vr(&mut self) {
        each_window!(self, |window| window.enable_vr())
    }

    fn vr_input(&self) -> crate::ui::vr::VrInput {
        match &self.backend {
            #[cfg(not(target_os = "ios"))]
            Backend::OpenGL(window) => window.vr_input(),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            Backend::Metal(window) => window.vr_input(),
            #[cfg(windows)]
            Backend::D3D12(window) => window.vr_input(),
            #[cfg(windows)]
            Backend::D3D11(window) => window.vr_input(),
            #[cfg(not(target_os = "ios"))]
            Backend::Vulkan(window) => window.vr_input(),
        }
    }
}

impl GfxWindow {
    fn with_backend<R>(&mut self, body: impl FnOnce(&mut dyn BackendGpu) -> R) -> R {
        match &mut self.backend {
            #[cfg(not(target_os = "ios"))]
            Backend::OpenGL(window) => body(window),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            Backend::Metal(window) => body(window),
            #[cfg(windows)]
            Backend::D3D12(window) => body(window),
            #[cfg(windows)]
            Backend::D3D11(window) => body(window),
            #[cfg(not(target_os = "ios"))]
            Backend::Vulkan(window) => body(window),
        }
    }

    fn seed(&mut self) {
        let mut index = 1;

        while index <= gfx::IDX_SKINNED {
            let id = gfx::pack(gfx::KIND_SHADER, 1, index);

            if let Some(shader) = self.with_backend(|window| window.builtin_shader(index)) {
                let owned = true;
                self.gpu.put_shader(id, shader, owned, true);
            }

            index += 1;
        }

        index = 1;

        while index <= gfx::IDX_SKINNED {
            let id = gfx::pack(gfx::KIND_PIPELINE, 1, index);

            if let Some(pipeline) = self.with_backend(|window| window.builtin_pipeline(index)) {
                let screen = index == gfx::IDX_COLOR || index == gfx::IDX_TEXT;
                let stride = if index == gfx::IDX_TEXT {
                    gfx::SCREEN_FLOATS as u8
                } else if index == gfx::IDX_COLOR {
                    6
                } else {
                    0
                };
                self.gpu
                    .put_pipeline(id, pipeline, false, true, screen, stride);
            }

            index += 1;
        }

        index = 1;

        while index <= gfx::IDX_FLAT {
            let id = gfx::pack(gfx::KIND_TEXTURE, 1, index);

            if let Some(texture) = self.with_backend(|window| window.builtin_texture(index)) {
                self.gpu.put_texture(id, texture, false, true, None);
            }

            index += 1;
        }

        index = 1;

        while index <= gfx::IDX_CLAMP {
            let id = gfx::pack(gfx::KIND_SAMPLER, 1, index);

            if let Some(sampler) = self.with_backend(|window| window.builtin_sampler(index)) {
                self.gpu.put_sampler(id, sampler, false, true);
            }

            index += 1;
        }
    }

    pub fn create_shader(&mut self, id: u32, source: &str) {
        match self.with_backend(|window| window.make_shader(source)) {
            Ok(shader) => self.gpu.put_shader(id, shader, true, false),
            Err(err) => log::warn!("[gfx] shader {err}"),
        }
    }

    pub fn create_texture(&mut self, id: u32, path: &str) {
        let image = match gfx::image_file(path) {
            Ok(image) => image,
            Err(err) => {
                log::warn!("[gfx] texture {err}");

                return;
            }
        };

        match self.with_backend(|window| window.make_texture(&image)) {
            Ok(texture) => self.gpu.put_texture(id, texture, true, false, None),
            Err(err) => log::warn!("[gfx] texture {err}"),
        }
    }

    pub fn create_image(&mut self, id: u32, image: &CpuImage) {
        match self.with_backend(|window| window.make_texture(image)) {
            Ok(texture) => self.gpu.put_texture(id, texture, true, false, None),
            Err(err) => log::warn!("[gfx] image {err}"),
        }
    }

    pub fn create_rgba(&mut self, id: u32, width: u32, height: u32, bytes: Vec<u8>) {
        let Some(image) = rgba_image(width, height, bytes) else {
            log::warn!("[gfx] image");

            return;
        };

        self.create_image(id, &image);
    }

    pub fn draw_screen(&mut self, verts: &[f32], texture: u32, sampler: u32) {
        let texture_item = if texture == 0 {
            self.resolve_texture(gfx::TEX_WHITE)
        } else {
            self.resolve_texture(texture)
        };
        let sampler_item = self.resolve_sampler(sampler);

        self.with_backend(|window| {
            window.draw_screen(verts, texture_item.as_ref(), sampler_item.as_ref());
        });
    }

    pub fn create_material(&mut self, id: u32, name: &str) {
        let alias = self.with_backend(|window| window.material_alias(name));

        if let Some(texture) = alias {
            self.gpu
                .put_texture(id, texture, false, false, Some(name.to_string()));

            return;
        }

        let Some(image) = gfx::material_image(name) else {
            log::warn!("[gfx] material {name}");

            return;
        };

        match self.with_backend(|window| window.make_texture(&image)) {
            Ok(texture) => self.gpu.put_texture(id, texture, true, false, None),
            Err(err) => log::warn!("[gfx] material {err}"),
        }
    }

    pub fn create_target(&mut self, id: u32, width: u32, height: u32) {
        match self.with_backend(|window| window.make_target(width, height)) {
            Ok(target) => self.gpu.put_target(id, target, true),
            Err(err) => log::warn!("[gfx] target {err}"),
        }
    }

    pub fn create_buffer(&mut self, id: u32, bytes: &[u8]) {
        match self.with_backend(|window| window.make_buffer(bytes)) {
            Ok(buffer) => self.gpu.put_buffer(id, buffer, true),
            Err(err) => log::warn!("[gfx] buffer {err}"),
        }
    }

    pub fn create_sampler(&mut self, id: u32, linear: bool, repeat: bool) {
        match self.with_backend(|window| window.make_sampler(linear, repeat)) {
            Ok(sampler) => self.gpu.put_sampler(id, sampler, true, false),
            Err(err) => log::warn!("[gfx] sampler {err}"),
        }
    }

    pub fn create_pipeline(&mut self, id: u32, shader: u32, screen: bool) {
        let Some(source) = self.gpu.shader(shader).cloned() else {
            log::warn!("[gfx] pipeline");

            return;
        };
        let stride = if screen {
            gfx::SCREEN_FLOATS as u8
        } else {
            gfx::MESH_FLOATS as u8
        };

        match self.with_backend(|window| window.make_pipeline(&source, screen)) {
            Ok(pipeline) => self
                .gpu
                .put_pipeline(id, pipeline, true, false, screen, stride),
            Err(err) => log::warn!("[gfx] pipeline {err}"),
        }
    }

    pub fn create_mesh(&mut self, id: u32, verts: &[f32], screen: bool) {
        match self.with_backend(|window| window.make_mesh(verts, screen)) {
            Ok(mesh) => self.gpu.put_mesh(id, mesh, true, screen),
            Err(err) => log::warn!("[gfx] mesh {err}"),
        }
    }

    pub fn free_gpu(&mut self, id: u32) {
        if gfx::is_builtin(id) {
            return;
        }

        match gfx::kind_of(id) {
            gfx::KIND_SHADER => {
                if let Some(slot) = self.gpu.take_shader(id) {
                    self.release_shader(slot);
                }
            }
            gfx::KIND_TEXTURE => {
                if let Some(slot) = self.gpu.take_texture(id) {
                    self.release_texture(slot);
                }
            }
            gfx::KIND_BUFFER => {
                if let Some(slot) = self.gpu.take_buffer(id) {
                    self.release_buffer(slot);
                }
            }
            gfx::KIND_SAMPLER => {
                if let Some(slot) = self.gpu.take_sampler(id) {
                    self.release_sampler(slot);
                }
            }
            gfx::KIND_PIPELINE => {
                if let Some(slot) = self.gpu.take_pipeline(id) {
                    self.release_pipeline(slot);
                }
            }
            gfx::KIND_TARGET => {
                if let Some(slot) = self.gpu.take_target(id) {
                    self.release_target(slot);
                }
            }
            gfx::KIND_MESH => {
                if let Some(slot) = self.gpu.take_mesh(id) {
                    self.release_mesh(slot);
                }
            }
            _ => {}
        }
    }

    pub fn draw_mesh(
        &mut self,
        mesh: u32,
        pipeline: u32,
        texture: u32,
        sampler: u32,
        view: &SceneView,
    ) {
        if gfx::kind_of(mesh) == gfx::KIND_BUFFER {
            self.draw_buffer(mesh, pipeline, texture, sampler, view);

            return;
        }

        let mesh_screen = self.gpu.mesh_slot(mesh).map(|slot| slot.screen);
        let pipe_screen = self.gpu.pipeline_slot(pipeline).map(|slot| slot.screen);
        let (Some(mesh_screen), Some(pipe_screen)) = (mesh_screen, pipe_screen) else {
            return;
        };

        if mesh_screen != pipe_screen {
            return;
        }

        let mesh_item = self.gpu.mesh(mesh).cloned();
        let pipe_item = self.gpu.pipeline(pipeline).cloned();
        let texture_item = self.resolve_texture(texture);
        let sampler_item = self.resolve_sampler(sampler);
        let (Some(mesh_item), Some(pipe_item)) = (mesh_item, pipe_item) else {
            return;
        };

        self.with_backend(|window| {
            window.draw_mesh(
                &mesh_item,
                &pipe_item,
                texture_item.as_ref(),
                sampler_item.as_ref(),
                view,
            );
        });
    }

    pub fn draw_sprite(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
        texture: u32,
        pipeline: u32,
        sampler: u32,
    ) {
        let pipe_item = if pipeline == 0 {
            self.gpu.pipeline(gfx::PIPE_TEXT).cloned()
        } else {
            let screen = self.gpu.pipeline_slot(pipeline).map(|slot| slot.screen);

            if screen == Some(false) {
                return;
            }

            self.gpu.pipeline(pipeline).cloned()
        };
        let texture_item = if texture == 0 {
            self.resolve_texture(gfx::TEX_WHITE)
        } else {
            self.resolve_texture(texture)
        };
        let sampler_item = self.resolve_sampler(sampler);

        self.with_backend(|window| {
            window.draw_sprite(
                x,
                y,
                w,
                h,
                color.as_rgba_f32(),
                texture_item.as_ref(),
                pipe_item.as_ref(),
                sampler_item.as_ref(),
            );
        });
    }

    pub fn draw_text_user(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: Color,
        texture: u32,
        pipeline: u32,
        sampler: u32,
    ) {
        let pipe_item = if pipeline == 0 {
            self.gpu.pipeline(gfx::PIPE_TEXT).cloned()
        } else {
            self.gpu.pipeline(pipeline).cloned()
        };
        let texture_item = if texture == 0 {
            None
        } else {
            self.resolve_texture(texture)
        };
        let sampler_item = self.resolve_sampler(sampler);

        self.with_backend(|window| {
            window.draw_text_user(
                text,
                x,
                y,
                scale,
                color.as_rgba_f32(),
                texture_item.as_ref(),
                pipe_item.as_ref(),
                sampler_item.as_ref(),
            );
        });
    }

    pub fn set_target(&mut self, id: u32) {
        if id == 0 || gfx::kind_of(id) != gfx::KIND_TARGET {
            self.with_backend(|window| window.set_target(None));

            return;
        }

        let target = self.gpu.target(id).cloned();
        self.with_backend(|window| window.set_target(target.as_ref()));
    }

    pub fn target_bound(&mut self) -> bool {
        self.with_backend(|window| window.target_bound())
    }

    pub fn update_buffer(&mut self, id: u32, bytes: &[u8]) {
        let Some(slot) = self.gpu.take_buffer(id) else {
            return;
        };
        let owned = slot.owned;
        let item = self.with_backend(|window| window.update_buffer(slot.item, bytes));
        self.gpu.put_buffer(id, item, owned);
    }

    pub fn update_mesh(&mut self, id: u32, verts: &[f32]) {
        let Some(slot) = self.gpu.take_mesh(id) else {
            return;
        };
        let owned = slot.owned;
        let screen = slot.screen;
        let item = self.with_backend(|window| window.update_mesh(slot.item, verts));
        self.gpu.put_mesh(id, item, owned, screen);
    }

    pub fn update_image(&mut self, id: u32, image: &CpuImage) {
        let Some(slot) = self.gpu.take_texture(id) else {
            return;
        };
        let owned = slot.owned;
        let builtin = slot.builtin;
        let material = slot.material;
        let item = self.with_backend(|window| window.update_texture(slot.item, image));
        self.gpu.put_texture(id, item, owned, builtin, material);
    }

    pub fn update_rgba(&mut self, id: u32, width: u32, height: u32, bytes: Vec<u8>) {
        let Some(image) = rgba_image(width, height, bytes) else {
            log::warn!("[gfx] image");

            return;
        };

        self.update_image(id, &image);
    }

    pub fn update_texture(&mut self, id: u32, path: &str) {
        let image = match gfx::image_file(path) {
            Ok(image) => image,
            Err(err) => {
                log::warn!("[gfx] texture {err}");

                return;
            }
        };
        let Some(slot) = self.gpu.take_texture(id) else {
            return;
        };
        let owned = slot.owned;
        let builtin = slot.builtin;
        let material = slot.material;
        let item = self.with_backend(|window| window.update_texture(slot.item, &image));
        self.gpu.put_texture(id, item, owned, builtin, material);
    }

    pub fn update_target(&mut self, id: u32, width: u32, height: u32) {
        let Some(slot) = self.gpu.take_target(id) else {
            return;
        };
        let owned = slot.owned;

        match self.with_backend(|window| window.resize_target(slot.item, width, height)) {
            Ok(target) => self.gpu.put_target(id, target, owned),
            Err(err) => log::warn!("[gfx] target {err}"),
        }
    }

    fn draw_buffer(
        &mut self,
        buffer: u32,
        pipeline: u32,
        texture: u32,
        sampler: u32,
        view: &SceneView,
    ) {
        let buffer_item = self.gpu.buffer(buffer).cloned();
        let pipe_item = self.gpu.pipeline(pipeline).cloned();
        let texture_item = self.resolve_texture(texture);
        let sampler_item = self.resolve_sampler(sampler);
        let (Some(buffer_item), Some(pipe_item)) = (buffer_item, pipe_item) else {
            return;
        };

        self.with_backend(|window| {
            window.draw_buffer(
                &buffer_item,
                &pipe_item,
                texture_item.as_ref(),
                sampler_item.as_ref(),
                view,
            );
        });
    }

    fn resolve_texture(&mut self, id: u32) -> Option<gfx::Texture> {
        if id == 0 {
            return None;
        }

        if gfx::kind_of(id) == gfx::KIND_TARGET {
            let target = self.gpu.target(id).cloned()?;

            return self.with_backend(|window| window.target_color(&target));
        }

        if gfx::is_builtin(id) {
            let index = gfx::index_of(id);

            return self.with_backend(|window| window.builtin_texture(index));
        }

        if let Some(name) = self
            .gpu
            .texture_slot(id)
            .and_then(|slot| slot.material.clone())
        {
            if let Some(texture) = self.with_backend(|window| window.material_alias(&name)) {
                return Some(texture);
            }
        }

        self.gpu.texture(id).cloned()
    }

    fn resolve_sampler(&mut self, id: u32) -> Option<gfx::Sampler> {
        if id == 0 {
            return self.with_backend(|window| window.builtin_sampler(gfx::IDX_CLAMP));
        }

        if gfx::is_builtin(id) {
            let index = gfx::index_of(id);

            return self.with_backend(|window| window.builtin_sampler(index));
        }

        self.gpu.sampler(id).cloned()
    }

    fn release_shader(&mut self, slot: gfx::Slot<gfx::Shader>) {
        if slot.owned {
            self.with_backend(|window| window.destroy_shader(slot.item));
        }
    }

    fn release_texture(&mut self, slot: gfx::Slot<gfx::Texture>) {
        if slot.owned {
            self.with_backend(|window| window.destroy_texture(slot.item));
        }
    }

    fn release_buffer(&mut self, slot: gfx::Slot<gfx::Buffer>) {
        if slot.owned {
            self.with_backend(|window| window.destroy_buffer(slot.item));
        }
    }

    fn release_sampler(&mut self, slot: gfx::Slot<gfx::Sampler>) {
        if slot.owned {
            self.with_backend(|window| window.destroy_sampler(slot.item));
        }
    }

    fn release_pipeline(&mut self, slot: gfx::Slot<gfx::Pipeline>) {
        if slot.owned {
            self.with_backend(|window| window.destroy_pipeline(slot.item));
        }
    }

    fn release_target(&mut self, slot: gfx::Slot<gfx::Target>) {
        if slot.owned {
            self.with_backend(|window| window.destroy_target(slot.item));
        }
    }

    fn release_mesh(&mut self, slot: gfx::Slot<gfx::Mesh>) {
        if slot.owned {
            self.with_backend(|window| window.destroy_mesh(slot.item));
        }
    }
}

impl Drop for GfxWindow {
    fn drop(&mut self) {
        self.with_backend(|window| window.before_destroy());
        let mut shaders = self.gpu.drain_shaders();
        let mut textures = self.gpu.drain_textures();
        let mut buffers = self.gpu.drain_buffers();
        let mut samplers = self.gpu.drain_samplers();
        let mut pipelines = self.gpu.drain_pipelines();
        let mut targets = self.gpu.drain_targets();
        let mut meshes = self.gpu.drain_meshes();

        while let Some(slot) = shaders.pop() {
            self.release_shader(slot);
        }

        while let Some(slot) = textures.pop() {
            self.release_texture(slot);
        }

        while let Some(slot) = buffers.pop() {
            self.release_buffer(slot);
        }

        while let Some(slot) = samplers.pop() {
            self.release_sampler(slot);
        }

        while let Some(slot) = pipelines.pop() {
            self.release_pipeline(slot);
        }

        while let Some(slot) = targets.pop() {
            self.release_target(slot);
        }

        while let Some(slot) = meshes.pop() {
            self.release_mesh(slot);
        }
    }
}

fn rgba_image(width: u32, height: u32, bytes: Vec<u8>) -> Option<CpuImage> {
    if width == 0 || height == 0 {
        return None;
    }

    let need = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;

    if bytes.len() != need {
        return None;
    }

    Some(CpuImage {
        width,
        height,
        format: PixelFormat::Rgba8,
        bytes,
        mips: Vec::new(),
    })
}
