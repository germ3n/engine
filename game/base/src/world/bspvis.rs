use super::material::{material_key, pak_from_bsp, sky_paths, vmt_path, FileSource, MaterialBank};
use super::surface::{
    push_vertex, tri_normal, tri_tangent, CpuImage, CubeImage, DrawMesh, MapGraphics, SurfaceRange,
    CUBEMAP_NONE, PASS_ALPHA, PASS_BLEND, PASS_DECAL, PASS_OPAQUE, STRIDE,
};
use std::collections::HashMap;
use vbsp::data::TextureFlags;

struct Probe {
    origin: [f32; 3],
}

struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

struct Atlas {
    size: u32,
    x: u32,
    y: u32,
    row: u32,
    layers: [Vec<u8>; 4],
}

struct Bucket {
    material: u16,
    cubemap: u16,
    pass: u8,
    verts: Vec<f32>,
}

pub struct BspVisual {
    pub mesh: DrawMesh,
    pub graphics: MapGraphics,
}

pub fn build(bytes: &[u8], map_name: &str) -> Result<BspVisual, String> {
    let bsp = vbsp::Bsp::read(bytes).map_err(|err| format!("bsp: {err}"))?;
    let lighting = lump_bytes(bytes, 8);
    let probes = cube_probes(bytes);
    let stem = map_stem(map_name);
    let mut names = Vec::new();
    let world = bsp.models().next();
    let mut face_names = Vec::new();

    if let Some(model) = &world {
        for face in model.faces() {
            let name = face.texture().name().to_string();
            names.push(vmt_path(&name));
            face_names.push(name);
        }
    }

    for entity in bsp.entities.iter() {
        if entity.prop("classname") == Some("info_overlay") {
            if let Some(material) = entity.prop("material") {
                names.push(vmt_path(material));
            }
        }

        if entity.prop("classname") == Some("worldspawn") {
            if let Some(sky) = entity.prop("skyname") {
                for path in sky_paths(sky) {
                    names.push(path);
                }
            }
        }
    }

    let mut idx = 0;

    while idx < probes.len() {
        names.push(cubemap_path(&stem, probes[idx].origin));
        idx += 1;
    }

    let mut bank = MaterialBank::new(FileSource::with_pak(pak_from_bsp(&bsp, &names)));
    let mut material_of = HashMap::new();
    idx = 0;

    while idx < face_names.len() {
        let key = material_key(&face_names[idx]);

        if !material_of.contains_key(&key) {
            material_of.insert(key.clone(), bank.load_packed(Some(&bsp), &key));
        }

        idx += 1;
    }

    let mut graphics = MapGraphics::plain();
    graphics.material_names = bank.ordered_names();
    graphics.materials = bank.materials().to_vec();
    let mut atlas = Atlas::new(512);
    let mut buckets: HashMap<(u16, u16, u8), Bucket> = HashMap::new();
    let mut polygons = Vec::new();

    if let Some(model) = world {
        for face in model.faces() {
            let name = face.texture().name();
            let flags = face.texture().flags;
            polygons.push(
                face.vertices()
                    .map(|vert| [vert.position.x, vert.position.y, vert.position.z])
                    .collect::<Vec<_>>(),
            );

            if hidden(name, flags) {
                continue;
            }

            let key = material_key(name);
            let material = material_of.get(&key).copied().unwrap_or(u16::MAX);
            let pass = pass_of(&graphics, material, flags);
            let info = face.texture();
            let tex_u = [
                info.texture_transforms_u[0],
                info.texture_transforms_u[1],
                info.texture_transforms_u[2],
            ];
            let tex_v = [
                info.texture_transforms_v[0],
                info.texture_transforms_v[1],
                info.texture_transforms_v[2],
            ];
            let tangent = tangent_of(
                tex_u,
                tex_v,
                [face.normal().x, face.normal().y, face.normal().z],
            );
            let light = light_rect(&mut atlas, &lighting, &face);
            let centroid = face_centroid(&face);
            let cubemap = if material_wants_env(&graphics, material) {
                nearest_probe(&probes, centroid)
            } else {
                CUBEMAP_NONE
            };
            let mins = face.light_map_texture_min;
            let size = face.light_map_texture_size;
            let bucket = buckets
                .entry((material, cubemap, pass))
                .or_insert_with(|| Bucket {
                    material,
                    cubemap,
                    pass,
                    verts: Vec::new(),
                });

            if let Some(disp) = face.displacement() {
                emit_displacement(
                    bucket,
                    &disp,
                    &info,
                    tangent,
                    &light,
                    mins,
                    size,
                    atlas.size,
                    material_blend(&graphics, material),
                );
            } else {
                for tri in face.triangulate() {
                    emit_tri(
                        bucket,
                        [
                            [tri[0].x, tri[0].y, tri[0].z],
                            [tri[1].x, tri[1].y, tri[1].z],
                            [tri[2].x, tri[2].y, tri[2].z],
                        ],
                        [0.0, 0.0, 0.0],
                        &info,
                        tangent,
                        &light,
                        mins,
                        size,
                        atlas.size,
                    );
                }
            }
        }
    }

    emit_overlays(
        &bsp,
        &mut bank,
        &mut graphics,
        &mut buckets,
        &polygons,
        &probes,
    );
    graphics.material_names = bank.ordered_names();
    graphics.materials = bank.materials().to_vec();
    graphics.lightmaps = atlas.images();
    graphics.cubemaps = load_cubemaps(&bank, &stem, &probes);
    graphics.sky = load_sky(&bsp, &bank);
    let mesh = flatten(buckets);

    Ok(BspVisual { mesh, graphics })
}

