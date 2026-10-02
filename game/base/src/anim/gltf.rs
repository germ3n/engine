use crate::anim::format::{Bone, Mesh, MAX_BONES, MAX_NAME};
use crate::anim::pose::{self, ClipSet, Sequence, Track, FLAG_LOOP};
use std::collections::HashSet;
use std::io::Cursor;

const MAX_VERTICES: usize = 1_000_000;
const MAX_INDICES: usize = 3_000_000;
const MAX_ALBEDO: usize = 16 * 1024 * 1024;
const MAX_KEYS: usize = 8192;

pub struct Loaded {
    pub mesh: Mesh,
    pub clips: ClipSet,
}

struct NodeRec {
    name: String,
    parent: Option<usize>,
    children: Vec<usize>,
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
    mesh: Option<usize>,
    skin: Option<usize>,
    key_scale: f32,
    vertex_scale: f32,
}

struct PrimRec {
    node: usize,
    skin: Option<usize>,
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    uvs: Vec<[f32; 2]>,
    joints: Vec<Vec<[u16; 4]>>,
    weights: Vec<Vec<[f32; 4]>>,
    indices: Vec<u32>,
    material: Option<usize>,
}

struct SkinRec {
    joints: Vec<usize>,
    ibms: Vec<[f32; 16]>,
}

struct ChannelRec {
    node: usize,
    times: Vec<f32>,
    translation: Option<Vec<[f32; 3]>>,
    rotation: Option<Vec<[f32; 4]>>,
}

struct AnimRec {
    name: String,
    channels: Vec<ChannelRec>,
}

#[derive(Clone)]
struct Rgba {
    w: u32,
    h: u32,
    pixels: Vec<u8>,
}

struct MaterialRec {
    factor: [f32; 4],
    image: Option<usize>,
}

struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

pub fn is_gltf(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);

    name.len() >= 5 && name[name.len() - 5..].eq_ignore_ascii_case(".gltf")
        || name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case(".glb")
}

pub fn load_path(path: &str) -> Result<Loaded, String> {
    let bytes = crate::fs::read(path)?;
    let dir = parent_dir(path).to_string();

    load_bytes(&bytes, &mut |uri| {
        let full = join_uri(&dir, uri)?;

        crate::fs::read(&full)
    })
}

pub fn load_bytes(
    bytes: &[u8],
    read_uri: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Loaded, String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|err| err.to_string())?;
    let buffers = load_buffers(&gltf, read_uri)?;
    let mut nodes = read_nodes(&gltf)?;
    let scene = scene_mask(&gltf, &nodes);
    let prims = read_prims(&gltf, &nodes, &scene, &buffers)?;
    let skins = read_skins(&gltf, &buffers)?;
    let anims = read_anims(&gltf, &buffers)?;
    let materials = read_materials(&gltf);
    let images = read_images(&gltf, &buffers, read_uri)?;
    drop(gltf);
    let baked = bake_tree(&mut nodes)?;
    convert_nodes(&mut nodes);
    let (bones, node_bone) = build_bones(&nodes, &prims, &skins)?;
    let bones = assign_binds(bones, &node_bone, &skins, baked)?;
    let (atlas, rects) = build_atlas(&prims, &materials, &images)?;
    let mesh = build_mesh(&nodes, &prims, &skins, &bones, &node_bone, &rects, atlas)?;
    let clips = build_clips(&nodes, &bones, &node_bone, &anims)?;

    Ok(Loaded { mesh, clips })
}

fn load_buffers(
    gltf: &gltf::Gltf,
    read_uri: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<Vec<u8>>, String> {
    let mut buffers = Vec::new();

    for buffer in gltf.buffers() {
        if buffer.index() != buffers.len() {
            return Err("gltf buffer".to_string());
        }

        let data = match buffer.source() {
            gltf::buffer::Source::Bin => {
                let blob = gltf.blob.as_deref().ok_or_else(|| "gltf buffer".to_string())?;

                if blob.len() < buffer.length() {
                    return Err("gltf buffer".to_string());
                }

                blob[..buffer.length()].to_vec()
            }
            gltf::buffer::Source::Uri(uri) => {
                let data = fetch_uri(uri, read_uri)?;

                if data.len() < buffer.length() {
                    return Err("gltf buffer".to_string());
                }

                data[..buffer.length()].to_vec()
            }
        };
        buffers.push(data);
    }

    Ok(buffers)
}

fn read_nodes(gltf: &gltf::Gltf) -> Result<Vec<NodeRec>, String> {
    let mut nodes = Vec::new();

    for node in gltf.nodes() {
        if node.index() != nodes.len() {
            return Err("gltf node".to_string());
        }

        let (translation, rotation, scale) = node.transform().decomposed();

        if !translation.iter().all(|value| value.is_finite()) || !rotation.iter().all(|value| value.is_finite()) {
            return Err("gltf transform".to_string());
        }

        nodes.push(NodeRec {
            name: node.name().unwrap_or("").to_string(),
            parent: None,
            children: Vec::new(),
            translation,
            rotation: normalize_quat(rotation),
            scale,
            mesh: node.mesh().map(|mesh| mesh.index()),
            skin: node.skin().map(|skin| skin.index()),
            key_scale: 1.0,
            vertex_scale: 1.0,
        });
    }

    for node in gltf.nodes() {
        for child in node.children() {
            if child.index() >= nodes.len() {
                return Err("gltf node".to_string());
            }

            nodes[node.index()].children.push(child.index());
            nodes[child.index()].parent = Some(node.index());
        }
    }

    Ok(nodes)
}

fn scene_mask(gltf: &gltf::Gltf, nodes: &[NodeRec]) -> Vec<bool> {
    let mut seen = vec![false; nodes.len()];
    let scene = gltf.default_scene().or_else(|| gltf.scenes().next());

    if let Some(scene) = scene {
        for node in scene.nodes() {
            mark_tree(node.index(), nodes, &mut seen);
        }
    } else {
        let mut idx = 0;

        while idx < seen.len() {
            seen[idx] = true;
            idx += 1;
        }
    }

    seen
}

fn mark_tree(idx: usize, nodes: &[NodeRec], seen: &mut [bool]) {
    if idx >= nodes.len() || seen[idx] {
        return;
    }

    seen[idx] = true;
    let mut child_idx = 0;

    while child_idx < nodes[idx].children.len() {
        mark_tree(nodes[idx].children[child_idx], nodes, seen);
        child_idx += 1;
    }
}

fn read_prims(
    gltf: &gltf::Gltf,
    nodes: &[NodeRec],
    scene: &[bool],
    buffers: &[Vec<u8>],
) -> Result<Vec<PrimRec>, String> {
    let mut prims = Vec::new();
    let mut idx = 0;

    while idx < nodes.len() {
        if scene[idx] {
            if let Some(mesh_index) = nodes[idx].mesh {
                let mesh = gltf.meshes().nth(mesh_index).ok_or_else(|| "gltf mesh".to_string())?;

                for primitive in mesh.primitives() {
                    if let Some(prim) = read_prim(idx, nodes[idx].skin, &primitive, buffers)? {
                        prims.push(prim);
                    }
                }
            }
        }

        idx += 1;
    }

    if prims.is_empty() {
        return Err("gltf mesh".to_string());
    }

    Ok(prims)
}

fn read_prim(
    node: usize,
    skin: Option<usize>,
    primitive: &gltf::mesh::Primitive,
    buffers: &[Vec<u8>],
) -> Result<Option<PrimRec>, String> {
    match primitive.mode() {
        gltf::mesh::Mode::Points | gltf::mesh::Mode::Lines | gltf::mesh::Mode::LineLoop | gltf::mesh::Mode::LineStrip => {
            return Ok(None);
        }
        gltf::mesh::Mode::Triangles | gltf::mesh::Mode::TriangleStrip | gltf::mesh::Mode::TriangleFan => {}
    }

    let get_buffer = |buffer: gltf::Buffer<'_>| buffers.get(buffer.index()).map(Vec::as_slice);
    let reader = primitive.reader(get_buffer);
    let positions = reader
        .read_positions()
        .ok_or_else(|| "gltf position".to_string())?
        .collect::<Vec<_>>();

    if positions.is_empty() || positions.len() > MAX_VERTICES {
        return Err("gltf position".to_string());
    }

    if positions.iter().any(|position| !position.iter().all(|value| value.is_finite())) {
        return Err("gltf position".to_string());
    }

    let normals = reader.read_normals().map(|iter| iter.collect::<Vec<_>>());

    if let Some(normals) = &normals {
        if normals.len() != positions.len() {
            return Err("gltf normal".to_string());
        }
    }

    let tex_coord = primitive
        .material()
        .pbr_metallic_roughness()
        .base_color_texture()
        .map(|info| info.tex_coord())
        .unwrap_or(0);
    let uvs = match reader.read_tex_coords(tex_coord).or_else(|| reader.read_tex_coords(0)) {
        Some(coords) => {
            let uvs = coords.into_f32().collect::<Vec<_>>();

            if uvs.len() != positions.len() {
                return Err("gltf texcoord".to_string());
            }

            uvs
        }
        None => vec![[0.0, 0.0]; positions.len()],
    };
    let mut joints = Vec::new();
    let mut weights = Vec::new();
    let mut set = 0u32;

    while set < 2 {
        match (reader.read_joints(set), reader.read_weights(set)) {
            (Some(joint_iter), Some(weight_iter)) => {
                let joint_values = joint_iter.into_u16().collect::<Vec<_>>();
                let weight_values = weight_iter.into_f32().collect::<Vec<_>>();

                if joint_values.len() != positions.len() || weight_values.len() != positions.len() {
                    return Err("gltf joints".to_string());
                }

                joints.push(joint_values);
                weights.push(weight_values);
            }
            (None, None) => {}
            _ => return Err("gltf joints".to_string()),
        }

        set += 1;
    }

    if skin.is_some() && joints.is_empty() {
        return Err("gltf joints".to_string());
    }

    let raw_indices = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect::<Vec<_>>(),
        None => {
            let mut indices = Vec::with_capacity(positions.len());
            let mut vert_idx = 0u32;

            while (vert_idx as usize) < positions.len() {
                indices.push(vert_idx);
                vert_idx += 1;
            }

            indices
        }
    };
    let indices = expand_indices(primitive.mode(), &raw_indices, positions.len())?;

    if indices.is_empty() {
        return Ok(None);
    }

    Ok(Some(PrimRec {
        node,
        skin,
        positions,
        normals,
        uvs,
        joints,
        weights,
        indices,
        material: primitive.material().index(),
    }))
}

