use crate::physics::PhysicsAccess;
use crate::script::libs::ents::{AnimAccess, EntityAccess};
use crate::script::libs::vector3::Vector3;
use crate::script::Realm;
use crate::world::gen::{seed_from_f64, GenSettings};
use crate::world::{
    cwd_vmap_path, Block, BlockPos, BrushMap, BrushPlane, Face, HitAll, TraceFilter, VoxelWorld,
};
use mlua::{Function, Lua, Table, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use r#macro::document;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

const ENGINE_TABLE: &str = "engine";

#[document(
    parent = "engine",
    name = "tick_interval",
    kind = "field",
    realm = "shared",
    summary = "Seconds between simulation ticks.",
    returns = { ty = "number", desc = "Fixed interval passed in when the Lua state is created." },
)]
fn engine_tick_interval() {}

#[document(
    parent = "engine",
    name = "curtime",
    kind = "field",
    realm = "shared",
    summary = "Simulation time in seconds.",
    returns = { ty = "number", desc = "Seconds since the server started simulating." },
)]
fn engine_curtime() {}

#[document(
    parent = "engine",
    name = "frametime",
    kind = "field",
    realm = "shared",
    summary = "Seconds since the previous frame.",
    returns = { ty = "number", desc = "Frame delta used for rendering, not the tick interval." },
)]
fn engine_frametime() {}

#[document(
    parent = "engine",
    name = "tick_count",
    kind = "field",
    realm = "shared",
    summary = "Simulation ticks since the server started.",
    returns = { ty = "number", desc = "Increments once per tick." },
)]
fn engine_tick_count() {}

#[document(
    parent = "engine",
    name = "first_time_predicted",
    kind = "field",
    realm = "shared",
    summary = "True during the first prediction of a command, and whenever the server runs move simulation. Not networked; set for the active UserCmd only.",
    returns = { ty = "boolean", desc = "False while the client replays a saved command." },
    see_also = "Entity:predicted_think, UserCmd",
)]
fn engine_first_time_predicted() {}

pub fn publish_clock(lua: &Lua, cur_time: f64, frame_time: f64, tick_count: u64) {
    let engine: mlua::Table = lua.named_registry_value(ENGINE_TABLE).unwrap();
    engine.set("curtime", cur_time).unwrap();
    engine.set("frametime", frame_time).unwrap();
    engine.set("tick_count", tick_count).unwrap();
}

#[document(
    kind = "library",
    name = "engine",
    realm = "shared",
    summary = "Simulation clock. curtime and tick_count advance with the server. frametime is the last frame delta. tick_interval is fixed for the session."
)]
pub type BrushAccess = Arc<AtomicPtr<BrushMap>>;
pub type VoxelAccess = Arc<AtomicPtr<VoxelWorld>>;
pub type MotionAccess = Arc<AtomicPtr<f64>>;

pub struct WorldScope<'a> {
    brush: &'a AtomicPtr<BrushMap>,
    voxels: &'a AtomicPtr<VoxelWorld>,
    motion: &'a AtomicPtr<f64>,
    previous_brush: *mut BrushMap,
    previous_voxels: *mut VoxelWorld,
    previous_motion: *mut f64,
}

impl<'a> WorldScope<'a> {
    pub fn new(
        brush_access: &'a AtomicPtr<BrushMap>,
        brush: *mut BrushMap,
        voxel_access: &'a AtomicPtr<VoxelWorld>,
        voxels: *mut VoxelWorld,
        motion_access: &'a AtomicPtr<f64>,
        motion: *mut f64,
    ) -> Self {
        Self {
            brush: brush_access,
            voxels: voxel_access,
            motion: motion_access,
            previous_brush: brush_access.swap(brush, Ordering::Relaxed),
            previous_voxels: voxel_access.swap(voxels, Ordering::Relaxed),
            previous_motion: motion_access.swap(motion, Ordering::Relaxed),
        }
    }
}

impl Drop for WorldScope<'_> {
    fn drop(&mut self) {
        self.brush.store(self.previous_brush, Ordering::Relaxed);
        self.voxels.store(self.previous_voxels, Ordering::Relaxed);
        self.motion.store(self.previous_motion, Ordering::Relaxed);
    }
}

#[document(
    parent = "engine",
    name = "map_scale",
    kind = "function",
    realm = "shared",
    summary = "Absolute brush map scale. 1 is the authored size.",
    returns = { ty = "number", desc = "Current brush scale, or 1 when the map is not available." },
)]
fn engine_map_scale() {}

#[document(
    parent = "engine",
    name = "voxel_scale",
    kind = "function",
    realm = "shared",
    summary = "Absolute voxel scale. 1 is one world unit per block.",
    returns = { ty = "number", desc = "Current voxel scale, or 1 when the map is not available." },
)]
fn engine_voxel_scale() {}

#[document(
    parent = "engine",
    name = "set_map_scale",
    kind = "function",
    realm = "server",
    summary = "Sets the brush map scale and moves entities by the change.",
    params = {
        scale = { ty = "number", desc = "Absolute scale. 1 is the authored size." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the scale is not positive." },
)]
fn engine_set_map_scale() {}

#[document(
    parent = "engine",
    name = "set_voxel_scale",
    kind = "function",
    realm = "server",
    summary = "Sets the voxel scale and moves entities by the change.",
    params = {
        scale = { ty = "number", desc = "Absolute scale. 1 is one world unit per block." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the scale is not positive." },
)]
fn engine_set_voxel_scale() {}

#[document(
    parent = "engine",
    name = "voxel_get",
    kind = "function",
    realm = "shared",
    summary = "Block id at a world position. 0 is air.",
    params = {
        pos = { ty = "Vector3", desc = "World position. The block containing this point is read." },
    },
    returns = { ty = "number", desc = "Block id, or 0 when the world is not available." },
)]
fn engine_voxel_get() {}

#[document(
    parent = "engine",
    name = "voxel_set",
    kind = "function",
    realm = "server",
    summary = "Sets the block that contains a world position. 0 removes it.",
    params = {
        pos = { ty = "Vector3", desc = "World position inside the block to write." },
        block = { ty = "number", desc = "Block id. 0 is air." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the position or block id is invalid." },
)]
fn engine_voxel_set() {}

#[document(
    parent = "engine",
    name = "voxel_fill",
    kind = "function",
    realm = "server",
    summary = "Fills every block overlapped by a world box. Corners can be passed in either order.",
    params = {
        min = { ty = "Vector3", desc = "One corner of the world box." },
        max = { ty = "Vector3", desc = "The opposite corner of the world box." },
        block = { ty = "number", desc = "Block id. 0 clears the blocks." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server, the box is invalid, or it covers more than 1000000 blocks." },
)]
fn engine_voxel_fill() {}

#[document(
    parent = "engine",
    name = "brush_box",
    kind = "function",
    realm = "server",
    summary = "Adds a solid box brush. Material 1 is the default flat color.",
    params = {
        min = { ty = "Vector3", desc = "One corner of the box in world units." },
        max = { ty = "Vector3", desc = "The opposite corner of the box in world units." },
        material = { ty = "number", desc = "Color id. Omitted values use 1." },
    },
    returns = { ty = "number", desc = "Brush index, or nil when the caller is not the server or the box has no volume." },
)]
fn engine_brush_box() {}

#[document(
    parent = "engine",
    name = "voxel_delete",
    kind = "function",
    realm = "server",
    summary = "Deletes one block, or every block overlapped by a world box.",
    params = {
        pos = { ty = "Vector3", desc = "World position to delete, or one corner of a box." },
        max = { ty = "Vector3", desc = "Opposite corner. Omit it to delete the single block at pos." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the box is invalid." },
)]
fn engine_voxel_delete() {}