fn emit_displacement(
    bucket: &mut Bucket,
    disp: &vbsp::Handle<vbsp::data::DisplacementInfo>,
    info: &vbsp::Handle<vbsp::data::TextureInfo>,
    tangent: [f32; 4],
    light: &Option<Rect>,
    mins: [i32; 2],
    size: [i32; 2],
    atlas: u32,
    blend_material: bool,
) {
    let steps = 2usize.pow(disp.power as u32);
    let verts: Vec<_> = disp.displaced_vertices().collect();
    let alphas: Vec<f32> = disp
        .displacement_vertices()
        .map(|vert| vert.alpha)
        .collect();
    let mut x = 0;

    while x < steps {
        let mut y = 0;

        while y < steps {
            let index = |px: usize, py: usize| py * (steps + 1) + px;
            let tris = [
                [index(x, y), index(x + 1, y), index(x, y + 1)],
                [index(x + 1, y), index(x + 1, y + 1), index(x, y + 1)],
            ];
            let mut tri = 0;

            while tri < 2 {
                let ids = tris[tri];
                let positions = [
                    [verts[ids[0]].x, verts[ids[0]].y, verts[ids[0]].z],
                    [verts[ids[1]].x, verts[ids[1]].y, verts[ids[1]].z],
                    [verts[ids[2]].x, verts[ids[2]].y, verts[ids[2]].z],
                ];
                let blend = if blend_material {
                    [
                        alphas.get(ids[0]).copied().unwrap_or(0.0),
                        alphas.get(ids[1]).copied().unwrap_or(0.0),
                        alphas.get(ids[2]).copied().unwrap_or(0.0),
                    ]
                } else {
                    [0.0, 0.0, 0.0]
                };
                emit_tri(
                    bucket, positions, blend, info, tangent, light, mins, size, atlas,
                );
                tri += 1;
            }

            y += 1;
        }

        x += 1;
    }
}

fn emit_tri(
    bucket: &mut Bucket,
    positions: [[f32; 3]; 3],
    blend: [f32; 3],
    info: &vbsp::Handle<vbsp::data::TextureInfo>,
    tangent: [f32; 4],
    light: &Option<Rect>,
    mins: [i32; 2],
    size: [i32; 2],
    atlas: u32,
) {
    let normal = tri_normal(positions[0], positions[1], positions[2]);
    let mut corner = 0;

    while corner < 3 {
        let pos = positions[corner];
        let vector = vbsp::Vector {
            x: pos[0],
            y: pos[1],
            z: pos[2],
        };
        let uv = info.uv(vector);
        let light_uv = light_uv(info, vector, mins, size, light, atlas);
        push_vertex(
            &mut bucket.verts,
            pos,
            normal,
            tangent,
            uv,
            light_uv,
            [1.0, 1.0, 1.0],
            blend[corner],
            packed_id(bucket.material, bucket.cubemap),
        );
        corner += 1;
    }
}

