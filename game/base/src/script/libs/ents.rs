use crate::entities::{EntityHandle, EntityList, Player, ScriptedEntity};
use crate::movement::UserCommand;
use crate::network::events::{EntityNetworked, NetValue, NetVar};
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use mlua::{Error, Function, Lua, Result, Table, Value};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

pub type EntityAccess = Arc<AtomicPtr<EntityList>>;

const TAG_NIL: u8 = 0;
const TAG_BOOL: u8 = 1;
const TAG_INT: u8 = 2;
const TAG_FLOAT: u8 = 3;
const TAG_STRING: u8 = 4;
const TAG_VECTOR3: u8 = 5;
const TAG_ANGLE3: u8 = 6;
const TAG_ENTITY: u8 = 7;
const VAR_STRIDE: usize = 5;

pub const ENTS_THINK: &str = "EntsThink";
pub const ENTS_REMOVED: &str = "EntsRemoved";
pub const ENTS_NET_SPAWN: &str = "EntsNetSpawn";
pub const ENTS_COLLECT_NETWORKED: &str = "EntsCollectNetworked";
pub const ENTS_APPLY_NETWORKED: &str = "EntsApplyNetworked";
pub const ENTS_NETWORKED_STATE: &str = "EntsNetworkedState";
pub const ENTS_PREDICTED: &str = "EntsPredicted";
pub const ENTS_PREDICTED_STATE: &str = "EntsPredictedState";
pub const ENTS_BEGIN_RECONCILE: &str = "EntsBeginReconcile";
pub const ENTS_END_RECONCILE: &str = "EntsEndReconcile";
pub const ENTS_OWNER_CHANGED: &str = "EntsOwnerChanged";
pub const ENTS_SET_LOCAL: &str = "EntsSetLocal";

pub struct EntityScope<'a> {
    access: &'a AtomicPtr<EntityList>,
    previous: *mut EntityList,
}

impl<'a> EntityScope<'a> {
    pub fn new(access: &'a AtomicPtr<EntityList>, entities: *mut EntityList) -> Self {
        let previous = access.swap(entities, Ordering::Relaxed);

        Self { access, previous }
    }
}

impl Drop for EntityScope<'_> {
    fn drop(&mut self) {
        self.access.store(self.previous, Ordering::Relaxed);
    }
}

fn entities(access: &AtomicPtr<EntityList>) -> Result<&mut EntityList> {
    let ptr = access.load(Ordering::Relaxed);

    unsafe { ptr.as_mut() }
        .ok_or_else(|| Error::RuntimeError("entity list is not available".to_string()))
}

fn invalid(raw: u32) -> Error {
    Error::RuntimeError(format!("invalid entity {raw}"))
}

fn add_native<F, A, R>(lua: &Lua, native: &Table, name: &str, func: F)
where
    F: Fn(&Lua, A) -> Result<R> + mlua::MaybeSend + 'static,
    A: mlua::FromLuaMulti,
    R: mlua::IntoLuaMulti,
{
    let function = lua
        .create_function(func)
        .unwrap_or_else(|err| panic!("[ents] Failed to create {name}: {err}"));
    native
        .set(name, function)
        .unwrap_or_else(|err| panic!("[ents] Failed setting {name}: {err}"));
}