#[document(
    parent = "engine",
    name = "voxel_sphere",
    kind = "function",
    realm = "server",
    summary = "Fills every block the sphere touches.",
    params = {
        center = { ty = "Vector3", desc = "Sphere center in world units." },
        radius = { ty = "number", desc = "Sphere radius in world units." },
        block = { ty = "number", desc = "Block id. 0 clears those blocks." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server, the sphere is invalid, or it covers more than 1000000 blocks." },
)]
fn engine_voxel_sphere() {}

#[document(
    parent = "engine",
    name = "voxel_clear",
    kind = "function",
    realm = "server",
    summary = "Removes every voxel block.",
    returns = { ty = "boolean", desc = "False when the caller is not the server." },
)]
fn engine_voxel_clear() {}

#[document(
    parent = "engine",
    name = "voxel_trace",
    kind = "function",
    realm = "shared",
    summary = "First solid block hit by a line.",
    params = {
        start = { ty = "Vector3", desc = "Start of the line in world units." },
        end_pos = { ty = "Vector3", desc = "End of the line in world units." },
        filter = { ty = "function", desc = "Called as filter(pos, block) for each solid block on the line, where pos is the block coordinate Vector3. Return false or nil to pass through that block.", optional = true },
    },
    returns = { ty = "table", desc = "Nil on a miss. Otherwise pos, block, face, distance, and position." },
    panics = "Rethrows the first error raised by the filter.",
)]
fn engine_voxel_trace() {}

#[document(
    parent = "engine",
    name = "voxel_seed",
    kind = "function",
    realm = "shared",
    summary = "Seed used for voxel generation.",
    returns = { ty = "number", desc = "Current seed. The default is 1." },
)]
fn engine_voxel_seed() {}

#[document(
    parent = "engine",
    name = "set_voxel_seed",
    kind = "function",
    realm = "server",
    summary = "Sets the generation seed. After streaming has started, generated chunks are cleared and built again. Chunks loaded from a vmap stay.",
    params = {
        seed = { ty = "number", desc = "Integer seed." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the seed is invalid." },
)]
fn engine_set_voxel_seed() {}

#[document(
    parent = "engine",
    name = "voxel_gen",
    kind = "function",
    realm = "server",
    summary = "Turns procedural chunk streaming on or off. It is off until this is called.",
    params = {
        enabled = { ty = "boolean", desc = "True starts generating around players and spawns." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server." },
    example = "engine.voxel_gen(true)",
)]
fn engine_voxel_gen() {}

#[document(
    parent = "engine",
    name = "voxel_gen_radius",
    kind = "function",
    realm = "server",
    summary = "Sets how many chunks around each player and spawn are generated.",
    params = {
        radius = { ty = "number", desc = "Horizontal radius in chunks, clamped from 1 to 32." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the radius is invalid." },
)]
fn engine_voxel_gen_radius() {}

#[document(
    parent = "engine",
    name = "voxel_gen_bounds",
    kind = "function",
    realm = "server",
    summary = "Sets the inclusive minimum and exclusive maximum block Z that generation fills.",
    params = {
        min_z = { ty = "number", desc = "Lowest block Z." },
        max_z = { ty = "number", desc = "One past the highest block Z." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the range is empty." },
)]
fn engine_voxel_gen_bounds() {}

#[document(
    parent = "engine",
    name = "voxel_sea_level",
    kind = "function",
    realm = "server",
    summary = "Sets the block Z that ocean biomes fill with water.",
    params = {
        level = { ty = "number", desc = "Sea level in blocks." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the level is invalid." },
)]
fn engine_voxel_sea_level() {}

#[document(
    parent = "engine",
    name = "voxel_set_solid",
    kind = "function",
    realm = "server",
    summary = "Sets whether a block id collides. Water is not solid by default. Non-air blocks still hide faces.",
    params = {
        block = { ty = "number", desc = "Block id. 0 is always air." },
        solid = { ty = "boolean", desc = "True makes the id solid." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the block id is invalid." },
)]
fn engine_voxel_set_solid() {}

#[document(
    parent = "engine",
    name = "voxel_save",
    kind = "function",
    realm = "server",
    summary = "Writes the current voxel world as VMAP version 2 to maps/<map>.vmap in the working directory.",
    returns = { ty = "boolean", desc = "False when the caller is not the server or the file cannot be written." },
)]
fn engine_voxel_save() {}

#[document(
    parent = "engine",
    name = "brush_convex",
    kind = "function",
    realm = "server",
    summary = "Adds a convex brush from planes. Each plane is { normal = Vector3, distance = number }.",
    params = {
        planes = { ty = "table", desc = "At least four planes. distance is the plane offset along normal." },
        material = { ty = "number", desc = "Color id. Omitted values use 1." },
    },
    returns = { ty = "number", desc = "Brush index, or nil when the solid is invalid." },
)]
fn engine_brush_convex() {}

#[document(
    parent = "engine",
    name = "brush_remove",
    kind = "function",
    realm = "server",
    summary = "Removes one brush. Later brushes keep their order, so their indexes move down by one.",
    params = {
        index = { ty = "number", desc = "Index returned by brush_box or brush_convex." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the index is missing." },
)]
fn engine_brush_remove() {}

#[document(
    parent = "engine",
    name = "brush_move",
    kind = "function",
    realm = "server",
    summary = "Moves one brush by a world offset.",
    params = {
        index = { ty = "number", desc = "Index returned by brush_box or brush_convex." },
        delta = { ty = "Vector3", desc = "World offset added to the brush." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the index is missing." },
)]
fn engine_brush_move() {}

#[document(
    parent = "engine",
    name = "brush_clear",
    kind = "function",
    realm = "server",
    summary = "Removes every brush, including the loaded map.",
    returns = { ty = "boolean", desc = "False when the caller is not the server." },
)]
fn engine_brush_clear() {}

#[document(
    parent = "engine",
    name = "trace_line",
    kind = "function",
    realm = "shared",
    summary = "Traces a line through the world and entity bone volumes and returns the nearest hit. SURF_BRUSH and SURF_VOXEL in the mask test brushes and voxels (SURF_WORLD is both); SURF_PHYSICS tests awake prop colliders; other bits are matched against each bone volume's flags.",
    params = {
        start = { ty = "Vector3", desc = "Start of the line in world units." },
        end_pos = { ty = "Vector3", desc = "End of the line in world units." },
        mask = { ty = "number", desc = "SURF_* flags to test. Defaults to MASK_SHOT (SURF_WORLD | SURF_PHYSICS | SURF_HITBOX | SURF_SOLID).", optional = true },
        filter = { ty = "function|Entity|table", desc = "An entity to ignore, a table of entities to ignore, or a function. A function is called as filter(brush) for brushes, filter(pos, block) for voxels, filter(entity) once per entity, then filter(entity, bone, group) per bone volume. Return false or nil to pass through.", optional = true },
    },
    returns = { ty = "table", desc = "Nil on a miss. Otherwise type (\"brush\", \"voxel\" or \"entity\"), distance, fraction and position, plus brush and normal for brushes, pos, block and face for voxels, or entity and normal for entities, with bone and group for bone volumes and physics = true for prop colliders." },
    example = "local hit = engine.trace_line(eye, eye + aim * 4096, MASK_SHOT, { me })\nif hit and hit.type == \"entity\" then print(hit.entity, hit.bone, hit.group) end",
    panics = "Rethrows the first error raised by the filter.",
)]
fn engine_trace_line() {}

#[document(
    parent = "engine",
    name = "brush_trace",
    kind = "function",
    realm = "shared",
    summary = "First brush hit by a line.",
    params = {
        start = { ty = "Vector3", desc = "Start of the line in world units." },
        end_pos = { ty = "Vector3", desc = "End of the line in world units." },
        filter = { ty = "function", desc = "Called as filter(brush) with each candidate brush index. Return false or nil to pass through that brush.", optional = true },
    },
    returns = { ty = "table", desc = "Nil on a miss. Otherwise brush, distance, position, and normal." },
    panics = "Rethrows the first error raised by the filter.",
)]
fn engine_brush_trace() {}

