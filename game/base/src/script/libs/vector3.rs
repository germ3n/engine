use mlua::{Error, FromLua, IntoLua, Lua, Function, Result, Table, Value};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vector3 {
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    #[inline]
    pub fn len_sq(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    #[inline]
    pub fn len(self) -> f32 {
        self.len_sq().sqrt()
    }

    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    #[inline]
    pub fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    #[inline]
    pub fn normalize(self) -> Self {
        let len = self.len();
        if len > 0.0 {
            let inv = 1.0 / len;
            Self {
                x: self.x * inv,
                y: self.y * inv,
                z: self.z * inv,
            }
        } else {
            self
        }
    }
}

impl FromLua for Vector3 {
    fn from_lua(value: Value, _lua: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => {
                let ptr = ud.to_pointer() as *const Vector3;
                if !ptr.is_null() {
                    unsafe { Ok(*ptr) }
                } else {
                    Err(Error::external("Null pointer on Vector3 cdata"))
                }
            }
            Value::Table(t) => Ok(Vector3 {
                x: t.get("x")?,
                y: t.get("y")?,
                z: t.get("z")?,
            }),
            _ => Err(Error::external("Expected Vector3 cdata or table")),
        }
    }
}

impl IntoLua for Vector3 {
    fn into_lua(self, lua: &Lua) -> Result<Value> {
        let ctor: Function = lua.named_registry_value("Vector3Ctor")?;
        ctor.call((self.x, self.y, self.z))
    }
}

pub fn register_vector3_lib(lua: &Lua) {
    let vector_lib_data = include_bytes!("vector3.lua");
    let exports: Table = lua.load(&vector_lib_data[..]).eval().expect("Failed to execute vector3.lua");

    let ctor: Function = exports.get("ctor").expect("Failed to get Vector3Ctor");
    let module: Table = exports.get("module").expect("Failed to get module");

    lua.set_named_registry_value("Vector3Ctor", ctor).expect("Failed to set Vector3Ctor");
    lua.globals().set("Vector3", module).expect("Failed to get Vector3");
}