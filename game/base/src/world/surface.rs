pub const STRIDE: usize = 19;
pub const PASS_SKY: u8 = 0;
pub const PASS_OPAQUE: u8 = 1;
pub const PASS_ALPHA: u8 = 2;
pub const PASS_DECAL: u8 = 3;
pub const PASS_BLEND: u8 = 4;
pub const MATERIAL_NONE: u16 = u16::MAX;
pub const CUBEMAP_NONE: u16 = u16::MAX;

pub const MODE_UNLIT: f32 = 0.0;
pub const MODE_LIGHT: f32 = 1.0;
pub const MODE_BLEND: f32 = 2.0;
pub const MODE_WATER: f32 = 3.0;
pub const MODE_ADD: f32 = 4.0;
pub const MODE_MODULATE: f32 = 5.0;

pub const FLAG_BUMP: u32 = 1;
pub const FLAG_SSBUMP: u32 = 2;
pub const FLAG_DETAIL: u32 = 4;
pub const FLAG_ENV: u32 = 8;
pub const FLAG_BASE2: u32 = 16;
pub const FLAG_SELF: u32 = 32;
pub const FLAG_ALPHA: u32 = 64;
pub const FLAG_PHONG: u32 = 128;
pub const FLAG_BLENDMOD: u32 = 256;
pub const FLAG_MASK: u32 = 512;
pub const FLAG_BUMP2: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba8,
    Bc1,
    Bc2,
    Bc3,
    Bc5,
    Bc7,
    Rgba16f,
}

#[derive(Clone, Debug)]
pub struct CpuImage {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub bytes: Vec<u8>,
    pub mips: Vec<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct CubeImage {
    pub faces: [CpuImage; 6],
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct MaterialGpu {
    pub tint: [f32; 4],
    pub params: [f32; 4],
    pub detail: [f32; 4],
    pub env: [f32; 4],
    pub extra: [f32; 4],
    pub fog: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct CpuMaterial {
    pub gpu: MaterialGpu,
    pub width: u32,
    pub height: u32,
    pub base: CpuImage,
    pub base2: CpuImage,
    pub bump: CpuImage,
    pub bump2: CpuImage,
    pub detail: CpuImage,
    pub blend: CpuImage,
    pub mask: CpuImage,
}

#[derive(Clone, Debug)]
pub struct MapGraphics {
    pub materials: Vec<CpuMaterial>,
    pub material_names: Vec<String>,
    pub lightmaps: [CpuImage; 4],
    pub cubemaps: Vec<CubeImage>,
    pub sky: Option<CubeImage>,
}

#[derive(Clone, Copy, Debug)]
pub struct SurfaceRange {
    pub first: u32,
    pub count: u32,
    pub material: u16,
    pub cubemap: u16,
    pub pass: u8,
}

#[derive(Clone, Debug)]
pub struct DrawMesh {
    pub vertices: Vec<f32>,
    pub ranges: Vec<SurfaceRange>,
}

impl MaterialGpu {
    pub fn unlit() -> Self {
        Self {
            tint: [1.0, 1.0, 1.0, 1.0],
            params: [MODE_UNLIT, 0.5, 0.0, 1.0],
            detail: [1.0, 1.0, 1.0, 0.0],
            env: [1.0, 1.0, 1.0, 0.0],
            extra: [16.0, 0.0, 0.0, 0.0],
            fog: [0.2, 0.4, 0.45, 2.0],
        }
    }

    pub fn shaded() -> Self {
        let mut gpu = Self::unlit();
        gpu.params[0] = MODE_LIGHT;

        gpu
    }
}

impl CpuImage {
    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let mut bytes = Vec::with_capacity((width as usize) * (height as usize) * 4);
        let mut idx = 0;
        let count = (width as usize) * (height as usize);

        while idx < count {
            bytes.extend_from_slice(&rgba);
            idx += 1;
        }

        Self {
            width,
            height,
            format: PixelFormat::Rgba8,
            bytes,
            mips: Vec::new(),
        }
    }

    pub fn white() -> Self {
        Self::solid(1, 1, [255, 255, 255, 255])
    }

    pub fn flat_normal() -> Self {
        Self::solid(1, 1, [128, 128, 255, 255])
    }

    pub fn checker() -> Self {
        let width = 64u32;
        let height = 64u32;
        let mut bytes = Vec::with_capacity((width as usize) * (height as usize) * 4);
        let mut y = 0;

        while y < height {
            let mut x = 0;

            while x < width {
                let mag = ((x / 8) + (y / 8)) % 2 == 0;

                if mag {
                    bytes.extend_from_slice(&[255, 0, 255, 255]);
                } else {
                    bytes.extend_from_slice(&[0, 0, 0, 255]);
                }

                x += 1;
            }

            y += 1;
        }

        Self {
            width,
            height,
            format: PixelFormat::Rgba8,
            bytes,
            mips: Vec::new(),
        }
    }
}

impl CubeImage {
    pub fn solid(image: CpuImage) -> Self {
        Self {
            faces: [
                image.clone(),
                image.clone(),
                image.clone(),
                image.clone(),
                image.clone(),
                image,
            ],
        }
    }
}

impl CpuMaterial {
    pub fn missing() -> Self {
        let checker = CpuImage::checker();

        Self {
            gpu: MaterialGpu::shaded(),
            width: checker.width,
            height: checker.height,
            base: checker,
            base2: CpuImage::white(),
            bump: CpuImage::flat_normal(),
            bump2: CpuImage::flat_normal(),
            detail: CpuImage::white(),
            blend: CpuImage::white(),
            mask: CpuImage::white(),
        }
    }
}

impl MapGraphics {
    pub fn plain() -> Self {
        Self {
            materials: Vec::new(),
            material_names: Vec::new(),
            lightmaps: [
                CpuImage::white(),
                CpuImage::white(),
                CpuImage::white(),
                CpuImage::white(),
            ],
            cubemaps: Vec::new(),
            sky: None,
        }
    }
}

impl DrawMesh {
    pub fn empty() -> Self {
        Self {
            vertices: Vec::new(),
            ranges: Vec::new(),
        }
    }

    pub fn colored(vertices: Vec<f32>) -> Self {
        let count = (vertices.len() / STRIDE) as u32;
        let ranges = if count == 0 {
            Vec::new()
        } else {
            vec![SurfaceRange {
                first: 0,
                count,
                material: MATERIAL_NONE,
                cubemap: CUBEMAP_NONE,
                pass: PASS_OPAQUE,
            }]
        };

        Self { vertices, ranges }
    }
}

pub fn push_vertex(
    vertices: &mut Vec<f32>,
    position: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 4],
    uv: [f32; 2],
    light_uv: [f32; 2],
    color: [f32; 3],
    blend: f32,
    material: f32,
) {
    vertices.extend_from_slice(&position);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&tangent);
    vertices.extend_from_slice(&uv);
    vertices.extend_from_slice(&light_uv);
    vertices.extend_from_slice(&color);
    vertices.push(blend);
    vertices.push(material);
}

