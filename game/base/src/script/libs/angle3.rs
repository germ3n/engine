use mlua::{Error, FromLua, IntoLua, Lua, Function, Result, Table, Value};
use wincode::{SchemaWrite, SchemaRead};

#[repr(C)]
#[derive(SchemaWrite, SchemaRead, Copy, Clone, Debug, Default, PartialEq)]
pub struct Angle3 {
    pub p: f32,
    pub y: f32,
    pub r: f32,
}

impl Angle3 {
    #[inline]
    pub const fn new(p: f32, y: f32, r: f32) -> Self {
        Self { p, y, r }
    }

    #[inline]
    pub fn normalize(self) -> Self {
        Self {
            p: (self.p + 180.0).rem_euclid(360.0) - 180.0,
            y: (self.y + 180.0).rem_euclid(360.0) - 180.0,
            r: (self.r + 180.0).rem_euclid(360.0) - 180.0,
        }
    }
}

impl FromLua for Angle3 {
    fn from_lua(value: Value, _lua: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => {
                let ptr = ud.to_pointer() as *const Angle3;
                if !ptr.is_null() {
                    unsafe { Ok(*ptr) }
                } else {
                    Err(Error::external("Null pointer on Angle3 cdata"))
                }
            }
            Value::Table(t) => Ok(Angle3 {
                p: t.get("p")?,
                y: t.get("y")?,
                r: t.get("r")?,
            }),
            _ => Err(Error::external("Expected Angle3 cdata or table")),
        }
    }
}

impl IntoLua for Angle3 {
    fn into_lua(self, lua: &Lua) -> Result<Value> {
        let ctor: Function = lua.named_registry_value("Angle3Ctor")?;
        ctor.call((self.p, self.y, self.r))
    }
}

pub fn register_angle3_lib(lua: &Lua) {
    let angle_lib_data = include_bytes!("angle3.lua");
    let exports: Table = lua.load(&angle_lib_data[..]).eval().expect("Failed to execute angle3.lua");

    let ctor: Function = exports.get("ctor").expect("Failed to get Angle3Ctor");
    let module: Table = exports.get("module").expect("Failed to get module");

    lua.set_named_registry_value("Angle3Ctor", ctor).expect("Failed to set Angle3Ctor");
    lua.globals().set("Angle3", module).expect("Failed to get Angle3");
}