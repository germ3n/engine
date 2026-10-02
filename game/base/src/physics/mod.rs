use crate::anchor::SHIFT;
use crate::anim::{angles_from_pose, pose_matrix};
use crate::entities::{EntityHandle, EntityList};
use crate::movement::{STAND_MAXS, STAND_MINS};
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use crate::world::{BrushMap, VoxelWorld};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{
    BroadPhaseBvh, CCDSolver, ColliderBuilder, ColliderHandle, ColliderSet, ImpulseJointSet,
    IntegrationParameters, IslandManager, MultibodyJointSet, NarrowPhase, PhysicsPipeline,
    RigidBodyBuilder, RigidBodyHandle, RigidBodySet, SoftBodySet,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

const DENSITY: f32 = 300.0;
const MIN_HALF: f32 = 0.02;
const VOXEL_STRIDE: usize = 6;
const ORIGIN_SNAP: f64 = 64.0;

pub type PhysicsAccess = Arc<AtomicPtr<PhysicsWorld>>;

pub struct PhysicsScope<'a> {
    access: &'a AtomicPtr<PhysicsWorld>,
    previous: *mut PhysicsWorld,
}

impl<'a> PhysicsScope<'a> {
    pub fn new(access: &'a AtomicPtr<PhysicsWorld>, world: *mut PhysicsWorld) -> Self {
        let previous = access.swap(world, Ordering::Relaxed);

        Self { access, previous }
    }
}

impl Drop for PhysicsScope<'_> {
    fn drop(&mut self) {
        self.access.store(self.previous, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy)]
struct Link {
    body: RigidBodyHandle,
    collider: ColliderHandle,
    kinematic: bool,
}

#[derive(Clone, Copy)]
struct Prop {
    center: [f32; 3],
    half: [f32; 3],
    mass: f32,
    body: Option<RigidBodyHandle>,
    collider: Option<ColliderHandle>,
}