fn emit_overlays(
    bsp: &vbsp::Bsp,
    bank: &mut MaterialBank,
    graphics: &mut MapGraphics,
    buckets: &mut HashMap<(u16, u16, u8), Bucket>,
    polygons: &[Vec<[f32; 3]>],
    probes: &[Probe],
) {
    for entity in bsp.entities.iter() {
        if entity.prop("classname") != Some("info_overlay") {
            continue;
        }

        let Some(material_name) = entity.prop("material") else {
            continue;
        };
        let material = bank.load_packed(Some(bsp), material_name);
        graphics.material_names = bank.ordered_names();
        graphics.materials = bank.materials().to_vec();
        let origin = parse_vec(entity.prop("BasisOrigin").unwrap_or("0 0 0"));
        let axis_u = parse_vec(entity.prop("BasisU").unwrap_or("1 0 0"));
        let axis_v = parse_vec(entity.prop("BasisV").unwrap_or("0 1 0"));
        let normal = parse_vec(entity.prop("BasisNormal").unwrap_or("0 0 1"));
        let start_u = parse_f32(entity.prop("StartU").unwrap_or("0"));
        let end_u = parse_f32(entity.prop("EndU").unwrap_or("1"));
        let start_v = parse_f32(entity.prop("StartV").unwrap_or("0"));
        let end_v = parse_f32(entity.prop("EndV").unwrap_or("1"));
        let corners = [
            mad(origin, axis_u, axis_v, start_u, start_v),
            mad(origin, axis_u, axis_v, end_u, start_v),
            mad(origin, axis_u, axis_v, end_u, end_v),
            mad(origin, axis_u, axis_v, start_u, end_v),
        ];
        let uvs = [
            parse_uv(entity.prop("uv0"), [0.0, 1.0]),
            parse_uv(entity.prop("uv1"), [1.0, 1.0]),
            parse_uv(entity.prop("uv2"), [1.0, 0.0]),
            parse_uv(entity.prop("uv3"), [0.0, 0.0]),
        ];
        let cubemap = if material_wants_env(graphics, material) {
            nearest_probe(probes, origin)
        } else {
            CUBEMAP_NONE
        };
        let sides = entity.prop("sides").unwrap_or("");
        let mut clipped = false;

        for side in sides.split_whitespace() {
            let Ok(index) = side.parse::<usize>() else {
                continue;
            };
            let Some(face) = polygons.get(index) else {
                continue;
            };

            if face.len() < 3 {
                continue;
            }

            let poly = clip_polygon(&corners, face, normal);

            if poly.len() < 3 {
                continue;
            }

            clipped = true;
            push_overlay(buckets, material, cubemap, &poly, &uvs, normal);
        }

        if !clipped {
            push_overlay(buckets, material, cubemap, &corners, &uvs, normal);
        }
    }
}

fn push_overlay(
    buckets: &mut HashMap<(u16, u16, u8), Bucket>,
    material: u16,
    cubemap: u16,
    poly: &[[f32; 3]],
    uvs: &[[f32; 2]; 4],
    normal: [f32; 3],
) {
    if poly.len() < 3 {
        return;
    }

    let bucket = buckets
        .entry((material, cubemap, PASS_DECAL))
        .or_insert_with(|| Bucket {
            material,
            cubemap,
            pass: PASS_DECAL,
            verts: Vec::new(),
        });
    let tangent = tri_tangent(poly[0], poly[1], normal);
    let mut idx = 1;

    while idx + 1 < poly.len() {
        let tri = [poly[0], poly[idx], poly[idx + 1]];
        let tri_uv = [uvs[0], uvs[idx.min(3)], uvs[(idx + 1).min(3)]];
        let mut corner = 0;

        while corner < 3 {
            push_vertex(
                &mut bucket.verts,
                tri[corner],
                normal,
                tangent,
                tri_uv[corner],
                [0.0, 0.0],
                [1.0, 1.0, 1.0],
                0.0,
                packed_id(material, cubemap),
            );
            corner += 1;
        }

        idx += 1;
    }
}