pub fn push_shaded_tri(
    vertices: &mut Vec<f32>,
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    color: [f32; 3],
) {
    let normal = tri_normal(a, b, c);
    let tangent = tri_tangent(a, b, normal);
    push_vertex(
        vertices,
        a,
        normal,
        tangent,
        [0.0, 0.0],
        [0.0, 0.0],
        color,
        0.0,
        0.0,
    );
    push_vertex(
        vertices,
        b,
        normal,
        tangent,
        [0.0, 0.0],
        [0.0, 0.0],
        color,
        0.0,
        0.0,
    );
    push_vertex(
        vertices,
        c,
        normal,
        tangent,
        [0.0, 0.0],
        [0.0, 0.0],
        color,
        0.0,
        0.0,
    );
}

pub fn tri_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];

    normalize3(n)
}

pub fn tri_tangent(a: [f32; 3], b: [f32; 3], normal: [f32; 3]) -> [f32; 4] {
    let edge = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let reject = [
        edge[0] - normal[0] * dot3(edge, normal),
        edge[1] - normal[1] * dot3(edge, normal),
        edge[2] - normal[2] * dot3(edge, normal),
    ];
    let tangent = normalize3(reject);

    [tangent[0], tangent[1], tangent[2], 1.0]
}

pub fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let len = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();

    if len <= 1e-8 {
        return [0.0, 0.0, 1.0];
    }

    [value[0] / len, value[1] / len, value[2] / len]
}

pub fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn ordered_ranges(vertices: &[f32], ranges: &[SurfaceRange], eye: [f32; 3]) -> Vec<usize> {
    let mut order = Vec::with_capacity(ranges.len());
    let mut idx = 0;

    while idx < ranges.len() {
        order.push(idx);
        idx += 1;
    }

    order.sort_by(|left, right| {
        let a = &ranges[*left];
        let b = &ranges[*right];
        let pass = a.pass.cmp(&b.pass);

        if pass != std::cmp::Ordering::Equal || a.pass != PASS_BLEND {
            return pass;
        }

        let far = range_distance(vertices, b, eye);
        let near = range_distance(vertices, a, eye);
        far.partial_cmp(&near).unwrap_or(std::cmp::Ordering::Equal)
    });

    order
}

fn range_distance(vertices: &[f32], range: &SurfaceRange, eye: [f32; 3]) -> f32 {
    let start = range.first as usize * STRIDE;

    if start + 3 > vertices.len() {
        return 0.0;
    }

    let dx = vertices[start] - eye[0];
    let dy = vertices[start + 1] - eye[1];
    let dz = vertices[start + 2] - eye[2];

    dx * dx + dy * dy + dz * dz
}

