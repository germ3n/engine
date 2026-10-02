use glyph_brush::ab_glyph::FontArc;
use glyph_brush::{BrushAction, BrushError, Extra, GlyphBrush, GlyphBrushBuilder, Section, Text};

pub fn bytes_of(values: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr() as *const u8, values.len() * 4) }
}

pub fn push_rect(verts: &mut Vec<f32>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
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

pub fn push_outline(
    verts: &mut Vec<f32>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    thickness: f32,
    color: [f32; 4],
) {
    push_rect(verts, x, y, w, thickness, color);
    push_rect(verts, x, y + h - thickness, w, thickness, color);
    push_rect(
        verts,
        x,
        y + thickness,
        thickness,
        h - 2.0 * thickness,
        color,
    );
    push_rect(
        verts,
        x + w - thickness,
        y + thickness,
        thickness,
        h - 2.0 * thickness,
        color,
    );
}

pub fn grow(current: u32, needed: u32) -> u32 {
    let mut size = current.max(256);

    while size < needed {
        size = size.saturating_mul(2);
    }

    size
}

pub fn grow64(current: u64, needed: u64) -> u64 {
    let mut size = current.max(256);

    while size < needed {
        size = size.saturating_mul(2);
    }

    size
}

#[derive(Clone, Copy)]
struct GlyphQuad {
    verts: [[f32; 8]; 6],
}

pub struct TextFrame {
    glyphs: GlyphBrush<GlyphQuad>,
    pub verts: Vec<f32>,
    pub pixels: Vec<u8>,
    pub size: (u32, u32),
    pub dirty: bool,
}

impl TextFrame {
    pub fn new() -> Result<Self, String> {
        let font = FontArc::try_from_slice(include_bytes!("font_default.ttf"))
            .map_err(|err| err.to_string())?;
        let glyphs = GlyphBrushBuilder::using_font(font)
            .initial_cache_size((512, 512))
            .build();

        Ok(Self {
            glyphs,
            verts: Vec::new(),
            pixels: vec![0; 512 * 512],
            size: (512, 512),
            dirty: true,
        })
    }

    pub fn queue(&mut self, text: &str, x: f32, y: f32, scale: f32, color: [f32; 4]) {
        self.glyphs.queue(
            Section::default()
                .add_text(Text::new(text).with_scale(scale).with_color(color))
                .with_screen_position((x, y)),
        );
    }

    pub fn build(&mut self) {
        for _attempt in 0..4 {
            let result = self.glyphs.process_queued(
                |rect, data| {
                    let width = (rect.max[0] - rect.min[0]) as usize;
                    let height = (rect.max[1] - rect.min[1]) as usize;

                    if width == 0 || height == 0 {
                        return;
                    }

                    let mut row = 0;

                    while row < height {
                        let dst = (rect.min[1] as usize + row) * self.size.0 as usize
                            + rect.min[0] as usize;
                        let src = row * width;
                        self.pixels[dst..dst + width].copy_from_slice(&data[src..src + width]);
                        row += 1;
                    }

                    self.dirty = true;
                },
                glyph_quad,
            );

            match result {
                Ok(BrushAction::Draw(quads)) => {
                    self.verts.clear();

                    for quad in quads {
                        for idx in 0..6 {
                            self.verts.extend_from_slice(&quad.verts[idx]);
                        }
                    }

                    return;
                }
                Ok(BrushAction::ReDraw) => {
                    return;
                }
                Err(BrushError::TextureTooSmall { suggested }) => {
                    self.glyphs.resize_texture(suggested.0, suggested.1);
                    self.pixels = vec![0; suggested.0 as usize * suggested.1 as usize];
                    self.size = suggested;
                    self.dirty = true;
                }
            }
        }
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
