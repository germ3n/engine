use crate::anim::format::{write_clips, write_mesh, Bone, Mesh};
use crate::anim::pose::{quat_mul, quat_z, ClipEvent, ClipSet, Sequence, Track, FLAG_LOOP, FLAG_ROOT};

const BONES: [&str; 12] = [
    "pelvis", "spine", "chest", "head", "arm_l", "fore_l", "arm_r", "fore_r", "thigh_l", "shin_l",
    "thigh_r", "shin_r",
];

struct Joint {
    name: &'static str,
    parent: i16,
    local: [f32; 3],
}

pub fn test_mesh() -> Mesh {
    let joints = joints();
    let mut bones = Vec::new();
    let mut worlds: Vec<[f32; 3]> = Vec::new();

    for joint in &joints {
        let world = if joint.parent < 0 {
            joint.local
        } else {
            let parent = worlds[joint.parent as usize];
            add(parent, joint.local)
        };
        bones.push(Bone {
            name: joint.name.to_string(),
            parent: joint.parent,
            inverse_bind: inverse_translation(world),
            local_pos: joint.local,
            local_rot: ident(),
        });
        worlds.push(world);
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let skin = [0.22, 0.28];
    let shirt = [0.72, 0.22];
    let pants = [0.22, 0.72];
    let shoe = [0.78, 0.78];

    segment(&mut vertices, &mut indices, worlds[0], worlds[1], 0.12, 0.11, 0, pants);
    segment(&mut vertices, &mut indices, worlds[1], worlds[2], 0.11, 0.13, 1, shirt);
    segment(&mut vertices, &mut indices, worlds[2], add(worlds[2], [0.0, 0.0, 0.18]), 0.15, 0.13, 2, shirt);
    push_head(&mut vertices, &mut indices, worlds[3]);
    segment(&mut vertices, &mut indices, worlds[4], worlds[5], 0.045, 0.04, 4, shirt);
    segment(&mut vertices, &mut indices, worlds[5], add(worlds[5], [0.0, 0.0, -0.24]), 0.038, 0.032, 5, skin);
    segment(&mut vertices, &mut indices, worlds[6], worlds[7], 0.045, 0.04, 6, shirt);
    segment(&mut vertices, &mut indices, worlds[7], add(worlds[7], [0.0, 0.0, -0.24]), 0.038, 0.032, 7, skin);
    segment(&mut vertices, &mut indices, worlds[8], worlds[9], 0.055, 0.048, 8, pants);
    segment(&mut vertices, &mut indices, worlds[9], add(worlds[9], [0.0, 0.0, -0.36]), 0.042, 0.036, 9, pants);
    segment(&mut vertices, &mut indices, add(worlds[9], [0.02, 0.0, -0.36]), add(worlds[9], [0.16, 0.0, -0.34]), 0.04, 0.03, 9, shoe);
    segment(&mut vertices, &mut indices, worlds[10], worlds[11], 0.055, 0.048, 10, pants);
    segment(&mut vertices, &mut indices, worlds[11], add(worlds[11], [0.0, 0.0, -0.36]), 0.042, 0.036, 11, pants);
    segment(&mut vertices, &mut indices, add(worlds[11], [0.02, 0.0, -0.36]), add(worlds[11], [0.16, 0.0, -0.34]), 0.04, 0.03, 11, shoe);

    Mesh {
        bones,
        vertices,
        indices,
        albedo_w: 32,
        albedo_h: 32,
        albedo: paint(),
    }
}

pub fn test_clips() -> ClipSet {
    let bones = BONES.into_iter().map(str::to_string).collect::<Vec<_>>();
    let rest = rest_pose();
    let idle = sequence("idle", FLAG_LOOP, 2.4, &[], &idle_tracks(&rest));
    let walk = sequence(
        "walk",
        FLAG_LOOP,
        1.0,
        &[
            ClipEvent {
                time: 0.0,
                name: "step".to_string(),
            },
            ClipEvent {
                time: 0.5,
                name: "step".to_string(),
            },
        ],
        &walk_tracks(&rest),
    );
    let lunge = sequence(
        "lunge",
        FLAG_ROOT,
        0.6,
        &[ClipEvent {
            time: 0.3,
            name: "hit".to_string(),
        }],
        &lunge_tracks(&rest),
    );
    let wave = sequence("wave", 0, 1.1, &[], &wave_tracks(&rest));

    ClipSet {
        bones,
        sequences: vec![idle, walk, lunge, wave],
    }
}

pub fn mesh_bytes() -> Vec<u8> {
    write_mesh(&test_mesh()).expect("test mesh")
}

pub fn clip_bytes() -> Vec<u8> {
    write_clips(&test_clips()).expect("test clips")
}

fn joints() -> [Joint; 12] {
    [
        Joint { name: "pelvis", parent: -1, local: [0.0, 0.0, 0.92] },
        Joint { name: "spine", parent: 0, local: [0.0, 0.0, 0.16] },
        Joint { name: "chest", parent: 1, local: [0.0, 0.0, 0.18] },
        Joint { name: "head", parent: 2, local: [0.0, 0.0, 0.32] },
        Joint { name: "arm_l", parent: 2, local: [0.0, 0.2, 0.12] },
        Joint { name: "fore_l", parent: 4, local: [0.0, 0.02, -0.26] },
        Joint { name: "arm_r", parent: 2, local: [0.0, -0.2, 0.12] },
        Joint { name: "fore_r", parent: 6, local: [0.0, -0.02, -0.26] },
        Joint { name: "thigh_l", parent: 0, local: [0.0, 0.08, -0.08] },
        Joint { name: "shin_l", parent: 8, local: [0.0, 0.0, -0.4] },
        Joint { name: "thigh_r", parent: 0, local: [0.0, -0.08, -0.08] },
        Joint { name: "shin_r", parent: 10, local: [0.0, 0.0, -0.4] },
    ]
}

fn rest_pose() -> [[f32; 3]; 12] {
    let joints = joints();
    let mut rest = [[0.0, 0.0, 0.0]; 12];
    let mut idx = 0;

    while idx < joints.len() {
        rest[idx] = joints[idx].local;
        idx += 1;
    }

    rest
}

fn idle_tracks(rest: &[[f32; 3]; 12]) -> Vec<Track> {
    let mut tracks = still(rest);
    tracks[1] = keyed(
        rest[1],
        &[0.0, 1.2, 2.4],
        &[ident(), quat_x(-0.08), ident()],
    );
    tracks[2] = keyed(
        rest[2],
        &[0.0, 1.2, 2.4],
        &[ident(), quat_x(-0.05), ident()],
    );
    tracks[3] = keyed(
        rest[3],
        &[0.0, 1.2, 2.4],
        &[ident(), quat_x(0.06), ident()],
    );
    tracks[0] = Track {
        pos_times: vec![0.0, 1.2, 2.4],
        pos: vec![rest[0], add(rest[0], [0.0, 0.0, 0.015]), rest[0]],
        rot_times: vec![0.0],
        rot: vec![ident()],
    };

    tracks
}

fn walk_tracks(rest: &[[f32; 3]; 12]) -> Vec<Track> {
    let mut tracks = still(rest);
    let times = [0.0, 0.25, 0.5, 0.75, 1.0];
    tracks[0] = Track {
        pos_times: times.to_vec(),
        pos: vec![
            rest[0],
            add(rest[0], [0.0, 0.0, 0.04]),
            rest[0],
            add(rest[0], [0.0, 0.0, 0.04]),
            rest[0],
        ],
        rot_times: times.to_vec(),
        rot: vec![
            quat_z(-0.06),
            ident(),
            quat_z(0.06),
            ident(),
            quat_z(-0.06),
        ],
    };
    tracks[1] = keyed(
        rest[1],
        &times,
        &[quat_y(0.08), ident(), quat_y(-0.08), ident(), quat_y(0.08)],
    );
    tracks[4] = keyed(
        rest[4],
        &times,
        &[
            quat_y(0.5),
            ident(),
            quat_y(-0.5),
            ident(),
            quat_y(0.5),
        ],
    );
    tracks[5] = keyed(
        rest[5],
        &times,
        &[
            quat_y(0.15),
            quat_y(0.35),
            quat_y(0.55),
            quat_y(0.35),
            quat_y(0.15),
        ],
    );
    tracks[6] = keyed(
        rest[6],
        &times,
        &[
            quat_y(-0.5),
            ident(),
            quat_y(0.5),
            ident(),
            quat_y(-0.5),
        ],
    );
    tracks[7] = keyed(
        rest[7],
        &times,
        &[
            quat_y(0.55),
            quat_y(0.35),
            quat_y(0.15),
            quat_y(0.35),
            quat_y(0.55),
        ],
    );
    tracks[8] = keyed(
        rest[8],
        &times,
        &[
            quat_y(-0.6),
            ident(),
            quat_y(0.6),
            ident(),
            quat_y(-0.6),
        ],
    );
    tracks[9] = keyed(
        rest[9],
        &times,
        &[
            quat_y(0.15),
            quat_y(0.2),
            quat_y(0.7),
            quat_y(0.2),
            quat_y(0.15),
        ],
    );
    tracks[10] = keyed(
        rest[10],
        &times,
        &[
            quat_y(0.6),
            ident(),
            quat_y(-0.6),
            ident(),
            quat_y(0.6),
        ],
    );
    tracks[11] = keyed(
        rest[11],
        &times,
        &[
            quat_y(0.7),
            quat_y(0.2),
            quat_y(0.15),
            quat_y(0.2),
            quat_y(0.7),
        ],
    );

    tracks
}

fn lunge_tracks(rest: &[[f32; 3]; 12]) -> Vec<Track> {
    let mut tracks = still(rest);
    let times = [0.0, 0.3, 0.6];
    tracks[0] = Track {
        pos_times: times.to_vec(),
        pos: vec![rest[0], add(rest[0], [0.7, 0.0, 0.0]), add(rest[0], [1.2, 0.0, 0.0])],
        rot_times: times.to_vec(),
        rot: vec![ident(), quat_z(12.0_f32.to_radians()), quat_z(30.0_f32.to_radians())],
    };
    tracks[2] = keyed(rest[2], &times, &[ident(), quat_y(0.45), quat_y(0.2)]);
    tracks[4] = keyed(rest[4], &times, &[ident(), quat_y(0.9), quat_y(0.3)]);
    tracks[6] = keyed(rest[6], &times, &[ident(), quat_y(0.9), quat_y(0.3)]);

    tracks
}

fn wave_tracks(rest: &[[f32; 3]; 12]) -> Vec<Track> {
    let mut tracks = still(rest);
    let raised = quat_mul(quat_y(-2.2), quat_z(0.35));
    tracks[6] = keyed(
        rest[6],
        &[0.0, 0.25, 0.55, 0.8, 1.1],
        &[ident(), raised, raised, raised, quat_y(-1.2)],
    );
    tracks[7] = keyed(
        rest[7],
        &[0.0, 0.25, 0.45, 0.7, 0.9, 1.1],
        &[
            quat_y(0.3),
            quat_y(0.2),
            quat_mul(quat_y(0.2), quat_z(0.6)),
            quat_mul(quat_y(0.2), quat_z(-0.45)),
            quat_mul(quat_y(0.2), quat_z(0.35)),
            quat_y(0.3),
        ],
    );
    tracks[3] = keyed(
        rest[3],
        &[0.0, 0.4, 1.1],
        &[ident(), quat_z(-0.15), ident()],
    );

    tracks
}

fn sequence(name: &str, flags: u16, duration: f32, events: &[ClipEvent], tracks: &[Track]) -> Sequence {
    Sequence {
        name: name.to_string(),
        flags,
        duration,
        events: events.to_vec(),
        tracks: tracks.to_vec(),
    }
}

fn still(rest: &[[f32; 3]; 12]) -> Vec<Track> {
    let mut tracks = Vec::new();
    let mut idx = 0;

    while idx < rest.len() {
        tracks.push(hold(rest[idx], ident()));
        idx += 1;
    }

    tracks
}

fn hold(pos: [f32; 3], rot: [f32; 4]) -> Track {
    Track {
        pos_times: vec![0.0],
        pos: vec![pos],
        rot_times: vec![0.0],
        rot: vec![rot],
    }
}

fn keyed(pos: [f32; 3], times: &[f32], rots: &[[f32; 4]]) -> Track {
    Track {
        pos_times: vec![0.0],
        pos: vec![pos],
        rot_times: times.to_vec(),
        rot: rots.to_vec(),
    }
}

fn segment(
    vertices: &mut Vec<f32>,
    indices: &mut Vec<u32>,
    from: [f32; 3],
    to: [f32; 3],
    radius_from: f32,
    radius_to: f32,
    bone: u8,
    uv: [f32; 2],
) {
    push_prism(vertices, indices, from, to, radius_from, radius_to, bone, uv);
}

fn push_head(vertices: &mut Vec<f32>, indices: &mut Vec<u32>, origin: [f32; 3]) {
    let half = [0.09, 0.08, 0.1];
    let center = add(origin, [0.0, 0.0, 0.02]);
    let faces: [([f32; 3], [f32; 3], [f32; 3], [f32; 4]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.06, 0.08, 0.42, 0.42]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [0.08, 0.08, 0.2, 0.2]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.25, 0.25, 0.16, 0.16]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.25, 0.25, 0.16, 0.16]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.08, 0.02, 0.28, 0.12]),
        ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.25, 0.25, 0.16, 0.16]),
    ];
    let mut face_idx = 0;

    while face_idx < faces.len() {
        let (normal, axis_u, axis_v, uv) = faces[face_idx];
        let face_origin = [
            center[0] + normal[0] * half[0],
            center[1] + normal[1] * half[1],
            center[2] + normal[2] * half[2],
        ];
        let span_u = axis_u[0].abs() * half[0] + axis_u[1].abs() * half[1] + axis_u[2].abs() * half[2];
        let span_v = axis_v[0].abs() * half[0] + axis_v[1].abs() * half[1] + axis_v[2].abs() * half[2];
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        let mut positions = Vec::new();
        let mut corner_idx = 0;
        let base = (vertices.len() / 16) as u32;

        while corner_idx < corners.len() {
            let (u, v) = corners[corner_idx];
            let position = [
                face_origin[0] + axis_u[0] * span_u * u + axis_v[0] * span_v * v,
                face_origin[1] + axis_u[1] * span_u * u + axis_v[1] * span_v * v,
                face_origin[2] + axis_u[2] * span_u * u + axis_v[2] * span_v * v,
            ];
            let tex = [uv[0] + (u * 0.5 + 0.5) * uv[2], uv[1] + (v * 0.5 + 0.5) * uv[3]];
            positions.push(position);
            push_vertex(vertices, position, normal, tex, 3);
            corner_idx += 1;
        }

        push_tri(indices, base, 0, 1, 2, &positions, normal);
        push_tri(indices, base, 0, 2, 3, &positions, normal);
        face_idx += 1;
    }
}