#[document(
    parent = "engine",
    name = "predicted_server_tick_count",
    kind = "function",
    realm = "client",
    summary = "The server tick the command being predicted now will run on: the server tick of the latest acknowledged state plus the commands still in flight. Not defined on the server.",
    returns = { ty = "number", desc = "The tick, or nil before the first state from the server arrives." },
)]
fn engine_predicted_server_tick_count() {}

static CLOCK_SHIFT: AtomicI64 = AtomicI64::new(0);
static CLOCK_SYNCED: AtomicBool = AtomicBool::new(false);

pub fn set_clock_shift(shift: i64) {
    CLOCK_SHIFT.store(shift, Ordering::Relaxed);
    CLOCK_SYNCED.store(true, Ordering::Relaxed);
}

pub fn reset_clock_shift() {
    CLOCK_SYNCED.store(false, Ordering::Relaxed);
}

fn predicted_server_tick(client_tick: u64) -> Option<u64> {
    if !CLOCK_SYNCED.load(Ordering::Relaxed) {
        return None;
    }

    let tick = client_tick as i64 - CLOCK_SHIFT.load(Ordering::Relaxed);

    Some(tick.max(0) as u64)
}

pub fn register_engine_lib(
    lua: &Lua,
    tick_interval: f64,
    realm: Realm,
    brush_access: BrushAccess,
    voxel_access: VoxelAccess,
    entity_access: EntityAccess,
    anim_access: AnimAccess,
    physics_access: PhysicsAccess,
    motion_access: MotionAccess,
    gen_settings: Arc<Mutex<GenSettings>>,
) {
    for (name, value) in [
        ("SURF_HITBOX", crate::anim::SURF_HITBOX),
        ("SURF_SOLID", crate::anim::SURF_SOLID),
        ("SURF_TRIGGER", crate::anim::SURF_TRIGGER),
        ("SURF_BRUSH", crate::anim::SURF_BRUSH),
        ("SURF_VOXEL", crate::anim::SURF_VOXEL),
        ("SURF_WORLD", crate::anim::SURF_WORLD),
        ("SURF_PHYSICS", crate::anim::SURF_PHYSICS),
        ("MASK_SHOT", crate::anim::MASK_SHOT),
        ("MASK_ALL", crate::anim::MASK_ALL),
    ] {
        lua.globals()
            .set(name, value)
            .unwrap_or_else(|err| panic!("[engine] Failed setting {name}: {err}"));
    }

    let engine_table = lua.create_table().expect("Failed to create engine table");
    engine_table
        .set("tick_interval", tick_interval)
        .expect("[engine] Failed setting tick_interval");
    engine_table
        .set("curtime", 0.0f64)
        .expect("[engine] Failed setting curtime");
    engine_table
        .set("frametime", 0.0f64)
        .expect("[engine] Failed setting frametime");
    engine_table
        .set("tick_count", 0u64)
        .expect("[engine] Failed setting tick_count");
    engine_table
        .set("first_time_predicted", true)
        .expect("[engine] Failed setting first_time_predicted");
    let server = matches!(realm, Realm::Server);
    if matches!(realm, Realm::Client) {
        engine_table
            .set(
                "predicted_server_tick_count",
                lua.create_function(|lua, ()| {
                    let engine: Table = lua.globals().get(ENGINE_TABLE)?;
                    let tick: u64 = engine.get("tick_count")?;

                    Ok(predicted_server_tick(tick))
                })
                .expect("[engine] Failed to create predicted_server_tick_count"),
            )
            .expect("[engine] Failed setting predicted_server_tick_count");
    }
    let brushes = brush_access.clone();
    engine_table
        .set(
            "map_scale",
            lua.create_function(move |_, ()| Ok(brush_scale(&brushes)))
                .expect("[engine] Failed to create map_scale"),
        )
        .expect("[engine] Failed setting map_scale");
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "voxel_scale",
            lua.create_function(move |_, ()| Ok(voxel_scale(&voxels)))
                .expect("[engine] Failed to create voxel_scale"),
        )
        .expect("[engine] Failed setting voxel_scale");
    let brushes = brush_access.clone();
    let entities = entity_access.clone();
    let physics = physics_access.clone();
    let motion = motion_access.clone();
    engine_table
        .set(
            "set_map_scale",
            lua.create_function(move |_, scale: f64| {
                if !server {
                    return Ok(false);
                }

                Ok(apply_brush(&brushes, &entities, &physics, &motion, scale))
            })
            .expect("[engine] Failed to create set_map_scale"),
        )
        .expect("[engine] Failed setting set_map_scale");
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "voxel_get",
            lua.create_function(move |_, point: Vector3| Ok(voxel_block(&voxels, point)))
                .expect("[engine] Failed to create voxel_get"),
        )
        .expect("[engine] Failed setting voxel_get");
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "voxel_set",
            lua.create_function(move |_, (point, block): (Vector3, f64)| {
                if !server {
                    return Ok(false);
                }

                Ok(voxel_write(&voxels, point, block))
            })
            .expect("[engine] Failed to create voxel_set"),
        )
        .expect("[engine] Failed setting voxel_set");
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "voxel_fill",
            lua.create_function(move |_, (min, max, block): (Vector3, Vector3, f64)| {
                if !server {
                    return Ok(false);
                }

                Ok(voxel_fill(&voxels, min, max, block))
            })
            .expect("[engine] Failed to create voxel_fill"),
        )
        .expect("[engine] Failed setting voxel_fill");
    let brushes = brush_access.clone();
    engine_table
        .set(
            "brush_box",
            lua.create_function(
                move |_, (min, max, material): (Vector3, Vector3, Option<f64>)| {
                    if !server {
                        return Ok(None);
                    }

                    Ok(brush_box(&brushes, min, max, material))
                },
            )
            .expect("[engine] Failed to create brush_box"),
        )
        .expect("[engine] Failed setting brush_box");
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "voxel_delete",
            lua.create_function(move |_, (pos, max): (Vector3, Option<Vector3>)| {
                if !server {
                    return Ok(false);
                }

                Ok(match max {
                    Some(max) => voxel_fill(&voxels, pos, max, 0.0),
                    None => voxel_write(&voxels, pos, 0.0),
                })
            })
            .expect("[engine] Failed to create voxel_delete"),
        )
        .expect("[engine] Failed setting voxel_delete");
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "voxel_sphere",
            lua.create_function(move |_, (center, radius, block): (Vector3, f64, f64)| {
                if !server {
                    return Ok(false);
                }

                Ok(voxel_sphere(&voxels, center, radius, block))
            })
            .expect("[engine] Failed to create voxel_sphere"),
        )
        .expect("[engine] Failed setting voxel_sphere");
    let voxels = voxel_access.clone();
    let gen = Arc::clone(&gen_settings);
    engine_table
        .set(
            "voxel_clear",
            lua.create_function(move |_, ()| {
                if !server {
                    return Ok(false);
                }

                Ok(voxel_clear(&voxels, &gen))
            })
            .expect("[engine] Failed to create voxel_clear"),
        )
        .expect("[engine] Failed setting voxel_clear");
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "voxel_trace",
            lua.create_function(
                move |lua, (start, end, filter): (Vector3, Vector3, Option<Function>)| {
                    voxel_trace(lua, &voxels, start, end, filter)
                },
            )
            .expect("[engine] Failed to create voxel_trace"),
        )
        .expect("[engine] Failed setting voxel_trace");
    let gen = Arc::clone(&gen_settings);
    engine_table
        .set(
            "voxel_seed",
            lua.create_function(move |_, ()| Ok(voxel_seed(&gen)))
                .expect("[engine] Failed to create voxel_seed"),
        )
        .expect("[engine] Failed setting voxel_seed");
    let gen = Arc::clone(&gen_settings);
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "set_voxel_seed",
            lua.create_function(move |_, seed: f64| {
                if !server {
                    return Ok(false);
                }

                Ok(set_voxel_seed(&voxels, &gen, seed))
            })
            .expect("[engine] Failed to create set_voxel_seed"),
        )
        .expect("[engine] Failed setting set_voxel_seed");
    let gen = Arc::clone(&gen_settings);
    engine_table
        .set(
            "voxel_gen",
            lua.create_function(move |_, enabled: bool| {
                if !server {
                    return Ok(false);
                }

                gen.lock().expect("gen settings").set_enabled(enabled);

                Ok(true)
            })
            .expect("[engine] Failed to create voxel_gen"),
        )
        .expect("[engine] Failed setting voxel_gen");
    let gen = Arc::clone(&gen_settings);
    engine_table
        .set(
            "voxel_gen_radius",
            lua.create_function(move |_, radius: f64| {
                if !server {
                    return Ok(false);
                }

                Ok(set_gen_radius(&gen, radius))
            })
            .expect("[engine] Failed to create voxel_gen_radius"),
        )
        .expect("[engine] Failed setting voxel_gen_radius");
    let gen = Arc::clone(&gen_settings);
    engine_table
        .set(
            "voxel_gen_bounds",
            lua.create_function(move |_, (min_z, max_z): (f64, f64)| {
                if !server {
                    return Ok(false);
                }

                Ok(set_gen_bounds(&gen, min_z, max_z))
            })
            .expect("[engine] Failed to create voxel_gen_bounds"),
        )
        .expect("[engine] Failed setting voxel_gen_bounds");
    let gen = Arc::clone(&gen_settings);
    engine_table
        .set(
            "voxel_sea_level",
            lua.create_function(move |_, level: f64| {
                if !server {
                    return Ok(false);
                }

                Ok(set_sea_level(&gen, level))
            })
            .expect("[engine] Failed to create voxel_sea_level"),
        )
        .expect("[engine] Failed setting voxel_sea_level");
    let gen = Arc::clone(&gen_settings);
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "voxel_set_solid",
            lua.create_function(move |_, (block, solid): (f64, bool)| {
                if !server {
                    return Ok(false);
                }

                Ok(set_block_solid(&voxels, &gen, block, solid))
            })
            .expect("[engine] Failed to create voxel_set_solid"),
        )
        .expect("[engine] Failed setting voxel_set_solid");
    let gen = gen_settings;
    let voxels = voxel_access.clone();
    engine_table
        .set(
            "voxel_save",
            lua.create_function(move |_, ()| {
                if !server {
                    return Ok(false);
                }

                Ok(voxel_save(&voxels, &gen))
            })
            .expect("[engine] Failed to create voxel_save"),
        )
        .expect("[engine] Failed setting voxel_save");
    let brushes = brush_access.clone();
    engine_table
        .set(
            "brush_convex",
            lua.create_function(move |_, (planes, material): (Table, Option<f64>)| {
                if !server {
                    return Ok(None);
                }

                let planes = read_planes(planes)?;

                Ok(brush_convex(&brushes, planes, material))
            })
            .expect("[engine] Failed to create brush_convex"),
        )
        .expect("[engine] Failed setting brush_convex");
    let brushes = brush_access.clone();
    engine_table
        .set(
            "brush_remove",
            lua.create_function(move |_, index: f64| {
                if !server {
                    return Ok(false);
                }

                Ok(brush_remove(&brushes, index))
            })
            .expect("[engine] Failed to create brush_remove"),
        )
        .expect("[engine] Failed setting brush_remove");
    let brushes = brush_access.clone();
    engine_table
        .set(
            "brush_move",
            lua.create_function(move |_, (index, delta): (f64, Vector3)| {
                if !server {
                    return Ok(false);
                }

                Ok(brush_move(&brushes, index, delta))
            })
            .expect("[engine] Failed to create brush_move"),
        )
        .expect("[engine] Failed setting brush_move");
    let brushes = brush_access.clone();
    engine_table
        .set(
            "brush_clear",
            lua.create_function(move |_, ()| {
                if !server {
                    return Ok(false);
                }

                Ok(brush_clear(&brushes))
            })
            .expect("[engine] Failed to create brush_clear"),
        )
        .expect("[engine] Failed setting brush_clear");
    let brushes = brush_access.clone();
    engine_table
        .set(
            "brush_trace",
            lua.create_function(
                move |lua, (start, end, filter): (Vector3, Vector3, Option<Function>)| {
                    brush_trace(lua, &brushes, start, end, filter)
                },
            )
            .expect("[engine] Failed to create brush_trace"),
        )
        .expect("[engine] Failed setting brush_trace");
    let trace_brushes = brush_access.clone();
    let trace_voxels = voxel_access.clone();
    let trace_entities = entity_access.clone();
    let trace_anims = anim_access;
    let trace_physics = physics_access.clone();
    engine_table
        .set(
            "trace_line",
            lua.create_function(
                move |lua, (start, end, mask, filter): (Vector3, Vector3, Option<u32>, Value)| {
                    trace_line(
                        lua,
                        tick_interval,
                        &trace_brushes,
                        &trace_voxels,
                        &trace_entities,
                        &trace_anims,
                        &trace_physics,
                        start,
                        end,
                        mask,
                        filter,
                    )
                },
            )
            .expect("[engine] Failed to create trace_line"),
        )
        .expect("[engine] Failed setting trace_line");
    let voxels = voxel_access;
    let entities = entity_access;
    let physics = physics_access;
    let motion = motion_access;
    engine_table
        .set(
            "set_voxel_scale",
            lua.create_function(move |_, scale: f64| {
                if !server {
                    return Ok(false);
                }

                Ok(apply_voxels(&voxels, &entities, &physics, &motion, scale))
            })
            .expect("[engine] Failed to create set_voxel_scale"),
        )
        .expect("[engine] Failed setting set_voxel_scale");
    lua.set_named_registry_value(ENGINE_TABLE, engine_table.clone())
        .expect("Failed to store engine table");
    lua.globals()
        .set("engine", engine_table)
        .expect("[net] Failed to set engine table")
}

