use crate::entities::{EntityHandle, EntityList};
use crate::physics::PhysicsWorld;
use crate::script::libs::vector3::Vector3;
use crate::world::{BrushMap, VoxelWorld};

pub fn shift_entities(
    entities: &mut EntityList,
    mut physics: Option<&mut PhysicsWorld>,
    ratio: f64,
) {
    if !ratio.is_finite() || (ratio - 1.0).abs() <= 1e-12 {
        return;
    }

    let handles = entities.handles();
    let mut idx = 0;

    while idx < handles.len() {
        let handle = handles[idx];
        idx += 1;
        let moved = shift_one(entities, handle, ratio);
        let Some((position, velocity)) = moved else {
            continue;
        };

        let Some(physics) = physics.as_mut() else {
            continue;
        };

        physics.teleport_position(handle, position.x, position.y, position.z);
        physics.teleport_velocity(handle, velocity);
    }
}

fn shift_one(
    entities: &mut EntityList,
    handle: EntityHandle,
    ratio: f64,
) -> Option<(Vector3, Vector3)> {
    let entity = entities.get_mut(handle)?;
    let base = entity.base_mut();
    base.position.x *= ratio;
    base.position.y *= ratio;
    base.position.z *= ratio;
    base.velocity.x *= ratio;
    base.velocity.y *= ratio;
    base.velocity.z *= ratio;

    Some((base.position, base.velocity))
}

pub fn scale_brush(
    brush: &mut BrushMap,
    entities: &mut EntityList,
    physics: Option<&mut PhysicsWorld>,
    motion: &mut f64,
    scale: f64,
) -> bool {
    let old = brush.scale();

    if !brush.set_scale(scale) {
        return false;
    }

    note_motion(entities, physics, motion, brush.scale() / old);

    true
}

pub fn scale_voxels(
    voxels: &mut VoxelWorld,
    entities: &mut EntityList,
    physics: Option<&mut PhysicsWorld>,
    motion: &mut f64,
    scale: f64,
) -> bool {
    let old = voxels.scale();

    if !voxels.set_scale(scale) {
        return false;
    }

    note_motion(entities, physics, motion, voxels.scale() / old);

    true
}

pub fn scale_both(
    brush: &mut BrushMap,
    voxels: &mut VoxelWorld,
    entities: &mut EntityList,
    physics: Option<&mut PhysicsWorld>,
    motion: &mut f64,
    ratio: f64,
) -> bool {
    if !ratio.is_finite() || ratio <= 0.0 {
        return false;
    }

    let brush_scale = brush.scale() * ratio;
    let voxel_scale = voxels.scale() * ratio;

    if !brush_scale.is_finite()
        || brush_scale <= 0.0
        || !voxel_scale.is_finite()
        || voxel_scale <= 0.0
    {
        return false;
    }

    let old_brush = brush.scale();
    let old_voxel = voxels.scale();

    if !brush.set_scale(brush_scale) {
        return false;
    }

    if !voxels.set_scale(voxel_scale) {
        let _ = brush.set_scale(old_brush);

        return false;
    }

    let moved =
        (brush.scale() - old_brush).abs() > 1e-12 || (voxels.scale() - old_voxel).abs() > 1e-12;

    if !moved {
        return true;
    }

    shift_entities(entities, physics, ratio);
    *motion *= ratio;

    true
}

fn note_motion(
    entities: &mut EntityList,
    physics: Option<&mut PhysicsWorld>,
    motion: &mut f64,
    ratio: f64,
) {
    if !ratio.is_finite() || (ratio - 1.0).abs() <= 1e-12 {
        return;
    }

    shift_entities(entities, physics, ratio);
    *motion *= ratio;
}