fn expand_indices(mode: gltf::mesh::Mode, indices: &[u32], vertex_count: usize) -> Result<Vec<u32>, String> {
    let mut out = Vec::new();
    let mut idx = 0;

    while idx < indices.len() {
        if indices[idx] as usize >= vertex_count {
            return Err("gltf index".to_string());
        }

        idx += 1;
    }

    match mode {
        gltf::mesh::Mode::Triangles => {
            let mut idx = 0;

            while idx + 2 < indices.len() {
                out.extend_from_slice(&indices[idx..idx + 3]);
                idx += 3;
            }
        }
        gltf::mesh::Mode::TriangleStrip => {
            let mut idx = 0;

            while idx + 2 < indices.len() {
                if idx % 2 == 0 {
                    out.extend_from_slice(&[indices[idx], indices[idx + 1], indices[idx + 2]]);
                } else {
                    out.extend_from_slice(&[indices[idx + 1], indices[idx], indices[idx + 2]]);
                }

                idx += 1;
            }
        }
        gltf::mesh::Mode::TriangleFan => {
            let mut idx = 1;

            while idx + 1 < indices.len() {
                out.extend_from_slice(&[indices[0], indices[idx], indices[idx + 1]]);
                idx += 1;
            }
        }
        _ => {}
    }

    if out.len() > MAX_INDICES {
        return Err("gltf index".to_string());
    }

    Ok(out)
}

fn read_skins(gltf: &gltf::Gltf, buffers: &[Vec<u8>]) -> Result<Vec<SkinRec>, String> {
    let mut skins = Vec::new();

    for skin in gltf.skins() {
        if skin.index() != skins.len() {
            return Err("gltf skin".to_string());
        }

        let joints = skin.joints().map(|node| node.index()).collect::<Vec<_>>();
        let get_buffer = |buffer: gltf::Buffer<'_>| buffers.get(buffer.index()).map(Vec::as_slice);
        let ibms = match skin.reader(get_buffer).read_inverse_bind_matrices() {
            Some(iter) => iter.map(flatten_mat).collect::<Vec<_>>(),
            None => Vec::new(),
        };

        if !ibms.is_empty() && ibms.len() < joints.len() {
            return Err("gltf skin".to_string());
        }

        skins.push(SkinRec { joints, ibms });
    }

    Ok(skins)
}

fn read_anims(gltf: &gltf::Gltf, buffers: &[Vec<u8>]) -> Result<Vec<AnimRec>, String> {
    let mut anims = Vec::new();
    let mut idx = 0;

    for anim in gltf.animations() {
        let mut channels = Vec::new();

        for channel in anim.channels() {
            if let Some(rec) = read_channel(&channel, buffers)? {
                channels.push(rec);
            }
        }

        let name = anim.name().unwrap_or("").to_string();
        let name = if name.is_empty() {
            format!("anim_{idx}")
        } else {
            name
        };
        anims.push(AnimRec { name, channels });
        idx += 1;
    }

    Ok(anims)
}