fn brush_scale(access: &BrushAccess) -> f64 {
    unsafe { access.load(Ordering::Relaxed).as_ref() }
        .map(BrushMap::scale)
        .unwrap_or(1.0)
}

fn voxel_scale(access: &VoxelAccess) -> f64 {
    unsafe { access.load(Ordering::Relaxed).as_ref() }
        .map(VoxelWorld::scale)
        .unwrap_or(1.0)
}

fn apply_brush(
    brush: &BrushAccess,
    entities: &EntityAccess,
    physics: &PhysicsAccess,
    motion: &MotionAccess,
    scale: f64,
) -> bool {
    let brush = unsafe { brush.load(Ordering::Relaxed).as_mut() };
    let entities = unsafe { entities.load(Ordering::Relaxed).as_mut() };
    let motion = unsafe { motion.load(Ordering::Relaxed).as_mut() };
    let physics = unsafe { physics.load(Ordering::Relaxed).as_mut() };
    let (Some(brush), Some(entities), Some(motion)) = (brush, entities, motion) else {
        return false;
    };

    crate::scale::scale_brush(brush, entities, physics, motion, scale)
}

fn apply_voxels(
    voxels: &VoxelAccess,
    entities: &EntityAccess,
    physics: &PhysicsAccess,
    motion: &MotionAccess,
    scale: f64,
) -> bool {
    let voxels = unsafe { voxels.load(Ordering::Relaxed).as_mut() };
    let entities = unsafe { entities.load(Ordering::Relaxed).as_mut() };
    let motion = unsafe { motion.load(Ordering::Relaxed).as_mut() };
    let physics = unsafe { physics.load(Ordering::Relaxed).as_mut() };
    let (Some(voxels), Some(entities), Some(motion)) = (voxels, entities, motion) else {
        return false;
    };

    crate::scale::scale_voxels(voxels, entities, physics, motion, scale)
}