fn build_native(lua: &Lua, access: &EntityAccess) -> Table {
    let native = lua.create_table().expect("Failed to create ents native table");

    let shared = access.clone();
    add_native(lua, &native, "create", move |_, class_hash: u32| {
        let list = entities(&shared)?;
        let handle = list.spawn(Box::new(ScriptedEntity::new(class_hash)));

        Ok(handle.map(|handle| handle.0))
    });

    let shared = access.clone();
    add_native(lua, &native, "spawn", move |_, raw: u32| {
        Ok(entities(&shared)?.mark_spawned(EntityHandle(raw)))
    });

    let shared = access.clone();
    add_native(lua, &native, "remove", move |_, raw: u32| {
        Ok(entities(&shared)?.remove(EntityHandle(raw)))
    });

    let shared = access.clone();
    add_native(lua, &native, "class_hash", move |_, raw: u32| {
        let list = entities(&shared)?;

        Ok(list.get(EntityHandle(raw)).map(|entity| entity.class_hash()))
    });

    let shared = access.clone();
    add_native(lua, &native, "is_spawned", move |_, raw: u32| {
        let list = entities(&shared)?;

        Ok(list
            .get(EntityHandle(raw))
            .map(|entity| entity.is_spawned())
            .unwrap_or(false))
    });

    let shared = access.clone();
    add_native(lua, &native, "raw_at", move |_, index: u32| {
        let list = entities(&shared)?;

        Ok(list.handle_at(index).map(|handle| handle.0))
    });

    let shared = access.clone();
    add_native(lua, &native, "count", move |_, ()| Ok(entities(&shared)?.len()));

    let shared = access.clone();
    add_native(lua, &native, "revision", move |_, ()| {
        Ok(entities(&shared)?.revision() as f64)
    });

    let shared = access.clone();
    add_native(lua, &native, "handles", move |lua, ()| {
        let list = entities(&shared)?;
        let out = lua.create_table_with_capacity(list.len(), 0)?;
        let mut idx = 1;

        for (handle, _) in list.iter() {
            out.raw_set(idx, handle.0)?;
            idx += 1;
        }

        Ok(out)
    });

    add_native(lua, &native, "handle", |_, raw: u32| Ok(EntityHandle(raw)));

    let shared = access.clone();
    add_native(lua, &native, "set_owner", move |_, (raw, owner): (u32, Option<u32>)| {
        let owner = EntityHandle(owner.unwrap_or(0));

        Ok(entities(&shared)?.set_owner(EntityHandle(raw), owner))
    });

    let shared = access.clone();
    add_native(lua, &native, "get_owner", move |_, raw: u32| {
        let list = entities(&shared)?;
        let owner = list
            .get(EntityHandle(raw))
            .map(|entity| entity.base().owner)
            .unwrap_or(EntityHandle::NULL);

        if owner.is_null() {
            return Ok(None);
        }

        Ok(Some(owner.0))
    });

    let shared = access.clone();
    add_native(lua, &native, "get_pos", move |_, raw: u32| {
        let list = entities(&shared)?;
        let entity = list.get(EntityHandle(raw)).ok_or_else(|| invalid(raw))?;
        let pos = entity.base().position;

        Ok((pos.x, pos.y, pos.z))
    });

    let shared = access.clone();
    add_native(lua, &native, "set_pos", move |_, (raw, x, y, z): (u32, f64, f64, f64)| {
        let list = entities(&shared)?;
        let entity = list.get_mut(EntityHandle(raw)).ok_or_else(|| invalid(raw))?;
        entity.base_mut().position = Vector3::new(x, y, z);

        Ok(())
    });

    let shared = access.clone();
    add_native(lua, &native, "get_angles", move |_, raw: u32| {
        let list = entities(&shared)?;
        let entity = list.get(EntityHandle(raw)).ok_or_else(|| invalid(raw))?;
        let angles = entity.base().angles;

        Ok((angles.p, angles.y, angles.r))
    });

    let shared = access.clone();
    add_native(lua, &native, "set_angles", move |_, (raw, p, y, r): (u32, f32, f32, f32)| {
        let list = entities(&shared)?;
        let entity = list.get_mut(EntityHandle(raw)).ok_or_else(|| invalid(raw))?;
        entity.base_mut().angles = Angle3::new(p, y, r);

        Ok(())
    });

    let shared = access.clone();
    add_native(lua, &native, "get_velocity", move |_, raw: u32| {
        let list = entities(&shared)?;
        let entity = list.get(EntityHandle(raw)).ok_or_else(|| invalid(raw))?;
        let velocity = entity.base().velocity;

        Ok((velocity.x, velocity.y, velocity.z))
    });

    let shared = access.clone();
    add_native(lua, &native, "set_velocity", move |_, (raw, x, y, z): (u32, f64, f64, f64)| {
        let list = entities(&shared)?;
        let entity = list.get_mut(EntityHandle(raw)).ok_or_else(|| invalid(raw))?;
        entity.base_mut().velocity = Vector3::new(x, y, z);

        Ok(())
    });

    let classes = lua.create_table().expect("Failed to create ents classes table");
    classes
        .raw_set(Player::CLASS_HASH, "Player")
        .expect("[ents] Failed setting Player class");
    native
        .set("classes", classes)
        .expect("[ents] Failed setting classes");

    native
}