pub fn sky_vertices(eye: [f32; 3], radius: f32) -> Vec<f32> {
    let faces: [[([f32; 3], [f32; 2]); 4]; 6] = [
        [
            ([1.0, -1.0, -1.0], [0.0, 1.0]),
            ([1.0, -1.0, 1.0], [0.0, 0.0]),
            ([1.0, 1.0, 1.0], [1.0, 0.0]),
            ([1.0, 1.0, -1.0], [1.0, 1.0]),
        ],
        [
            ([-1.0, 1.0, -1.0], [0.0, 1.0]),
            ([-1.0, 1.0, 1.0], [0.0, 0.0]),
            ([-1.0, -1.0, 1.0], [1.0, 0.0]),
            ([-1.0, -1.0, -1.0], [1.0, 1.0]),
        ],
        [
            ([1.0, 1.0, -1.0], [0.0, 1.0]),
            ([1.0, 1.0, 1.0], [0.0, 0.0]),
            ([-1.0, 1.0, 1.0], [1.0, 0.0]),
            ([-1.0, 1.0, -1.0], [1.0, 1.0]),
        ],
        [
            ([-1.0, -1.0, -1.0], [0.0, 1.0]),
            ([-1.0, -1.0, 1.0], [0.0, 0.0]),
            ([1.0, -1.0, 1.0], [1.0, 0.0]),
            ([1.0, -1.0, -1.0], [1.0, 1.0]),
        ],
        [
            ([-1.0, -1.0, 1.0], [0.0, 1.0]),
            ([-1.0, 1.0, 1.0], [0.0, 0.0]),
            ([1.0, 1.0, 1.0], [1.0, 0.0]),
            ([1.0, -1.0, 1.0], [1.0, 1.0]),
        ],
        [
            ([-1.0, 1.0, -1.0], [0.0, 1.0]),
            ([-1.0, -1.0, -1.0], [0.0, 0.0]),
            ([1.0, -1.0, -1.0], [1.0, 0.0]),
            ([1.0, 1.0, -1.0], [1.0, 1.0]),
        ],
    ];
    let mut vertices = Vec::with_capacity(6 * 6 * STRIDE);
    let mut face = 0;

    while face < 6 {
        let quad = &faces[face];
        let mut corners = [[0.0f32; 3]; 4];
        let mut uvs = [[0.0f32; 2]; 4];
        let mut corner = 0;

        while corner < 4 {
            corners[corner] = [
                eye[0] + quad[corner].0[0] * radius,
                eye[1] + quad[corner].0[1] * radius,
                eye[2] + quad[corner].0[2] * radius,
            ];
            uvs[corner] = quad[corner].1;
            corner += 1;
        }

        let normal = tri_normal(corners[0], corners[1], corners[2]);
        let tangent = tri_tangent(corners[0], corners[1], normal);
        push_sky(
            &mut vertices,
            corners[0],
            normal,
            tangent,
            uvs[0],
            face as f32,
        );
        push_sky(
            &mut vertices,
            corners[1],
            normal,
            tangent,
            uvs[1],
            face as f32,
        );
        push_sky(
            &mut vertices,
            corners[2],
            normal,
            tangent,
            uvs[2],
            face as f32,
        );
        push_sky(
            &mut vertices,
            corners[0],
            normal,
            tangent,
            uvs[0],
            face as f32,
        );
        push_sky(
            &mut vertices,
            corners[2],
            normal,
            tangent,
            uvs[2],
            face as f32,
        );
        push_sky(
            &mut vertices,
            corners[3],
            normal,
            tangent,
            uvs[3],
            face as f32,
        );
        face += 1;
    }

    vertices
}

fn push_sky(
    vertices: &mut Vec<f32>,
    position: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 4],
    uv: [f32; 2],
    face: f32,
) {
    push_vertex(
        vertices,
        position,
        normal,
        tangent,
        uv,
        [0.0, 0.0],
        [1.0, 1.0, 1.0],
        face,
        0.0,
    );
}

pub fn view_constants(view_proj: [f32; 16], eye: [f32; 3], time: f32, screen: [f32; 4]) -> [f32; 24] {
    let mut out = [0.0; 24];
    out[..16].copy_from_slice(&view_proj);
    out[16] = eye[0];
    out[17] = eye[1];
    out[18] = eye[2];
    out[19] = time;
    out[20] = screen[0];
    out[21] = screen[1];
    out[22] = screen[2];
    out[23] = screen[3];

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checker_is_magenta_and_black() {
        let image = CpuImage::checker();

        assert_eq!(image.width, 64);
        assert_eq!(&image.bytes[..4], &[255, 0, 255, 255]);
        assert_eq!(&image.bytes[8 * 4..8 * 4 + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn shaded_triangle_uses_the_shared_stride() {
        let mut vertices = Vec::new();
        push_shaded_tri(
            &mut vertices,
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.2, 0.4, 0.6],
        );

        assert_eq!(vertices.len(), STRIDE * 3);
        assert_eq!(vertices[14], 0.2);
    }
}