fn block_id(value: f64) -> Option<u16> {
    if !value.is_finite() || value < 0.0 || value > u16::MAX as f64 {
        return None;
    }

    Some(value as u16)
}

fn voxel_block(access: &VoxelAccess, point: Vector3) -> f64 {
    unsafe { access.load(Ordering::Relaxed).as_ref() }
        .map(|world| world.block_world(point).0 as f64)
        .unwrap_or(0.0)
}

fn voxel_write(access: &VoxelAccess, point: Vector3, block: f64) -> bool {
    let Some(block) = block_id(block) else {
        return false;
    };
    let world = unsafe { access.load(Ordering::Relaxed).as_mut() };
    let Some(world) = world else {
        return false;
    };

    world.set_at(point, Block(block))
}

fn voxel_fill(access: &VoxelAccess, min: Vector3, max: Vector3, block: f64) -> bool {
    let Some(block) = block_id(block) else {
        return false;
    };
    let world = unsafe { access.load(Ordering::Relaxed).as_mut() };
    let Some(world) = world else {
        return false;
    };

    world.fill_bounds(min, max, Block(block))
}

fn brush_box(
    access: &BrushAccess,
    min: Vector3,
    max: Vector3,
    material: Option<f64>,
) -> Option<f64> {
    let material = material_id(material)?;
    let Some(map) = brush_mut(access) else {
        return None;
    };
    let (lo, hi) = ordered_box(min, max);

    map.add_box_index(lo, hi, material)
        .map(|index| index as f64)
}

fn brush_convex(
    access: &BrushAccess,
    planes: Vec<BrushPlane>,
    material: Option<f64>,
) -> Option<f64> {
    let material = material_id(material)?;
    let Some(map) = brush_mut(access) else {
        return None;
    };

    map.add_convex_index(planes, material)
        .map(|index| index as f64)
}

fn brush_remove(access: &BrushAccess, index: f64) -> bool {
    let Some(index) = whole_index(index) else {
        return false;
    };
    let Some(map) = brush_mut(access) else {
        return false;
    };

    map.remove_brush(index)
}

fn brush_move(access: &BrushAccess, index: f64, delta: Vector3) -> bool {
    let Some(index) = whole_index(index) else {
        return false;
    };
    let Some(map) = brush_mut(access) else {
        return false;
    };

    map.move_brush(index, delta)
}

fn brush_clear(access: &BrushAccess) -> bool {
    let Some(map) = brush_mut(access) else {
        return false;
    };

    map.clear();

    true
}

pub(crate) struct LuaTraceFilter {
    predicate: Option<Function>,
    ignore: Vec<u32>,
    seen_entities: RefCell<HashMap<u32, bool>>,
    error: RefCell<Option<mlua::Error>>,
}

fn entity_handle(value: &Value) -> Option<u32> {
    match value {
        Value::Integer(raw) => u32::try_from(*raw).ok(),
        Value::Number(raw) if *raw >= 0.0 && raw.fract() == 0.0 => Some(*raw as u32),
        Value::Table(table) => table.get::<Option<u32>>("_handle").ok().flatten(),
        _ => None,
    }
}

impl LuaTraceFilter {
    pub(crate) fn new(predicate: Function) -> Self {
        Self {
            predicate: Some(predicate),
            ignore: Vec::new(),
            seen_entities: RefCell::new(HashMap::new()),
            error: RefCell::new(None),
        }
    }

    pub(crate) fn from_value(value: Value) -> mlua::Result<Self> {
        let mut filter = Self {
            predicate: None,
            ignore: Vec::new(),
            seen_entities: RefCell::new(HashMap::new()),
            error: RefCell::new(None),
        };

        match value {
            Value::Function(predicate) => filter.predicate = Some(predicate),
            Value::Table(table) => {
                if let Some(handle) = entity_handle(&Value::Table(table.clone())) {
                    filter.ignore.push(handle);
                } else {
                    for item in table.sequence_values::<Value>() {
                        let item = item?;
                        let handle = entity_handle(&item).ok_or_else(|| {
                            mlua::Error::RuntimeError(
                                "trace filter table must contain entities".to_string(),
                            )
                        })?;

                        filter.ignore.push(handle);
                    }
                }
            }
            Value::Integer(_) | Value::Number(_) => match entity_handle(&value) {
                Some(handle) => filter.ignore.push(handle),
                None => {
                    return Err(mlua::Error::RuntimeError("invalid entity handle".to_string()));
                }
            },
            Value::Nil => {}
            _ => {
                return Err(mlua::Error::RuntimeError(
                    "trace filter must be a function, entity or table of entities".to_string(),
                ));
            }
        }

        Ok(filter)
    }

    fn ask(&self, args: impl mlua::IntoLuaMulti) -> bool {
        if self.error.borrow().is_some() {
            return false;
        }

        let Some(predicate) = &self.predicate else {
            return true;
        };

        match predicate.call::<Value>(args) {
            Ok(value) => !matches!(value, Value::Nil | Value::Boolean(false)),
            Err(err) => {
                *self.error.borrow_mut() = Some(err);

                false
            }
        }
    }