pub struct PhysicsWorld {
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad_phase: BroadPhaseBvh,
    narrow_phase: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    soft_bodies: SoftBodySet,
    ccd: CCDSolver,
    links: HashMap<EntityHandle, Link>,
    props: HashMap<EntityHandle, Prop>,
    static_body: Option<RigidBodyHandle>,
    origin: Vector3,
    static_origin: Vector3,
    brush_revision: u64,
    voxel_revision: u64,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft_bodies: SoftBodySet::new(),
            ccd: CCDSolver::new(),
            links: HashMap::new(),
            props: HashMap::new(),
            static_body: None,
            origin: Vector3::new(0.0, 0.0, 0.0),
            static_origin: Vector3::new(f64::MAX, f64::MAX, f64::MAX),
            brush_revision: u64::MAX,
            voxel_revision: u64::MAX,
        }
    }

    pub fn enable_box(
        &mut self,
        handle: EntityHandle,
        _position: Vector3,
        _angles: Angle3,
        _velocity: Vector3,
        center: [f32; 3],
        half: [f32; 3],
    ) -> bool {
        if handle.is_null() {
            return false;
        }

        if self.links.get(&handle).is_some_and(|link| link.kinematic) {
            return false;
        }

        if self.props.contains_key(&handle) {
            return true;
        }

        let half = sanitize_half(half);
        self.props.insert(
            handle,
            Prop {
                center,
                half,
                mass: box_mass(half),
                body: None,
                collider: None,
            },
        );

        true
    }

    pub fn set_mass(&mut self, handle: EntityHandle, mass: f32) -> bool {
        if !mass.is_finite() || mass <= 0.0 {
            return false;
        }

        let Some(prop) = self.props.get_mut(&handle) else {
            return false;
        };

        prop.mass = mass;
        let collider = prop.collider;
        let body = prop.body;

        if let Some(collider) = collider {
            if let Some(shape) = self.colliders.get_mut(collider) {
                shape.set_mass(mass);
            }
        }

        if let Some(body) = body {
            if let Some(rigid) = self.bodies.get_mut(body) {
                rigid.wake_up(true);
            }
        }

        true
    }

    pub fn asleep_mass(&self, handle: EntityHandle) -> Option<f32> {
        let prop = self.props.get(&handle)?;

        if prop.body.is_some() {
            return None;
        }

        Some(prop.mass)
    }

    pub fn apply_impulse(&mut self, handle: EntityHandle, impulse: Vector3) -> bool {
        if !impulse.x.is_finite() || !impulse.y.is_finite() || !impulse.z.is_finite() {
            return false;
        }

        let Some(prop) = self.props.get(&handle).copied() else {
            return false;
        };

        let Some(body) = prop.body else {
            return false;
        };

        let Some(rigid) = self.bodies.get_mut(body) else {
            return false;
        };

        rigid.apply_impulse(
            Vector::new(impulse.x as f32, impulse.y as f32, impulse.z as f32),
            true,
        );

        true
    }

    pub fn teleport_position(&mut self, handle: EntityHandle, x: f64, y: f64, z: f64) {
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return;
        }

        if let Some(link) = self.links.get(&handle).copied() {
            if link.kinematic {
                let translation = self.player_sim(Vector3::new(x, y, z));
                let Some(body) = self.bodies.get_mut(link.body) else {
                    return;
                };
                body.set_translation(translation, true);
                body.set_next_kinematic_translation(translation);

                return;
            }
        }

        let Some(prop) = self.props.get(&handle).copied() else {
            return;
        };
        let Some(body_handle) = prop.body else {
            return;
        };
        let translation = self.to_sim(Vector3::new(x, y, z));
        let Some(body) = self.bodies.get_mut(body_handle) else {
            return;
        };

        body.set_translation(translation, true);
    }

    pub fn teleport_angles(&mut self, handle: EntityHandle, angles: Angle3) {
        let Some(prop) = self.props.get(&handle).copied() else {
            return;
        };
        let Some(body_handle) = prop.body else {
            return;
        };
        let Some(body) = self.bodies.get_mut(body_handle) else {
            return;
        };

        body.set_rotation(rotation_of(angles), true);
    }

    pub fn teleport_velocity(&mut self, handle: EntityHandle, velocity: Vector3) {
        let Some(prop) = self.props.get(&handle).copied() else {
            return;
        };
        let Some(body_handle) = prop.body else {
            return;
        };
        let Some(body) = self.bodies.get_mut(body_handle) else {
            return;
        };

        body.set_linvel(linear_velocity(velocity), true);
    }

    pub fn forget(&mut self, handle: EntityHandle) {
        if let Some(link) = self.links.remove(&handle) {
            self.remove_body(link.body);
        }

        if let Some(prop) = self.props.remove(&handle) {
            if let Some(body) = prop.body {
                self.remove_body(body);
            }
        }
    }

    pub fn step(
        &mut self,
        entities: &mut EntityList,
        players: &[EntityHandle],
        dt: f64,
        gravity: f64,
        brushes: &BrushMap,
        voxels: &VoxelWorld,
    ) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }

        self.forget_invalid(entities);
        self.recenter(entities, players);
        self.refresh_static(brushes, voxels);
        self.sync_props(entities);
        self.sync_players(entities, players);
        self.integrate(dt as f32, gravity as f32);
        self.write_dynamic(entities);
    }

    fn forget_invalid(&mut self, entities: &EntityList) {
        let mut stale = Vec::new();

        for handle in self.links.keys() {
            if !entities.is_valid(*handle) {
                stale.push(*handle);
            }
        }

        for handle in self.props.keys() {
            if !entities.is_valid(*handle) {
                stale.push(*handle);
            }
        }

        let mut idx = 0;

        while idx < stale.len() {
            self.forget(stale[idx]);
            idx += 1;
        }
    }

    fn recenter(&mut self, entities: &EntityList, players: &[EntityHandle]) {
        let mut focus = None;
        let mut idx = 0;

        while idx < players.len() {
            if let Some(entity) = entities.get(players[idx]) {
                focus = Some(entity.base().position);

                break;
            }

            idx += 1;
        }

        let Some(focus) = focus else {
            return;
        };

        if !farther(self.origin, focus, ORIGIN_SNAP) {
            return;
        }

        self.origin = focus;
        let handles: Vec<EntityHandle> = self.links.keys().copied().collect();
        idx = 0;

        while idx < handles.len() {
            let handle = handles[idx];
            idx += 1;
            let Some(link) = self.links.get(&handle).copied() else {
                continue;
            };
            let Some(entity) = entities.get(handle) else {
                continue;
            };
            let position = entity.base().position;
            let translation = if link.kinematic {
                self.player_sim(position)
            } else {
                self.to_sim(position)
            };
            let Some(body) = self.bodies.get_mut(link.body) else {
                continue;
            };

            if link.kinematic {
                let pose = Pose::from_translation(translation);
                body.set_position(pose, true);
                body.set_next_kinematic_position(pose);
            } else {
                body.set_translation(translation, true);
                body.set_rotation(rotation_of(entity.base().angles), true);
            }
        }

        let prop_handles: Vec<EntityHandle> = self.props.keys().copied().collect();
        idx = 0;

        while idx < prop_handles.len() {
            let handle = prop_handles[idx];
            idx += 1;
            let Some(prop) = self.props.get(&handle).copied() else {
                continue;
            };
            let Some(body_handle) = prop.body else {
                continue;
            };
            let Some(entity) = entities.get(handle) else {
                continue;
            };
            let position = entity.base().position;
            let angles = entity.base().angles;
            let translation = self.to_sim(position);
            let Some(body) = self.bodies.get_mut(body_handle) else {
                continue;
            };
            body.set_translation(translation, true);
            body.set_rotation(rotation_of(angles), true);
        }
    }

    fn refresh_static(&mut self, brushes: &BrushMap, voxels: &VoxelWorld) {
        let brush_rev = brushes.revision();
        let voxel_rev = voxels.revision();

        if self.static_body.is_some()
            && self.brush_revision == brush_rev
            && self.voxel_revision == voxel_rev
            && self.static_origin == self.origin
        {
            return;
        }

        self.brush_revision = brush_rev;
        self.voxel_revision = voxel_rev;
        self.static_origin = self.origin;

        if let Some(handle) = self.static_body.take() {
            self.remove_body(handle);
        }

        let body = self.bodies.insert(RigidBodyBuilder::fixed().build());
        self.static_body = Some(body);
        let reach = SHIFT;
        let min = Vector3::new(
            self.origin.x - reach,
            self.origin.y - reach,
            self.origin.z - reach,
        );
        let max = Vector3::new(
            self.origin.x + reach,
            self.origin.y + reach,
            self.origin.z + reach,
        );
        let hulls = brushes.hulls_in(min, max);
        let mut idx = 0;

        while idx < hulls.len() {
            self.add_hull(body, &hulls[idx]);
            idx += 1;
        }

        let voxel_mesh = voxels.mesh_box(self.origin, min, max);
        self.add_voxels(body, &voxel_mesh);
    }

    fn add_hull(&mut self, body: RigidBodyHandle, points: &[Vector3]) {
        if points.len() < 4 {
            return;
        }

        let mut cloud = Vec::with_capacity(points.len());
        let mut idx = 0;

        while idx < points.len() {
            let point = points[idx];

            if point.x.is_finite() && point.y.is_finite() && point.z.is_finite() {
                cloud.push(self.to_sim(point));
            }

            idx += 1;
        }

        let Some(builder) = ColliderBuilder::convex_hull(&cloud) else {
            return;
        };

        self.attach(body, builder);
    }

    fn add_voxels(&mut self, body: RigidBodyHandle, floats: &[f32]) {
        let Some((points, indices)) = voxel_trimesh(floats) else {
            return;
        };

        match ColliderBuilder::trimesh(points, indices) {
            Ok(builder) => {
                self.attach(body, builder);
            }
            Err(err) => {
                log::warn!("[physics] voxel mesh: {err:?}");
            }
        }
    }

    fn sync_players(&mut self, entities: &EntityList, players: &[EntityHandle]) {
        let mut drop_list = Vec::new();

        for (handle, link) in &self.links {
            if link.kinematic && !players.contains(handle) {
                drop_list.push(*handle);
            }
        }

        let mut idx = 0;

        while idx < drop_list.len() {
            self.forget(drop_list[idx]);
            idx += 1;
        }

        idx = 0;

        while idx < players.len() {
            let handle = players[idx];
            idx += 1;

            if !entities.is_valid(handle) {
                continue;
            }

            let Some(entity) = entities.get(handle) else {
                continue;
            };
            let position = entity.base().position;
            self.sync_player(handle, position);
        }
    }

    fn sync_player(&mut self, handle: EntityHandle, position: Vector3) {
        let pose = Pose::from_translation(self.player_sim(position));

        if let Some(link) = self.links.get(&handle).copied() {
            if !link.kinematic {
                return;
            }

            if let Some(body) = self.bodies.get_mut(link.body) {
                body.set_next_kinematic_position(pose);
            }

            return;
        }

        let (hx, hy, hz) = player_half();
        let body = self.bodies.insert(
            RigidBodyBuilder::kinematic_position_based()
                .pose(pose)
                .build(),
        );
        let collider = self.attach(body, ColliderBuilder::cuboid(hx, hy, hz));
        self.links.insert(
            handle,
            Link {
                body,
                collider,
                kinematic: true,
            },
        );
    }

    fn integrate(&mut self, dt: f32, gravity: f32) {
        let mut params = IntegrationParameters::default();
        params.dt = dt;
        let down = if gravity.is_finite() {
            -gravity.max(0.0)
        } else {
            0.0
        };
        self.pipeline.step(
            Vector::new(0.0, 0.0, down),
            &params,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            &mut self.ccd,
            &(),
            &(),
        );
    }

    fn sync_props(&mut self, entities: &mut EntityList) {
        let handles: Vec<EntityHandle> = self.props.keys().copied().collect();
        let mut idx = 0;

        while idx < handles.len() {
            let handle = handles[idx];
            idx += 1;

            if !entities.is_valid(handle) {
                self.forget(handle);

                continue;
            }

            let Some(entity) = entities.get(handle) else {
                continue;
            };
            let position = entity.base().position;
            let angles = entity.base().angles;
            let velocity = entity.base().velocity;
            let active = self
                .props
                .get(&handle)
                .and_then(|prop| prop.body)
                .is_some();
            let dist = dist_sq(self.origin, position);

            if active && dist > SHIFT * SHIFT {
                self.sleep_prop(handle, entities);
            } else if !active && dist <= SHIFT * SHIFT {
                self.wake_prop(handle, position, angles, velocity);
            }
        }
    }

    fn wake_prop(
        &mut self,
        handle: EntityHandle,
        position: Vector3,
        angles: Angle3,
        velocity: Vector3,
    ) {
        let Some(prop) = self.props.get(&handle).copied() else {
            return;
        };

        if prop.body.is_some() {
            return;
        }

        let pose = Pose::from_parts(self.to_sim(position), rotation_of(angles));
        let body = self.bodies.insert(
            RigidBodyBuilder::dynamic()
                .pose(pose)
                .linvel(linear_velocity(velocity))
                .ccd_enabled(true)
                .build(),
        );
        let collider = self.attach(
            body,
            ColliderBuilder::cuboid(prop.half[0], prop.half[1], prop.half[2])
                .translation(Vector::new(prop.center[0], prop.center[1], prop.center[2]))
                .mass(prop.mass),
        );

        if let Some(stored) = self.props.get_mut(&handle) {
            stored.body = Some(body);
            stored.collider = Some(collider);
        }
    }

    fn sleep_prop(&mut self, handle: EntityHandle, entities: &mut EntityList) {
        let Some(prop) = self.props.get(&handle).copied() else {
            return;
        };
        let Some(body_handle) = prop.body else {
            return;
        };

        if let Some(body) = self.bodies.get(body_handle) {
            let position = self.from_sim(body.translation());
            let angles = angles_of(body.rotation());
            let vel = body.linvel();

            if let Some(entity) = entities.get_mut(handle) {
                let base = entity.base_mut();
                base.position = position;
                base.angles = angles;
                base.velocity = Vector3::new(vel.x as f64, vel.y as f64, vel.z as f64);
            }
        }

        self.remove_body(body_handle);

        if let Some(stored) = self.props.get_mut(&handle) {
            stored.body = None;
            stored.collider = None;
        }
    }

    fn write_dynamic(&self, entities: &mut EntityList) {
        for (handle, prop) in &self.props {
            let Some(body_handle) = prop.body else {
                continue;
            };
            let Some(body) = self.bodies.get(body_handle) else {
                continue;
            };
            let translation = body.translation();
            let angles = angles_of(body.rotation());
            let vel = body.linvel();
            let Some(entity) = entities.get_mut(*handle) else {
                continue;
            };
            let position = self.from_sim(translation);
            let base = entity.base_mut();
            base.position = position;
            base.angles = angles;
            base.velocity = Vector3::new(vel.x as f64, vel.y as f64, vel.z as f64);
        }
    }

    fn to_sim(&self, position: Vector3) -> Vector {
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            return Vector::ZERO;
        }

        Vector::new(
            (position.x - self.origin.x) as f32,
            (position.y - self.origin.y) as f32,
            (position.z - self.origin.z) as f32,
        )
    }

    fn from_sim(&self, translation: Vector) -> Vector3 {
        Vector3::new(
            f64::from(translation.x) + self.origin.x,
            f64::from(translation.y) + self.origin.y,
            f64::from(translation.z) + self.origin.z,
        )
    }

    fn player_sim(&self, position: Vector3) -> Vector {
        let offset_z = (STAND_MINS.z + STAND_MAXS.z) * 0.5;

        self.to_sim(Vector3::new(position.x, position.y, position.z + offset_z))
    }

    fn attach(&mut self, body: RigidBodyHandle, builder: ColliderBuilder) -> ColliderHandle {
        self.colliders
            .insert_with_parent(builder.build(), body, &mut self.bodies)
    }

    fn remove_body(&mut self, handle: RigidBodyHandle) {
        self.bodies.remove(
            handle,
            &mut self.islands,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            true,
        );
    }
}

