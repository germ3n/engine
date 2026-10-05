use crate::anim::format::{BoneVolume, Capsule, Hitbox};
#[cfg(test)]
use crate::anim::format::SURF_HITBOX;
use crate::anim::pose::{mul_mat, trs};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoneHit {
    pub bone: u16,
    pub group: u8,
    pub distance: f32,
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

fn volume_local(volume: &BoneVolume) -> ([f32; 3], [f32; 4]) {
    match volume {
        BoneVolume::Capsule(capsule) => (capsule.center, capsule.rot),
        BoneVolume::Hitbox(hitbox) => (hitbox.center, hitbox.rot),
    }
}

fn rotate(mat: &[f32; 16], v: [f32; 3]) -> [f32; 3] {
    [
        mat[0] * v[0] + mat[4] * v[1] + mat[8] * v[2],
        mat[1] * v[0] + mat[5] * v[1] + mat[9] * v[2],
        mat[2] * v[0] + mat[6] * v[1] + mat[10] * v[2],
    ]
}

// Bone matrices are rigid (trs of pos/rot), so the inverse rotation is the transpose.
fn inverse_rotate(mat: &[f32; 16], v: [f32; 3]) -> [f32; 3] {
    [
        mat[0] * v[0] + mat[1] * v[1] + mat[2] * v[2],
        mat[4] * v[0] + mat[5] * v[1] + mat[6] * v[2],
        mat[8] * v[0] + mat[9] * v[1] + mat[10] * v[2],
    ]
}

fn inverse_point(mat: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    inverse_rotate(mat, [p[0] - mat[12], p[1] - mat[13], p[2] - mat[14]])
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn ray_box(origin: [f32; 3], dir: [f32; 3], half: [f32; 3], max: f32) -> Option<(f32, [f32; 3])> {
    let mut t_near = 0.0f32;
    let mut t_far = max;
    let mut normal = [0.0; 3];
    let mut axis = 0;

    while axis < 3 {
        if dir[axis].abs() < 1e-9 {
            if origin[axis].abs() > half[axis] {
                return None;
            }
        } else {
            let inv = 1.0 / dir[axis];
            let mut t0 = (-half[axis] - origin[axis]) * inv;
            let mut t1 = (half[axis] - origin[axis]) * inv;
            let mut sign = -1.0;

            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
                sign = 1.0;
            }

            if t0 > t_near {
                t_near = t0;
                normal = [0.0; 3];
                normal[axis] = sign;
            }

            t_far = t_far.min(t1);

            if t_near > t_far {
                return None;
            }
        }

        axis += 1;
    }

    Some((t_near, normal))
}

fn ray_sphere(origin: [f32; 3], dir: [f32; 3], center: [f32; 3], radius: f32) -> Option<f32> {
    let oc = [origin[0] - center[0], origin[1] - center[1], origin[2] - center[2]];
    let b = dot(oc, dir);
    let c = dot(oc, oc) - radius * radius;

    if c > 0.0 && b > 0.0 {
        return None;
    }

    let disc = b * b - dot(dir, dir) * c;

    if disc < 0.0 {
        return None;
    }

    let a = dot(dir, dir);
    let t = (-b - disc.sqrt()) / a;

    Some(t.max(0.0))
}

// Capsule axis is local Y. Returns the nearest entry distance along the (unnormalized) dir.
fn ray_capsule(
    origin: [f32; 3],
    dir: [f32; 3],
    radius: f32,
    half_len: f32,
    max: f32,
) -> Option<(f32, [f32; 3])> {
    let mut best: Option<f32> = None;
    let a = dir[0] * dir[0] + dir[2] * dir[2];

    if a > 1e-12 {
        let b = origin[0] * dir[0] + origin[2] * dir[2];
        let c = origin[0] * origin[0] + origin[2] * origin[2] - radius * radius;

        if c <= 0.0 && origin[1].abs() <= half_len {
            best = Some(0.0);
        } else {
            let disc = b * b - a * c;

            if disc >= 0.0 {
                let t = (-b - disc.sqrt()) / a;
                let y = origin[1] + dir[1] * t;

                if t >= 0.0 && y.abs() <= half_len {
                    best = Some(t);
                }
            }
        }
    }

    for end in [-half_len, half_len] {
        if let Some(t) = ray_sphere(origin, dir, [0.0, end, 0.0], radius) {
            best = Some(best.map_or(t, |cur| cur.min(t)));
        }
    }

    let t = best?;

    if t > max {
        return None;
    }

    let p = [origin[0] + dir[0] * t, origin[1] + dir[1] * t, origin[2] + dir[2] * t];
    let axis_y = p[1].clamp(-half_len, half_len);
    let mut normal = [p[0], p[1] - axis_y, p[2]];
    let len = dot(normal, normal).sqrt();

    if len > 1e-9 {
        normal = [normal[0] / len, normal[1] / len, normal[2] / len];
    } else {
        normal = [-dir[0], -dir[1], -dir[2]];
    }

    Some((t, normal))
}

pub fn trace_volume(
    bone: u16,
    volume: &BoneVolume,
    bone_world: [f32; 16],
    origin: [f32; 3],
    dir: [f32; 3],
    max: f32,
) -> Option<BoneHit> {
    let (center, rot) = volume_local(volume);
    let world = mul_mat(bone_world, trs(center, rot));
    let local_origin = inverse_point(&world, origin);
    let local_dir = inverse_rotate(&world, dir);
    let (distance, local_normal) = match volume {
        BoneVolume::Hitbox(Hitbox { half, .. }) => ray_box(local_origin, local_dir, *half, max)?,
        BoneVolume::Capsule(Capsule {
            radius, half_len, ..
        }) => ray_capsule(local_origin, local_dir, *radius, *half_len, max)?,
    };

    if distance > max {
        return None;
    }

    Some(BoneHit {
        bone,
        group: volume.group(),
        distance,
        position: [
            origin[0] + dir[0] * distance,
            origin[1] + dir[1] * distance,
            origin[2] + dir[2] * distance,
        ],
        normal: rotate(&world, local_normal),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENT: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    fn hitbox() -> BoneVolume {
        BoneVolume::Hitbox(Hitbox {
            half: [1.0, 1.0, 1.0],
            center: [0.0; 3],
            rot: [0.0, 0.0, 0.0, 1.0],
            group: 3,
            flags: SURF_HITBOX,
        })
    }

    fn capsule() -> BoneVolume {
        BoneVolume::Capsule(Capsule {
            radius: 1.0,
            half_len: 2.0,
            center: [0.0; 3],
            rot: [0.0, 0.0, 0.0, 1.0],
            group: 5,
            flags: SURF_HITBOX,
        })
    }

    #[test]
    fn box_hit_and_miss() {
        let hit = trace_volume(0, &hitbox(), IDENT, [-5.0, 0.0, 0.0], [1.0, 0.0, 0.0], 100.0)
            .expect("hit");

        assert!((hit.distance - 4.0).abs() < 1e-4);
        assert_eq!(hit.group, 3);
        assert_eq!(hit.normal, [-1.0, 0.0, 0.0]);
        assert!(trace_volume(0, &hitbox(), IDENT, [-5.0, 3.0, 0.0], [1.0, 0.0, 0.0], 100.0).is_none());
        assert!(trace_volume(0, &hitbox(), IDENT, [-5.0, 0.0, 0.0], [1.0, 0.0, 0.0], 3.0).is_none());
    }

    #[test]
    fn capsule_side_and_cap() {
        let side = trace_volume(0, &capsule(), IDENT, [-5.0, 1.0, 0.0], [1.0, 0.0, 0.0], 100.0)
            .expect("side");

        assert!((side.distance - 4.0).abs() < 1e-4);

        let cap = trace_volume(0, &capsule(), IDENT, [0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 100.0)
            .expect("cap");

        assert!((cap.distance - 7.0).abs() < 1e-4);
        assert!(trace_volume(0, &capsule(), IDENT, [-5.0, 4.0, 0.0], [1.0, 0.0, 0.0], 100.0).is_none());
    }
}