    pub(crate) fn finish(self) -> mlua::Result<()> {
        match self.error.into_inner() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

impl TraceFilter for LuaTraceFilter {
    fn should_hit_brush(&self, brush: usize) -> bool {
        self.ask(brush as f64)
    }

    fn should_hit_entity(&self, entity: u32) -> bool {
        if self.ignore.contains(&entity) {
            return false;
        }

        // Broad-phase queries and per-bone loops can ask about the same entity many times in one
        // trace, so the predicate only runs once per entity.
        if let Some(seen) = self.seen_entities.borrow().get(&entity) {
            return *seen;
        }

        let hit = self.ask(entity as f64);

        self.seen_entities.borrow_mut().insert(entity, hit);

        hit
    }

    fn should_hit_bone(&self, entity: u32, bone: u16, group: u8) -> bool {
        self.ask((entity as f64, bone as f64, group as f64))
    }

    fn should_hit_voxel(&self, pos: BlockPos, block: Block) -> bool {
        self.ask((
            Vector3::new(pos.x as f64, pos.y as f64, pos.z as f64),
            block.0 as f64,
        ))
    }
}

fn resolve_entity(lua: &Lua, raw: u32) -> mlua::Result<Value> {
    match lua
        .globals()
        .get::<Table>("ents")
        .and_then(|ents| ents.get::<Function>("get"))
    {
        Ok(get) => get.call::<Value>(raw as f64),
        Err(_) => Ok(Value::Number(raw as f64)),
    }
}

#[allow(clippy::too_many_arguments)]
fn trace_line(
    lua: &Lua,
    tick_interval: f64,
    brushes: &BrushAccess,
    voxels: &VoxelAccess,
    entities: &EntityAccess,
    anims: &AnimAccess,
    physics: &PhysicsAccess,
    start: Vector3,
    end: Vector3,
    mask: Option<u32>,
    filter: Value,
) -> mlua::Result<Option<Table>> {
    let mask = mask.unwrap_or(crate::anim::MASK_SHOT);
    let filter = LuaTraceFilter::from_value(filter)?;
    let mut brush_hit = None;
    let mut voxel_hit = None;
    let mut bone_hit: Option<(u32, crate::anim::BoneHit)> = None;
    let mut prop_hit = None;

    if mask & crate::anim::SURF_BRUSH != 0 {
        if let Some(map) = unsafe { brushes.load(Ordering::Relaxed).as_ref() } {
            brush_hit = map.trace_filtered(start, end, &filter);
        }
    }

    if mask & crate::anim::SURF_VOXEL != 0 {
        if let Some(world) = unsafe { voxels.load(Ordering::Relaxed).as_ref() } {
            voxel_hit = world.trace_filtered(start, end, &filter);
        }
    }

    let list = unsafe { entities.load(Ordering::Relaxed).as_ref() };
    let bank = unsafe { anims.load(Ordering::Relaxed).as_mut() };

    if let (Some(list), Some(bank)) = (list, bank) {
        let time = lua
            .globals()
            .get::<Table>(ENGINE_TABLE)
            .and_then(|engine| engine.get::<u64>("tick_count"))
            .unwrap_or(0) as f64;
        let from = [start.x as f32, start.y as f32, start.z as f32];
        let to = [end.x as f32, end.y as f32, end.z as f32];

        for (handle, entity) in list.iter() {
            let base = entity.base();
            let playback = base.anim;
            let position = [base.position.x as f32, base.position.y as f32, base.position.z as f32];
            let angles = [base.angles.p as f32, base.angles.y as f32, base.angles.r as f32];
            let hit = bank.trace_bones(
                handle.0, &playback, position, angles, time, tick_interval, from, to, mask, &filter,
            );

            if let Some(hit) = hit {
                if bone_hit.map_or(true, |(_, cur)| hit.distance < cur.distance) {
                    bone_hit = Some((handle.0, hit));
                }
            }
        }
    }

    if mask & crate::anim::SURF_PHYSICS != 0 {
        if let Some(world) = unsafe { physics.load(Ordering::Relaxed).as_ref() } {
            prop_hit = world.trace_colliders(start, end, &|handle| filter.should_hit_entity(handle.0));
        }
    }

    filter.finish()?;

    let brush_dist = brush_hit.as_ref().map_or(f64::INFINITY, |hit| hit.distance);
    let voxel_dist = voxel_hit.as_ref().map_or(f64::INFINITY, |hit| hit.distance);
    let bone_dist = bone_hit.map_or(f64::INFINITY, |(_, hit)| hit.distance as f64);
    let prop_dist = prop_hit.map_or(f64::INFINITY, |hit| hit.distance);
    let nearest = brush_dist.min(voxel_dist).min(bone_dist).min(prop_dist);

    if !nearest.is_finite() {
        return Ok(None);
    }

    let table = lua.create_table()?;
    let length = {
        let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);

        (delta.x * delta.x + delta.y * delta.y + delta.z * delta.z).sqrt()
    };

    table.set("distance", nearest)?;
    table.set("fraction", if length > 0.0 { nearest / length } else { 0.0 })?;

    if nearest == brush_dist {
        let hit = brush_hit.expect("brush hit");

        table.set("type", "brush")?;
        table.set("brush", hit.brush as f64)?;
        table.set("position", hit.position)?;
        table.set("normal", hit.normal)?;
    } else if nearest == voxel_dist {
        let hit = voxel_hit.expect("voxel hit");
        let world = unsafe { voxels.load(Ordering::Relaxed).as_ref() };

        table.set("type", "voxel")?;
        table.set(
            "pos",
            Vector3::new(hit.block.x as f64, hit.block.y as f64, hit.block.z as f64),
        )?;
        table.set("block", world.map_or(0.0, |world| world.get(hit.block).0 as f64))?;
        table.set("position", hit.position)?;
        table.set("face", hit.face.map(face_name))?;
    } else if nearest == prop_dist {
        let hit = prop_hit.expect("prop hit");

        table.set("type", "entity")?;
        table.set("entity", resolve_entity(lua, hit.entity.0)?)?;
        table.set("physics", true)?;
        table.set("bone", hit.bone)?;
        table.set("position", hit.position)?;
        table.set("normal", hit.normal)?;
    } else {
        let (raw, hit) = bone_hit.expect("bone hit");
        let vec = |v: [f32; 3]| Vector3::new(v[0] as f64, v[1] as f64, v[2] as f64);

        table.set("type", "entity")?;
        table.set("entity", resolve_entity(lua, raw)?)?;
        table.set("bone", hit.bone)?;
        table.set("group", hit.group)?;
        table.set("position", vec(hit.position))?;
        table.set("normal", vec(hit.normal))?;
    }

    Ok(Some(table))
}

fn brush_trace(
    lua: &Lua,
    access: &BrushAccess,
    start: Vector3,
    end: Vector3,
    filter: Option<Function>,
) -> mlua::Result<Option<Table>> {
    let map = unsafe { access.load(Ordering::Relaxed).as_ref() };
    let Some(map) = map else {
        return Ok(None);
    };
    let hit = match filter {
        Some(predicate) => {
            let filter = LuaTraceFilter::new(predicate);
            let hit = map.trace_filtered(start, end, &filter);
            filter.finish()?;

            hit
        }
        None => map.trace_filtered(start, end, &HitAll),
    };
    let Some(hit) = hit else {
        return Ok(None);
    };
    let table = lua.create_table()?;
    table.set("brush", hit.brush as f64)?;
    table.set("distance", hit.distance)?;
    table.set("position", hit.position)?;
    table.set("normal", hit.normal)?;

    Ok(Some(table))
}

fn voxel_sphere(access: &VoxelAccess, center: Vector3, radius: f64, block: f64) -> bool {
    let Some(block) = block_id(block) else {
        return false;
    };
    let Some(world) = voxels_mut(access) else {
        return false;
    };

    world.fill_sphere(center, radius, Block(block))
}

fn voxel_clear(access: &VoxelAccess, settings: &Arc<Mutex<GenSettings>>) -> bool {
    let Some(world) = voxels_mut(access) else {
        return false;
    };

    world.clear();
    let mut settings = settings.lock().expect("gen settings");
    settings.epoch = settings.epoch.wrapping_add(1);
    settings.forget = true;

    true
}

fn voxel_seed(settings: &Arc<Mutex<GenSettings>>) -> f64 {
    settings.lock().expect("gen settings").seed as f64
}

fn set_voxel_seed(access: &VoxelAccess, settings: &Arc<Mutex<GenSettings>>, seed: f64) -> bool {
    let Some(seed) = seed_from_f64(seed) else {
        return false;
    };
    settings.lock().expect("gen settings").set_seed(seed);

    if let Some(world) = voxels_mut(access) {
        world.set_seed(seed);
    }

    true
}

fn set_gen_radius(settings: &Arc<Mutex<GenSettings>>, radius: f64) -> bool {
    if !radius.is_finite() {
        return false;
    }

    settings
        .lock()
        .expect("gen settings")
        .set_radius(radius as i32);

    true
}

fn set_gen_bounds(settings: &Arc<Mutex<GenSettings>>, min_z: f64, max_z: f64) -> bool {
    if !min_z.is_finite() || !max_z.is_finite() {
        return false;
    }

    if min_z < i32::MIN as f64 || max_z > i32::MAX as f64 {
        return false;
    }

    settings
        .lock()
        .expect("gen settings")
        .set_bounds(min_z as i32, max_z as i32)
}

fn set_sea_level(settings: &Arc<Mutex<GenSettings>>, level: f64) -> bool {
    settings.lock().expect("gen settings").set_sea_level(level)
}

fn set_block_solid(
    access: &VoxelAccess,
    settings: &Arc<Mutex<GenSettings>>,
    block: f64,
    solid: bool,
) -> bool {
    let Some(id) = block_id(block) else {
        return false;
    };

    if id == 0 {
        return false;
    }

    settings
        .lock()
        .expect("gen settings")
        .set_block_solid(id, solid);

    if let Some(world) = voxels_mut(access) {
        world.set_block_solid(id, solid);
    }

    true
}

fn voxel_save(access: &VoxelAccess, settings: &Arc<Mutex<GenSettings>>) -> bool {
    let (path, seed) = {
        let settings = settings.lock().expect("gen settings");
        let Some(path) = cwd_vmap_path(&settings.map_name) else {
            return false;
        };

        (path, settings.seed)
    };
    let Some(world) = voxels_mut(access) else {
        return false;
    };

    world.set_seed(seed);
    world.save_file(&path).is_ok()
}

fn voxel_trace(
    lua: &Lua,
    access: &VoxelAccess,
    start: Vector3,
    end: Vector3,
    filter: Option<Function>,
) -> mlua::Result<Option<Table>> {
    let world = unsafe { access.load(Ordering::Relaxed).as_ref() };
    let Some(world) = world else {
        return Ok(None);
    };
    let hit = match filter {
        Some(predicate) => {
            let filter = LuaTraceFilter::new(predicate);
            let hit = world.trace_filtered(start, end, &filter);
            filter.finish()?;

            hit
        }
        None => world.trace_filtered(start, end, &HitAll),
    };
    let Some(hit) = hit else {
        return Ok(None);
    };
    let table = lua.create_table()?;
    table.set(
        "pos",
        Vector3::new(hit.block.x as f64, hit.block.y as f64, hit.block.z as f64),
    )?;
    table.set("block", world.get(hit.block).0 as f64)?;
    table.set("distance", hit.distance)?;
    table.set("position", hit.position)?;
    table.set("face", hit.face.map(face_name))?;

    Ok(Some(table))
}

fn material_id(material: Option<f64>) -> Option<u16> {
    match material {
        Some(value) => block_id(value),
        None => Some(1),
    }
}

fn whole_index(value: f64) -> Option<usize> {
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f64 {
        return None;
    }

    let index = value as u32;

    if f64::from(index) != value {
        return None;
    }

    Some(index as usize)
}

fn ordered_box(min: Vector3, max: Vector3) -> (Vector3, Vector3) {
    (
        Vector3::new(min.x.min(max.x), min.y.min(max.y), min.z.min(max.z)),
        Vector3::new(min.x.max(max.x), min.y.max(max.y), min.z.max(max.z)),
    )
}

fn brush_mut(access: &BrushAccess) -> Option<&mut BrushMap> {
    unsafe { access.load(Ordering::Relaxed).as_mut() }
}

fn voxels_mut(access: &VoxelAccess) -> Option<&mut VoxelWorld> {
    unsafe { access.load(Ordering::Relaxed).as_mut() }
}

fn face_name(face: Face) -> &'static str {
    match face {
        Face::NegX => "-x",
        Face::PosX => "+x",
        Face::NegY => "-y",
        Face::PosY => "+y",
        Face::NegZ => "-z",
        Face::PosZ => "+z",
    }
}