fn push_prism(
    vertices: &mut Vec<f32>,
    indices: &mut Vec<u32>,
    from: [f32; 3],
    to: [f32; 3],
    radius_from: f32,
    radius_to: f32,
    bone: u8,
    uv: [f32; 2],
) {
    let axis = normalize(sub(to, from));
    let side = perpendicular(axis);
    let binormal = normalize(cross(axis, side));
    let sides = 6i32;
    let base = (vertices.len() / 16) as u32;
    let mut ring = 0;

    while ring < 2 {
        let center = if ring == 0 { from } else { to };
        let radius = if ring == 0 { radius_from } else { radius_to };
        let mut idx = 0;

        while idx < sides {
            let angle = idx as f32 / sides as f32 * std::f32::consts::TAU;
            let (sin, cos) = angle.sin_cos();
            let normal = normalize(add(scale(side, cos), scale(binormal, sin)));
            let position = add(center, scale(normal, radius));
            push_vertex(vertices, position, normal, uv, bone);
            idx += 1;
        }

        ring += 1;
    }

    let mut idx = 0;

    while idx < sides {
        let next = (idx + 1) % sides;
        let a = idx as u32;
        let b = next as u32;
        let c = sides as u32 + next as u32;
        let d = sides as u32 + idx as u32;
        let positions = prism_positions(vertices, base, sides as u32);
        let outward = prism_normal(&positions, a, b);
        push_tri(indices, base, a, d, c, &positions, outward);
        push_tri(indices, base, a, c, b, &positions, outward);
        idx += 1;
    }

    let positions = prism_positions(vertices, base, sides as u32);
    let mut idx = 1;

    while idx + 1 < sides {
        push_tri(indices, base, 0, idx as u32 + 1, idx as u32, &positions, scale(axis, -1.0));
        push_tri(
            indices,
            base,
            sides as u32,
            sides as u32 + idx as u32,
            sides as u32 + idx as u32 + 1,
            &positions,
            axis,
        );
        idx += 1;
    }
}