fn clip_polygon(poly: &[[f32; 3]; 4], face: &[[f32; 3]], normal: [f32; 3]) -> Vec<[f32; 3]> {
    let mut output = poly.to_vec();
    let mut edge = 0;

    while edge < face.len() {
        let a = face[edge];
        let b = face[(edge + 1) % face.len()];
        let edge_dir = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let plane_n = [
            normal[1] * edge_dir[2] - normal[2] * edge_dir[1],
            normal[2] * edge_dir[0] - normal[0] * edge_dir[2],
            normal[0] * edge_dir[1] - normal[1] * edge_dir[0],
        ];
        let input = std::mem::take(&mut output);
        let mut idx = 0;

        while idx < input.len() {
            let current = input[idx];
            let previous = input[(idx + input.len() - 1) % input.len()];
            let current_in = side(plane_n, a, current) <= 1e-3;
            let previous_in = side(plane_n, a, previous) <= 1e-3;

            if current_in {
                if !previous_in {
                    if let Some(hit) = intersect_plane(previous, current, plane_n, a) {
                        output.push(hit);
                    }
                }

                output.push(current);
            } else if previous_in {
                if let Some(hit) = intersect_plane(previous, current, plane_n, a) {
                    output.push(hit);
                }
            }

            idx += 1;
        }

        edge += 1;
    }

    output
}

fn side(normal: [f32; 3], origin: [f32; 3], point: [f32; 3]) -> f32 {
    (point[0] - origin[0]) * normal[0]
        + (point[1] - origin[1]) * normal[1]
        + (point[2] - origin[2]) * normal[2]
}

fn intersect_plane(
    a: [f32; 3],
    b: [f32; 3],
    normal: [f32; 3],
    origin: [f32; 3],
) -> Option<[f32; 3]> {
    let dir = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let denom = dir[0] * normal[0] + dir[1] * normal[1] + dir[2] * normal[2];

    if denom.abs() <= 1e-8 {
        return None;
    }

    let t = side(normal, a, origin) / denom;

    if !(0.0..=1.0).contains(&t) {
        return None;
    }

    Some([a[0] + dir[0] * t, a[1] + dir[1] * t, a[2] + dir[2] * t])
}

fn packed_id(material: u16, cubemap: u16) -> f32 {
    let cube = if cubemap == CUBEMAP_NONE {
        0.0
    } else {
        cubemap as f32 + 1.0
    };

    material as f32 + cube * 65536.0
}

fn flatten(buckets: HashMap<(u16, u16, u8), Bucket>) -> DrawMesh {
    let mut vertices = Vec::new();
    let mut ranges = Vec::new();
    let mut items: Vec<Bucket> = buckets.into_values().collect();
    items.sort_by(|left, right| {
        left.pass
            .cmp(&right.pass)
            .then(left.material.cmp(&right.material))
            .then(left.cubemap.cmp(&right.cubemap))
    });
    let mut idx = 0;

    while idx < items.len() {
        let bucket = &items[idx];
        let count = (bucket.verts.len() / STRIDE) as u32;

        if count > 0 {
            ranges.push(SurfaceRange {
                first: (vertices.len() / STRIDE) as u32,
                count,
                material: bucket.material,
                cubemap: bucket.cubemap,
                pass: bucket.pass,
            });
            vertices.extend_from_slice(&bucket.verts);
        }

        idx += 1;
    }

    DrawMesh { vertices, ranges }
}

fn hidden(name: &str, flags: TextureFlags) -> bool {
    if flags.intersects(
        TextureFlags::NODRAW
            | TextureFlags::HINT
            | TextureFlags::SKIP
            | TextureFlags::TRIGGER
            | TextureFlags::SKY
            | TextureFlags::SKY2D,
    ) {
        return true;
    }

    let lower = name.to_ascii_lowercase();

    lower.contains("toolsnodraw")
        || lower.contains("toolsclip")
        || lower.contains("toolstrigger")
        || lower.contains("toolshint")
        || lower.contains("toolsskip")
        || lower.contains("toolsinvisible")
        || lower.contains("toolsblock")
        || lower.contains("toolsorigin")
        || lower.contains("toolsoccluder")
        || lower.contains("toolsplayerclip")
        || lower.contains("toolsnpcclip")
}

fn pass_of(graphics: &MapGraphics, material: u16, flags: TextureFlags) -> u8 {
    if flags.intersects(TextureFlags::WARP) {
        return PASS_BLEND;
    }

    let Some(cpu) = graphics.materials.get(material as usize) else {
        return PASS_OPAQUE;
    };

    if cpu.gpu.params[0] == super::surface::MODE_WATER
        || cpu.gpu.params[0] == super::surface::MODE_ADD
    {
        return PASS_BLEND;
    }

    if flags.intersects(TextureFlags::TRANS) || cpu.gpu.params[3] < 0.999 {
        return PASS_BLEND;
    }

    if (cpu.gpu.detail[3].to_bits() & super::surface::FLAG_ALPHA) != 0 {
        return PASS_ALPHA;
    }

    PASS_OPAQUE
}

