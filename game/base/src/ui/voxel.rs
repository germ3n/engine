#[derive(Clone, Copy)]
pub struct SceneView {
    pub eye: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub fov_y: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
    pub tangents: Option<[f32; 4]>,
    pub scale: f32,
}

pub struct FlyCamera {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl FlyCamera {
    pub fn new() -> Self {
        Self {
            x: 0.0,
            y: -24.0,
            z: 14.0,
            yaw: std::f32::consts::FRAC_PI_2,
            pitch: -0.45,
        }
    }

    pub fn look(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * 0.0025;
        self.pitch -= dy * 0.0025;

        if self.pitch > 1.5 {
            self.pitch = 1.5;
        }

        if self.pitch < -1.5 {
            self.pitch = -1.5;
        }
    }

    pub fn fly(&mut self, wish_forward: f32, wish_right: f32, wish_up: f32, dt: f32, speed: f32) {
        let (fx, fy, fz) = self.forward();
        let (rx, ry, rz) = self.right();
        let step = f64::from(speed * dt);
        self.x += f64::from(fx * wish_forward + rx * wish_right) * step;
        self.y += f64::from(fy * wish_forward + ry * wish_right) * step;
        self.z += f64::from(fz * wish_forward + rz * wish_right + wish_up) * step;
    }

    pub fn scene(&self, aspect: f32, scale: f32) -> SceneView {
        self.scene_at(aspect, scale, crate::anchor::Anchor::ZERO)
    }

    pub fn scene_at(
        &self,
        aspect: f32,
        scale: f32,
        anchor: crate::anchor::Anchor,
    ) -> SceneView {
        let (fx, fy, fz) = self.forward();

        SceneView {
            eye: anchor.relative(self.x, self.y, self.z),
            forward: [fx, fy, fz],
            up: [0.0, 0.0, 1.0],
            fov_y: 70.0_f32.to_radians(),
            aspect: aspect.max(0.01),
            near: (scale * 0.05).max(0.01),
            far: (scale * 4000.0).max(200.0),
            tangents: None,
            scale: scale.max(0.001),
        }
    }

    pub fn fly_facing(
        &mut self,
        yaw: f32,
        wish_forward: f32,
        wish_right: f32,
        wish_up: f32,
        dt: f32,
        speed: f32,
    ) {
        let fx = yaw.cos();
        let fy = yaw.sin();
        let rx = fy;
        let ry = -fx;
        let step = f64::from(speed * dt);
        self.x += f64::from(fx * wish_forward + rx * wish_right) * step;
        self.y += f64::from(fy * wish_forward + ry * wish_right) * step;
        self.z += f64::from(wish_up) * step;
    }

    fn forward(&self) -> (f32, f32, f32) {
        let cp = self.pitch.cos();
        let sp = self.pitch.sin();
        let cy = self.yaw.cos();
        let sy = self.yaw.sin();

        (cp * cy, cp * sy, sp)
    }

    fn right(&self) -> (f32, f32, f32) {
        let (fx, fy, fz) = self.forward();
        let right = normalize(cross([fx, fy, fz], [0.0, 0.0, 1.0]));

        (right[0], right[1], right[2])
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();

    if len <= 0.0 {
        return [0.0, 0.0, 0.0];
    }

    [v[0] / len, v[1] / len, v[2] / len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_frames_the_origin_block() {
        let camera = FlyCamera::new();
        let view = camera.scene(4.0 / 3.0, 1.0);
        let target = [0.5, 0.5, 0.5];
        let to = [
            target[0] - view.eye[0],
            target[1] - view.eye[1],
            target[2] - view.eye[2],
        ];
        let forward = normalize(view.forward);
        let up = normalize(view.up);
        let right = normalize(cross(forward, up));
        let ahead = dot(to, forward);
        let half_y = (view.fov_y * 0.5).tan() * ahead;
        let half_x = half_y * view.aspect;

        assert!(ahead > view.near);
        assert!(ahead < view.far);
        assert!(dot(to, right).abs() < half_x);
        assert!(dot(to, up).abs() < half_y);
    }
}