fn prism_positions(vertices: &[f32], base: u32, count: u32) -> Vec<[f32; 3]> {
    let mut positions = Vec::new();
    let mut idx = 0;

    while idx < count * 2 {
        let at = ((base + idx) * 16) as usize;
        positions.push([vertices[at], vertices[at + 1], vertices[at + 2]]);
        idx += 1;
    }

    positions
}

fn prism_normal(positions: &[[f32; 3]], a: u32, b: u32) -> [f32; 3] {
    let mid = scale(add(positions[a as usize], positions[b as usize]), 0.5);
    let center = scale(
        add(
            add(positions[0], positions[1]),
            add(positions[2], positions[3]),
        ),
        0.25,
    );

    normalize(sub(mid, center))
}

fn push_tri(
    indices: &mut Vec<u32>,
    base: u32,
    a: u32,
    b: u32,
    c: u32,
    positions: &[[f32; 3]],
    outward: [f32; 3],
) {
    let ab = sub(positions[b as usize], positions[a as usize]);
    let ac = sub(positions[c as usize], positions[a as usize]);
    let facing = cross(ab, ac);

    if dot(facing, outward) < 0.0 {
        indices.extend_from_slice(&[base + a, base + c, base + b]);

        return;
    }

    indices.extend_from_slice(&[base + a, base + b, base + c]);
}

