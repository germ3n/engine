use crate::entities::{EntityHandle, EntityList, Player, ScriptedEntity};
use crate::movement::UserCommand;
use crate::network::events::{EntityNetworked, NetValue, NetVar};
use crate::physics::{box_from_bounds, fallback_box, PhysicsAccess, PhysicsWorld};
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use mlua::{Error, Function, Lua, Result, Table, Value};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

pub type EntityAccess = Arc<AtomicPtr<EntityList>>;
pub type AnimAccess = Arc<AtomicPtr<crate::anim::AnimAssets>>;

const TAG_NIL: u8 = 0;
const TAG_BOOL: u8 = 1;
const TAG_INT: u8 = 2;
const TAG_FLOAT: u8 = 3;
const TAG_STRING: u8 = 4;
const TAG_VECTOR3: u8 = 5;
const TAG_ANGLE3: u8 = 6;
const TAG_ENTITY: u8 = 7;

pub const ENTS_THINK: &str = "EntsThink";
pub const ENTS_REMOVED: &str = "EntsRemoved";
pub const ENTS_NET_SPAWN: &str = "EntsNetSpawn";
pub const ENTS_COLLECT_NETWORKED: &str = "EntsCollectNetworked";
pub const ENTS_APPLY_NETWORKED: &str = "EntsApplyNetworked";
pub const ENTS_PRESENT: &str = "EntsPresent";
pub const ENTS_NETWORKED_STATE: &str = "EntsNetworkedState";
pub const ENTS_PREDICTED: &str = "EntsPredicted";
pub const ENTS_PREDICTED_STATE: &str = "EntsPredictedState";
pub const ENTS_BEGIN_RECONCILE: &str = "EntsBeginReconcile";
pub const ENTS_END_RECONCILE: &str = "EntsEndReconcile";
pub const ENTS_OWNER_CHANGED: &str = "EntsOwnerChanged";
pub const ENTS_SET_LOCAL: &str = "EntsSetLocal";
pub const ENTS_ANIM_EVENT: &str = "EntsAnimEvent";

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

pub struct AnimScope<'a> {
    access: &'a AtomicPtr<crate::anim::AnimAssets>,
    previous: *mut crate::anim::AnimAssets,
}

impl<'a> AnimScope<'a> {
    pub fn new(
        access: &'a AtomicPtr<crate::anim::AnimAssets>,
        anims: *mut crate::anim::AnimAssets,
    ) -> Self {
        let previous = access.swap(anims, Ordering::Relaxed);

        Self { access, previous }
    }
}

impl Drop for AnimScope<'_> {
    fn drop(&mut self) {
        self.access.store(self.previous, Ordering::Relaxed);
    }
}

fn anims(access: &AtomicPtr<crate::anim::AnimAssets>) -> Result<&mut crate::anim::AnimAssets> {
    let ptr = access.load(Ordering::Relaxed);

    unsafe { ptr.as_mut() }
        .ok_or_else(|| Error::RuntimeError("animation is not available".to_string()))
}

fn clock(lua: &Lua) -> u64 {
    let Ok(engine) = lua.globals().get::<Table>("engine") else {
        return 0;
    };
    let Ok(tick) = engine.get::<u64>("tick_count") else {
        return 0;
    };

    tick
}

fn entities(access: &AtomicPtr<EntityList>) -> Result<&mut EntityList> {
    let ptr = access.load(Ordering::Relaxed);

    unsafe { ptr.as_mut() }
        .ok_or_else(|| Error::RuntimeError("entity list is not available".to_string()))
}