fn read_planes(planes: Table) -> mlua::Result<Vec<BrushPlane>> {
    let mut out = Vec::new();

    for plane in planes.sequence_values::<Table>() {
        let plane = plane?;
        let normal: Vector3 = plane.get("normal")?;
        let distance: f64 = plane.get("distance")?;
        out.push(BrushPlane { normal, distance });
    }

    Ok(out)
}

#[cfg(test)]
mod trace_filter_tests {
    use super::*;

    fn filter_from(lua: &Lua, source: &str) -> mlua::Result<LuaTraceFilter> {
        LuaTraceFilter::from_value(lua.load(source).eval::<Value>()?)
    }

    #[test]
    fn nil_filter_hits_everything() {
        let lua = Lua::new();
        let filter = filter_from(&lua, "return nil").unwrap();

        assert!(filter.should_hit_entity(1));
        assert!(filter.should_hit_bone(1, 2, 3));
        assert!(filter.should_hit_brush(4));
        assert!(filter.finish().is_ok());
    }

    #[test]
    fn single_entity_is_ignored() {
        let lua = Lua::new();
        let by_object = filter_from(&lua, "return { _handle = 5 }").unwrap();
        let by_handle = filter_from(&lua, "return 5").unwrap();

        for filter in [by_object, by_handle] {
            assert!(!filter.should_hit_entity(5));
            assert!(filter.should_hit_entity(6));
            assert!(filter.should_hit_bone(5, 0, 0));
        }
    }

    #[test]
    fn table_of_entities_is_ignored() {
        let lua = Lua::new();
        let filter = filter_from(&lua, "return { { _handle = 2 }, 3, { _handle = 9 } }").unwrap();

        assert!(!filter.should_hit_entity(2));
        assert!(!filter.should_hit_entity(3));
        assert!(!filter.should_hit_entity(9));
        assert!(filter.should_hit_entity(4));
    }

    #[test]
    fn empty_table_ignores_nothing() {
        let lua = Lua::new();
        let filter = filter_from(&lua, "return {}").unwrap();

        assert!(filter.should_hit_entity(0));
    }

    #[test]
    fn bad_filters_are_rejected() {
        let lua = Lua::new();

        assert!(filter_from(&lua, "return { 'nope' }").is_err());
        assert!(filter_from(&lua, "return { 1, {} }").is_err());
        assert!(filter_from(&lua, "return 'text'").is_err());
        assert!(filter_from(&lua, "return true").is_err());
        assert!(filter_from(&lua, "return -1").is_err());
        assert!(filter_from(&lua, "return 1.5").is_err());
    }

    #[test]
    fn function_filter_receives_entity_then_bone_arguments() {
        let lua = Lua::new();
        let filter = filter_from(
            &lua,
            "return function(entity, bone, group)
                if bone == nil then return entity ~= 4 end
                return group ~= 9
            end",
        )
        .unwrap();

        assert!(!filter.should_hit_entity(4));
        assert!(filter.should_hit_entity(5));
        assert!(filter.should_hit_bone(5, 1, 8));
        assert!(!filter.should_hit_bone(5, 1, 9));
    }

    #[test]
    fn function_returning_nil_or_false_passes_through() {
        let lua = Lua::new();
        let nil = filter_from(&lua, "return function() return nil end").unwrap();
        let no = filter_from(&lua, "return function() return false end").unwrap();
        let yes = filter_from(&lua, "return function() return 0 end").unwrap();

        assert!(!nil.should_hit_entity(1));
        assert!(!no.should_hit_bone(1, 1, 1));
        assert!(yes.should_hit_entity(1));
    }

    #[test]
    fn function_error_is_reported_once_and_stops_hits() {
        let lua = Lua::new();
        let filter = filter_from(&lua, "return function() error('boom') end").unwrap();

        assert!(!filter.should_hit_entity(1));
        assert!(!filter.should_hit_bone(1, 1, 1));
        assert!(filter.finish().is_err());
    }

    #[test]
    fn ignore_list_wins_over_function() {
        let lua = Lua::new();
        let mut filter = filter_from(&lua, "return function() return true end").unwrap();

        filter.ignore.push(3);

        assert!(!filter.should_hit_entity(3));
        assert!(filter.should_hit_entity(4));
    }

    struct World {
        brushes: Box<BrushMap>,
        voxels: Box<VoxelWorld>,
        physics: Box<crate::physics::PhysicsWorld>,
    }

    impl World {
        // Voxel block near x=50, brush box near x=100, both on the line y=z=0.5.
        fn new() -> Self {
            let mut brushes = Box::new(BrushMap::new());
            let mut voxels = Box::new(VoxelWorld::new());

            assert!(brushes.add_box(Vector3::new(100.0, 0.0, 0.0), Vector3::new(101.0, 1.0, 1.0), 1));
            voxels.set(BlockPos::new(50, 0, 0), Block(1));

            let mut physics = Box::new(crate::physics::PhysicsWorld::new());

            // Awake prop box (half 1) centered at x=75, between the voxel and the brush.
            physics.add_test_prop(EntityHandle(5), [75.0, 0.5, 0.5]);
            // A second, farther prop (entity 6) behind the first.
            physics.add_test_prop(EntityHandle(6), [90.0, 0.5, 0.5]);

            Self {
                brushes,
                voxels,
                physics,
            }
        }

