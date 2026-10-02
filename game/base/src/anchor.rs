use crate::script::libs::vector3::Vector3;

pub const SHIFT: f64 = 512.0;

#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Anchor {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn from_vec(value: Vector3) -> Self {
        Self {
            x: value.x,
            y: value.y,
            z: value.z,
        }
    }

    pub fn to_vec(self) -> Vector3 {
        Vector3::new(self.x, self.y, self.z)
    }

    pub fn relative(self, x: f64, y: f64, z: f64) -> [f32; 3] {
        [(x - self.x) as f32, (y - self.y) as f32, (z - self.z) as f32]
    }

    pub fn drifted(self, x: f64, y: f64, z: f64) -> bool {
        let dx = x - self.x;
        let dy = y - self.y;
        let dz = z - self.z;

        dx * dx + dy * dy + dz * dz > SHIFT * SHIFT
    }
}