fn physics(access: &AtomicPtr<PhysicsWorld>) -> Option<&mut PhysicsWorld> {
    let ptr = access.load(Ordering::Relaxed);

    unsafe { ptr.as_mut() }
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

fn build_native(
    lua: &Lua,
    access: &EntityAccess,
    anim_access: &AnimAccess,
    physics_access: &PhysicsAccess,
) -> Table {
    let native = lua
        .create_table()
        .expect("Failed to create ents native table");

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
    let bodies = physics_access.clone();
    add_native(lua, &native, "remove", move |_, raw: u32| {
        let handle = EntityHandle(raw);
        let removed = entities(&shared)?.remove(handle);

        if removed {
            if let Some(world) = physics(&bodies) {
                world.forget(handle);
            }
        }

        Ok(removed)
    });

    let shared = access.clone();
    add_native(lua, &native, "class_hash", move |_, raw: u32| {
        let list = entities(&shared)?;

        Ok(list
            .get(EntityHandle(raw))
            .map(|entity| entity.class_hash()))
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
    add_native(lua, &native, "count", move |_, ()| {
        Ok(entities(&shared)?.len())
    });

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
    add_native(
        lua,
        &native,
        "set_owner",
        move |_, (raw, owner): (u32, Option<u32>)| {
            let owner = EntityHandle(owner.unwrap_or(0));

            Ok(entities(&shared)?.set_owner(EntityHandle(raw), owner))
        },
    );

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
    let bodies = physics_access.clone();
    add_native(
        lua,
        &native,
        "set_pos",
        move |_, (raw, x, y, z): (u32, f64, f64, f64)| {
            let list = entities(&shared)?;
            let handle = EntityHandle(raw);
            let entity = list.get_mut(handle).ok_or_else(|| invalid(raw))?;
            entity.base_mut().position = Vector3::new(x, y, z);

            if let Some(world) = physics(&bodies) {
                world.teleport_position(handle, x, y, z);
            }

            Ok(())
        },
    );

    let shared = access.clone();
    add_native(lua, &native, "get_angles", move |_, raw: u32| {
        let list = entities(&shared)?;
        let entity = list.get(EntityHandle(raw)).ok_or_else(|| invalid(raw))?;
        let angles = entity.base().angles;

        Ok((angles.p, angles.y, angles.r))
    });

    let shared = access.clone();
    let bodies = physics_access.clone();
    add_native(
        lua,
        &native,
        "set_angles",
        move |_, (raw, p, y, r): (u32, f32, f32, f32)| {
            let list = entities(&shared)?;
            let handle = EntityHandle(raw);
            let entity = list.get_mut(handle).ok_or_else(|| invalid(raw))?;
            let angles = Angle3::new(p, y, r);
            entity.base_mut().angles = angles;

            if let Some(world) = physics(&bodies) {
                world.teleport_angles(handle, angles);
            }

            Ok(())
        },
    );

    let shared = access.clone();
    add_native(lua, &native, "get_velocity", move |_, raw: u32| {
        let list = entities(&shared)?;
        let entity = list.get(EntityHandle(raw)).ok_or_else(|| invalid(raw))?;
        let velocity = entity.base().velocity;

        Ok((velocity.x, velocity.y, velocity.z))
    });

    let shared = access.clone();
    let bodies = physics_access.clone();
    add_native(
        lua,
        &native,
        "set_velocity",
        move |_, (raw, x, y, z): (u32, f64, f64, f64)| {
            let list = entities(&shared)?;
            let handle = EntityHandle(raw);
            let entity = list.get_mut(handle).ok_or_else(|| invalid(raw))?;
            let velocity = Vector3::new(x, y, z);
            entity.base_mut().velocity = velocity;

            if let Some(world) = physics(&bodies) {
                world.teleport_velocity(handle, velocity);
            }

            Ok(())
        },
    );

    let ents_access = access.clone();
    let anims_access = anim_access.clone();
    let bodies = physics_access.clone();
    add_native(lua, &native, "enable_physics", move |_, raw: u32| {
        let list = entities(&ents_access)?;
        let handle = EntityHandle(raw);
        let (position, angles, velocity, mesh, class_hash) = {
            let entity = list.get(handle).ok_or_else(|| invalid(raw))?;
            let base = entity.base();

            (
                base.position,
                base.angles,
                base.velocity,
                base.anim.mesh,
                entity.class_hash(),
            )
        };

        if class_hash == Player::CLASS_HASH {
            return Ok(false);
        }

        let (center, half) = match anims(&anims_access)?.mesh_bounds(mesh) {
            Some((min, max)) => box_from_bounds(min, max),
            None => fallback_box(),
        };
        let Some(world) = physics(&bodies) else {
            return Ok(false);
        };

        Ok(world.enable_box(handle, position, angles, velocity, center, half))
    });

    let bodies = physics_access.clone();
    let ents_access = access.clone();
    add_native(
        lua,
        &native,
        "set_mass",
        move |_, (raw, mass): (u32, f64)| {
            let handle = EntityHandle(raw);

            if entities(&ents_access)?.get(handle).is_none() {
                return Err(invalid(raw));
            }

            let Some(world) = physics(&bodies) else {
                return Ok(false);
            };

            Ok(world.set_mass(handle, mass as f32))
        },
    );

    let bodies = physics_access.clone();
    let ents_access = access.clone();
    add_native(
        lua,
        &native,
        "apply_impulse",
        move |_, (raw, x, y, z): (u32, f64, f64, f64)| {
            let handle = EntityHandle(raw);

            if entities(&ents_access)?.get(handle).is_none() {
                return Err(invalid(raw));
            }

            let Some(world) = physics(&bodies) else {
                return Ok(false);
            };
            let impulse = Vector3::new(x, y, z);

            if world.apply_impulse(handle, impulse) {
                return Ok(true);
            }

            let Some(mass) = world.asleep_mass(handle) else {
                return Ok(false);
            };
            let list = entities(&ents_access)?;
            let entity = list.get_mut(handle).ok_or_else(|| invalid(raw))?;
            let velocity = entity.base().velocity;
            let scale = 1.0 / f64::from(mass);
            entity.base_mut().velocity = Vector3::new(
                velocity.x + impulse.x * scale,
                velocity.y + impulse.y * scale,
                velocity.z + impulse.z * scale,
            );

            Ok(true)
        },
    );

    let ents_access = access.clone();
    let anims_access = anim_access.clone();
    add_native(
        lua,
        &native,
        "set_model",
        move |lua, (raw, mesh, clips): (u32, String, String)| {
            let list = entities(&ents_access)?;
            let bank = anims(&anims_access)?;
            let entity = list
                .get_mut(EntityHandle(raw))
                .ok_or_else(|| invalid(raw))?;
            bank.assign(raw, &mut entity.base_mut().anim, &mesh, &clips)
                .map_err(Error::RuntimeError)?;
            let _ = lua;

            Ok(())
        },
    );

    let ents_access = access.clone();
    let anims_access = anim_access.clone();
    add_native(
        lua,
        &native,
        "set_sequence",
        move |lua, (raw, name, rate): (u32, String, f64)| {
            let list = entities(&ents_access)?;
            let bank = anims(&anims_access)?;
            let entity = list
                .get_mut(EntityHandle(raw))
                .ok_or_else(|| invalid(raw))?;
            let clips = entity.base().anim.clips;
            let id = bank
                .sequence_id(clips, &name)
                .ok_or_else(|| Error::RuntimeError(format!("unknown sequence {name}")))?;
            entity
                .base_mut()
                .anim
                .set_sequence(id, clock(lua), rate as f32);

            Ok(())
        },
    );

    let ents_access = access.clone();
    let anims_access = anim_access.clone();
    add_native(
        lua,
        &native,
        "play_gesture",
        move |lua, (raw, name, rate, weight): (u32, String, f64, f64)| {
            let list = entities(&ents_access)?;
            let bank = anims(&anims_access)?;
            let entity = list
                .get_mut(EntityHandle(raw))
                .ok_or_else(|| invalid(raw))?;
            let clips = entity.base().anim.clips;
            let id = bank
                .sequence_id(clips, &name)
                .ok_or_else(|| Error::RuntimeError(format!("unknown gesture {name}")))?;
            entity
                .base_mut()
                .anim
                .set_gesture(id, clock(lua), rate as f32, weight as f32);

            Ok(())
        },
    );

    let ents_access = access.clone();
    add_native(lua, &native, "stop_gesture", move |_, raw: u32| {
        let list = entities(&ents_access)?;
        let entity = list
            .get_mut(EntityHandle(raw))
            .ok_or_else(|| invalid(raw))?;
        entity.base_mut().anim.clear_gesture();

        Ok(())
    });

    let classes = lua
        .create_table()
        .expect("Failed to create ents classes table");
    classes
        .raw_set(Player::CLASS_HASH, "Player")
        .expect("[ents] Failed setting Player class");
    native
        .set("classes", classes)
        .expect("[ents] Failed setting classes");

    native
}

pub fn register_ents_lib(
    lua: &Lua,
    access: EntityAccess,
    anim_access: AnimAccess,
    physics_access: PhysicsAccess,
) {
    let install: Function = crate::script::eval(lua, "ents.lua", "lua/libs/ents.luac");
    let native = build_native(lua, &access, &anim_access, &physics_access);
    let exports: Table = install
        .call(native)
        .unwrap_or_else(|err| panic!("Failed to install ents lib: {err}"));

    for (key, name) in [
        ("think", ENTS_THINK),
        ("removed", ENTS_REMOVED),
        ("net_spawn", ENTS_NET_SPAWN),
        ("collect_networked", ENTS_COLLECT_NETWORKED),
        ("apply_networked", ENTS_APPLY_NETWORKED),
        ("present_interpolated", ENTS_PRESENT),
        ("networked_state", ENTS_NETWORKED_STATE),
        ("predicted", ENTS_PREDICTED),
        ("predicted_state", ENTS_PREDICTED_STATE),
        ("begin_reconcile", ENTS_BEGIN_RECONCILE),
        ("end_reconcile", ENTS_END_RECONCILE),
        ("owner_changed", ENTS_OWNER_CHANGED),
        ("set_local", ENTS_SET_LOCAL),
        ("anim_event", ENTS_ANIM_EVENT),
    ] {
        let function: Function = exports
            .get(key)
            .unwrap_or_else(|err| panic!("[ents] missing export {key}: {err}"));
        lua.set_named_registry_value(name, function)
            .unwrap_or_else(|err| panic!("Failed to set {name}: {err}"));
    }
}

fn truncated() -> Error {
    Error::RuntimeError("truncated netvar blob".to_string())
}

fn too_long() -> Error {
    Error::RuntimeError("netvar string is too long".to_string())
}

struct Blob<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Blob<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or_else(truncated)?;

        if end > self.bytes.len() {
            return Err(truncated());
        }

        let out = &self.bytes[self.at..end];
        self.at = end;

        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;

        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;

        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i32(&mut self) -> Result<i32> {
        let bytes = self.take(4)?;

        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn f32(&mut self) -> Result<f32> {
        let bytes = self.take(4)?;

        Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn f64(&mut self) -> Result<f64> {
        let bytes = self.take(8)?;

        Ok(f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn text(&mut self) -> Result<String> {
        let n = self.u16()? as usize;
        let bytes = self.take(n)?;

        String::from_utf8(bytes.to_vec())
            .map_err(|_| Error::RuntimeError("netvar string is not utf-8".to_string()))
    }
}

fn write_str(out: &mut Vec<u8>, text: &str) -> Result<()> {
    let n = text.len();

    if n > u16::MAX as usize {
        return Err(too_long());
    }

    out.extend_from_slice(&(n as u16).to_le_bytes());
    out.extend_from_slice(text.as_bytes());

    Ok(())
}

fn write_value(out: &mut Vec<u8>, value: &NetValue) -> Result<()> {
    match value {
        NetValue::Nil => out.push(TAG_NIL),
        NetValue::Bool(value) => {
            out.push(TAG_BOOL);
            out.push(u8::from(*value));
        }
        NetValue::Int(value) => {
            out.push(TAG_INT);
            out.extend_from_slice(&value.to_le_bytes());
        }
        NetValue::Float(value) => {
            out.push(TAG_FLOAT);
            out.extend_from_slice(&value.to_le_bytes());
        }
        NetValue::String(value) => {
            out.push(TAG_STRING);
            write_str(out, value)?;
        }
        NetValue::Vector3(value) => {
            out.push(TAG_VECTOR3);
            out.extend_from_slice(&value.x.to_le_bytes());
            out.extend_from_slice(&value.y.to_le_bytes());
            out.extend_from_slice(&value.z.to_le_bytes());
        }
        NetValue::Angle3(value) => {
            out.push(TAG_ANGLE3);
            out.extend_from_slice(&value.p.to_le_bytes());
            out.extend_from_slice(&value.y.to_le_bytes());
            out.extend_from_slice(&value.r.to_le_bytes());
        }
        NetValue::Entity(handle) => {
            out.push(TAG_ENTITY);
            out.extend_from_slice(&handle.0.to_le_bytes());
        }
    }

    Ok(())
}

fn write_var(out: &mut Vec<u8>, var: &NetVar) -> Result<()> {
    write_str(out, &var.key)?;
    write_value(out, &var.value)
}

fn read_value(blob: &mut Blob<'_>) -> Result<NetValue> {
    let tag = blob.u8()?;

    let value = match tag {
        TAG_NIL => NetValue::Nil,
        TAG_BOOL => NetValue::Bool(blob.u8()? != 0),
        TAG_INT => NetValue::Int(blob.i32()?),
        TAG_FLOAT => NetValue::Float(blob.f64()?),
        TAG_STRING => NetValue::String(blob.text()?),
        TAG_VECTOR3 => NetValue::Vector3(Vector3::new(blob.f64()?, blob.f64()?, blob.f64()?)),
        TAG_ANGLE3 => NetValue::Angle3(Angle3::new(blob.f32()?, blob.f32()?, blob.f32()?)),
        TAG_ENTITY => NetValue::Entity(EntityHandle(blob.u32()?)),
        _ => return Err(Error::RuntimeError("bad netvar tag".to_string())),
    };

    Ok(value)
}

fn decode_entities(bytes: &[u8]) -> Result<Vec<EntityNetworked>> {
    let mut blob = Blob { bytes, at: 0 };
    let mut out = Vec::new();

    while blob.at < bytes.len() {
        let handle = EntityHandle(blob.u32()?);
        let count = blob.u16()? as usize;
        let mut vars = Vec::with_capacity(count);

        for _ in 0..count {
            vars.push(NetVar {
                key: blob.text()?,
                value: read_value(&mut blob)?,
            });
        }

        out.push(EntityNetworked { handle, vars });
    }

    Ok(out)
}

fn encode_entities(entities: &[EntityNetworked]) -> Result<Vec<u8>> {
    let mut out = Vec::new();

    for entity in entities {
        if entity.vars.len() > u16::MAX as usize {
            return Err(too_long());
        }

        out.extend_from_slice(&entity.handle.0.to_le_bytes());
        out.extend_from_slice(&(entity.vars.len() as u16).to_le_bytes());

        for var in &entity.vars {
            write_var(&mut out, var)?;
        }
    }

    Ok(out)
}

fn encode_var_list(vars: &[NetVar]) -> Result<Vec<u8>> {
    if vars.is_empty() {
        return Ok(Vec::new());
    }

    if vars.len() > u16::MAX as usize {
        return Err(too_long());
    }

    let mut out = Vec::new();
    out.extend_from_slice(&(vars.len() as u16).to_le_bytes());

    for var in vars {
        write_var(&mut out, var)?;
    }

    Ok(out)
}

fn lua_blob(lua: &Lua, bytes: Vec<u8>) -> Result<Value> {
    if bytes.is_empty() {
        return Ok(Value::Nil);
    }

    Ok(Value::String(lua.create_string(bytes)?))
}

fn call_blob(lua: &Lua, name: &str, args: impl mlua::IntoLuaMulti) -> Result<Vec<EntityNetworked>> {
    let function: Function = lua.named_registry_value(name)?;
    let blob: Option<mlua::LuaString> = function.call(args)?;

    match blob {
        Some(blob) => decode_entities(&blob.as_bytes()),
        None => Ok(Vec::new()),
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

pub fn net_spawn(
    lua: &Lua,
    handle: EntityHandle,
    vars: &[NetVar],
    time: Option<f64>,
) -> Result<bool> {
    let function: Function = lua.named_registry_value(ENTS_NET_SPAWN)?;
    let blob = lua_blob(lua, encode_var_list(vars)?)?;

    function.call((handle.0, blob, time))
}

pub fn collect_networked(lua: &Lua) -> Result<Vec<EntityNetworked>> {
    call_blob(lua, ENTS_COLLECT_NETWORKED, ())
}

pub fn networked_state(lua: &Lua, handle: Option<EntityHandle>) -> Result<Vec<EntityNetworked>> {
    call_blob(lua, ENTS_NETWORKED_STATE, handle.map(|handle| handle.0))
}

pub fn apply_networked(
    lua: &Lua,
    entities: &[EntityNetworked],
    time: Option<f64>,
) -> Result<(usize, usize)> {
    let function: Function = lua.named_registry_value(ENTS_APPLY_NETWORKED)?;
    let blob = lua_blob(lua, encode_entities(entities)?)?;

    function.call((blob, time))
}

pub fn anim_event(lua: &Lua, handle: EntityHandle, name: &str) -> Result<()> {
    let function: Function = lua.named_registry_value(ENTS_ANIM_EVENT)?;
    let _: () = function.call((handle.0, name))?;

    Ok(())
}

pub fn present_interpolated(lua: &Lua, time: f64) -> Result<()> {
    let function: Function = lua.named_registry_value(ENTS_PRESENT)?;

    function.call(time)
}

pub fn predicted(
    lua: &Lua,
    handle: EntityHandle,
    cmd: &UserCommand,
    first_time: bool,
) -> Result<()> {
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
    call_blob(lua, ENTS_PREDICTED_STATE, handle.0)
}

pub fn begin_reconcile(lua: &Lua, entities: &[EntityNetworked]) -> Result<()> {
    let function: Function = lua.named_registry_value(ENTS_BEGIN_RECONCILE)?;
    let blob = lua_blob(lua, encode_entities(entities)?)?;

    function.call(blob)
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

#[cfg(test)]
mod tests {
    use super::{
        apply_networked, collect_networked, decode_entities, encode_entities, present_interpolated,
        set_local,
    };
    use crate::console::{ConVar, ConVarValue};
    use crate::entities::{EntityHandle, EntityList};
    use crate::network::events::{EntityNetworked, NetValue, NetVar};
    use crate::script::libs::angle3::Angle3;
    use crate::script::libs::vector3::Vector3;
    use crate::script::{Realm, ScriptEngine};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn value<'a>(vars: &'a [NetVar], key: &str) -> &'a NetValue {
        &vars
            .iter()
            .find(|var| var.key == key)
            .unwrap_or_else(|| panic!("missing {key}"))
            .value
    }

    #[test]
    fn netvar_blob_roundtrip() {
        let entities = vec![EntityNetworked {
            handle: EntityHandle(0x8000_0001),
            vars: vec![
                NetVar {
                    key: "ammo".to_string(),
                    value: NetValue::Int(-3),
                },
                NetVar {
                    key: "on".to_string(),
                    value: NetValue::Bool(false),
                },
                NetVar {
                    key: "rate".to_string(),
                    value: NetValue::Float(0.25),
                },
                NetVar {
                    key: "name".to_string(),
                    value: NetValue::String("blaster".to_string()),
                },
                NetVar {
                    key: "note".to_string(),
                    value: NetValue::String(String::new()),
                },
                NetVar {
                    key: "pos".to_string(),
                    value: NetValue::Vector3(Vector3::new(1.0, 2.0, 3.0)),
                },
                NetVar {
                    key: "ang".to_string(),
                    value: NetValue::Angle3(Angle3::new(10.0, 20.0, 30.0)),
                },
                NetVar {
                    key: "buddy".to_string(),
                    value: NetValue::Entity(EntityHandle(2097152)),
                },
                NetVar {
                    key: "gone".to_string(),
                    value: NetValue::Nil,
                },
            ],
        }];
        let bytes = encode_entities(&entities).unwrap();
        assert_eq!(decode_entities(&bytes).unwrap(), entities);
        assert!(decode_entities(&[0, 1]).is_err());
    }

    #[test]
    fn netvar_blob_crosses_lua() {
        if crate::fs::try_global().is_none() {
            let fs = crate::fs::Fs::boot().expect("fs boot");
            crate::fs::set_global(Arc::new(fs));
        }

        let mut cvars = HashMap::new();
        cvars.insert(
            "sv_gravity".to_string(),
            Arc::new(ConVar::new(
                "sv_gravity",
                ConVarValue::Float(24.0),
                "World gravity",
                Some(false),
                Some(true),
            )),
        );
        let binds = Arc::new(Mutex::new(crate::input::Binds::defaults()));
        let pads = Arc::new(Mutex::new(crate::platform::PadCache::new()));
        let engine = ScriptEngine::new(
            Realm::Server,
            1.0 / 60.0,
            Arc::new(cvars),
            binds,
            pads,
            std::ptr::null_mut(),
        );
        let mut list = EntityList::new();
        let _scope = super::EntityScope::new(&engine.entity_access, &mut list);
        let (raw, buddy): (f64, f64) = engine
            .lua
            .load(
                r#"
                local buddy = ents.create("base_entity");
                buddy:spawn();
                local ent = ents.create("base_entity");
                ent:spawn();
                ent:set_networked("ammo", 10);
                ent:set_networked("neg", -3);
                ent:set_networked("on", false);
                ent:set_networked("rate", 0.25);
                ent:set_networked("name", "blaster");
                ent:set_networked("note", "");
                ent:set_networked("pos", Vector3(1, 2, 3));
                ent:set_networked("ang", Angle3(10, 20, 30));
                ent:set_networked("gone", 1);
                ent:set_networked("gone", nil);
                ent:set_networked("buddy", buddy);
                test_ent = ent;
                test_buddy = buddy;
                return ent._handle, buddy._handle;
                "#,
            )
            .set_name("setup.lua")
            .eval()
            .unwrap();
        let collected = collect_networked(&engine.lua).unwrap();
        assert!(collect_networked(&engine.lua).unwrap().is_empty());
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].handle, EntityHandle(raw as u32));
        let vars = &collected[0].vars;
        assert_eq!(value(vars, "ammo"), &NetValue::Int(10));
        assert_eq!(value(vars, "neg"), &NetValue::Int(-3));
        assert_eq!(value(vars, "on"), &NetValue::Bool(false));
        assert_eq!(value(vars, "rate"), &NetValue::Float(0.25));
        assert_eq!(
            value(vars, "name"),
            &NetValue::String("blaster".to_string())
        );
        assert_eq!(value(vars, "note"), &NetValue::String(String::new()));
        assert_eq!(
            value(vars, "pos"),
            &NetValue::Vector3(Vector3::new(1.0, 2.0, 3.0))
        );
        assert_eq!(
            value(vars, "ang"),
            &NetValue::Angle3(Angle3::new(10.0, 20.0, 30.0))
        );
        assert_eq!(
            value(vars, "buddy"),
            &NetValue::Entity(EntityHandle(buddy as u32))
        );
        assert_eq!(value(vars, "gone"), &NetValue::Nil);

        let copy_raw: f64 = engine
            .lua
            .load(
                r#"
                test_copy = ents.create("base_entity");
                test_copy:spawn();
                return test_copy._handle;
                "#,
            )
            .eval()
            .unwrap();
        let mut copied = collected[0].clone();
        copied.handle = EntityHandle(copy_raw as u32);
        let (skipped, missing) = apply_networked(&engine.lua, &[copied], None).unwrap();
        assert_eq!((skipped, missing), (0, 0));
        let (ammo, neg, on, rate, name, note, x, y, z, p, yaw, roll, gone, buddy_back): (
            f64,
            f64,
            bool,
            f64,
            String,
            String,
            f64,
            f64,
            f64,
            f64,
            f64,
            f64,
            String,
            f64,
        ) = engine
            .lua
            .load(
                r#"
                local ent = test_copy;
                local pos = ent:get_networked("pos");
                local ang = ent:get_networked("ang");
                local buddy = ent:get_networked("buddy");
                return ent:get_networked("ammo"),
                    ent:get_networked("neg"),
                    ent:get_networked("on", true),
                    ent:get_networked("rate"),
                    ent:get_networked("name"),
                    ent:get_networked("note"),
                    tonumber(pos.x), tonumber(pos.y), tonumber(pos.z),
                    tonumber(ang.p), tonumber(ang.y), tonumber(ang.r),
                    ent:get_networked("gone", "missing"),
                    buddy._handle;
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(ammo, 10.0);
        assert_eq!(neg, -3.0);
        assert!(!on);
        assert_eq!(rate, 0.25);
        assert_eq!(name, "blaster");
        assert_eq!(note, "");
        assert_eq!((x, y, z), (1.0, 2.0, 3.0));
        assert_eq!((p, yaw, roll), (10.0, 20.0, 30.0));
        assert_eq!(gone, "missing");
        assert_eq!(buddy_back, buddy);

        engine
            .lua
            .load(r#"test_ent:set_networked("ammo", 10, true);"#)
            .exec()
            .unwrap();
        set_local(&engine.lua, EntityHandle(raw as u32)).unwrap();
        let (skipped, missing) = apply_networked(
            &engine.lua,
            &[EntityNetworked {
                handle: EntityHandle(raw as u32),
                vars: vec![
                    NetVar {
                        key: "ammo".to_string(),
                        value: NetValue::Int(99),
                    },
                    NetVar {
                        key: "name".to_string(),
                        value: NetValue::String("changed".to_string()),
                    },
                ],
            }],
            None,
        )
        .unwrap();
        assert_eq!((skipped, missing), (1, 0));
        let (ammo, name): (f64, String) = engine
            .lua
            .load(r#"return test_ent:get_networked("ammo"), test_ent:get_networked("name")"#)
            .eval()
            .unwrap();
        assert_eq!(ammo, 10.0);
        assert_eq!(name, "changed");

        let class_hash: f64 = engine
            .lua
            .load(r#"return scripted_ents.get("base_entity").class_hash"#)
            .eval()
            .unwrap();
        let mut spawned = crate::entities::ScriptedEntity::new(class_hash as u32);
        spawned.spawned = true;
        let handle = list.spawn(Box::new(spawned)).unwrap();
        let known = super::net_spawn(
            &engine.lua,
            handle,
            &[
                NetVar {
                    key: "ammo".to_string(),
                    value: NetValue::Int(7),
                },
                NetVar {
                    key: "name".to_string(),
                    value: NetValue::String("spawned".to_string()),
                },
            ],
            None,
        )
        .unwrap();
        assert!(known);
        let (ammo, name): (f64, String) = engine
            .lua
            .load(&format!(
                r#"
                local ent = ents.get_by_index({idx})
                return ent:get_networked("ammo"), ent:get_networked("name")
                "#,
                idx = handle.index()
            ))
            .eval()
            .unwrap();
        assert_eq!(ammo, 7.0);
        assert_eq!(name, "spawned");
    }

    #[test]
    fn interpolated_netvars_blend_between_samples() {
        if crate::fs::try_global().is_none() {
            let fs = crate::fs::Fs::boot().expect("fs boot");
            crate::fs::set_global(Arc::new(fs));
        }

        let mut cvars = HashMap::new();
        cvars.insert(
            "sv_gravity".to_string(),
            Arc::new(ConVar::new(
                "sv_gravity",
                ConVarValue::Float(24.0),
                "World gravity",
                Some(false),
                Some(true),
            )),
        );
        let binds = Arc::new(Mutex::new(crate::input::Binds::defaults()));
        let pads = Arc::new(Mutex::new(crate::platform::PadCache::new()));
        let engine = ScriptEngine::new(
            Realm::Server,
            1.0 / 60.0,
            Arc::new(cvars),
            binds,
            pads,
            std::ptr::null_mut(),
        );
        let mut list = EntityList::new();
        let _scope = super::EntityScope::new(&engine.entity_access, &mut list);
        let raw: f64 = engine
            .lua
            .load(
                r#"
                scripted_ents.register({
                    base = "base_entity",
                    interpolated = { yaw = true, aim = true, label = true },
                }, "sent_smooth");
                local ent = ents.create("sent_smooth");
                ent:spawn();
                ent:set_interpolated("heat");
                test_smooth = ent;
                return ent._handle;
                "#,
            )
            .set_name("interp.lua")
            .eval()
            .unwrap();
        let handle = EntityHandle(raw as u32);

        let apply = |vars: Vec<NetVar>, time: f64| {
            apply_networked(&engine.lua, &[EntityNetworked { handle, vars }], Some(time)).unwrap();
        };

        apply(
            vec![
                NetVar {
                    key: "yaw".to_string(),
                    value: NetValue::Float(0.0),
                },
                NetVar {
                    key: "heat".to_string(),
                    value: NetValue::Float(0.0),
                },
                NetVar {
                    key: "aim".to_string(),
                    value: NetValue::Angle3(Angle3::new(0.0, 350.0, 0.0)),
                },
                NetVar {
                    key: "label".to_string(),
                    value: NetValue::String("a".to_string()),
                },
                NetVar {
                    key: "count".to_string(),
                    value: NetValue::Int(1),
                },
            ],
            0.0,
        );
        apply(
            vec![
                NetVar {
                    key: "yaw".to_string(),
                    value: NetValue::Float(10.0),
                },
                NetVar {
                    key: "heat".to_string(),
                    value: NetValue::Float(10.0),
                },
                NetVar {
                    key: "aim".to_string(),
                    value: NetValue::Angle3(Angle3::new(0.0, 10.0, 0.0)),
                },
                NetVar {
                    key: "label".to_string(),
                    value: NetValue::String("b".to_string()),
                },
                NetVar {
                    key: "count".to_string(),
                    value: NetValue::Int(2),
                },
            ],
            1.0,
        );

        let (yaw, heat, label, count): (f64, f64, String, f64) = engine
            .lua
            .load(
                r#"
                local ent = test_smooth;
                return ent:get_networked("yaw"), ent:get_networked("heat"), ent:get_networked("label"), ent:get_networked("count");
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(yaw, 0.0);
        assert_eq!(heat, 0.0);
        assert_eq!(label, "b");
        assert_eq!(count, 2.0);

        present_interpolated(&engine.lua, 0.5).unwrap();
        let (yaw, heat, aim_y): (f64, f64, f64) = engine
            .lua
            .load(
                r#"
                local ent = test_smooth;
                return ent:get_networked("yaw"), ent:get_networked("heat"), tonumber(ent:get_networked("aim").y);
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(yaw, 5.0);
        assert_eq!(heat, 5.0);
        assert_eq!(aim_y, 360.0);

        engine
            .lua
            .load(r#"test_smooth:set_interpolated("yaw", false);"#)
            .exec()
            .unwrap();
        apply(
            vec![NetVar {
                key: "yaw".to_string(),
                value: NetValue::Float(40.0),
            }],
            2.0,
        );
        let yaw: f64 = engine
            .lua
            .load(r#"return test_smooth:get_networked("yaw")"#)
            .eval()
            .unwrap();
        assert_eq!(yaw, 40.0);
        present_interpolated(&engine.lua, 2.0).unwrap();
        let yaw: f64 = engine
            .lua
            .load(r#"return test_smooth:get_networked("yaw")"#)
            .eval()
            .unwrap();
        assert_eq!(yaw, 40.0);
    }
}