pub fn register_ents_lib(lua: &Lua, access: EntityAccess) {
    let install: Function = crate::script::eval(lua, "ents.lua", "lua/libs/ents.luac");
    let native = build_native(lua, &access);
    let exports: Table = install
        .call(native)
        .unwrap_or_else(|err| panic!("Failed to install ents lib: {err}"));

    for (key, name) in [
        ("think", ENTS_THINK),
        ("removed", ENTS_REMOVED),
        ("net_spawn", ENTS_NET_SPAWN),
        ("collect_networked", ENTS_COLLECT_NETWORKED),
        ("apply_networked", ENTS_APPLY_NETWORKED),
        ("networked_state", ENTS_NETWORKED_STATE),
        ("predicted", ENTS_PREDICTED),
        ("predicted_state", ENTS_PREDICTED_STATE),
        ("begin_reconcile", ENTS_BEGIN_RECONCILE),
        ("end_reconcile", ENTS_END_RECONCILE),
        ("owner_changed", ENTS_OWNER_CHANGED),
        ("set_local", ENTS_SET_LOCAL),
    ] {
        let function: Function = exports
            .get(key)
            .unwrap_or_else(|err| panic!("[ents] missing export {key}: {err}"));
        lua.set_named_registry_value(name, function)
            .unwrap_or_else(|err| panic!("Failed to set {name}: {err}"));
    }
}

fn number(value: &Value) -> f64 {
    match value {
        Value::Integer(value) => *value as f64,
        Value::Number(value) => *value,
        _ => 0.0,
    }
}

fn decode_value(flat: &Table, at: usize) -> Result<NetValue> {
    let tag: u8 = flat.raw_get(at + 1)?;
    let first: Value = flat.raw_get(at + 2)?;

    let value = match tag {
        TAG_BOOL => NetValue::Bool(matches!(first, Value::Boolean(true))),
        TAG_INT => NetValue::Int(number(&first) as i32),
        TAG_FLOAT => NetValue::Float(number(&first)),
        TAG_STRING => match first {
            Value::String(text) => NetValue::String(text.to_str()?.to_string()),
            _ => NetValue::Nil,
        },
        TAG_VECTOR3 => {
            let y: Value = flat.raw_get(at + 3)?;
            let z: Value = flat.raw_get(at + 4)?;

            NetValue::Vector3(Vector3::new(number(&first), number(&y), number(&z)))
        }
        TAG_ANGLE3 => {
            let y: Value = flat.raw_get(at + 3)?;
            let r: Value = flat.raw_get(at + 4)?;

            NetValue::Angle3(Angle3::new(
                number(&first) as f32,
                number(&y) as f32,
                number(&r) as f32,
            ))
        }
        TAG_ENTITY => NetValue::Entity(EntityHandle(number(&first) as u32)),
        _ => NetValue::Nil,
    };

    Ok(value)
}

fn decode_flat(flat: &Table, len: usize) -> Result<Vec<EntityNetworked>> {
    let mut out = Vec::new();
    let mut at = 1;

    while at < len {
        let raw: u32 = flat.raw_get(at)?;
        let count: usize = flat.raw_get(at + 1)?;
        at += 2;
        let mut vars = Vec::with_capacity(count);

        for _ in 0..count {
            let key: String = flat.raw_get(at)?;
            let value = decode_value(flat, at)?;
            vars.push(NetVar { key, value });
            at += VAR_STRIDE;
        }

        out.push(EntityNetworked {
            handle: EntityHandle(raw),
            vars,
        });
    }

    Ok(out)
}

fn encode_vars(lua: &Lua, flat: &Table, mut at: usize, vars: &[NetVar]) -> Result<usize> {
    for var in vars {
        flat.raw_set(at, lua.create_string(&var.key)?)?;

        match &var.value {
            NetValue::Nil => {
                flat.raw_set(at + 1, TAG_NIL)?;
            }
            NetValue::Bool(value) => {
                flat.raw_set(at + 1, TAG_BOOL)?;
                flat.raw_set(at + 2, *value)?;
            }
            NetValue::Int(value) => {
                flat.raw_set(at + 1, TAG_INT)?;
                flat.raw_set(at + 2, *value)?;
            }
            NetValue::Float(value) => {
                flat.raw_set(at + 1, TAG_FLOAT)?;
                flat.raw_set(at + 2, *value)?;
            }
            NetValue::String(value) => {
                flat.raw_set(at + 1, TAG_STRING)?;
                flat.raw_set(at + 2, lua.create_string(value)?)?;
            }
            NetValue::Vector3(value) => {
                flat.raw_set(at + 1, TAG_VECTOR3)?;
                flat.raw_set(at + 2, value.x)?;
                flat.raw_set(at + 3, value.y)?;
                flat.raw_set(at + 4, value.z)?;
            }
            NetValue::Angle3(value) => {
                flat.raw_set(at + 1, TAG_ANGLE3)?;
                flat.raw_set(at + 2, value.p)?;
                flat.raw_set(at + 3, value.y)?;
                flat.raw_set(at + 4, value.r)?;
            }
            NetValue::Entity(handle) => {
                flat.raw_set(at + 1, TAG_ENTITY)?;
                flat.raw_set(at + 2, handle.0)?;
            }
        }

        at += VAR_STRIDE;
    }

    Ok(at)
}