fn read_channel(
    channel: &gltf::animation::Channel,
    buffers: &[Vec<u8>],
) -> Result<Option<ChannelRec>, String> {
    let property = channel.target().property();

    if matches!(
        property,
        gltf::animation::Property::Scale | gltf::animation::Property::MorphTargetWeights
    ) {
        return Ok(None);
    }

    let get_buffer = |buffer: gltf::Buffer<'_>| buffers.get(buffer.index()).map(Vec::as_slice);
    let reader = channel.reader(get_buffer);
    let times = reader
        .read_inputs()
        .ok_or_else(|| "gltf keys".to_string())?
        .collect::<Vec<_>>();
    let outputs = reader.read_outputs().ok_or_else(|| "gltf keys".to_string())?;

    if times.is_empty() || times.len() > MAX_KEYS || times.iter().any(|time| !time.is_finite()) {
        return Err("gltf keys".to_string());
    }

    let mut idx = 1;

    while idx < times.len() {
        if times[idx] < times[idx - 1] {
            return Err("gltf keys".to_string());
        }

        idx += 1;
    }

    let interpolation = channel.sampler().interpolation();
    let node = channel.target().node().index();
    let rec = match property {
        gltf::animation::Property::Translation => {
            let values = match outputs {
                gltf::animation::util::ReadOutputs::Translations(values) => values.collect::<Vec<_>>(),
                _ => return Err("gltf keys".to_string()),
            };
            let values = finish_vec3(interpolation, &times, values)?;

            ChannelRec {
                node,
                times: values.0,
                translation: Some(values.1),
                rotation: None,
            }
        }
        gltf::animation::Property::Rotation => {
            let values = match outputs {
                gltf::animation::util::ReadOutputs::Rotations(values) => values.into_f32().collect::<Vec<_>>(),
                _ => return Err("gltf keys".to_string()),
            };
            let values = finish_quat(interpolation, &times, values)?;

            ChannelRec {
                node,
                times: values.0,
                translation: None,
                rotation: Some(values.1),
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(rec))
}

fn finish_vec3(
    interpolation: gltf::animation::Interpolation,
    times: &[f32],
    values: Vec<[f32; 3]>,
) -> Result<(Vec<f32>, Vec<[f32; 3]>), String> {
    match interpolation {
        gltf::animation::Interpolation::Linear => {
            if values.len() != times.len() {
                return Err("gltf keys".to_string());
            }

            Ok((times.to_vec(), values))
        }
        gltf::animation::Interpolation::Step => {
            if values.len() != times.len() {
                return Err("gltf keys".to_string());
            }

            Ok(step_keys(times, &values))
        }
        gltf::animation::Interpolation::CubicSpline => sample_cubic3(times, &values),
    }
}

fn finish_quat(
    interpolation: gltf::animation::Interpolation,
    times: &[f32],
    values: Vec<[f32; 4]>,
) -> Result<(Vec<f32>, Vec<[f32; 4]>), String> {
    match interpolation {
        gltf::animation::Interpolation::Linear => {
            if values.len() != times.len() {
                return Err("gltf keys".to_string());
            }

            Ok((times.to_vec(), values))
        }
        gltf::animation::Interpolation::Step => {
            if values.len() != times.len() {
                return Err("gltf keys".to_string());
            }

            Ok(step_keys(times, &values))
        }
        gltf::animation::Interpolation::CubicSpline => sample_cubic4(times, &values),
    }
}

fn step_keys<T: Copy>(times: &[f32], values: &[T]) -> (Vec<f32>, Vec<T>) {
    let mut out_times = Vec::new();
    let mut out_values = Vec::new();
    let mut idx = 0;

    while idx < times.len() {
        out_times.push(times[idx]);
        out_values.push(values[idx]);

        if idx + 1 < times.len() {
            let span = times[idx + 1] - times[idx];
            let eps = (span * 0.5).min(1.0e-4);

            if eps > 0.0 {
                out_times.push(times[idx + 1] - eps);
                out_values.push(values[idx]);
            }
        }

        idx += 1;
    }

    (out_times, out_values)
}

fn sample_cubic3(times: &[f32], values: &[[f32; 3]]) -> Result<(Vec<f32>, Vec<[f32; 3]>), String> {
    if values.len() != times.len() * 3 || times.len() < 2 {
        return Err("gltf keys".to_string());
    }

    let frames = sample_count(times);
    let mut out_times = Vec::with_capacity(frames);
    let mut out_values = Vec::with_capacity(frames);
    let start = times[0];
    let end = times[times.len() - 1];
    let mut idx = 0;

    while idx < frames {
        let time = if frames == 1 {
            start
        } else {
            start + (end - start) * (idx as f32) / (frames as f32 - 1.0)
        };
        out_times.push(time);
        out_values.push(cubic_at3(times, values, time));
        idx += 1;
    }

    Ok((out_times, out_values))
}

fn sample_cubic4(times: &[f32], values: &[[f32; 4]]) -> Result<(Vec<f32>, Vec<[f32; 4]>), String> {
    if values.len() != times.len() * 3 || times.len() < 2 {
        return Err("gltf keys".to_string());
    }

    let frames = sample_count(times);
    let mut out_times = Vec::with_capacity(frames);
    let mut out_values = Vec::with_capacity(frames);
    let start = times[0];
    let end = times[times.len() - 1];
    let mut idx = 0;

    while idx < frames {
        let time = if frames == 1 {
            start
        } else {
            start + (end - start) * (idx as f32) / (frames as f32 - 1.0)
        };
        out_times.push(time);
        out_values.push(normalize_quat(cubic_at4(times, values, time)));
        idx += 1;
    }

    Ok((out_times, out_values))
}

fn sample_count(times: &[f32]) -> usize {
    let duration = times[times.len() - 1] - times[0];
    let count = (duration * 30.0).ceil() as usize + 1;

    count.clamp(2, 4096)
}

fn cubic_span(times: &[f32], time: f32) -> (usize, f32) {
    if time <= times[0] {
        return (0, 0.0);
    }

    let last = times.len() - 1;

    if time >= times[last] {
        return (last - 1, 1.0);
    }

    let mut lo = 0;
    let mut hi = last;

    while hi - lo > 1 {
        let mid = (lo + hi) / 2;

        if times[mid] <= time {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    let span = times[hi] - times[lo];
    let u = if span > 1.0e-6 {
        ((time - times[lo]) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };

    (lo, u)
}

fn cubic_at3(times: &[f32], values: &[[f32; 3]], time: f32) -> [f32; 3] {
    let (idx, u) = cubic_span(times, time);
    let span = times[idx + 1] - times[idx];
    let p0 = values[idx * 3 + 1];
    let m0 = mul_scalar(values[idx * 3 + 2], span);
    let p1 = values[(idx + 1) * 3 + 1];
    let m1 = mul_scalar(values[(idx + 1) * 3], span);
    hermite3(p0, m0, p1, m1, u)
}

fn cubic_at4(times: &[f32], values: &[[f32; 4]], time: f32) -> [f32; 4] {
    let (idx, u) = cubic_span(times, time);
    let span = times[idx + 1] - times[idx];
    let p0 = values[idx * 3 + 1];
    let m0 = mul_scalar4(values[idx * 3 + 2], span);
    let p1 = values[(idx + 1) * 3 + 1];
    let m1 = mul_scalar4(values[(idx + 1) * 3], span);
    hermite4(p0, m0, p1, m1, u)
}

fn hermite3(p0: [f32; 3], m0: [f32; 3], p1: [f32; 3], m1: [f32; 3], t: f32) -> [f32; 3] {
    let (h00, h10, h01, h11) = hermite_basis(t);

    [
        h00 * p0[0] + h10 * m0[0] + h01 * p1[0] + h11 * m1[0],
        h00 * p0[1] + h10 * m0[1] + h01 * p1[1] + h11 * m1[1],
        h00 * p0[2] + h10 * m0[2] + h01 * p1[2] + h11 * m1[2],
    ]
}

fn hermite4(p0: [f32; 4], m0: [f32; 4], p1: [f32; 4], m1: [f32; 4], t: f32) -> [f32; 4] {
    let (h00, h10, h01, h11) = hermite_basis(t);

    [
        h00 * p0[0] + h10 * m0[0] + h01 * p1[0] + h11 * m1[0],
        h00 * p0[1] + h10 * m0[1] + h01 * p1[1] + h11 * m1[1],
        h00 * p0[2] + h10 * m0[2] + h01 * p1[2] + h11 * m1[2],
        h00 * p0[3] + h10 * m0[3] + h01 * p1[3] + h11 * m1[3],
    ]
}

fn hermite_basis(t: f32) -> (f32, f32, f32, f32) {
    let t2 = t * t;
    let t3 = t2 * t;

    (2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + t, -2.0 * t3 + 3.0 * t2, t3 - t2)
}

fn read_materials(gltf: &gltf::Gltf) -> Vec<MaterialRec> {
    gltf.materials()
        .map(|material| {
            let pbr = material.pbr_metallic_roughness();
            let image = pbr
                .base_color_texture()
                .map(|info| info.texture().source().index());

            MaterialRec {
                factor: pbr.base_color_factor(),
                image,
            }
        })
        .collect()
}

fn read_images(
    gltf: &gltf::Gltf,
    buffers: &[Vec<u8>],
    read_uri: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<Rgba>, String> {
    let mut images = Vec::new();

    for image in gltf.images() {
        if image.index() != images.len() {
            return Err("gltf image".to_string());
        }

        let (bytes, mime) = match image.source() {
            gltf::image::Source::View { view, mime_type } => {
                let buffer = buffers.get(view.buffer().index()).ok_or_else(|| "gltf image".to_string())?;
                let start = view.offset();
                let end = start.checked_add(view.length()).ok_or_else(|| "gltf image".to_string())?;

                if end > buffer.len() {
                    return Err("gltf image".to_string());
                }

                (buffer[start..end].to_vec(), mime_type.to_string())
            }
            gltf::image::Source::Uri { uri, mime_type } => {
                let bytes = fetch_uri(uri, read_uri)?;
                let mime = mime_type.unwrap_or("").to_string();

                (bytes, mime)
            }
        };
        images.push(decode_image(&bytes, &mime)?);
    }

    Ok(images)
}

fn decode_image(bytes: &[u8], mime: &str) -> Result<Rgba, String> {
    let mime = mime.to_ascii_lowercase();

    if mime.contains("png") || bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return decode_png(bytes);
    }

    if mime.contains("jpeg") || mime.contains("jpg") || bytes.starts_with(&[0xFF, 0xD8]) {
        return decode_jpeg(bytes);
    }

    Err("gltf image".to_string())
}

fn decode_png(bytes: &[u8]) -> Result<Rgba, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    decoder.set_limits(png::Limits { bytes: MAX_ALBEDO });
    let mut reader = decoder.read_info().map_err(|err| format!("gltf png {err}"))?;
    let (width, height) = {
        let info = reader.info();

        (info.width, info.height)
    };

    if width == 0 || height == 0 || (width as usize).saturating_mul(height as usize).saturating_mul(4) > MAX_ALBEDO {
        return Err("gltf image".to_string());
    }

    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).map_err(|err| format!("gltf png {err}"))?;

    if frame.bit_depth != png::BitDepth::Eight {
        return Err("gltf image".to_string());
    }

    let pixels = rgba_from_samples(&buf[..frame.buffer_size()], frame.width, frame.height, frame.color_type)?;

    Ok(Rgba {
        w: frame.width,
        h: frame.height,
        pixels,
    })
}

fn rgba_from_samples(samples: &[u8], width: u32, height: u32, color: png::ColorType) -> Result<Vec<u8>, String> {
    let count = (width as usize).saturating_mul(height as usize);
    let mut pixels = Vec::with_capacity(count * 4);

    match color {
        png::ColorType::Rgba => {
            if samples.len() < count * 4 {
                return Err("gltf image".to_string());
            }

            pixels.extend_from_slice(&samples[..count * 4]);
        }
        png::ColorType::Rgb => {
            if samples.len() < count * 3 {
                return Err("gltf image".to_string());
            }

            let mut idx = 0;

            while idx < count {
                let at = idx * 3;
                pixels.extend_from_slice(&[samples[at], samples[at + 1], samples[at + 2], 255]);
                idx += 1;
            }
        }
        png::ColorType::GrayscaleAlpha => {
            if samples.len() < count * 2 {
                return Err("gltf image".to_string());
            }

            let mut idx = 0;

            while idx < count {
                let at = idx * 2;
                pixels.extend_from_slice(&[samples[at], samples[at], samples[at], samples[at + 1]]);
                idx += 1;
            }
        }
        png::ColorType::Grayscale => {
            if samples.len() < count {
                return Err("gltf image".to_string());
            }

            let mut idx = 0;

            while idx < count {
                let value = samples[idx];
                pixels.extend_from_slice(&[value, value, value, 255]);
                idx += 1;
            }
        }
        png::ColorType::Indexed => return Err("gltf image".to_string()),
    }

    Ok(pixels)
}

fn decode_jpeg(bytes: &[u8]) -> Result<Rgba, String> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    let pixels = decoder.decode().map_err(|err| format!("gltf jpeg {err}"))?;
    let info = decoder.info().ok_or_else(|| "gltf jpeg".to_string())?;
    let width = info.width as u32;
    let height = info.height as u32;

    if width == 0 || height == 0 || (width as usize).saturating_mul(height as usize).saturating_mul(4) > MAX_ALBEDO {
        return Err("gltf image".to_string());
    }

    let count = (width as usize) * (height as usize);
    let mut rgba = Vec::with_capacity(count * 4);

    match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            if pixels.len() < count * 3 {
                return Err("gltf jpeg".to_string());
            }

            let mut idx = 0;

            while idx < count {
                let at = idx * 3;
                rgba.extend_from_slice(&[pixels[at], pixels[at + 1], pixels[at + 2], 255]);
                idx += 1;
            }
        }
        jpeg_decoder::PixelFormat::L8 => {
            if pixels.len() < count {
                return Err("gltf jpeg".to_string());
            }

            let mut idx = 0;

            while idx < count {
                let value = pixels[idx];
                rgba.extend_from_slice(&[value, value, value, 255]);
                idx += 1;
            }
        }
        jpeg_decoder::PixelFormat::L16 => {
            if pixels.len() < count * 2 {
                return Err("gltf jpeg".to_string());
            }

            let mut idx = 0;

            while idx < count {
                let value = pixels[idx * 2 + 1];
                rgba.extend_from_slice(&[value, value, value, 255]);
                idx += 1;
            }
        }
        jpeg_decoder::PixelFormat::CMYK32 => return Err("gltf jpeg".to_string()),
    }

    Ok(Rgba {
        w: width,
        h: height,
        pixels: rgba,
    })
}