fn paint() -> Vec<u8> {
    let mut pixels = vec![0u8; 32 * 32 * 4];
    fill(&mut pixels, 0, 0, 16, 16, [232, 196, 168]);
    fill(&mut pixels, 16, 0, 16, 16, [52, 132, 168]);
    fill(&mut pixels, 0, 16, 16, 16, [46, 62, 112]);
    fill(&mut pixels, 16, 16, 16, 16, [42, 36, 34]);
    fill(&mut pixels, 2, 1, 12, 3, [62, 42, 32]);
    fill(&mut pixels, 4, 6, 2, 2, [36, 28, 24]);
    fill(&mut pixels, 9, 6, 2, 2, [36, 28, 24]);
    fill(&mut pixels, 6, 10, 4, 1, [176, 112, 96]);

    pixels
}

fn fill(pixels: &mut [u8], x0: usize, y0: usize, w: usize, h: usize, color: [u8; 3]) {
    let mut y = y0;

    while y < y0 + h && y < 32 {
        let mut x = x0;

        while x < x0 + w && x < 32 {
            let at = (y * 32 + x) * 4;
            pixels[at] = color[0];
            pixels[at + 1] = color[1];
            pixels[at + 2] = color[2];
            pixels[at + 3] = 255;
            x += 1;
        }

        y += 1;
    }
}