fn material_wants_env(graphics: &MapGraphics, material: u16) -> bool {
    graphics
        .materials
        .get(material as usize)
        .map(|cpu| (cpu.gpu.detail[3].to_bits() & super::surface::FLAG_ENV) != 0)
        .unwrap_or(false)
}

fn material_blend(graphics: &MapGraphics, material: u16) -> bool {
    graphics
        .materials
        .get(material as usize)
        .map(|cpu| cpu.gpu.params[0] == super::surface::MODE_BLEND)
        .unwrap_or(false)
}

fn tangent_of(axis_u: [f32; 3], axis_v: [f32; 3], normal: [f32; 3]) -> [f32; 4] {
    let tangent = super::surface::normalize3(axis_u);
    let bitangent = super::surface::normalize3(axis_v);
    let cross = [
        normal[1] * tangent[2] - normal[2] * tangent[1],
        normal[2] * tangent[0] - normal[0] * tangent[2],
        normal[0] * tangent[1] - normal[1] * tangent[0],
    ];
    let sign = if super::surface::dot3(cross, bitangent) < 0.0 {
        -1.0
    } else {
        1.0
    };

    [tangent[0], tangent[1], tangent[2], sign]
}

fn light_uv(
    info: &vbsp::Handle<vbsp::data::TextureInfo>,
    pos: vbsp::Vector,
    mins: [i32; 2],
    size: [i32; 2],
    light: &Option<Rect>,
    atlas: u32,
) -> [f32; 2] {
    let Some(rect) = light else {
        return [0.0, 0.0];
    };
    let luxel_u = info.light_map_scale[0] * pos.x
        + info.light_map_scale[1] * pos.y
        + info.light_map_scale[2] * pos.z
        + info.light_map_scale[3];
    let luxel_v = info.light_map_transform[0] * pos.x
        + info.light_map_transform[1] * pos.y
        + info.light_map_transform[2] * pos.z
        + info.light_map_transform[3];
    let span_u = (size[0] + 1).max(1) as f32;
    let span_v = (size[1] + 1).max(1) as f32;
    let u = (luxel_u - mins[0] as f32) / span_u;
    let v = (luxel_v - mins[1] as f32) / span_v;

    if atlas == 0 {
        return [0.0, 0.0];
    }

    [
        (rect.x as f32 + u.clamp(0.0, 1.0) * rect.w as f32) / atlas as f32,
        (rect.y as f32 + v.clamp(0.0, 1.0) * rect.h as f32) / atlas as f32,
    ]
}

fn light_rect(
    atlas: &mut Atlas,
    lighting: &[u8],
    face: &vbsp::Handle<vbsp::data::Face>,
) -> Option<Rect> {
    if face.light_offset < 0 || lighting.is_empty() {
        return None;
    }

    let width = (face.light_map_texture_size[0] + 1).max(1) as usize;
    let height = (face.light_map_texture_size[1] + 1).max(1) as usize;
    let bumped = face.texture().flags.intersects(TextureFlags::BUMPLIGHT);
    let maps = if bumped { 4 } else { 1 };
    let bytes = width * height * maps * 4;
    let start = face.light_offset as usize;

    if start + bytes > lighting.len() {
        return None;
    }

    let block = &lighting[start..start + bytes];
    let mut layers = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut map = 0;

    while map < 4 {
        let mut pixels = vec![255u8; width * height * 4];
        let source = if bumped { map } else { 0 };
        let mut idx = 0;

        while idx < width * height {
            let src = (source * width * height + idx) * 4;

            if src + 4 <= block.len() {
                let color = rgb_exp(&block[src..src + 4]);
                pixels[idx * 4] = color[0];
                pixels[idx * 4 + 1] = color[1];
                pixels[idx * 4 + 2] = color[2];
                pixels[idx * 4 + 3] = 255;
            }

            idx += 1;
        }

        layers[map] = pixels;
        map += 1;
    }

    atlas.blit(width as u32, height as u32, &layers)
}

fn rgb_exp(px: &[u8]) -> [u8; 3] {
    let scale = 2f32.powi(px[3] as i8 as i32);
    let chan = |value: u8| ((value as f32 * scale / 255.0).clamp(0.0, 1.0) * 255.0) as u8;

    [chan(px[0]), chan(px[1]), chan(px[2])]
}