pub(crate) fn fallback_box() -> ([f32; 3], [f32; 3]) {
    ([0.0, 0.0, 0.25], [0.25, 0.25, 0.25])
}

pub(crate) fn box_from_bounds(min: [f32; 3], max: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let mut idx = 0;

    while idx < 3 {
        if !min[idx].is_finite() || !max[idx].is_finite() {
            return fallback_box();
        }

        idx += 1;
    }

    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let half = sanitize_half([
        (max[0] - min[0]) * 0.5,
        (max[1] - min[1]) * 0.5,
        (max[2] - min[2]) * 0.5,
    ]);

    (center, half)
}

fn sanitize_half(half: [f32; 3]) -> [f32; 3] {
    let mut out = half;
    let mut idx = 0;

    while idx < 3 {
        if !out[idx].is_finite() || out[idx] < MIN_HALF {
            out[idx] = MIN_HALF;
        }

        idx += 1;
    }

    out
}

fn box_mass(half: [f32; 3]) -> f32 {
    let volume = (half[0] * 2.0) * (half[1] * 2.0) * (half[2] * 2.0);

    (volume * DENSITY).max(0.1)
}

fn farther(origin: Vector3, point: Vector3, reach: f64) -> bool {
    dist_sq(origin, point) > reach * reach
}