fn encode_flat(lua: &Lua, entities: &[EntityNetworked]) -> Result<(Table, usize)> {
    let flat = lua.create_table()?;
    let mut at = 1;

    for entity in entities {
        flat.raw_set(at, entity.handle.0)?;
        flat.raw_set(at + 1, entity.vars.len())?;
        at = encode_vars(lua, &flat, at + 2, &entity.vars)?;
    }

    Ok((flat, at))
}

fn call_flat(lua: &Lua, name: &str, args: impl mlua::IntoLuaMulti) -> Result<Vec<EntityNetworked>> {
    let function: Function = lua.named_registry_value(name)?;
    let (flat, len): (Option<Table>, Option<usize>) = function.call(args)?;

    match (flat, len) {
        (Some(flat), Some(len)) => decode_flat(&flat, len),
        _ => Ok(Vec::new()),
    }
}

pub fn think(lua: &Lua, cur_time: f64) -> Result<()> {
    let function: Function = lua.named_registry_value(ENTS_THINK)?;

    function.call(cur_time)
}

pub fn removed(lua: &Lua, handles: &[(EntityHandle, bool)]) -> Result<()> {
    let function: Function = lua.named_registry_value(ENTS_REMOVED)?;
    let list = lua.create_table_with_capacity(handles.len(), 0)?;
    let mut idx = 0;

    while idx < handles.len() {
        list.raw_set(idx + 1, handles[idx].0 .0)?;
        idx += 1;
    }

    function.call((list, handles.len()))
}

pub fn net_spawn(lua: &Lua, handle: EntityHandle, vars: &[NetVar]) -> Result<bool> {
    let function: Function = lua.named_registry_value(ENTS_NET_SPAWN)?;
    let flat = lua.create_table()?;
    let len = encode_vars(lua, &flat, 1, vars)?;

    function.call((handle.0, flat, vars.len(), len))
}

pub fn collect_networked(lua: &Lua) -> Result<Vec<EntityNetworked>> {
    call_flat(lua, ENTS_COLLECT_NETWORKED, ())
}

pub fn networked_state(lua: &Lua, handle: Option<EntityHandle>) -> Result<Vec<EntityNetworked>> {
    call_flat(lua, ENTS_NETWORKED_STATE, handle.map(|handle| handle.0))
}

pub fn apply_networked(lua: &Lua, entities: &[EntityNetworked]) -> Result<(usize, usize)> {
    let function: Function = lua.named_registry_value(ENTS_APPLY_NETWORKED)?;
    let (flat, len) = encode_flat(lua, entities)?;

    function.call((flat, len))
}

pub fn predicted(lua: &Lua, handle: EntityHandle, cmd: &UserCommand, first_time: bool) -> Result<()> {
    let function: Function = lua.named_registry_value(ENTS_PREDICTED)?;

    function.call((
        handle.0,
        cmd.tick as f64,
        cmd.buttons.0 as f64,
        cmd.wish.x,
        cmd.wish.y,
        cmd.wish.z,
        cmd.view.p,
        cmd.view.y,
        cmd.view.r,
        first_time,
    ))
}

pub fn predicted_state(lua: &Lua, handle: EntityHandle) -> Result<Vec<EntityNetworked>> {
    call_flat(lua, ENTS_PREDICTED_STATE, handle.0)
}

pub fn begin_reconcile(lua: &Lua, entities: &[EntityNetworked]) -> Result<()> {
    let function: Function = lua.named_registry_value(ENTS_BEGIN_RECONCILE)?;
    let (flat, len) = encode_flat(lua, entities)?;

    function.call((flat, len))
}

pub fn end_reconcile(lua: &Lua) -> Result<usize> {
    let function: Function = lua.named_registry_value(ENTS_END_RECONCILE)?;

    function.call(())
}

pub fn owner_changed(lua: &Lua, handle: EntityHandle, owner: EntityHandle) -> Result<()> {
    let function: Function = lua.named_registry_value(ENTS_OWNER_CHANGED)?;

    function.call((handle.0, owner.0))
}

pub fn set_local(lua: &Lua, handle: EntityHandle) -> Result<()> {
    let function: Function = lua.named_registry_value(ENTS_SET_LOCAL)?;

    function.call(handle.0)
}