fn bake_tree(nodes: &mut [NodeRec]) -> Result<bool, String> {
    let mut state = vec![0u8; nodes.len()];
    let mut baked = false;
    let mut idx = 0;

    while idx < nodes.len() {
        if nodes[idx].parent.is_none() {
            bake_node(nodes, idx, 1.0, &mut state, &mut baked)?;
        }

        idx += 1;
    }

    idx = 0;

    while idx < nodes.len() {
        if state[idx] != 2 {
            return Err("gltf hierarchy".to_string());
        }

        idx += 1;
    }

    Ok(baked)
}

fn bake_node(
    nodes: &mut [NodeRec],
    idx: usize,
    accumulated: f32,
    state: &mut [u8],
    baked: &mut bool,
) -> Result<(), String> {
    if state[idx] == 1 {
        return Err("gltf hierarchy".to_string());
    }

    if state[idx] == 2 {
        return Ok(());
    }

    state[idx] = 1;
    let scale = nodes[idx].scale;

    if !scale_ok(scale) {
        return Err("gltf non-uniform scale".to_string());
    }

    if (scale[0] - 1.0).abs() > 1.0e-4 {
        *baked = true;
    }

    nodes[idx].key_scale = accumulated;
    nodes[idx].vertex_scale = scale[0];
    nodes[idx].translation = mul_scalar(nodes[idx].translation, accumulated);
    nodes[idx].scale = [1.0, 1.0, 1.0];
    let next = accumulated * scale[0];
    let children = nodes[idx].children.clone();
    let mut child_idx = 0;

    while child_idx < children.len() {
        bake_node(nodes, children[child_idx], next, state, baked)?;
        child_idx += 1;
    }

    state[idx] = 2;

    Ok(())
}

fn scale_ok(scale: [f32; 3]) -> bool {
    scale[0].is_finite()
        && scale[1].is_finite()
        && scale[2].is_finite()
        && scale[0] > 1.0e-6
        && (scale[0] - scale[1]).abs() <= 1.0e-3 * scale[0].abs().max(1.0)
        && (scale[1] - scale[2]).abs() <= 1.0e-3 * scale[0].abs().max(1.0)
}

fn convert_nodes(nodes: &mut [NodeRec]) {
    let mut idx = 0;

    while idx < nodes.len() {
        nodes[idx].translation = convert_point(nodes[idx].translation);
        nodes[idx].rotation = convert_quat(nodes[idx].rotation);
        idx += 1;
    }
}

fn build_bones(
    nodes: &[NodeRec],
    prims: &[PrimRec],
    skins: &[SkinRec],
) -> Result<(Vec<Bone>, Vec<Option<usize>>), String> {
    let mut used = vec![false; nodes.len()];
    let mut idx = 0;

    while idx < prims.len() {
        if let Some(skin) = prims[idx].skin {
            let skin = skins.get(skin).ok_or_else(|| "gltf skin".to_string())?;
            let mut joint_idx = 0;

            while joint_idx < skin.joints.len() {
                mark_chain(skin.joints[joint_idx], nodes, &mut used)?;
                joint_idx += 1;
            }
        } else {
            mark_chain(prims[idx].node, nodes, &mut used)?;
        }

        idx += 1;
    }

    let mut order = Vec::new();
    let mut state = vec![0u8; nodes.len()];
    idx = 0;

    while idx < nodes.len() {
        if used[idx] {
            append_bone(idx, nodes, &used, &mut state, &mut order)?;
        }

        idx += 1;
    }

    if order.is_empty() || order.len() > MAX_BONES {
        return Err("gltf bones".to_string());
    }

    let mut used_names = HashSet::new();
    let mut bones = Vec::with_capacity(order.len());
    let mut node_bone = vec![None; nodes.len()];
    idx = 0;

    while idx < order.len() {
        let node_idx = order[idx];
        let parent = match nodes[node_idx].parent {
            Some(parent) if used[parent] => node_bone[parent].ok_or_else(|| "gltf bones".to_string())? as i16,
            _ => -1,
        };

        if parent >= idx as i16 {
            return Err("gltf bones".to_string());
        }

        let name = unique_name(&nodes[node_idx].name, node_idx, &mut used_names);
        node_bone[node_idx] = Some(idx);
        bones.push(Bone {
            name,
            parent,
            inverse_bind: pose::IDENTITY,
            local_pos: nodes[node_idx].translation,
            local_rot: nodes[node_idx].rotation,
        });
        idx += 1;
    }

    Ok((bones, node_bone))
}

fn mark_chain(mut idx: usize, nodes: &[NodeRec], used: &mut [bool]) -> Result<(), String> {
    let mut guard = 0;

    loop {
        if idx >= nodes.len() {
            return Err("gltf bones".to_string());
        }

        if used[idx] {
            return Ok(());
        }

        used[idx] = true;
        guard += 1;

        if guard > nodes.len() {
            return Err("gltf hierarchy".to_string());
        }

        match nodes[idx].parent {
            Some(parent) => idx = parent,
            None => return Ok(()),
        }
    }
}

fn append_bone(
    idx: usize,
    nodes: &[NodeRec],
    used: &[bool],
    state: &mut [u8],
    order: &mut Vec<usize>,
) -> Result<(), String> {
    if state[idx] == 2 {
        return Ok(());
    }

    if state[idx] == 1 {
        return Err("gltf hierarchy".to_string());
    }

    state[idx] = 1;

    if let Some(parent) = nodes[idx].parent {
        if used[parent] {
            append_bone(parent, nodes, used, state, order)?;
        }
    }

    order.push(idx);
    state[idx] = 2;

    Ok(())
}

fn assign_binds(
    mut bones: Vec<Bone>,
    node_bone: &[Option<usize>],
    skins: &[SkinRec],
    baked: bool,
) -> Result<Vec<Bone>, String> {
    if baked {
        let worlds = bone_worlds(&bones);
        let mut idx = 0;

        while idx < bones.len() {
            bones[idx].inverse_bind = invert_affine(worlds[idx])?;
            idx += 1;
        }

        return Ok(bones);
    }

    let mut idx = 0;

    while idx < skins.len() {
        if skins[idx].ibms.is_empty() {
            idx += 1;

            continue;
        }

        let mut joint_idx = 0;

        while joint_idx < skins[idx].joints.len() {
            let node = skins[idx].joints[joint_idx];
            let bone = node_bone.get(node).and_then(|bone| *bone).ok_or_else(|| "gltf joint".to_string())?;
            bones[bone].inverse_bind = convert_mat(skins[idx].ibms[joint_idx]);
            joint_idx += 1;
        }

        idx += 1;
    }

    Ok(bones)
}

fn bone_worlds(bones: &[Bone]) -> Vec<[f32; 16]> {
    let mut worlds = vec![pose::IDENTITY; bones.len()];
    let mut idx = 0;

    while idx < bones.len() {
        let local = pose::trs(bones[idx].local_pos, bones[idx].local_rot);
        let parent = bones[idx].parent;
        worlds[idx] = if parent >= 0 {
            pose::mul_mat(worlds[parent as usize], local)
        } else {
            local
        };
        idx += 1;
    }

    worlds
}

fn build_atlas(
    prims: &[PrimRec],
    materials: &[MaterialRec],
    images: &[Rgba],
) -> Result<(Rgba, Vec<(Option<usize>, Rect)>), String> {
    let mut slots = Vec::new();
    let mut idx = 0;

    while idx < prims.len() {
        let material = prims[idx].material;

        if !slots.contains(&material) {
            slots.push(material);
        }

        idx += 1;
    }

    if slots.is_empty() {
        slots.push(None);
    }

    let mut placed = Vec::new();
    idx = 0;

    while idx < slots.len() {
        let image = material_image(slots[idx], materials, images)?;
        placed.push((slots[idx], image));
        idx += 1;
    }

    pack_atlas(placed)
}