fn dist_sq(origin: Vector3, point: Vector3) -> f64 {
    let dx = point.x - origin.x;
    let dy = point.y - origin.y;
    let dz = point.z - origin.z;

    dx * dx + dy * dy + dz * dz
}

fn linear_velocity(velocity: Vector3) -> Vector {
    if !velocity.x.is_finite() || !velocity.y.is_finite() || !velocity.z.is_finite() {
        return Vector::ZERO;
    }

    Vector::new(velocity.x as f32, velocity.y as f32, velocity.z as f32)
}

fn player_half() -> (f32, f32, f32) {
    (
        ((STAND_MAXS.x - STAND_MINS.x) * 0.5) as f32,
        ((STAND_MAXS.y - STAND_MINS.y) * 0.5) as f32,
        ((STAND_MAXS.z - STAND_MINS.z) * 0.5) as f32,
    )
}

fn rotation_of(angles: Angle3) -> Rotation {
    if !angles.p.is_finite() || !angles.y.is_finite() || !angles.r.is_finite() {
        return Rotation::IDENTITY;
    }

    let quat = quat_from_pose(pose_matrix(
        [0.0, 0.0, 0.0],
        angles.p,
        angles.y,
        angles.r,
    ));

    Rotation::from_xyzw(quat[0], quat[1], quat[2], quat[3])
}

