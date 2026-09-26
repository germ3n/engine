use crate::ui::opengl::OpenGLWindow;
use crate::ui::window::Window;
use crate::ui::Color;
use crate::ui::voxel::SceneView;

pub enum GfxWindow {
    OpenGL(OpenGLWindow),
    #[cfg(target_os = "macos")]
    Metal(crate::ui::metal::MetalWindow),
}

pub fn create() -> GfxWindow {
    #[cfg(target_os = "macos")]
    {
        match crate::ui::metal::MetalWindow::try_new() {
            Ok(window) => {
                println!("[gfx] metal");

                return GfxWindow::Metal(window);
            }
            Err(err) => {
                println!("[gfx] metal failed: {err}");
            }
        }
    }

    println!("[gfx] opengl");

    GfxWindow::OpenGL(OpenGLWindow::create_window())
}

impl Window for GfxWindow {
    fn create_window() -> Self {
        create()
    }

    fn set_window_title(&mut self, title: &str) {
        match self {
            GfxWindow::OpenGL(window) => window.set_window_title(title),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.set_window_title(title),
        }
    }

    fn set_size(&mut self, w: u32, h: u32) {
        match self {
            GfxWindow::OpenGL(window) => window.set_size(w, h),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.set_size(w, h),
        }
    }

    fn winit_window(&self) -> &winit::window::Window {
        match self {
            GfxWindow::OpenGL(window) => window.winit_window(),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.winit_window(),
        }
    }

    fn take_event_loop(&mut self) -> winit::event_loop::EventLoop<()> {
        match self {
            GfxWindow::OpenGL(window) => window.take_event_loop(),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.take_event_loop(),
        }
    }

    fn begin_frame(&mut self, red: f32, green: f32, blue: f32) {
        match self {
            GfxWindow::OpenGL(window) => window.begin_frame(red, green, blue),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.begin_frame(red, green, blue),
        }
    }

    fn draw_colored_mesh(&mut self, vertices: &[f32], revision: u64, view: &SceneView) {
        match self {
            GfxWindow::OpenGL(window) => window.draw_colored_mesh(vertices, revision, view),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.draw_colored_mesh(vertices, revision, view),
        }
    }

    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        match self {
            GfxWindow::OpenGL(window) => window.draw_rectangle(x, y, w, h, color),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.draw_rectangle(x, y, w, h, color),
        }
    }

    fn draw_outlined_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, thickness: f32, color: Color) {
        match self {
            GfxWindow::OpenGL(window) => window.draw_outlined_rectangle(x, y, w, h, thickness, color),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.draw_outlined_rectangle(x, y, w, h, thickness, color),
        }
    }

    fn draw_text(&mut self, font: &str, text: &str, x: f32, y: f32, scale: f32, color: Color) {
        match self {
            GfxWindow::OpenGL(window) => window.draw_text(font, text, x, y, scale, color),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.draw_text(font, text, x, y, scale, color),
        }
    }

    fn render_text(&mut self) {
        match self {
            GfxWindow::OpenGL(window) => window.render_text(),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.render_text(),
        }
    }

    fn present(&mut self) {
        match self {
            GfxWindow::OpenGL(window) => window.present(),
            #[cfg(target_os = "macos")]
            GfxWindow::Metal(window) => window.present(),
        }
    }
}