fn material_image(material: Option<usize>, materials: &[MaterialRec], images: &[Rgba]) -> Result<Rgba, String> {
    let Some(index) = material else {
        return Ok(solid([1.0, 1.0, 1.0, 1.0]));
    };
    let material = materials.get(index).ok_or_else(|| "gltf material".to_string())?;
    let mut image = match material.image {
        Some(image) => images.get(image).ok_or_else(|| "gltf image".to_string())?.clone(),
        None => return Ok(solid(material.factor)),
    };
    tint(&mut image, material.factor);

    Ok(image)
}

fn solid(factor: [f32; 4]) -> Rgba {
    Rgba {
        w: 1,
        h: 1,
        pixels: vec![
            scale_byte(255, factor[0]),
            scale_byte(255, factor[1]),
            scale_byte(255, factor[2]),
            scale_byte(255, factor[3]),
        ],
    }
}

fn tint(image: &mut Rgba, factor: [f32; 4]) {
    if (factor[0] - 1.0).abs() < 1.0e-4
        && (factor[1] - 1.0).abs() < 1.0e-4
        && (factor[2] - 1.0).abs() < 1.0e-4
        && (factor[3] - 1.0).abs() < 1.0e-4
    {
        return;
    }

    let mut idx = 0;

    while idx + 3 < image.pixels.len() {
        image.pixels[idx] = scale_byte(image.pixels[idx], factor[0]);
        image.pixels[idx + 1] = scale_byte(image.pixels[idx + 1], factor[1]);
        image.pixels[idx + 2] = scale_byte(image.pixels[idx + 2], factor[2]);
        image.pixels[idx + 3] = scale_byte(image.pixels[idx + 3], factor[3]);
        idx += 4;
    }
}

fn scale_byte(value: u8, factor: f32) -> u8 {
    if !factor.is_finite() {
        return 0;
    }

    (value as f32 * factor).round().clamp(0.0, 255.0) as u8
}

fn pack_atlas(images: Vec<(Option<usize>, Rgba)>) -> Result<(Rgba, Vec<(Option<usize>, Rect)>), String> {
    let mut x = 0u32;
    let mut y = 0u32;
    let mut row_h = 0u32;
    let mut width = 0u32;
    let mut rects = Vec::new();
    let mut idx = 0;

    while idx < images.len() {
        let image = &images[idx].1;

        if image.w == 0 || image.h == 0 || image.w > 2048 || image.h > 2048 {
            return Err("gltf image".to_string());
        }

        if x > 0 && x + image.w > 2048 {
            y = y.saturating_add(row_h);
            x = 0;
            row_h = 0;
        }

        rects.push((
            images[idx].0,
            Rect {
                x,
                y,
                w: image.w,
                h: image.h,
            },
        ));
        x += image.w;
        width = width.max(x);
        row_h = row_h.max(image.h);
        idx += 1;
    }

    let height = y.saturating_add(row_h).max(1);
    let width = width.max(1);
    let bytes = (width as usize).saturating_mul(height as usize).saturating_mul(4);

    if bytes == 0 || bytes > MAX_ALBEDO {
        return Err("gltf albedo".to_string());
    }

    let mut pixels = vec![255u8; bytes];
    idx = 0;

    while idx < images.len() {
        blit(&mut pixels, width, &rects[idx].1, &images[idx].1);
        idx += 1;
    }

    Ok((
        Rgba {
            w: width,
            h: height,
            pixels,
        },
        rects,
    ))
}

fn blit(atlas: &mut [u8], atlas_w: u32, rect: &Rect, image: &Rgba) {
    let mut y = 0u32;

    while y < rect.h {
        let mut x = 0u32;

        while x < rect.w {
            let src = ((y as usize) * (image.w as usize) + x as usize) * 4;
            let dst = (((rect.y + y) as usize) * (atlas_w as usize) + (rect.x + x) as usize) * 4;

            if src + 3 < image.pixels.len() && dst + 3 < atlas.len() {
                atlas[dst] = image.pixels[src];
                atlas[dst + 1] = image.pixels[src + 1];
                atlas[dst + 2] = image.pixels[src + 2];
                atlas[dst + 3] = image.pixels[src + 3];
            }

            x += 1;
        }

        y += 1;
    }
}

fn build_mesh(
    nodes: &[NodeRec],
    prims: &[PrimRec],
    skins: &[SkinRec],
    bones: &[Bone],
    node_bone: &[Option<usize>],
    rects: &[(Option<usize>, Rect)],
    atlas: Rgba,
) -> Result<Mesh, String> {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut idx = 0;

    while idx < prims.len() {
        append_prim(
            &prims[idx],
            nodes,
            skins,
            bones,
            node_bone,
            rects,
            atlas.w,
            atlas.h,
            &mut vertices,
            &mut indices,
        )?;
        idx += 1;
    }

    if vertices.is_empty() || indices.is_empty() {
        return Err("gltf mesh".to_string());
    }

    Ok(Mesh {
        bones: bones.to_vec(),
        vertices,
        indices,
        albedo_w: atlas.w,
        albedo_h: atlas.h,
        albedo: atlas.pixels,
    })
}

fn append_prim(
    prim: &PrimRec,
    nodes: &[NodeRec],
    skins: &[SkinRec],
    bones: &[Bone],
    node_bone: &[Option<usize>],
    rects: &[(Option<usize>, Rect)],
    atlas_w: u32,
    atlas_h: u32,
    vertices: &mut Vec<f32>,
    indices_out: &mut Vec<u32>,
) -> Result<(), String> {
    let base = vertices.len() / 16;

    if base + prim.positions.len() > MAX_VERTICES || indices_out.len() + prim.indices.len() > MAX_INDICES {
        return Err("gltf mesh".to_string());
    }

    let rect = rects
        .iter()
        .find(|(material, _)| *material == prim.material)
        .map(|(_, rect)| rect)
        .ok_or_else(|| "gltf material".to_string())?;
    let mut positions = Vec::with_capacity(prim.positions.len());
    let mut vert_idx = 0;

    while vert_idx < prim.positions.len() {
        let mut position = prim.positions[vert_idx];

        if prim.skin.is_none() {
            position = mul_scalar(position, nodes[prim.node].vertex_scale);
        }

        positions.push(convert_point(position));
        vert_idx += 1;
    }

    let mut normals = match &prim.normals {
        Some(normals) => {
            let mut converted = Vec::with_capacity(normals.len());
            let mut vert_idx = 0;

            while vert_idx < normals.len() {
                converted.push(normalize3(convert_point(normals[vert_idx])));
                vert_idx += 1;
            }

            converted
        }
        None => Vec::new(),
    };

    if prim.skin.is_none() {
        let bone = node_bone.get(prim.node).and_then(|bone| *bone).ok_or_else(|| "gltf bones".to_string())?;
        let inverse = bones[bone].inverse_bind;

        if !is_identity(inverse) {
            let pose = invert_affine(inverse)?;
            let mut vert_idx = 0;

            while vert_idx < positions.len() {
                positions[vert_idx] = mul_point(pose, positions[vert_idx]);

                if vert_idx < normals.len() {
                    normals[vert_idx] = normalize3(mul_transpose_dir(inverse, normals[vert_idx]));
                }

                vert_idx += 1;
            }
        }
    }

    if normals.len() != positions.len() {
        normals = generate_normals(&positions, &prim.indices);
    }

    let influences = if prim.skin.is_some() {
        skin_influences(prim, skins, node_bone)?
    } else {
        let bone = node_bone[prim.node].ok_or_else(|| "gltf bones".to_string())? as f32;
        vec![([bone, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]); positions.len()]
    };
    let mut vert_idx = 0;

    while vert_idx < positions.len() {
        let uv = remap_uv(prim.uvs[vert_idx], rect, atlas_w, atlas_h);
        push_vertex(vertices, positions[vert_idx], normals[vert_idx], uv, influences[vert_idx].0, influences[vert_idx].1);
        vert_idx += 1;
    }

    let mut index_idx = 0;

    while index_idx < prim.indices.len() {
        indices_out.push(prim.indices[index_idx] + base as u32);
        index_idx += 1;
    }

    Ok(())
}

fn skin_influences(
    prim: &PrimRec,
    skins: &[SkinRec],
    node_bone: &[Option<usize>],
) -> Result<Vec<([f32; 4], [f32; 4])>, String> {
    let skin = skins.get(prim.skin.unwrap_or(usize::MAX)).ok_or_else(|| "gltf skin".to_string())?;
    let mut out = Vec::with_capacity(prim.positions.len());
    let mut vert_idx = 0;

    while vert_idx < prim.positions.len() {
        let mut pairs = Vec::new();
        let mut set = 0;

        while set < prim.joints.len() {
            let mut slot = 0;

            while slot < 4 {
                let joint = prim.joints[set][vert_idx][slot] as usize;
                let weight = prim.weights[set][vert_idx][slot];

                if weight > 0.0 {
                    if joint >= skin.joints.len() {
                        return Err("gltf joint".to_string());
                    }

                    let node = skin.joints[joint];
                    let bone = node_bone.get(node).and_then(|bone| *bone).ok_or_else(|| "gltf joint".to_string())?;
                    pairs.push((bone as u32, weight));
                }

                slot += 1;
            }

            set += 1;
        }

        out.push(pack_influences(&pairs));
        vert_idx += 1;
    }

    Ok(out)
}