fn angles_of(rotation: &Rotation) -> Angle3 {
    let x_axis = *rotation * Vector::X;
    let y_axis = *rotation * Vector::Y;
    let z_axis = *rotation * Vector::Z;
    let angles = angles_from_pose([
        x_axis.x,
        x_axis.y,
        x_axis.z,
        0.0,
        y_axis.x,
        y_axis.y,
        y_axis.z,
        0.0,
        z_axis.x,
        z_axis.y,
        z_axis.z,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]);

    Angle3::new(angles[0], angles[1], angles[2]).normalize()
}

fn quat_from_pose(mat: [f32; 16]) -> [f32; 4] {
    let m00 = mat[0];
    let m10 = mat[1];
    let m20 = mat[2];
    let m01 = mat[4];
    let m11 = mat[5];
    let m21 = mat[6];
    let m02 = mat[8];
    let m12 = mat[9];
    let m22 = mat[10];
    let trace = m00 + m11 + m22;
    let (x, y, z, w) = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        (
            (m21 - m12) / s,
            (m02 - m20) / s,
            (m10 - m01) / s,
            0.25 * s,
        )
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        (
            0.25 * s,
            (m01 + m10) / s,
            (m02 + m20) / s,
            (m21 - m12) / s,
        )
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        (
            (m01 + m10) / s,
            0.25 * s,
            (m12 + m21) / s,
            (m02 - m20) / s,
        )
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        (
            (m02 + m20) / s,
            (m12 + m21) / s,
            0.25 * s,
            (m10 - m01) / s,
        )
    };
    let len = (x * x + y * y + z * z + w * w).sqrt();

    if len <= 1e-8 {
        return [0.0, 0.0, 0.0, 1.0];
    }

    let inv = 1.0 / len;

    [x * inv, y * inv, z * inv, w * inv]
}