        fn trace_table(
            &mut self,
            lua: &Lua,
            from: f64,
            to: f64,
            mask: Option<u32>,
            filter: Value,
        ) -> Option<Table> {
            let ctor = lua
                .create_function(|lua, (x, y, z): (f64, f64, f64)| {
                    let table = lua.create_table()?;

                    table.set("x", x)?;
                    table.set("y", y)?;
                    table.set("z", z)?;

                    Ok(table)
                })
                .expect("ctor");

            lua.set_named_registry_value("Vector3Ctor", ctor).expect("registry");

            let brushes: BrushAccess = Arc::new(AtomicPtr::new(&mut *self.brushes));
            let voxels: VoxelAccess = Arc::new(AtomicPtr::new(&mut *self.voxels));
            let entities: EntityAccess = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
            let anims: AnimAccess = Arc::new(AtomicPtr::new(std::ptr::null_mut()));
            let physics: PhysicsAccess = Arc::new(AtomicPtr::new(&mut *self.physics));

            trace_line(
                lua,
                1.0 / 60.0,
                &brushes,
                &voxels,
                &entities,
                &anims,
                &physics,
                Vector3::new(from, 0.5, 0.5),
                Vector3::new(to, 0.5, 0.5),
                mask,
                filter,
            )
            .expect("trace")
        }

        fn trace(&mut self, from: f64, to: f64, mask: Option<u32>) -> Option<(String, f64)> {
            let lua = Lua::new();

            self.trace_table(&lua, from, to, mask, Value::Nil).map(|table| {
                (
                    table.get::<String>("type").expect("type"),
                    table.get::<f64>("distance").expect("distance"),
                )
            })
        }
    }

    use crate::anim::{MASK_SHOT, SURF_BRUSH, SURF_HITBOX, SURF_PHYSICS, SURF_VOXEL, SURF_WORLD};
    use crate::entities::EntityHandle;

    #[test]
    fn brush_flag_hits_only_brushes() {
        let mut world = World::new();
        let (kind, distance) = world.trace(-10.0, 200.0, Some(SURF_BRUSH)).expect("brush");

        assert_eq!(kind, "brush");
        assert!((distance - 110.0).abs() < 1e-3);
    }

    #[test]
    fn voxel_flag_hits_only_voxels() {
        let mut world = World::new();
        let (kind, distance) = world.trace(-10.0, 200.0, Some(SURF_VOXEL)).expect("voxel");

        assert_eq!(kind, "voxel");
        assert!(distance > 55.0 && distance < 65.0);
    }

    #[test]
    fn world_flag_returns_the_nearest_of_both() {
        let mut world = World::new();

        assert_eq!(world.trace(-10.0, 200.0, Some(SURF_WORLD)).expect("hit").0, "voxel");
        assert_eq!(world.trace(200.0, -10.0, Some(SURF_WORLD)).expect("hit").0, "brush");
        assert_eq!(world.trace(-10.0, 200.0, Some(SURF_BRUSH | SURF_VOXEL)).expect("hit").0, "voxel");
    }

    #[test]
    fn missing_flag_skips_that_geometry_entirely() {
        let mut world = World::new();

        // Voxel is nearer going +X, but a brush-only mask must pass through it.
        assert_eq!(world.trace(-10.0, 200.0, Some(SURF_BRUSH)).expect("hit").0, "brush");
        // Brush is nearer going -X, but a voxel-only mask must pass through it.
        assert_eq!(world.trace(200.0, -10.0, Some(SURF_VOXEL)).expect("hit").0, "voxel");
    }

    #[test]
    fn masks_without_world_flags_miss_the_world() {
        let mut world = World::new();

        assert!(world.trace(-10.0, 200.0, Some(0)).is_none());
        assert!(world.trace(-10.0, 200.0, Some(SURF_HITBOX)).is_none());
    }

    #[test]
    fn default_mask_includes_the_world() {
        let mut world = World::new();

        assert_eq!(world.trace(-10.0, 200.0, None).expect("hit").0, "voxel");
        assert_eq!(MASK_SHOT & SURF_WORLD, SURF_WORLD);
        assert_eq!(world.trace(-10.0, 40.0, Some(SURF_WORLD)), None);
    }

    #[test]
    fn physics_flag_hits_only_props() {
        let mut world = World::new();
        let (kind, distance) = world.trace(-10.0, 200.0, Some(SURF_PHYSICS)).expect("prop");

        assert_eq!(kind, "entity");
        assert!((distance - 84.0).abs() < 1e-2);
        assert!(world.trace(-10.0, 60.0, Some(SURF_PHYSICS)).is_none());
    }

    #[test]
    fn world_flags_do_not_hit_props() {
        let mut world = World::new();

        // The prop sits between the voxel and the brush; a brush-only trace goes past it.
        assert_eq!(world.trace(200.0, -10.0, Some(SURF_BRUSH)).expect("hit").0, "brush");
        assert_eq!(world.trace(60.0, 200.0, Some(SURF_BRUSH)).expect("hit").1.round(), 40.0);
        assert!(world.trace(60.0, 200.0, Some(SURF_VOXEL)).is_none());
    }

    #[test]
    fn nearest_hit_wins_across_world_and_props() {
        let mut world = World::new();

        assert_eq!(world.trace(-10.0, 200.0, Some(SURF_WORLD | SURF_PHYSICS)).expect("hit").0, "voxel");
        assert_eq!(world.trace(200.0, -10.0, Some(SURF_WORLD | SURF_PHYSICS)).expect("hit").0, "brush");
        assert_eq!(world.trace(70.0, 200.0, Some(MASK_SHOT)).expect("hit").0, "entity");
    }

    fn entity_of(table: &Table) -> f64 {
        table.get::<f64>("entity").expect("entity")
    }

    #[test]
    fn ignored_props_are_passed_through() {
        let mut world = World::new();
        let lua = Lua::new();
        let mask = Some(SURF_PHYSICS);
        let eval = |lua: &Lua, src: &str| lua.load(src).eval::<Value>().unwrap();

        let first = world.trace_table(&lua, -10.0, 200.0, mask, Value::Nil).expect("hit");
        assert_eq!(entity_of(&first), 5.0);
        assert!(first.get::<bool>("physics").unwrap());

        let filter = eval(&lua, "return { _handle = 5 }");
        let second = world.trace_table(&lua, -10.0, 200.0, mask, filter).expect("hit");
        assert_eq!(entity_of(&second), 6.0);

        let filter = eval(&lua, "return { { _handle = 5 }, 6 }");
        assert!(world.trace_table(&lua, -10.0, 200.0, mask, filter).is_none());
    }

    #[test]
    fn function_filter_controls_props_and_runs_once_per_entity() {
        let mut world = World::new();
        let lua = Lua::new();

        lua.load("calls = {}").exec().unwrap();

        let counting = lua
            .load("return function(e) calls[e] = (calls[e] or 0) + 1; return e ~= 5 end")
            .eval::<Value>()
            .unwrap();
        let hit = world
            .trace_table(&lua, -10.0, 200.0, Some(SURF_PHYSICS), counting)
            .expect("hit");

        assert_eq!(entity_of(&hit), 6.0);

        let calls: Table = lua.globals().get("calls").unwrap();
        let mut asked = 0;

        for pair in calls.pairs::<f64, f64>() {
            let (_, count) = pair.unwrap();

            assert_eq!(count, 1.0);
            asked += 1;
        }

        assert!((1..=2).contains(&asked));
        assert_eq!(calls.get::<f64>(5.0).unwrap(), 1.0);
    }

    #[test]
    fn function_filter_rejecting_every_prop_misses() {
        let mut world = World::new();
        let lua = Lua::new();
        let reject = lua.load("return function() return false end").eval::<Value>().unwrap();

        assert!(world.trace_table(&lua, -10.0, 200.0, Some(SURF_PHYSICS), reject).is_none());
    }

    #[test]
    fn entity_filter_results_are_remembered() {
        let lua = Lua::new();

        lua.load("n = 0").exec().unwrap();

        let filter = filter_from(&lua, "return function(e) n = n + 1; return e == 1 end").unwrap();

        assert!(filter.should_hit_entity(1));
        assert!(filter.should_hit_entity(1));
        assert!(!filter.should_hit_entity(2));
        assert!(!filter.should_hit_entity(2));
        assert_eq!(lua.globals().get::<f64>("n").unwrap(), 2.0);
        assert!(filter.finish().is_ok());
    }
}
