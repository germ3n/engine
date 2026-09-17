#[derive(Debug, Clone, Copy)]
pub enum Color {
    ColorRGBA { r: u8, g: u8, b: u8, a: u8 },
    ColorRGBAf { r: f32, g: f32, b: f32, a: f32 }
}

impl Color {
    pub fn as_rgba_f32(self) -> [f32; 4] {
        match self {
            Color::ColorRGBA { r, g, b, a } => {
                [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
            }
            Color::ColorRGBAf { r, g, b, a } => [r, g, b, a],
        }
    }
}