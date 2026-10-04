use crate::r#enum::InputButtons;
use crate::movement::UserCommand;
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use mlua::{Error, Function, IntoLua, Lua, Result, Table, Value};

#[repr(C)]
#[derive(Clone, Copy)]
struct LuaUserCmd {
    tick: f64,
    buttons: f64,
    wish: Vector3,
    view: Angle3,
    first_time_predicted: bool,
}

impl From<UserCommand> for LuaUserCmd {
    fn from(cmd: UserCommand) -> Self {
        Self {
            tick: cmd.tick as f64,
            buttons: cmd.buttons.0 as f64,
            wish: cmd.wish,
            view: cmd.view,
            first_time_predicted: true,
        }
    }
}

impl From<LuaUserCmd> for UserCommand {
    fn from(cmd: LuaUserCmd) -> Self {
        Self {
            tick: cmd.tick as u64,
            buttons: InputButtons(cmd.buttons as u64),
            wish: cmd.wish,
            view: cmd.view,
        }
    }
}

impl IntoLua for UserCommand {
    fn into_lua(self, lua: &Lua) -> Result<Value> {
        let ctor: Function = lua.named_registry_value("UserCmdCtor")?;

        ctor.call((
            self.tick as f64,
            self.buttons.0 as f64,
            self.wish.x,
            self.wish.y,
            self.wish.z,
            self.view.p,
            self.view.y,
            self.view.r,
        ))
    }
}

pub fn pull(value: &Value) -> Result<UserCommand> {
    let ptr = match value {
        Value::UserData(ud) => ud.to_pointer() as *const LuaUserCmd,
        Value::Other(_) => value.to_pointer() as *const LuaUserCmd,
        other => {
            return Err(Error::external(format!(
                "Expected UserCmd cdata, got {}",
                other.type_name()
            )));
        }
    };

    if ptr.is_null() {
        return Err(Error::external("Null pointer on UserCmd cdata"));
    }

    Ok(UserCommand::from(unsafe { *ptr }))
}

pub fn set_first_time_predicted(lua: &Lua, first_time: bool) -> Result<()> {
    let engine: Table = lua.named_registry_value("engine")?;

    engine.set("first_time_predicted", first_time)
}

pub fn register_usercmd_lib(lua: &Lua) {
    let exports: Table = crate::script::eval(lua, "usercmd.lua", "lua/libs/usercmd.luac");
    let ctor: Function = exports.get("ctor").expect("Failed to get UserCmdCtor");
    let module: Table = exports.get("module").expect("Failed to get UserCmd module");

    lua.set_named_registry_value("UserCmdCtor", ctor)
        .expect("Failed to set UserCmdCtor");
    lua.globals()
        .set("UserCmd", module)
        .expect("Failed to set UserCmd");
}