fn pack_influences(pairs: &[(u32, f32)]) -> ([f32; 4], [f32; 4]) {
    let mut best = [(0u32, 0.0f32); 4];
    let mut idx = 0;

    while idx < pairs.len() {
        let pair = pairs[idx];

        if pair.1.is_finite() && pair.1 > best[3].1 {
            best[3] = (pair.0, pair.1);
            let mut slot = 3;

            while slot > 0 && best[slot].1 > best[slot - 1].1 {
                best.swap(slot, slot - 1);
                slot -= 1;
            }
        }

        idx += 1;
    }

    let mut sum = 0.0;
    idx = 0;

    while idx < 4 {
        sum += best[idx].1;
        idx += 1;
    }

    if sum <= 1.0e-8 {
        return ([0.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]);
    }

    let inv = 1.0 / sum;

    (
        [best[0].0 as f32, best[1].0 as f32, best[2].0 as f32, best[3].0 as f32],
        [best[0].1 * inv, best[1].1 * inv, best[2].1 * inv, best[3].1 * inv],
    )
}

fn generate_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0, 0.0, 0.0]; positions.len()];
    let mut idx = 0;

    while idx + 2 < indices.len() {
        let a = indices[idx] as usize;
        let b = indices[idx + 1] as usize;
        let c = indices[idx + 2] as usize;

        if a < positions.len() && b < positions.len() && c < positions.len() {
            let ab = sub3(positions[b], positions[a]);
            let ac = sub3(positions[c], positions[a]);
            let face = cross(ab, ac);
            normals[a] = add3(normals[a], face);
            normals[b] = add3(normals[b], face);
            normals[c] = add3(normals[c], face);
        }

        idx += 3;
    }

    let mut vert_idx = 0;

    while vert_idx < normals.len() {
        normals[vert_idx] = normalize3(normals[vert_idx]);
        vert_idx += 1;
    }

    normals
}

fn build_clips(
    nodes: &[NodeRec],
    bones: &[Bone],
    node_bone: &[Option<usize>],
    anims: &[AnimRec],
) -> Result<ClipSet, String> {
    let mut names = HashSet::new();
    let bone_names = bones.iter().map(|bone| bone.name.clone()).collect::<Vec<_>>();
    let mut sequences = Vec::new();
    let mut idx = 0;

    while idx < anims.len() {
        let sequence = build_sequence(&anims[idx], idx, nodes, bones, node_bone, &mut names)?;
        sequences.push(sequence);
        idx += 1;
    }

    Ok(ClipSet {
        bones: bone_names,
        sequences,
    })
}

fn build_sequence(
    anim: &AnimRec,
    index: usize,
    nodes: &[NodeRec],
    bones: &[Bone],
    node_bone: &[Option<usize>],
    names: &mut HashSet<String>,
) -> Result<Sequence, String> {
    let mut pos = vec![None; bones.len()];
    let mut rot = vec![None; bones.len()];
    let mut duration = 0.0f32;
    let mut idx = 0;

    while idx < anim.channels.len() {
        let channel = &anim.channels[idx];
        let bone = match node_bone.get(channel.node).and_then(|bone| *bone) {
            Some(bone) => bone,
            None => {
                idx += 1;

                continue;
            }
        };
        let node = nodes.get(channel.node).ok_or_else(|| "gltf node".to_string())?;

        if let Some(values) = &channel.translation {
            let mut times = Vec::with_capacity(channel.times.len());
            let mut converted = Vec::with_capacity(values.len());
            let mut key_idx = 0;

            while key_idx < channel.times.len() && key_idx < values.len() {
                times.push(channel.times[key_idx]);
                converted.push(convert_point(mul_scalar(values[key_idx], node.key_scale)));
                duration = duration.max(channel.times[key_idx]);
                key_idx += 1;
            }

            pos[bone] = Some((times, converted));
        }

        if let Some(values) = &channel.rotation {
            let mut times = Vec::with_capacity(channel.times.len());
            let mut converted = Vec::with_capacity(values.len());
            let mut key_idx = 0;

            while key_idx < channel.times.len() && key_idx < values.len() {
                times.push(channel.times[key_idx]);
                converted.push(convert_quat(values[key_idx]));
                duration = duration.max(channel.times[key_idx]);
                key_idx += 1;
            }

            rot[bone] = Some((times, converted));
        }

        idx += 1;
    }

    let mut tracks = Vec::with_capacity(bones.len());
    idx = 0;

    while idx < bones.len() {
        let (pos_times, pos_values) = pos[idx].clone().unwrap_or_else(|| (Vec::new(), Vec::new()));
        let (rot_times, rot_values) = rot[idx].clone().unwrap_or_else(|| (Vec::new(), Vec::new()));
        tracks.push(Track {
            pos_times,
            pos: pos_values,
            rot_times,
            rot: rot_values,
        });
        idx += 1;
    }

    let fallback = if anim.name.is_empty() {
        format!("anim_{index}")
    } else {
        anim.name.clone()
    };

    Ok(Sequence {
        name: unique_name(&fallback, index, names),
        flags: FLAG_LOOP,
        duration,
        events: Vec::new(),
        tracks,
    })
}

fn remap_uv(uv: [f32; 2], rect: &Rect, atlas_w: u32, atlas_h: u32) -> [f32; 2] {
    [
        (rect.x as f32 + uv[0] * rect.w as f32) / atlas_w.max(1) as f32,
        (rect.y as f32 + uv[1] * rect.h as f32) / atlas_h.max(1) as f32,
    ]
}

fn push_vertex(vertices: &mut Vec<f32>, position: [f32; 3], normal: [f32; 3], uv: [f32; 2], joints: [f32; 4], weights: [f32; 4]) {
    vertices.extend_from_slice(&[
        position[0],
        position[1],
        position[2],
        normal[0],
        normal[1],
        normal[2],
        uv[0],
        uv[1],
        joints[0],
        joints[1],
        joints[2],
        joints[3],
        weights[0],
        weights[1],
        weights[2],
        weights[3],
    ]);
}

fn fetch_uri(uri: &str, read_uri: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    if let Some(bytes) = decode_data_uri(uri)? {
        return Ok(bytes);
    }

    let path = clean_relative(uri)?;

    read_uri(&path)
}

fn decode_data_uri(uri: &str) -> Result<Option<Vec<u8>>, String> {
    let Some(rest) = uri.strip_prefix("data:") else {
        return Ok(None);
    };
    let (meta, data) = rest.split_once(',').ok_or_else(|| "gltf uri".to_string())?;

    if meta.to_ascii_lowercase().contains(";base64") {
        return Ok(Some(base64_decode(data)?));
    }

    let text = percent_decode(data)?;

    Ok(Some(text.into_bytes()))
}

fn clean_relative(uri: &str) -> Result<String, String> {
    let decoded = percent_decode(uri)?;

    if decoded.contains(':') || decoded.starts_with('/') || decoded.contains('\\') {
        return Err("gltf uri".to_string());
    }

    let mut parts = Vec::new();

    for part in decoded.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }

        if part == ".." {
            return Err("gltf uri".to_string());
        }

        parts.push(part);
    }

    if parts.is_empty() {
        return Err("gltf uri".to_string());
    }

    Ok(parts.join("/"))
}

fn join_uri(dir: &str, uri: &str) -> Result<String, String> {
    let relative = clean_relative(uri)?;

    if dir.is_empty() {
        return Ok(relative);
    }

    Ok(format!("{dir}/{relative}"))
}

fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(idx) => &path[..idx],
        None => "",
    }
}

fn percent_decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;

    while idx < bytes.len() {
        if bytes[idx] == b'%' {
            if idx + 2 >= bytes.len() {
                return Err("gltf uri".to_string());
            }

            let hi = hex_val(bytes[idx + 1])?;
            let lo = hex_val(bytes[idx + 2])?;
            out.push((hi << 4) | lo);
            idx += 3;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }

    String::from_utf8(out).map_err(|_| "gltf uri".to_string())
}

fn hex_val(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("gltf uri".to_string()),
    }
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut codes = Vec::new();

    for byte in text.bytes() {
        if byte.is_ascii_whitespace() || byte == b'=' {
            continue;
        }

        codes.push(b64_val(byte)?);
    }

    if codes.len() % 4 == 1 {
        return Err("gltf uri".to_string());
    }

    let mut out = Vec::with_capacity(codes.len() / 4 * 3);
    let mut idx = 0;

    while idx + 3 < codes.len() {
        let chunk = ((codes[idx] as u32) << 18)
            | ((codes[idx + 1] as u32) << 12)
            | ((codes[idx + 2] as u32) << 6)
            | (codes[idx + 3] as u32);
        out.push((chunk >> 16) as u8);
        out.push((chunk >> 8) as u8);
        out.push(chunk as u8);
        idx += 4;
    }

    if idx + 1 < codes.len() {
        let chunk = ((codes[idx] as u32) << 18) | ((codes[idx + 1] as u32) << 12);
        out.push((chunk >> 16) as u8);

        if idx + 2 < codes.len() {
            let chunk = chunk | ((codes[idx + 2] as u32) << 6);
            out.push((chunk >> 8) as u8);
        }
    }

    Ok(out)
}