impl Atlas {
    fn new(size: u32) -> Self {
        let pixels = (size as usize) * (size as usize) * 4;
        Self {
            size,
            x: 1,
            y: 1,
            row: 0,
            layers: [
                vec![255; pixels],
                vec![255; pixels],
                vec![255; pixels],
                vec![255; pixels],
            ],
        }
    }

    fn blit(&mut self, width: u32, height: u32, layers: &[Vec<u8>; 4]) -> Option<Rect> {
        if self.x + width + 1 > self.size {
            self.x = 1;
            self.y += self.row + 1;
            self.row = 0;
        }

        if self.y + height + 1 > self.size {
            return None;
        }

        let rect = Rect {
            x: self.x,
            y: self.y,
            w: width,
            h: height,
        };
        let mut map = 0;

        while map < 4 {
            let mut row = 0;

            while row < height {
                let dst = ((self.y + row) as usize * self.size as usize + self.x as usize) * 4;
                let src = row as usize * width as usize * 4;
                let end = src + width as usize * 4;

                if end <= layers[map].len() && dst + width as usize * 4 <= self.layers[map].len() {
                    self.layers[map][dst..dst + width as usize * 4]
                        .copy_from_slice(&layers[map][src..end]);
                }

                row += 1;
            }

            map += 1;
        }

        self.x += width + 1;
        self.row = self.row.max(height);

        Some(rect)
    }

    fn images(self) -> [CpuImage; 4] {
        [
            CpuImage {
                width: self.size,
                height: self.size,
                format: super::surface::PixelFormat::Rgba8,
                bytes: self.layers[0].clone(),
                mips: Vec::new(),
            },
            CpuImage {
                width: self.size,
                height: self.size,
                format: super::surface::PixelFormat::Rgba8,
                bytes: self.layers[1].clone(),
                mips: Vec::new(),
            },
            CpuImage {
                width: self.size,
                height: self.size,
                format: super::surface::PixelFormat::Rgba8,
                bytes: self.layers[2].clone(),
                mips: Vec::new(),
            },
            CpuImage {
                width: self.size,
                height: self.size,
                format: super::surface::PixelFormat::Rgba8,
                bytes: self.layers[3].clone(),
                mips: Vec::new(),
            },
        ]
    }
}

fn face_centroid(face: &vbsp::Handle<vbsp::data::Face>) -> [f32; 3] {
    let mut sum = [0.0, 0.0, 0.0];
    let mut count = 0.0;

    for vert in face.vertices() {
        sum[0] += vert.position.x;
        sum[1] += vert.position.y;
        sum[2] += vert.position.z;
        count += 1.0;
    }

    if count == 0.0 {
        return [0.0, 0.0, 0.0];
    }

    [sum[0] / count, sum[1] / count, sum[2] / count]
}

fn nearest_probe(probes: &[Probe], point: [f32; 3]) -> u16 {
    if probes.is_empty() {
        return CUBEMAP_NONE;
    }

    let mut best = 0usize;
    let mut best_dist = f32::MAX;
    let mut idx = 0;

    while idx < probes.len() {
        let origin = probes[idx].origin;
        let dx = origin[0] - point[0];
        let dy = origin[1] - point[1];
        let dz = origin[2] - point[2];
        let dist = dx * dx + dy * dy + dz * dz;

        if dist < best_dist {
            best = idx;
            best_dist = dist;
        }

        idx += 1;
    }

    best as u16
}

fn load_cubemaps(bank: &MaterialBank, stem: &str, probes: &[Probe]) -> Vec<CubeImage> {
    let mut cubes = Vec::new();
    let mut idx = 0;

    while idx < probes.len() {
        let path = cubemap_path(stem, probes[idx].origin);
        let cube = bank
            .read_vtf_cube(&path)
            .or_else(|| bank.read_vtf_cube(&path.replace(".vtf", ".hdr.vtf")))
            .unwrap_or_else(|| CubeImage::solid(CpuImage::checker()));
        cubes.push(cube);
        idx += 1;
    }

    cubes
}