fn voxel_trimesh(floats: &[f32]) -> Option<(Vec<Vector>, Vec<[u32; 3]>)> {
    let verts = floats.len() / VOXEL_STRIDE;
    let tris = verts / 3;

    if tris == 0 {
        return None;
    }

    let mut points = Vec::with_capacity(tris * 3);
    let mut indices = Vec::with_capacity(tris);
    let mut tri = 0u32;

    while (tri as usize) < tris {
        let base = tri as usize * VOXEL_STRIDE * 3;
        points.push(Vector::new(floats[base], floats[base + 1], floats[base + 2]));
        points.push(Vector::new(
            floats[base + VOXEL_STRIDE],
            floats[base + VOXEL_STRIDE + 1],
            floats[base + VOXEL_STRIDE + 2],
        ));
        points.push(Vector::new(
            floats[base + VOXEL_STRIDE * 2],
            floats[base + VOXEL_STRIDE * 2 + 1],
            floats[base + VOXEL_STRIDE * 2 + 2],
        ));
        indices.push([tri * 3, tri * 3 + 1, tri * 3 + 2]);
        tri += 1;
    }

    Some((points, indices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::pose_matrix;
    use crate::entities::{EntityList, Player, ScriptedEntity};

    #[test]
    fn yaw_pose_matches_the_yaw_matrix() {
        let yaw = 35.0f32;
        let pose = pose_matrix([1.0, 2.0, 3.0], 0.0, yaw, 0.0);
        let (sin, cos) = yaw.to_radians().sin_cos();
        let expected = [
            cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0,
        ];
        let mut idx = 0;

        while idx < 16 {
            assert!((pose[idx] - expected[idx]).abs() < 1e-5, "{idx}");
            idx += 1;
        }
    }

    #[test]
    fn orientation_roundtrip_matches_the_draw_matrix() {
        let angles = Angle3::new(20.0, 90.0, -15.0);
        let rotation = rotation_of(angles);
        let x_axis = rotation * Vector::X;
        let y_axis = rotation * Vector::Y;
        let z_axis = rotation * Vector::Z;
        let mat = pose_matrix([0.0, 0.0, 0.0], angles.p, angles.y, angles.r);

        assert!((x_axis.x - mat[0]).abs() < 1e-4);
        assert!((x_axis.y - mat[1]).abs() < 1e-4);
        assert!((x_axis.z - mat[2]).abs() < 1e-4);
        assert!((y_axis.x - mat[4]).abs() < 1e-4);
        assert!((y_axis.y - mat[5]).abs() < 1e-4);
        assert!((y_axis.z - mat[6]).abs() < 1e-4);
        assert!((z_axis.x - mat[8]).abs() < 1e-4);
        assert!((z_axis.y - mat[9]).abs() < 1e-4);
        assert!((z_axis.z - mat[10]).abs() < 1e-4);

        let back = angles_of(&rotation);

        assert!((back.p - angles.p).abs() < 0.05, "{}", back.p);
        assert!((back.y - angles.y).abs() < 0.05, "{}", back.y);
        assert!((back.r - angles.r).abs() < 0.05, "{}", back.r);
    }

    #[test]
    fn box_rests_on_floor() {
        let mut world = PhysicsWorld::new();
        let mut brushes = BrushMap::new();

        assert!(brushes.add_box(
            Vector3::new(-2.0, -2.0, -0.5),
            Vector3::new(2.0, 2.0, 0.0),
            0,
        ));

        let voxels = VoxelWorld::new();
        let mut entities = EntityList::new();
        let handle = entities
            .spawn(Box::new(ScriptedEntity::new(1)))
            .expect("spawn");
        let start = Vector3::new(0.0, 0.0, 2.0);
        world.enable_box(
            handle,
            start,
            Angle3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0),
            [0.0, 0.0, 0.25],
            [0.25, 0.25, 0.25],
        );
        let dt = 1.0 / 60.0;
        let mut idx = 0;

        while idx < 120 {
            world.step(&mut entities, &[], dt, 24.0, &brushes, &voxels);
            idx += 1;
        }

        let entity = entities.get(handle).expect("entity");
        let pos = entity.base().position;
        let vel = entity.base().velocity;

        assert!(pos.z > -0.05 && pos.z < 0.2, "z {}", pos.z);
        assert!(vel.z.abs() < 0.5, "vz {}", vel.z);
    }

    #[test]
    fn box_rests_far_from_the_origin() {
        let mut world = PhysicsWorld::new();
        let mut brushes = BrushMap::new();
        let far = 50_000.0;

        assert!(brushes.add_box(
            Vector3::new(far - 2.0, -2.0, -0.5),
            Vector3::new(far + 2.0, 2.0, 0.0),
            0,
        ));

        let voxels = VoxelWorld::new();
        let mut entities = EntityList::new();
        let player = entities
            .spawn(Box::new(Player::new()))
            .expect("player");
        {
            let entity = entities.get_mut(player).expect("player entity");
            entity.base_mut().position = Vector3::new(far, 0.0, 0.0);
        }
        let handle = entities
            .spawn(Box::new(ScriptedEntity::new(1)))
            .expect("crate");
        {
            let entity = entities.get_mut(handle).expect("crate entity");
            entity.base_mut().position = Vector3::new(far, 1.5, 2.0);
        }
        world.enable_box(
            handle,
            Vector3::new(far, 1.5, 2.0),
            Angle3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0),
            [0.0, 0.0, 0.25],
            [0.25, 0.25, 0.25],
        );
        let dt = 1.0 / 60.0;
        let mut idx = 0;

        while idx < 120 {
            world.step(&mut entities, &[player], dt, 24.0, &brushes, &voxels);
            idx += 1;
        }

        let pos = entities.get(handle).expect("entity").base().position;

        assert!((pos.x - far).abs() < 0.05, "x {}", pos.x);
        assert!((pos.y - 1.5).abs() < 0.05, "y {}", pos.y);
        assert!(pos.z > -0.05 && pos.z < 0.2, "z {}", pos.z);
        assert!((world.origin.x - far).abs() < 1.0, "origin {}", world.origin.x);
    }

    #[test]
    fn box_outside_the_bubble_stays_put() {
        let mut world = PhysicsWorld::new();
        let mut brushes = BrushMap::new();

        assert!(brushes.add_box(
            Vector3::new(-2.0, -2.0, -0.5),
            Vector3::new(2.0, 2.0, 0.0),
            0,
        ));

        let voxels = VoxelWorld::new();
        let mut entities = EntityList::new();
        let player = entities.spawn(Box::new(Player::new())).expect("player");
        let handle = entities
            .spawn(Box::new(ScriptedEntity::new(1)))
            .expect("crate");
        {
            let entity = entities.get_mut(handle).expect("crate entity");
            entity.base_mut().position = Vector3::new(1000.0, 0.0, 5.0);
        }
        world.enable_box(
            handle,
            Vector3::new(1000.0, 0.0, 5.0),
            Angle3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0),
            [0.0, 0.0, 0.25],
            [0.25, 0.25, 0.25],
        );
        let dt = 1.0 / 60.0;
        let mut idx = 0;

        while idx < 30 {
            world.step(&mut entities, &[player], dt, 24.0, &brushes, &voxels);
            idx += 1;
        }

        let pos = entities.get(handle).expect("entity").base().position;

        assert!((pos.x - 1000.0).abs() < 0.01, "x {}", pos.x);
        assert!((pos.z - 5.0).abs() < 0.01, "z {}", pos.z);
    }
}