fn b64_val(byte: u8) -> Result<u8, String> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' | b'-' => Ok(62),
        b'/' | b'_' => Ok(63),
        _ => Err("gltf uri".to_string()),
    }
}

fn unique_name(raw: &str, fallback: usize, used: &mut HashSet<String>) -> String {
    let base = if raw.is_empty() {
        format!("bone_{fallback}")
    } else {
        raw.to_string()
    };
    let mut name = clip_name(&base);
    let mut n = 2u32;

    while used.contains(&name) || name.is_empty() {
        let suffix = format!("_{n}");
        let room = MAX_NAME.saturating_sub(suffix.len());
        let cut = clip_len(&base, room);
        name = format!("{cut}{suffix}");
        n += 1;
    }

    used.insert(name.clone());

    name
}

fn clip_name(name: &str) -> String {
    clip_len(name, MAX_NAME)
}

fn clip_len(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }

    let mut end = max;

    while end > 0 && !name.is_char_boundary(end) {
        end -= 1;
    }

    name[..end].to_string()
}

fn convert_point(value: [f32; 3]) -> [f32; 3] {
    [value[2], value[0], value[1]]
}

fn convert_quat(value: [f32; 4]) -> [f32; 4] {
    let change = [0.5, 0.5, 0.5, 0.5];
    let inverse = [-0.5, -0.5, -0.5, 0.5];

    normalize_quat(pose::quat_mul(pose::quat_mul(change, normalize_quat(value)), inverse))
}

fn convert_mat(value: [f32; 16]) -> [f32; 16] {
    pose::mul_mat(pose::mul_mat(basis(), value), basis_inv())
}

fn basis() -> [f32; 16] {
    [
        0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn basis_inv() -> [f32; 16] {
    [
        0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn flatten_mat(value: [[f32; 4]; 4]) -> [f32; 16] {
    let mut out = [0.0; 16];
    let mut col = 0;

    while col < 4 {
        let mut row = 0;

        while row < 4 {
            out[col * 4 + row] = value[col][row];
            row += 1;
        }

        col += 1;
    }

    out
}

fn invert_affine(mat: [f32; 16]) -> Result<[f32; 16], String> {
    let a = mat[0];
    let b = mat[4];
    let c = mat[8];
    let d = mat[1];
    let e = mat[5];
    let f = mat[9];
    let g = mat[2];
    let h = mat[6];
    let i = mat[10];
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);

    if !det.is_finite() || det.abs() < 1.0e-8 {
        return Err("gltf matrix".to_string());
    }

    let inv_det = 1.0 / det;
    let mut out = [0.0; 16];
    out[0] = (e * i - f * h) * inv_det;
    out[1] = (f * g - d * i) * inv_det;
    out[2] = (d * h - e * g) * inv_det;
    out[4] = (c * h - b * i) * inv_det;
    out[5] = (a * i - c * g) * inv_det;
    out[6] = (b * g - a * h) * inv_det;
    out[8] = (b * f - c * e) * inv_det;
    out[9] = (c * d - a * f) * inv_det;
    out[10] = (a * e - b * d) * inv_det;
    let translation = [mat[12], mat[13], mat[14]];
    out[12] = -(out[0] * translation[0] + out[4] * translation[1] + out[8] * translation[2]);
    out[13] = -(out[1] * translation[0] + out[5] * translation[1] + out[9] * translation[2]);
    out[14] = -(out[2] * translation[0] + out[6] * translation[1] + out[10] * translation[2]);
    out[15] = 1.0;

    Ok(out)
}

fn is_identity(mat: [f32; 16]) -> bool {
    let mut idx = 0;

    while idx < 16 {
        let expected = if idx % 5 == 0 { 1.0 } else { 0.0 };

        if (mat[idx] - expected).abs() > 1.0e-5 {
            return false;
        }

        idx += 1;
    }

    true
}

fn mul_point(mat: [f32; 16], point: [f32; 3]) -> [f32; 3] {
    [
        mat[0] * point[0] + mat[4] * point[1] + mat[8] * point[2] + mat[12],
        mat[1] * point[0] + mat[5] * point[1] + mat[9] * point[2] + mat[13],
        mat[2] * point[0] + mat[6] * point[1] + mat[10] * point[2] + mat[14],
    ]
}

fn mul_transpose_dir(mat: [f32; 16], direction: [f32; 3]) -> [f32; 3] {
    [
        mat[0] * direction[0] + mat[1] * direction[1] + mat[2] * direction[2],
        mat[4] * direction[0] + mat[5] * direction[1] + mat[6] * direction[2],
        mat[8] * direction[0] + mat[9] * direction[1] + mat[10] * direction[2],
    ]
}

fn normalize_quat(value: [f32; 4]) -> [f32; 4] {
    let len = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2] + value[3] * value[3]).sqrt();

    if !len.is_finite() || len <= 1.0e-8 {
        return [0.0, 0.0, 0.0, 1.0];
    }

    let inv = 1.0 / len;

    [value[0] * inv, value[1] * inv, value[2] * inv, value[3] * inv]
}

fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let len = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();

    if !len.is_finite() || len <= 1.0e-8 {
        return [0.0, 0.0, 1.0];
    }

    let inv = 1.0 / len;

    [value[0] * inv, value[1] * inv, value[2] * inv]
}

