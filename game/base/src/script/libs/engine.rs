use crate::physics::PhysicsAccess;
use crate::script::libs::ents::EntityAccess;
use crate::script::Realm;
use crate::world::{BrushMap, VoxelWorld};
use mlua::Lua;
use r#macro::document;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

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

pub fn register_engine_lib(
    lua: &Lua,
    tick_interval: f64,
    realm: Realm,
    brush_access: BrushAccess,
    voxel_access: VoxelAccess,
    entity_access: EntityAccess,
    physics_access: PhysicsAccess,
    motion_access: MotionAccess,
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
