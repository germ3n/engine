use crate::ui::Color;
use crate::ui::voxel::SceneView;

pub trait Window {
    fn create_window() -> Self where Self: Sized;
    fn set_window_title(&mut self, title: &str);
    fn set_size(&mut self, w: u32, h: u32);
    fn winit_window(&self) -> &winit::window::Window;
    fn take_event_loop(&mut self) -> winit::event_loop::EventLoop<()>;
    fn begin_frame(&mut self, red: f32, green: f32, blue: f32);
    fn draw_colored_mesh(&mut self, vertices: &[f32], revision: u64, view: &SceneView);
    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color);
    fn draw_outlined_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, thickness: f32, color: Color);
    fn draw_text(&mut self, font: &str, text: &str, x: f32, y: f32, scale: f32, color: Color);
    fn render_text(&mut self);
    fn present(&mut self);
}
