pub const FLAG_LOOP: u16 = 1;
pub const FLAG_ROOT: u16 = 2;
pub const FADE_SECONDS: f32 = 0.2;
pub const MAX_EVENT_CYCLES: u32 = 8;

#[derive(Clone, Debug)]
pub struct Track {
    pub pos_times: Vec<f32>,
    pub pos: Vec<[f32; 3]>,
    pub rot_times: Vec<f32>,
    pub rot: Vec<[f32; 4]>,
}

#[derive(Clone, Debug)]
pub struct ClipEvent {
    pub time: f32,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Sequence {
    pub name: String,
    pub flags: u16,
    pub duration: f32,
    pub events: Vec<ClipEvent>,
    pub tracks: Vec<Track>,
}

impl Sequence {
    pub fn loops(&self) -> bool {
        self.flags & FLAG_LOOP != 0
    }

    pub fn root_motion(&self) -> bool {
        self.flags & FLAG_ROOT != 0
    }
}

#[derive(Clone, Debug)]
pub struct ClipSet {
    pub bones: Vec<String>,
    pub sequences: Vec<Sequence>,
}

pub fn elapsed(time: f64, start: u64, rate: f32, dt: f64) -> f32 {
    let steps = (time - start as f64).max(0.0);

    (steps * dt * f64::from(rate.max(0.0))) as f32
}

pub fn wrap_time(time: f32, duration: f32, loops: bool) -> f32 {
    if duration <= 1e-5 {
        return 0.0;
    }

    if !loops {
        return time.clamp(0.0, duration);
    }

    let wrapped = time.rem_euclid(duration);

    if wrapped == 0.0 && time > 0.0 {
        return duration;
    }

    wrapped
}

pub fn sample_vec(times: &[f32], values: &[[f32; 3]], time: f32, fallback: [f32; 3]) -> [f32; 3] {
    if times.is_empty() || values.is_empty() {
        return fallback;
    }

    let count = times.len().min(values.len());

    if time <= times[0] {
        return values[0];
    }

    if time >= times[count - 1] {
        return values[count - 1];
    }

    let mut lo = 0usize;
    let mut hi = count - 1;

    while hi - lo > 1 {
        let mid = (lo + hi) / 2;

        if times[mid] <= time {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    let span = times[hi] - times[lo];
    let u = if span > 1e-6 {
        ((time - times[lo]) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };

    lerp3(values[lo], values[hi], u)
}

pub fn sample_quat(times: &[f32], values: &[[f32; 4]], time: f32, fallback: [f32; 4]) -> [f32; 4] {
    if times.is_empty() || values.is_empty() {
        return fallback;
    }

    let count = times.len().min(values.len());

    if time <= times[0] {
        return norm_quat(values[0]);
    }

    if time >= times[count - 1] {
        return norm_quat(values[count - 1]);
    }

    let mut lo = 0usize;
    let mut hi = count - 1;

    while hi - lo > 1 {
        let mid = (lo + hi) / 2;

        if times[mid] <= time {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    let span = times[hi] - times[lo];
    let u = if span > 1e-6 {
        ((time - times[lo]) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };

    nlerp(values[lo], values[hi], u)
}

pub fn events_between(sequence: &Sequence, from: f32, to: f32, out: &mut Vec<String>) {
    if to <= from || sequence.events.is_empty() {
        return;
    }

    let duration = sequence.duration;

    if duration <= 1e-5 || !sequence.loops() {
        let end = if duration > 0.0 { to.min(duration) } else { to };

        for event in &sequence.events {
            if event.time > from && event.time <= end {
                out.push(event.name.clone());
            }
        }

        return;
    }

    let mut cycle = (from / duration).floor() as i32;
    let last = (to / duration).floor() as i32;
    let mut steps = 0u32;

    while cycle <= last && steps < MAX_EVENT_CYCLES {
        let base = cycle as f32 * duration;

        for event in &sequence.events {
            let time = base + event.time;

            if time > from && time <= to {
                out.push(event.name.clone());
            }
        }

        cycle += 1;
        steps += 1;
    }
}

pub fn blend_locals(
    pos_a: &[[f32; 3]],
    rot_a: &[[f32; 4]],
    pos_b: &[[f32; 3]],
    rot_b: &[[f32; 4]],
    weight: f32,
    pos_out: &mut [[f32; 3]],
    rot_out: &mut [[f32; 4]],
) {
    let weight = weight.clamp(0.0, 1.0);
    let count = pos_out.len().min(rot_out.len());
    let mut idx = 0;

    while idx < count {
        let a_pos = pos_a.get(idx).copied().unwrap_or([0.0, 0.0, 0.0]);
        let b_pos = pos_b.get(idx).copied().unwrap_or(a_pos);
        let a_rot = rot_a.get(idx).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let b_rot = rot_b.get(idx).copied().unwrap_or(a_rot);
        pos_out[idx] = lerp3(a_pos, b_pos, weight);
        rot_out[idx] = nlerp(a_rot, b_rot, weight);
        idx += 1;
    }
}

pub fn locals_from_tracks(
    tracks: &[Track],
    map: &[u16],
    time: f32,
    bind_pos: &[[f32; 3]],
    bind_rot: &[[f32; 4]],
    pos_out: &mut [[f32; 3]],
    rot_out: &mut [[f32; 4]],
) {
    let count = pos_out.len().min(rot_out.len());
    let mut idx = 0;

    while idx < count {
        pos_out[idx] = bind_pos.get(idx).copied().unwrap_or([0.0, 0.0, 0.0]);
        rot_out[idx] = bind_rot.get(idx).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
        idx += 1;
    }

    let mut track_idx = 0;

    while track_idx < tracks.len() && track_idx < map.len() {
        let bone = map[track_idx] as usize;

        if bone < count && map[track_idx] != u16::MAX {
            let track = &tracks[track_idx];
            pos_out[bone] = sample_vec(&track.pos_times, &track.pos, time, pos_out[bone]);
            rot_out[bone] = sample_quat(&track.rot_times, &track.rot, time, rot_out[bone]);
        }

        track_idx += 1;
    }
}

pub fn strip_root(pos: &mut [f32; 3], rot: &mut [f32; 4], bind_pos: [f32; 3]) {
    pos[0] = bind_pos[0];
    pos[1] = bind_pos[1];
    *rot = remove_yaw(*rot);
}

pub fn palette(
    parents: &[i16],
    pos: &[[f32; 3]],
    rot: &[[f32; 4]],
    inverse_bind: &[[f32; 16]],
    worlds: &mut [[f32; 16]],
    out: &mut [[f32; 12]],
) {
    let count = out.len().min(parents.len()).min(worlds.len());
    let mut idx = 0;

    while idx < count {
        let local = trs(
            pos.get(idx).copied().unwrap_or([0.0, 0.0, 0.0]),
            rot.get(idx).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]),
        );
        let parent = parents.get(idx).copied().unwrap_or(-1);
        let world = if parent >= 0 && (parent as usize) < idx {
            mul_mat(worlds[parent as usize], local)
        } else {
            local
        };
        worlds[idx] = world;
        let inverse = inverse_bind.get(idx).copied().unwrap_or(IDENTITY);
        let skin = mul_mat(world, inverse);
        out[idx] = mat3x4(skin);
        idx += 1;
    }
}

pub fn root_delta(track: &Track, from: f32, to: f32, yaw_deg: f32) -> (f64, f64, f64) {
    let bind_pos = [0.0, 0.0, 0.0];
    let bind_rot = [0.0, 0.0, 0.0, 1.0];
    let pos0 = sample_vec(&track.pos_times, &track.pos, from, bind_pos);
    let pos1 = sample_vec(&track.pos_times, &track.pos, to, bind_pos);
    let rot0 = sample_quat(&track.rot_times, &track.rot, from, bind_rot);
    let rot1 = sample_quat(&track.rot_times, &track.rot, to, bind_rot);
    let local_x = f64::from(pos1[0] - pos0[0]);
    let local_y = f64::from(pos1[1] - pos0[1]);
    let yaw = f64::from(yaw_deg).to_radians();
    let (sin, cos) = yaw.sin_cos();
    let world_x = cos * local_x - sin * local_y;
    let world_y = sin * local_x + cos * local_y;
    let dyaw = wrap_pi(f64::from(yaw_of(rot1) - yaw_of(rot0))).to_degrees();

    (world_x, world_y, dyaw)
}

pub fn quat_z(radians: f32) -> [f32; 4] {
    let (sin, cos) = (radians * 0.5).sin_cos();

    [0.0, 0.0, sin, cos]
}

pub fn yaw_of(quat: [f32; 4]) -> f32 {
    let forward = quat_rotate(quat, [1.0, 0.0, 0.0]);

    forward[1].atan2(forward[0])
}

pub fn remove_yaw(quat: [f32; 4]) -> [f32; 4] {
    let inv = quat_z(-yaw_of(quat));

    norm_quat(quat_mul(inv, quat))
}

pub fn quat_rotate(quat: [f32; 4], value: [f32; 3]) -> [f32; 3] {
    let q = norm_quat(quat);
    let u = [q[0], q[1], q[2]];
    let s = q[3];
    let dot_uv = u[0] * value[0] + u[1] * value[1] + u[2] * value[2];
    let dot_uu = u[0] * u[0] + u[1] * u[1] + u[2] * u[2];
    let cross = [
        u[1] * value[2] - u[2] * value[1],
        u[2] * value[0] - u[0] * value[2],
        u[0] * value[1] - u[1] * value[0],
    ];

    [
        2.0 * dot_uv * u[0] + (s * s - dot_uu) * value[0] + 2.0 * s * cross[0],
        2.0 * dot_uv * u[1] + (s * s - dot_uu) * value[1] + 2.0 * s * cross[1],
        2.0 * dot_uv * u[2] + (s * s - dot_uu) * value[2] + 2.0 * s * cross[2],
    ]
}

pub fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub fn nlerp(a: [f32; 4], b: [f32; 4], weight: f32) -> [f32; 4] {
    let mut b = b;
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];

    if dot < 0.0 {
        b = [-b[0], -b[1], -b[2], -b[3]];
    }

    norm_quat([
        a[0] + (b[0] - a[0]) * weight,
        a[1] + (b[1] - a[1]) * weight,
        a[2] + (b[2] - a[2]) * weight,
        a[3] + (b[3] - a[3]) * weight,
    ])
}

pub fn lerp3(a: [f32; 3], b: [f32; 3], weight: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * weight,
        a[1] + (b[1] - a[1]) * weight,
        a[2] + (b[2] - a[2]) * weight,
    ]
}

pub fn trs(pos: [f32; 3], rot: [f32; 4]) -> [f32; 16] {
    let quat = norm_quat(rot);
    let (x, y, z, w) = (quat[0], quat[1], quat[2], quat[3]);
    let x2 = x + x;
    let y2 = y + y;
    let z2 = z + z;
    let xx = x * x2;
    let xy = x * y2;
    let xz = x * z2;
    let yy = y * y2;
    let yz = y * z2;
    let zz = z * z2;
    let wx = w * x2;
    let wy = w * y2;
    let wz = w * z2;

    [
        1.0 - (yy + zz),
        xy + wz,
        xz - wy,
        0.0,
        xy - wz,
        1.0 - (xx + zz),
        yz + wx,
        0.0,
        xz + wy,
        yz - wx,
        1.0 - (xx + yy),
        0.0,
        pos[0],
        pos[1],
        pos[2],
        1.0,
    ]
}

pub fn mul_mat(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut out = [0.0; 16];
    let mut col = 0;

    while col < 4 {
        let mut row = 0;

        while row < 4 {
            out[col * 4 + row] = a[row] * b[col * 4]
                + a[4 + row] * b[col * 4 + 1]
                + a[8 + row] * b[col * 4 + 2]
                + a[12 + row] * b[col * 4 + 3];
            row += 1;
        }

        col += 1;
    }

    out
}

pub fn mat3x4(mat: [f32; 16]) -> [f32; 12] {
    [
        mat[0], mat[4], mat[8], mat[12], mat[1], mat[5], mat[9], mat[13], mat[2], mat[6], mat[10],
        mat[14],
    ]
}

pub fn pose_matrix(position: [f32; 3], pitch_deg: f32, yaw_deg: f32, roll_deg: f32) -> [f32; 16] {
    if pitch_deg == 0.0 && roll_deg == 0.0 {
        return yaw_matrix(position, yaw_deg);
    }

    let (sy, cy) = yaw_deg.to_radians().sin_cos();
    let (sp, cp) = (-pitch_deg).to_radians().sin_cos();
    let (sr, cr) = roll_deg.to_radians().sin_cos();
    let r00 = cy * cp;
    let r10 = sy * cp;
    let r20 = -sp;
    let r01 = cy * sp * sr - sy * cr;
    let r11 = sy * sp * sr + cy * cr;
    let r21 = cp * sr;
    let r02 = cy * sp * cr + sy * sr;
    let r12 = sy * sp * cr - cy * sr;
    let r22 = cp * cr;

    [
        r00,
        r10,
        r20,
        0.0,
        r01,
        r11,
        r21,
        0.0,
        r02,
        r12,
        r22,
        0.0,
        position[0],
        position[1],
        position[2],
        1.0,
    ]
}

pub fn angles_from_pose(mat: [f32; 16]) -> [f32; 3] {
    let r00 = mat[0];
    let r10 = mat[1];
    let r20 = mat[2];
    let r21 = mat[6];
    let r22 = mat[10];
    let pitch = r20.clamp(-1.0, 1.0).asin();
    let (yaw, roll) = if r20.abs() < 0.9999 {
        (r10.atan2(r00), r21.atan2(r22))
    } else {
        (mat[9].atan2(mat[8]), 0.0)
    };

    [pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees()]
}

pub fn yaw_matrix(position: [f32; 3], yaw_deg: f32) -> [f32; 16] {
    let (sin, cos) = yaw_deg.to_radians().sin_cos();

    [
        cos,
        sin,
        0.0,
        0.0,
        -sin,
        cos,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        position[0],
        position[1],
        position[2],
        1.0,
    ]
}

pub const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn norm_quat(quat: [f32; 4]) -> [f32; 4] {
    let len =
        (quat[0] * quat[0] + quat[1] * quat[1] + quat[2] * quat[2] + quat[3] * quat[3]).sqrt();

    if len <= 1e-8 {
        return [0.0, 0.0, 0.0, 1.0];
    }

    let inv = 1.0 / len;

    [quat[0] * inv, quat[1] * inv, quat[2] * inv, quat[3] * inv]
}

fn wrap_pi(radians: f64) -> f64 {
    let turns = (radians + std::f64::consts::PI) / (std::f64::consts::PI * 2.0);
    let wrapped = (radians + std::f64::consts::PI) - turns.floor() * (std::f64::consts::PI * 2.0);

    wrapped - std::f64::consts::PI
}

pub fn sees(
    eye: [f32; 3],
    forward: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    tan_y: f32,
    aspect: f32,
    far: f32,
    point: [f32; 3],
    radius: f32,
) -> bool {
    let delta = [point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]];
    let depth = delta[0] * forward[0] + delta[1] * forward[1] + delta[2] * forward[2];

    if depth < -radius || depth > far + radius {
        return false;
    }

    let side = (delta[0] * right[0] + delta[1] * right[1] + delta[2] * right[2]).abs();
    let rise = (delta[0] * up[0] + delta[1] * up[1] + delta[2] * up[2]).abs();
    let limit_y = (depth + radius).max(0.0) * tan_y + radius;
    let limit_x = limit_y * aspect.max(0.01) + radius;

    side <= limit_x && rise <= limit_y
}
