use crate::platform::Surface;
#[cfg(not(target_os = "ios"))]
use crate::ui::opengl::OpenGLWindow;
use crate::ui::skin::SkinBatch;
use crate::ui::voxel::SceneView;
#[cfg(not(target_os = "ios"))]
use crate::ui::vulkan::VulkanWindow;
use crate::ui::window::Window;
use crate::ui::Color;

pub enum GfxWindow {
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

pub fn create(surface: &Surface) -> GfxWindow {
    #[cfg(target_os = "ios")]
    {
        log::info!("[gfx] metal");

        return GfxWindow::Metal(crate::ui::metal::MetalWindow::try_new(surface).expect("metal"));
    }

    #[cfg(target_os = "macos")]
    if chosen("metal") {
        match crate::ui::metal::MetalWindow::try_new(surface) {
            Ok(window) => {
                log::info!("[gfx] metal");

                return GfxWindow::Metal(window);
            }
            Err(err) => {
                log::warn!("[gfx] metal failed: {err}");
            }
        }
    }

    #[cfg(windows)]
    if chosen("d3d12") {
        match crate::ui::d3d::D3D12Window::try_new(surface) {
            Ok(window) => {
                log::info!("[gfx] d3d12");

                return GfxWindow::D3D12(window);
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

                return GfxWindow::D3D11(window);
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

                return GfxWindow::Vulkan(window);
            }
            Err(err) => {
                log::warn!("[gfx] vulkan failed: {err}");
            }
        }
    }

    #[cfg(not(target_os = "ios"))]
    {
        log::info!("[gfx] opengl");

        return GfxWindow::OpenGL(OpenGLWindow::attach(surface));
    }
}

#[cfg(target_os = "android")]
pub fn android_window(surface: &Surface) -> GfxWindow {
    log::info!("[gfx] opengl es");
    let mut window = OpenGLWindow::attach(surface);
    window.enable_vr();

    GfxWindow::OpenGL(window)
}

fn chosen(name: &str) -> bool {
    match std::env::var("ENGINE_GFX") {
        Ok(value) => value.eq_ignore_ascii_case(name),
        Err(_) => true,
    }
}

macro_rules! each_window {
    ($self:ident, |$window:ident| $body:expr) => {
        match $self {
            #[cfg(not(target_os = "ios"))]
            GfxWindow::OpenGL($window) => $body,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            GfxWindow::Metal($window) => $body,
            #[cfg(windows)]
            GfxWindow::D3D12($window) => $body,
            #[cfg(windows)]
            GfxWindow::D3D11($window) => $body,
            #[cfg(not(target_os = "ios"))]
            GfxWindow::Vulkan($window) => $body,
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

    fn draw_colored_mesh(&mut self, vertices: &[f32], revision: u64, view: &SceneView) {
        each_window!(self, |window| window
            .draw_colored_mesh(vertices, revision, view))
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
        each_window!(self, |window| window.vr_input())
    }
}