fn push_vertex(vertices: &mut Vec<f32>, position: [f32; 3], normal: [f32; 3], uv: [f32; 2], bone: u8) {
    vertices.extend_from_slice(&[
        position[0],
        position[1],
        position[2],
        normal[0],
        normal[1],
        normal[2],
        uv[0],
        uv[1],
        bone as f32,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
    ]);
}

fn ident() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

fn quat_x(radians: f32) -> [f32; 4] {
    let (sin, cos) = (radians * 0.5).sin_cos();

    [sin, 0.0, 0.0, cos]
}

fn quat_y(radians: f32) -> [f32; 4] {
    let (sin, cos) = (radians * 0.5).sin_cos();

    [0.0, sin, 0.0, cos]
}

fn inverse_translation(pos: [f32; 3]) -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -pos[0], -pos[1], -pos[2], 1.0,
    ]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(value: [f32; 3], factor: f32) -> [f32; 3] {
    [value[0] * factor, value[1] * factor, value[2] * factor]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(value: [f32; 3]) -> [f32; 3] {
    let len = dot(value, value).sqrt();

    if len <= 1e-6 {
        return [0.0, 0.0, 1.0];
    }

    scale(value, 1.0 / len)
}

fn perpendicular(axis: [f32; 3]) -> [f32; 3] {
    let helper = if axis[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };

    normalize(cross(axis, helper))
}