fn load_sky(bsp: &vbsp::Bsp, bank: &MaterialBank) -> Option<CubeImage> {
    for entity in bsp.entities.iter() {
        if entity.prop("classname") != Some("worldspawn") {
            continue;
        }

        let sky = entity.prop("skyname")?;
        let paths = sky_paths(sky);

        return bank.read_cube([
            &paths[0], &paths[1], &paths[2], &paths[3], &paths[4], &paths[5],
        ]);
    }

    None
}

fn cubemap_path(stem: &str, origin: [f32; 3]) -> String {
    format!(
        "materials/maps/{stem}/c{}_{}_{}.vtf",
        origin[0] as i32, origin[1] as i32, origin[2] as i32
    )
}

fn cube_probes(bytes: &[u8]) -> Vec<Probe> {
    let lump = lump_bytes(bytes, 42);
    let mut probes = Vec::new();
    let mut idx = 0;

    while idx + 16 <= lump.len() {
        let x = i32::from_le_bytes([lump[idx], lump[idx + 1], lump[idx + 2], lump[idx + 3]]);
        let y = i32::from_le_bytes([lump[idx + 4], lump[idx + 5], lump[idx + 6], lump[idx + 7]]);
        let z = i32::from_le_bytes([lump[idx + 8], lump[idx + 9], lump[idx + 10], lump[idx + 11]]);
        probes.push(Probe {
            origin: [x as f32, y as f32, z as f32],
        });
        idx += 16;
    }

    probes
}

fn lump_bytes(bytes: &[u8], index: usize) -> Vec<u8> {
    if bytes.len() < 8 + 64 * 16 || &bytes[..4] != b"VBSP" {
        return Vec::new();
    }

    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let base = 8 + index * 16;

    if base + 16 > bytes.len() {
        return Vec::new();
    }

    let mut offset = u32::from_le_bytes([
        bytes[base],
        bytes[base + 1],
        bytes[base + 2],
        bytes[base + 3],
    ]);
    let mut length = u32::from_le_bytes([
        bytes[base + 4],
        bytes[base + 5],
        bytes[base + 6],
        bytes[base + 7],
    ]);
    let lump_version = u32::from_le_bytes([
        bytes[base + 8],
        bytes[base + 9],
        bytes[base + 10],
        bytes[base + 11],
    ]);

    if version >= 21 {
        let stored_offset = length;
        let stored_length = lump_version;
        offset = stored_offset;
        length = stored_length;
    }

    let start = offset as usize;
    let end = start.saturating_add(length as usize);
    let Some(slice) = bytes.get(start..end) else {
        return Vec::new();
    };

    if slice.starts_with(b"LZMA") {
        return decompress_lzma(slice).unwrap_or_else(|| slice.to_vec());
    }

    slice.to_vec()
}

fn decompress_lzma(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 17 || &data[..4] != b"LZMA" {
        return None;
    }

    let actual = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let mut cursor = std::io::Cursor::new(&data[12..]);
    let mut output = Vec::with_capacity(actual);
    lzma_rs::lzma_decompress_with_options(
        &mut cursor,
        &mut output,
        &lzma_rs::decompress::Options {
            unpacked_size: lzma_rs::decompress::UnpackedSize::UseProvided(Some(actual as u64)),
            allow_incomplete: false,
            memlimit: None,
        },
    )
    .ok()?;

    if output.len() != actual {
        return None;
    }

    Some(output)
}

fn map_stem(name: &str) -> String {
    let path = std::path::Path::new(name);
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name)
        .to_ascii_lowercase()
}

fn parse_vec(text: &str) -> [f32; 3] {
    let mut parts = text.split_whitespace();
    let x = parts
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or(0.0);
    let y = parts
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or(0.0);
    let z = parts
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or(0.0);

    [x, y, z]
}

fn parse_f32(text: &str) -> f32 {
    text.trim().parse().unwrap_or(0.0)
}

fn parse_uv(text: Option<&str>, fallback: [f32; 2]) -> [f32; 2] {
    let Some(text) = text else {
        return fallback;
    };
    let mut parts = text.split_whitespace();
    let u = parts
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or(fallback[0]);
    let v = parts
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or(fallback[1]);

    [u, v]
}

fn mad(origin: [f32; 3], axis_u: [f32; 3], axis_v: [f32; 3], u: f32, v: f32) -> [f32; 3] {
    [
        origin[0] + axis_u[0] * u + axis_v[0] * v,
        origin[1] + axis_u[1] * u + axis_v[1] * v,
        origin[2] + axis_u[2] * u + axis_v[2] * v,
    ]
}
