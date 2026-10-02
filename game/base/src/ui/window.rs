use crate::platform::Surface;
use crate::ui::skin::SkinBatch;
use crate::ui::voxel::SceneView;
use crate::ui::vr::VrInput;
use crate::ui::Color;
use crate::world::{MapGraphics, SurfaceRange};

pub trait Window {
    fn attach(surface: &Surface) -> Self
    where
        Self: Sized;
    fn set_size(&mut self, w: u32, h: u32);
    fn begin_frame(&mut self, red: f32, green: f32, blue: f32);
    fn draw_colored_mesh(
        &mut self,
        vertices: &[f32],
        ranges: &[SurfaceRange],
        graphics: &MapGraphics,
        revision: u64,
        view: &SceneView,
    );
    fn draw_skinned(&mut self, _batch: &SkinBatch, _view: &SceneView) {}
    fn draw_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color);
    fn draw_outlined_rectangle(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        thickness: f32,
        color: Color,
    );
    fn draw_text(&mut self, font: &str, text: &str, x: f32, y: f32, scale: f32, color: Color);
    fn render_text(&mut self);
    fn present(&mut self);
    fn enable_vr(&mut self);
    fn vr_input(&self) -> VrInput;
}