fn mul_scalar(value: [f32; 3], scale: f32) -> [f32; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

fn mul_scalar4(value: [f32; 4], scale: f32) -> [f32; 4] {
    [value[0] * scale, value[1] * scale, value[2] * scale, value[3] * scale]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::pose::{sample_quat, FLAG_LOOP};

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-3
    }

    fn near3(a: [f32; 3], b: [f32; 3]) -> bool {
        near(a[0], b[0]) && near(a[1], b[1]) && near(a[2], b[2])
    }

    fn f32s(values: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::new();

        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        bytes
    }

    fn u16s(values: &[u16]) -> Vec<u8> {
        let mut bytes = Vec::new();

        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        bytes
    }

    struct Bin {
        bytes: Vec<u8>,
        views: Vec<(usize, usize)>,
    }

    impl Bin {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                views: Vec::new(),
            }
        }

        fn push(&mut self, data: &[u8]) -> usize {
            while self.bytes.len() % 4 != 0 {
                self.bytes.push(0);
            }

            let offset = self.bytes.len();
            self.bytes.extend_from_slice(data);
            self.views.push((offset, data.len()));

            self.views.len() - 1
        }

        fn views_json(&self) -> String {
            let mut out = String::from("[");
            let mut idx = 0;

            while idx < self.views.len() {
                if idx > 0 {
                    out.push(',');
                }

                out.push_str(&format!(
                    "{{\"buffer\":0,\"byteOffset\":{},\"byteLength\":{}}}",
                    self.views[idx].0, self.views[idx].1
                ));
                idx += 1;
            }

            out.push(']');

            out
        }
    }

    fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
        let mut json_bytes = json.as_bytes().to_vec();

        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }

        let mut bin_bytes = bin.to_vec();

        while bin_bytes.len() % 4 != 0 {
            bin_bytes.push(0);
        }

        let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
        let mut out = Vec::new();
        out.extend_from_slice(&0x46546C67u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        out.extend_from_slice(&json_bytes);
        out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E4942u32.to_le_bytes());
        out.extend_from_slice(&bin_bytes);

        out
    }

    fn load(bytes: &[u8]) -> Loaded {
        load_bytes(bytes, &mut |_| Err("gltf uri".to_string())).unwrap()
    }

    fn vertex(mesh: &Mesh, index: usize) -> [f32; 3] {
        let at = index * 16;

        [mesh.vertices[at], mesh.vertices[at + 1], mesh.vertices[at + 2]]
    }

    fn skinned(mesh: &Mesh, index: usize, pos: &[[f32; 3]], rot: &[[f32; 4]]) -> [f32; 3] {
        let count = mesh.bones.len();
        let mut parents = vec![0i16; count];
        let mut inverse = vec![pose::IDENTITY; count];
        let mut idx = 0;

        while idx < count {
            parents[idx] = mesh.bones[idx].parent;
            inverse[idx] = mesh.bones[idx].inverse_bind;
            idx += 1;
        }

        let mut worlds = vec![[0.0; 16]; count];
        let mut palette = vec![[0.0; 12]; count];
        pose::palette(&parents, pos, rot, &inverse, &mut worlds, &mut palette);
        let at = index * 16;
        let point = [mesh.vertices[at], mesh.vertices[at + 1], mesh.vertices[at + 2], 1.0];
        let joints = [
            mesh.vertices[at + 8],
            mesh.vertices[at + 9],
            mesh.vertices[at + 10],
            mesh.vertices[at + 11],
        ];
        let weights = [
            mesh.vertices[at + 12],
            mesh.vertices[at + 13],
            mesh.vertices[at + 14],
            mesh.vertices[at + 15],
        ];
        let mut out = [0.0, 0.0, 0.0];
        idx = 0;

        while idx < 4 {
            let bone = joints[idx] as usize;
            let row = palette[bone];
            let weight = weights[idx];
            out[0] += weight * (row[0] * point[0] + row[1] * point[1] + row[2] * point[2] + row[3]);
            out[1] += weight * (row[4] * point[0] + row[5] * point[1] + row[6] * point[2] + row[7]);
            out[2] += weight * (row[8] * point[0] + row[9] * point[1] + row[10] * point[2] + row[11]);
            idx += 1;
        }

        out
    }

    fn bind_pose(mesh: &Mesh) -> (Vec<[f32; 3]>, Vec<[f32; 4]>) {
        let pos = mesh.bones.iter().map(|bone| bone.local_pos).collect();
        let rot = mesh.bones.iter().map(|bone| bone.local_rot).collect();

        (pos, rot)
    }

    #[test]
    fn basis_keeps_rotations() {
        let samples = [
            [0.2, -0.4, 0.1, 0.8],
            [0.0, 0.70710678, 0.0, 0.70710678],
            [0.70710678, 0.0, 0.0, 0.70710678],
        ];
        let points = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.3, -0.2, 0.7]];
        let mut sample_idx = 0;

        while sample_idx < samples.len() {
            let quat = normalize_quat(samples[sample_idx]);
            let mut point_idx = 0;

            while point_idx < points.len() {
                let turned = pose::quat_rotate(convert_quat(quat), convert_point(points[point_idx]));
                let expected = convert_point(pose::quat_rotate(quat, points[point_idx]));

                assert!(near3(turned, expected));
                point_idx += 1;
            }

            sample_idx += 1;
        }

        assert!(near3(convert_point([0.0, 1.0, 1.0]), [1.0, 0.0, 1.0]));
    }

    #[test]
    fn glb_places_up_and_forward() {
        let mut bin = Bin::new();
        let positions = bin.push(&f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]));
        let indices = bin.push(&u16s(&[0, 1, 2]));
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"name":"root","mesh":0,"translation":[0,2,0]}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],"accessors":[{{"bufferView":{positions},"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,0,1]}},{{"bufferView":{indices},"componentType":5123,"count":3,"type":"SCALAR"}}],"bufferViews":{},"buffers":[{{"byteLength":{}}}]}}"#,
            bin.views_json(),
            bin.bytes.len()
        );
        let loaded = load(&glb(&json, &bin.bytes));

        assert_eq!(loaded.mesh.bones.len(), 1);
        assert_eq!(loaded.mesh.bones[0].name, "root");
        assert!(near3(loaded.mesh.bones[0].local_pos, [0.0, 0.0, 2.0]));
        assert!(near3(vertex(&loaded.mesh, 1), [0.0, 1.0, 0.0]));
        let (pos, rot) = bind_pose(&loaded.mesh);

        assert!(near3(skinned(&loaded.mesh, 1, &pos, &rot), [0.0, 1.0, 2.0]));
        assert!(near3(skinned(&loaded.mesh, 2, &pos, &rot), [1.0, 0.0, 2.0]));
        assert!(loaded.clips.sequences.is_empty());
        assert_eq!(loaded.clips.bones, vec!["root".to_string()]);
    }

    #[test]
    fn skin_rotates_about_the_engine_up_axis() {
        let mut bin = Bin::new();
        let positions = bin.push(&f32s(&[0.0, 1.0, 0.5, 0.1, 1.0, 0.5, 0.0, 1.1, 0.5]));
        let joints = bin.push(&[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
        let weights = bin.push(&f32s(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]));
        let indices = bin.push(&u16s(&[0, 1, 2]));
        let ibms = bin.push(&f32s(&[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 1.0,
        ]));
        let half = std::f32::consts::FRAC_PI_4.sin();
        let times = bin.push(&f32s(&[0.0, 1.0]));
        let rotations = bin.push(&f32s(&[0.0, 0.0, 0.0, 1.0, half, 0.0, 0.0, half]));
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0,2]}}],"nodes":[{{"name":"hip","children":[1]}},{{"name":"spine","translation":[0,1,0]}},{{"name":"mesh","mesh":0,"skin":0}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"JOINTS_0":1,"WEIGHTS_0":2}},"indices":3}}]}}],"skins":[{{"joints":[0,1],"inverseBindMatrices":4}}],"animations":[{{"name":"bend","channels":[{{"sampler":0,"target":{{"node":1,"path":"rotation"}}}}],"samplers":[{{"input":5,"output":6,"interpolation":"LINEAR"}}]}}],"accessors":[{{"bufferView":{positions},"componentType":5126,"count":3,"type":"VEC3","min":[0,1,0.5],"max":[0.1,1.1,0.5]}},{{"bufferView":{joints},"componentType":5121,"count":3,"type":"VEC4"}},{{"bufferView":{weights},"componentType":5126,"count":3,"type":"VEC4"}},{{"bufferView":{indices},"componentType":5123,"count":3,"type":"SCALAR"}},{{"bufferView":{ibms},"componentType":5126,"count":2,"type":"MAT4"}},{{"bufferView":{times},"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]}},{{"bufferView":{rotations},"componentType":5126,"count":2,"type":"VEC4"}}],"bufferViews":{},"buffers":[{{"byteLength":{}}}]}}"#,
            bin.views_json(),
            bin.bytes.len()
        );
        let loaded = load(&glb(&json, &bin.bytes));

        assert_eq!(loaded.mesh.bones.len(), 2);
        assert_eq!(loaded.mesh.bones[1].name, "spine");
        assert_eq!(loaded.mesh.bones[1].parent, 0);
        assert!(near3(loaded.mesh.bones[1].local_pos, [0.0, 0.0, 1.0]));
        let (pos, rot) = bind_pose(&loaded.mesh);

        assert!(near3(skinned(&loaded.mesh, 0, &pos, &rot), [0.5, 0.0, 1.0]));
        let sequence = &loaded.clips.sequences[0];

        assert_eq!(sequence.name, "bend");
        assert_ne!(sequence.flags & FLAG_LOOP, 0);
        assert!(near(sequence.duration, 1.0));
        let mut posed_rot = rot.clone();
        posed_rot[1] = sample_quat(&sequence.tracks[1].rot_times, &sequence.tracks[1].rot, 1.0, rot[1]);
        assert!(near3(skinned(&loaded.mesh, 0, &pos, &posed_rot), [0.0, 0.0, 0.5]));
    }

    #[test]
    fn uniform_scale_bakes_into_vertices() {
        let mut bin = Bin::new();
        let _positions = bin.push(&f32s(&[1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0]));
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"name":"root","mesh":0,"scale":[2,2,2]}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[1,0,0],"max":[1,1,1]}}],"bufferViews":{},"buffers":[{{"byteLength":{}}}]}}"#,
            bin.views_json(),
            bin.bytes.len()
        );
        let loaded = load(&glb(&json, &bin.bytes));
        let (pos, rot) = bind_pose(&loaded.mesh);

        assert!(near3(skinned(&loaded.mesh, 0, &pos, &rot), [0.0, 2.0, 0.0]));
    }

    #[test]
    fn non_uniform_scale_is_rejected() {
        let err = load_bytes(
            &glb(
                r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0,"scale":[1,2,1]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],"buffers":[{"byteLength":36}]}"#,
                &f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
            ),
            &mut |_| Err("gltf uri".to_string()),
        );

        assert!(err.is_err());
    }

    #[test]
    fn png_becomes_albedo() {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut encoder = png::Encoder::new(&mut cursor, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255, 0, 0, 255]).unwrap();
        }
        let png = cursor.into_inner();
        let mut bin = Bin::new();
        let positions = bin.push(&f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]));
        let uvs = bin.push(&f32s(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
        let image = bin.push(&png);
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":{positions},"TEXCOORD_0":{uvs}}},"material":0}}]}}],"materials":[{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":0}}}}}}],"textures":[{{"source":0}}],"images":[{{"bufferView":{image},"mimeType":"image/png"}}],"accessors":[{{"bufferView":{positions},"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,0,1]}},{{"bufferView":{uvs},"componentType":5126,"count":3,"type":"VEC2"}}],"bufferViews":{},"buffers":[{{"byteLength":{}}}]}}"#,
            bin.views_json(),
            bin.bytes.len()
        );
        let loaded = load(&glb(&json, &bin.bytes));

        assert_eq!(loaded.mesh.albedo_w, 1);
        assert_eq!(loaded.mesh.albedo_h, 1);
        assert_eq!(&loaded.mesh.albedo[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn external_buffer_uses_the_callback() {
        let bin = f32s(&[0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0]);
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,1,0],"max":[1,1,1]}}],"bufferViews":[{{"buffer":0,"byteLength":36}}],"buffers":[{{"uri":"mesh.bin","byteLength":36}}]}}"#
        );
        let mut seen = String::new();
        let loaded = load_bytes(json.as_bytes(), &mut |uri| {
            seen = uri.to_string();

            Ok(bin.clone())
        })
        .unwrap();

        assert_eq!(seen, "mesh.bin");
        assert!(near3(vertex(&loaded.mesh, 0), [0.0, 0.0, 1.0]));
    }
}
