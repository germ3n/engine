use crate::physics::PhysicsAccess;
use crate::script::libs::ents::EntityAccess;
use crate::script::libs::vector3::Vector3;
use crate::script::Realm;
use crate::world::gen::{seed_from_f64, GenSettings};
use crate::world::{cwd_vmap_path, Block, BrushMap, BrushPlane, Face, VoxelWorld};
use mlua::{Lua, Table};
use r#macro::document;
use std::sync::atomic::{AtomicPtr, Ordering};
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
    summary = "True during the first prediction of a command, and whenever the server runs predicted_think.",
    returns = { ty = "boolean", desc = "False while the client replays a saved command." },
    see_also = "Entity:predicted_think",
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
    },
    returns = { ty = "table", desc = "Nil on a miss. Otherwise pos, block, face, distance, and position." },
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
    name = "brush_trace",
    kind = "function",
    realm = "shared",
    summary = "First brush hit by a line.",
    params = {
        start = { ty = "Vector3", desc = "Start of the line in world units." },
        end_pos = { ty = "Vector3", desc = "End of the line in world units." },
    },
    returns = { ty = "table", desc = "Nil on a miss. Otherwise brush, distance, position, and normal." },
)]
fn engine_brush_trace() {}

pub fn register_engine_lib(
    lua: &Lua,
    tick_interval: f64,
    realm: Realm,
    brush_access: BrushAccess,
    voxel_access: VoxelAccess,
    entity_access: EntityAccess,
    physics_access: PhysicsAccess,
    motion_access: MotionAccess,
    gen_settings: Arc<Mutex<GenSettings>>,
) {
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
            lua.create_function(move |lua, (start, end): (Vector3, Vector3)| {
                voxel_trace(lua, &voxels, start, end)
            })
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
            lua.create_function(move |lua, (start, end): (Vector3, Vector3)| {
                brush_trace(lua, &brushes, start, end)
            })
            .expect("[engine] Failed to create brush_trace"),
        )
        .expect("[engine] Failed setting brush_trace");
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

fn brush_trace(
    lua: &Lua,
    access: &BrushAccess,
    start: Vector3,
    end: Vector3,
) -> mlua::Result<Option<Table>> {
    let map = unsafe { access.load(Ordering::Relaxed).as_ref() };
    let Some(map) = map else {
        return Ok(None);
    };
    let Some(hit) = map.trace(start, end) else {
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
) -> mlua::Result<Option<Table>> {
    let world = unsafe { access.load(Ordering::Relaxed).as_ref() };
    let Some(world) = world else {
        return Ok(None);
    };
    let Some(hit) = world.trace(start, end) else {
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
