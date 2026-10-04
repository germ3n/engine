use crate::console::{ConVar, ConVarValue};
use crate::entities::EntityHandle;
use crate::r#enum::InputButtons;
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use crate::world::{BrushHit, BrushMap, Face, TraceHit, VoxelWorld};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use wincode::{SchemaRead, SchemaWrite};

static NOCLIP_SERVER: AtomicBool = AtomicBool::new(false);
static NOCLIP_CLIENT: AtomicBool = AtomicBool::new(false);

pub(crate) const STAND_MINS: Vector3 = Vector3::new(-0.28, -0.28, 0.0);
pub(crate) const STAND_MAXS: Vector3 = Vector3::new(0.28, 0.28, 1.65);
pub(crate) const DUCK_MAXS: Vector3 = Vector3::new(0.28, 0.28, 0.9);
pub(crate) const VIEW_OFFSET: Vector3 = Vector3::new(0.0, 0.0, 1.45);
pub(crate) const VIEW_OFFSET_DUCKED: Vector3 = Vector3::new(0.0, 0.0, 0.78);

#[derive(Clone, Copy, Debug)]
pub struct PlayerBody {
    pub mins: Vector3,
    pub maxs: Vector3,
    pub duck_mins: Vector3,
    pub duck_maxs: Vector3,
    pub view_offset: Vector3,
    pub view_offset_ducked: Vector3,
}

impl Default for PlayerBody {
    fn default() -> Self {
        Self {
            mins: STAND_MINS,
            maxs: STAND_MAXS,
            duck_mins: STAND_MINS,
            duck_maxs: DUCK_MAXS,
            view_offset: VIEW_OFFSET,
            view_offset_ducked: VIEW_OFFSET_DUCKED,
        }
    }
}
const SKIN: f64 = 0.002;
const STEP_HEIGHT: f64 = 0.45;
const GROUND_PROBE: f64 = 0.12;
const SNAP_DIST: f64 = 0.3;
const WALK_SPEED: f64 = 4.0;
const RUN_SPEED: f64 = 8.0;
const SPRINT_SPEED: f64 = 13.0;
const GROUND_ACCEL: f64 = 12.0;
const AIR_ACCEL: f64 = 3.0;
const FRICTION: f64 = 8.0;
const STOP_SPEED: f64 = 1.5;
const JUMP_HEIGHT: f64 = 1.15;
const MAX_HISTORY: usize = 128;

#[derive(SchemaWrite, SchemaRead, Clone, Copy, Debug)]
pub struct UserCommand {
    pub tick: u64,
    pub buttons: InputButtons,
    pub wish: Vector3,
    pub view: Angle3,
}

pub struct Prediction {
    pub local: EntityHandle,
    cmds: VecDeque<UserCommand>,
    ack: u64,
    prev_buttons: InputButtons,
    pub look: Angle3,
    pub arm_look: bool,
    span_from: Vector3,
    span_to: Vector3,
    span_ready: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct NetPose {
    pub tick: u64,
    pub time: f64,
    pub position: Vector3,
    pub angles: Angle3,
    pub velocity: Vector3,
}

struct SweepHit {
    distance: f64,
    normal: Vector3,
    stuck: bool,
}

impl Prediction {
    pub fn new() -> Self {
        Self {
            local: EntityHandle::NULL,
            cmds: VecDeque::new(),
            ack: 0,
            prev_buttons: InputButtons::NONE,
            look: Angle3::new(0.0, 0.0, 0.0),
            arm_look: false,
            span_from: Vector3::new(0.0, 0.0, 0.0),
            span_to: Vector3::new(0.0, 0.0, 0.0),
            span_ready: false,
        }
    }

    pub fn clear(&mut self) {
        self.local = EntityHandle::NULL;
        self.cmds.clear();
        self.ack = 0;
        self.prev_buttons = InputButtons::NONE;
        self.arm_look = false;
        self.span_ready = false;
    }

    pub fn possess(&mut self, handle: EntityHandle) {
        if handle.is_null() || self.local == handle {
            return;
        }

        self.local = handle;
        self.cmds.clear();
        self.ack = 0;
        self.prev_buttons = InputButtons::NONE;
        self.arm_look = true;
        self.span_ready = false;
    }

    pub fn note_step(&mut self, from: Vector3, to: Vector3) {
        self.span_from = from;
        self.span_to = to;
        self.span_ready = true;
    }

    pub fn scale_span(&mut self, ratio: f64) {
        if !self.span_ready {
            return;
        }

        self.span_from.x *= ratio;
        self.span_from.y *= ratio;
        self.span_from.z *= ratio;
        self.span_to.x *= ratio;
        self.span_to.y *= ratio;
        self.span_to.z *= ratio;
    }

    pub fn snap_view(&mut self, position: Vector3) {
        self.span_from = position;
        self.span_to = position;
        self.span_ready = true;
    }

    pub fn correct_view(&mut self, position: Vector3) {
        if !self.span_ready {
            self.snap_view(position);

            return;
        }

        let dx = position.x - self.span_to.x;
        let dy = position.y - self.span_to.y;
        let dz = position.z - self.span_to.z;

        if dx * dx + dy * dy + dz * dz < 1e-8 {
            return;
        }

        self.span_from.x += dx;
        self.span_from.y += dy;
        self.span_from.z += dz;
        self.span_to = position;
    }

    pub fn view_origin(&self, alpha: f64) -> Option<Vector3> {
        if !self.span_ready {
            return None;
        }

        Some(lerp_vec(
            self.span_from,
            self.span_to,
            alpha.clamp(0.0, 1.0),
        ))
    }

    pub fn previous(&self) -> InputButtons {
        match self.cmds.back() {
            Some(cmd) => cmd.buttons,
            None => self.prev_buttons,
        }
    }

    pub fn pending(&self) -> usize {
        self.cmds.len()
    }

    pub fn acked(&self) -> u64 {
        self.ack
    }

    pub fn push(&mut self, cmd: UserCommand) {
        self.cmds.push_back(cmd);

        while self.cmds.len() > MAX_HISTORY {
            if let Some(cmd) = self.cmds.pop_front() {
                self.prev_buttons = cmd.buttons;
            }
        }
    }

    pub fn take_ack(&mut self, ack: u64) -> bool {
        if ack <= self.ack {
            return false;
        }

        self.ack = ack;

        loop {
            let Some(tick) = self.cmds.front().map(|cmd| cmd.tick) else {
                break;
            };

            if tick > ack {
                break;
            }

            if let Some(cmd) = self.cmds.pop_front() {
                self.prev_buttons = cmd.buttons;
            }
        }

        true
    }

    pub fn replay(
        &self,
        position: &mut Vector3,
        velocity: &mut Vector3,
        angles: &mut Angle3,
        dt: f64,
        gravity: f64,
        brushes: &BrushMap,
        voxels: &VoxelWorld,
    ) {
        let mut prev = self.prev_buttons;

        let body = PlayerBody::default();

        for cmd in &self.cmds {
            step(
                position,
                velocity,
                angles,
                cmd,
                prev,
                dt,
                gravity,
                brushes,
                voxels,
                None,
                &body,
            );
            prev = cmd.buttons;
        }
    }

    pub fn base_buttons(&self) -> InputButtons {
        self.prev_buttons
    }

    pub fn commands(&self) -> Vec<UserCommand> {
        self.cmds.iter().copied().collect()
    }
}

pub fn gravity(cvars: &HashMap<String, Arc<ConVar>>) -> f64 {
    let Some(var) = cvars.get("sv_gravity") else {
        return 24.0;
    };

    match &*var.value.lock().unwrap() {
        ConVarValue::Float(value) => value.max(0.0),
        ConVarValue::Integer(value) => (*value as f64).max(0.0),
        _ => 24.0,
    }
}

pub fn remember_pose(samples: &mut VecDeque<NetPose>, mut pose: NetPose, interval: f64) {
    if let Some(last) = samples.back() {
        if pose.tick <= last.tick {
            return;
        }

        if pose.time <= last.time {
            pose.time = last.time + interval.max(1e-4);
        }
    }

    samples.push_back(pose);

    while samples.len() > 32 {
        samples.pop_front();
    }
}

pub fn forget_old_poses(samples: &mut VecDeque<NetPose>, time: f64) {
    while samples.len() > 2 {
        let Some(next) = samples.get(1) else {
            break;
        };

        if next.time >= time {
            break;
        }

        samples.pop_front();
    }
}

pub fn sample_clock(samples: &VecDeque<NetPose>, time: f64) -> Option<(u64, f32)> {
    let Some(first) = samples.front() else {
        return None;
    };

    if samples.len() == 1 || time <= first.time {
        return Some((first.tick, 0.0));
    }

    let last = samples[samples.len() - 1];

    if time >= last.time {
        return Some((last.tick, 0.0));
    }

    let mut idx = 0;

    while idx + 1 < samples.len() && samples[idx + 1].time < time {
        idx += 1;
    }

    let from = samples[idx];
    let to = samples[idx + 1];
    let span = to.time - from.time;
    let alpha = if span > 1e-8 {
        ((time - from.time) / span).clamp(0.0, 1.0) as f32
    } else {
        1.0
    };
    let steps = to.tick.saturating_sub(from.tick).max(1) as f32;

    Some((from.tick, alpha * steps))
}

pub fn blend_poses(samples: &VecDeque<NetPose>, time: f64, extra_limit: f64) -> Option<NetPose> {
    let Some(first) = samples.front() else {
        return None;
    };

    if samples.len() == 1 || time <= first.time {
        return Some(*first);
    }

    let last_idx = samples.len() - 1;
    let last = samples[last_idx];

    if time >= last.time {
        let extra = (time - last.time).max(0.0).min(extra_limit.max(0.0));
        let mut pose = last;
        pose.position = add(last.position, mul(last.velocity, extra));
        pose.time = time;

        return Some(pose);
    }

    let mut idx = 0;

    while idx + 1 < samples.len() && samples[idx + 1].time < time {
        idx += 1;
    }

    let from = samples[idx];
    let to = samples[idx + 1];
    let span = to.time - from.time;
    let alpha = if span > 1e-8 {
        ((time - from.time) / span).clamp(0.0, 1.0)
    } else {
        1.0
    };

    Some(NetPose {
        tick: to.tick,
        time,
        position: lerp_vec(from.position, to.position, alpha),
        angles: lerp_angles(from.angles, to.angles, alpha as f32),
        velocity: lerp_vec(from.velocity, to.velocity, alpha),
    })
}

pub fn sanitize(cmd: &mut UserCommand) {
    cmd.wish.x = finite_axis(cmd.wish.x);
    cmd.wish.y = finite_axis(cmd.wish.y);
    cmd.wish.z = finite_axis(cmd.wish.z);

    if !cmd.view.p.is_finite() || !cmd.view.y.is_finite() || !cmd.view.r.is_finite() {
        cmd.view = Angle3::new(0.0, 0.0, 0.0);
    }

    cmd.view = cmd.view.normalize();
    cmd.view.p = cmd.view.p.clamp(-89.0, 89.0);
}

#[derive(Clone, Copy, Debug)]
pub struct RootStep {
    pub dx: f64,
    pub dy: f64,
    pub dyaw: f64,
}

pub fn step(
    position: &mut Vector3,
    velocity: &mut Vector3,
    angles: &mut Angle3,
    cmd: &UserCommand,
    prev: InputButtons,
    dt: f64,
    gravity: f64,
    brushes: &BrushMap,
    voxels: &VoxelWorld,
    root: Option<RootStep>,
    body: &PlayerBody,
) {
    if dt <= 0.0 {
        return;
    }

    let mut cmd = *cmd;
    sanitize(&mut cmd);
    let root_yaw = root.map(|step| angles.y + step.dyaw as f32);
    *angles = cmd.view;

    if let Some(yaw) = root_yaw {
        angles.y = yaw;
        *angles = angles.normalize();
    }

    repair(velocity);
    *position = unstuck(*position, cmd.buttons, brushes, voxels, body);

    let (mins, maxs) = hull_for(cmd.buttons, *position, brushes, voxels, body);
    let mut on_ground = grounded(*position, *velocity, mins, maxs, brushes, voxels);

    if root.is_none() && on_ground {
        friction(velocity, dt);
    }

    if root.is_none() {
        accelerate(velocity, &cmd, on_ground, dt);
    }

    let jump = cmd.buttons.contains(InputButtons::IN_JUMP) && !prev.contains(InputButtons::IN_JUMP);

    if on_ground && jump {
        velocity.z = jump_impulse(gravity);
        on_ground = false;
    } else if !on_ground {
        velocity.z -= gravity * dt;
    } else {
        velocity.z = 0.0;
    }

    clamp_speed(velocity);

    if let Some(step) = root {
        velocity.x = step.dx / dt;
        velocity.y = step.dy / dt;
    }

    if on_ground {
        let start = *position;
        let start_vel = *velocity;
        slide(position, velocity, dt, mins, maxs, brushes, voxels);
        try_step(
            start, start_vel, position, velocity, dt, mins, maxs, brushes, voxels,
        );
    } else {
        slide(position, velocity, dt, mins, maxs, brushes, voxels);
    }

    if on_ground || velocity.z <= 0.0 {
        snap_ground(position, velocity, mins, maxs, brushes, voxels);
    }

    repair(position);
    repair(velocity);
}

pub fn root_move(
    position: &mut Vector3,
    velocity: &mut Vector3,
    angles: &mut Angle3,
    step: RootStep,
    dt: f64,
    gravity: f64,
    brushes: &BrushMap,
    voxels: &VoxelWorld,
) {
    if dt <= 0.0 {
        return;
    }

    angles.y += step.dyaw as f32;
    *angles = angles.normalize();
    repair(velocity);
    let body = PlayerBody::default();
    *position = unstuck(*position, InputButtons::NONE, brushes, voxels, &body);
    let mins = body.mins;
    let maxs = body.maxs;
    let on_ground = grounded(*position, *velocity, mins, maxs, brushes, voxels);
    velocity.x = step.dx / dt;
    velocity.y = step.dy / dt;

    if !on_ground {
        velocity.z -= gravity * dt;
    } else {
        velocity.z = 0.0;
    }

    clamp_speed(velocity);
    slide(position, velocity, dt, mins, maxs, brushes, voxels);

    if on_ground || velocity.z <= 0.0 {
        snap_ground(position, velocity, mins, maxs, brushes, voxels);
    }

    repair(position);
    repair(velocity);
}

fn finite_axis(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn repair(value: &mut Vector3) {
    if !value.x.is_finite() {
        value.x = 0.0;
    }

    if !value.y.is_finite() {
        value.y = 0.0;
    }

    if !value.z.is_finite() {
        value.z = 0.0;
    }
}

fn jump_impulse(gravity: f64) -> f64 {
    (2.0 * gravity.max(0.0) * JUMP_HEIGHT).sqrt()
}

fn max_speed(buttons: InputButtons) -> f64 {
    let mut speed = RUN_SPEED;

    if buttons.contains(InputButtons::IN_SPRINT) {
        speed = SPRINT_SPEED;
    }

    if buttons.contains(InputButtons::IN_WALK) {
        speed = WALK_SPEED;
    }

    if buttons.contains(InputButtons::IN_DUCK) {
        speed *= 0.45;
    }

    speed
}

fn hull_for(
    buttons: InputButtons,
    position: Vector3,
    brushes: &BrushMap,
    voxels: &VoxelWorld,
    body: &PlayerBody,
) -> (Vector3, Vector3) {
    if buttons.contains(InputButtons::IN_DUCK)
        || overlapping(position, body.mins, body.maxs, brushes, voxels)
    {
        return (body.duck_mins, body.duck_maxs);
    }

    (body.mins, body.maxs)
}

fn yaw_basis(yaw_deg: f32) -> (Vector3, Vector3) {
    let yaw = (yaw_deg as f64).to_radians();
    let forward = Vector3::new(yaw.cos(), yaw.sin(), 0.0);
    let right = Vector3::new(yaw.sin(), -yaw.cos(), 0.0);

    (forward, right)
}

fn accelerate(velocity: &mut Vector3, cmd: &UserCommand, on_ground: bool, dt: f64) {
    let (forward, right) = yaw_basis(cmd.view.y);
    let mut wish = Vector3::new(
        forward.x * cmd.wish.x + right.x * cmd.wish.y,
        forward.y * cmd.wish.x + right.y * cmd.wish.y,
        0.0,
    );
    let len = wish.len();

    if len < 1e-6 {
        return;
    }

    let inv = 1.0 / len;
    wish = Vector3::new(wish.x * inv, wish.y * inv, 0.0);
    let wish_speed = max_speed(cmd.buttons);
    let current = velocity.x * wish.x + velocity.y * wish.y;
    let add = wish_speed - current;

    if add <= 0.0 {
        return;
    }

    let accel = if on_ground { GROUND_ACCEL } else { AIR_ACCEL };
    let mut gain = accel * wish_speed * dt;

    if gain > add {
        gain = add;
    }

    velocity.x += wish.x * gain;
    velocity.y += wish.y * gain;
}

fn friction(velocity: &mut Vector3, dt: f64) {
    let speed = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();

    if speed < 1e-4 {
        velocity.x = 0.0;
        velocity.y = 0.0;

        return;
    }

    let control = speed.max(STOP_SPEED);
    let drop = control * FRICTION * dt;
    let mut new_speed = speed - drop;

    if new_speed < 0.0 {
        new_speed = 0.0;
    }

    let scale = new_speed / speed;
    velocity.x *= scale;
    velocity.y *= scale;
}

fn clamp_speed(velocity: &mut Vector3) {
    let horizontal = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();

    if horizontal > 30.0 {
        let scale = 30.0 / horizontal;
        velocity.x *= scale;
        velocity.y *= scale;
    }

    velocity.z = velocity.z.clamp(-80.0, 40.0);
}

fn clip_velocity(velocity: Vector3, normal: Vector3) -> Vector3 {
    let backoff = velocity.dot(normal);

    if backoff >= 0.0 {
        return velocity;
    }

    let mut out = Vector3::new(
        velocity.x - normal.x * backoff,
        velocity.y - normal.y * backoff,
        velocity.z - normal.z * backoff,
    );

    if out.x.abs() < 1e-6 {
        out.x = 0.0;
    }

    if out.y.abs() < 1e-6 {
        out.y = 0.0;
    }

    if out.z.abs() < 1e-6 {
        out.z = 0.0;
    }

    out
}

fn lerp_vec(from: Vector3, to: Vector3, alpha: f64) -> Vector3 {
    Vector3::new(
        from.x + (to.x - from.x) * alpha,
        from.y + (to.y - from.y) * alpha,
        from.z + (to.z - from.z) * alpha,
    )
}

fn lerp_angle(from: f32, to: f32, alpha: f32) -> f32 {
    let mut delta = (to - from) % 360.0;

    if delta > 180.0 {
        delta -= 360.0;
    }

    if delta < -180.0 {
        delta += 360.0;
    }

    from + delta * alpha
}

fn lerp_angles(from: Angle3, to: Angle3, alpha: f32) -> Angle3 {
    Angle3::new(
        lerp_angle(from.p, to.p, alpha),
        lerp_angle(from.y, to.y, alpha),
        lerp_angle(from.r, to.r, alpha),
    )
}

fn add(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn mul(a: Vector3, scale: f64) -> Vector3 {
    Vector3::new(a.x * scale, a.y * scale, a.z * scale)
}

fn flat_dist_sq(a: Vector3, b: Vector3) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;

    dx * dx + dy * dy
}

fn move_along(start: Vector3, end: Vector3, distance: f64) -> Vector3 {
    let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
    let span = delta.len();

    if span <= 1e-8 {
        return start;
    }

    let scale = (distance / span).clamp(0.0, 1.0);

    Vector3::new(
        start.x + delta.x * scale,
        start.y + delta.y * scale,
        start.z + delta.z * scale,
    )
}

fn unstuck(
    position: Vector3,
    buttons: InputButtons,
    brushes: &BrushMap,
    voxels: &VoxelWorld,
    body: &PlayerBody,
) -> Vector3 {
    let (mins, maxs) = hull_for(buttons, position, brushes, voxels, body);

    if !overlapping(position, mins, maxs, brushes, voxels) {
        return position;
    }

    let mut lift = 0.05;

    while lift <= 2.0 {
        let raised = Vector3::new(position.x, position.y, position.z + lift);

        if !overlapping(raised, mins, maxs, brushes, voxels) {
            return raised;
        }

        lift += 0.05;
    }

    position
}

fn overlapping(
    position: Vector3,
    mins: Vector3,
    maxs: Vector3,
    brushes: &BrushMap,
    voxels: &VoxelWorld,
) -> bool {
    match sweep(brushes, voxels, position, position, mins, maxs) {
        Some(hit) => hit.stuck,
        None => false,
    }
}

fn grounded(
    position: Vector3,
    velocity: Vector3,
    mins: Vector3,
    maxs: Vector3,
    brushes: &BrushMap,
    voxels: &VoxelWorld,
) -> bool {
    if velocity.z > 0.5 {
        return false;
    }

    let end = Vector3::new(position.x, position.y, position.z - GROUND_PROBE);

    match sweep(brushes, voxels, position, end, mins, maxs) {
        Some(hit) if !hit.stuck && hit.normal.z >= 0.7 => true,
        _ => false,
    }
}

fn snap_ground(
    position: &mut Vector3,
    velocity: &mut Vector3,
    mins: Vector3,
    maxs: Vector3,
    brushes: &BrushMap,
    voxels: &VoxelWorld,
) {
    let end = Vector3::new(position.x, position.y, position.z - SNAP_DIST);
    let Some(hit) = sweep(brushes, voxels, *position, end, mins, maxs) else {
        return;
    };

    if hit.stuck || hit.normal.z < 0.7 || hit.distance > SNAP_DIST {
        return;
    }

    let dist = (hit.distance - SKIN).max(0.0);
    position.z -= dist;

    if velocity.z < 0.0 {
        velocity.z = 0.0;
    }
}

fn slide(
    position: &mut Vector3,
    velocity: &mut Vector3,
    dt: f64,
    mins: Vector3,
    maxs: Vector3,
    brushes: &BrushMap,
    voxels: &VoxelWorld,
) {
    let mut time_left = dt;
    let mut bumps = 0;

    while time_left > 1e-6 && bumps < 4 {
        bumps += 1;
        let travel = Vector3::new(
            velocity.x * time_left,
            velocity.y * time_left,
            velocity.z * time_left,
        );
        let end = add(*position, travel);
        let Some(hit) = sweep(brushes, voxels, *position, end, mins, maxs) else {
            *position = end;

            break;
        };

        if hit.stuck {
            *velocity = Vector3::new(0.0, 0.0, 0.0);

            break;
        }

        let span = travel.len();

        if span <= 1e-8 {
            break;
        }

        let used = (hit.distance / span).clamp(0.0, 1.0);
        let moved = (hit.distance - SKIN).max(0.0).min(span);
        *position = add(*position, mul(travel, moved / span));

        if hit.normal.z >= 0.7 && velocity.z < 0.0 {
            velocity.z = 0.0;
        }

        *velocity = clip_velocity(*velocity, hit.normal);

        if used >= 1.0 - 1e-4 {
            break;
        }

        time_left *= 1.0 - used;
    }
}

fn try_step(
    start: Vector3,
    start_vel: Vector3,
    position: &mut Vector3,
    velocity: &mut Vector3,
    dt: f64,
    mins: Vector3,
    maxs: Vector3,
    brushes: &BrushMap,
    voxels: &VoxelWorld,
) {
    let speed_sq = start_vel.x * start_vel.x + start_vel.y * start_vel.y;

    if speed_sq < 1e-4 {
        return;
    }

    let wish = speed_sq * dt * dt;
    let direct = flat_dist_sq(*position, start);

    if direct + 1e-4 >= wish {
        return;
    }

    let up = Vector3::new(start.x, start.y, start.z + STEP_HEIGHT);
    let raised = match sweep(brushes, voxels, start, up, mins, maxs) {
        Some(hit) if hit.stuck || hit.distance <= SKIN => {
            return;
        }
        Some(hit) => move_along(start, up, (hit.distance - SKIN).max(0.0)),
        None => up,
    };
    let mut stepped_pos = raised;
    let mut stepped_vel = start_vel;
    slide(
        &mut stepped_pos,
        &mut stepped_vel,
        dt,
        mins,
        maxs,
        brushes,
        voxels,
    );
    let down = Vector3::new(stepped_pos.x, stepped_pos.y, stepped_pos.z - STEP_HEIGHT);
    let Some(land) = sweep(brushes, voxels, stepped_pos, down, mins, maxs) else {
        return;
    };

    if land.stuck || land.normal.z < 0.7 {
        return;
    }

    stepped_pos = move_along(stepped_pos, down, (land.distance - SKIN).max(0.0));

    if flat_dist_sq(stepped_pos, start) <= direct + 1e-6 {
        return;
    }

    *position = stepped_pos;
    *velocity = stepped_vel;
    velocity.z = 0.0;
}

fn sweep(
    brushes: &BrushMap,
    voxels: &VoxelWorld,
    start: Vector3,
    end: Vector3,
    mins: Vector3,
    maxs: Vector3,
) -> Option<SweepHit> {
    let brush = brushes.sweep(start, end, mins, maxs).map(sweep_from_brush);
    let voxel = voxels.sweep(start, end, mins, maxs).map(sweep_from_voxel);

    match (brush, voxel) {
        (Some(left), Some(right)) => {
            if left.distance <= right.distance {
                Some(left)
            } else {
                Some(right)
            }
        }
        (Some(hit), None) | (None, Some(hit)) => Some(hit),
        (None, None) => None,
    }
}

fn sweep_from_brush(hit: BrushHit) -> SweepHit {
    match hit.normal {
        Some(normal) if normal.len_sq() > 1e-8 => SweepHit {
            distance: hit.distance,
            normal,
            stuck: false,
        },
        _ => SweepHit {
            distance: hit.distance,
            normal: Vector3::new(0.0, 0.0, 0.0),
            stuck: true,
        },
    }
}

fn sweep_from_voxel(hit: TraceHit) -> SweepHit {
    let normal = match hit.face {
        Some(Face::NegX) => Vector3::new(-1.0, 0.0, 0.0),
        Some(Face::PosX) => Vector3::new(1.0, 0.0, 0.0),
        Some(Face::NegY) => Vector3::new(0.0, -1.0, 0.0),
        Some(Face::PosY) => Vector3::new(0.0, 1.0, 0.0),
        Some(Face::NegZ) => Vector3::new(0.0, 0.0, -1.0),
        Some(Face::PosZ) => Vector3::new(0.0, 0.0, 1.0),
        None => Vector3::new(0.0, 0.0, 0.0),
    };
    let stuck = normal.len_sq() < 1e-8;

    SweepHit {
        distance: hit.distance,
        normal,
        stuck,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 0.08
    }

    fn arena() -> (BrushMap, VoxelWorld) {
        let mut brushes = BrushMap::new();
        assert!(brushes.add_box(
            Vector3::new(-40.0, -40.0, -1.0),
            Vector3::new(40.0, 40.0, 0.0),
            1
        ));

        (brushes, VoxelWorld::new())
    }

    fn run(cmds: &[UserCommand], brushes: &BrushMap, voxels: &VoxelWorld) -> (Vector3, Vector3) {
        let dt = 1.0 / 60.0;
        let mut position = Vector3::new(0.0, 0.0, 0.05);
        let mut velocity = Vector3::new(0.0, 0.0, 0.0);
        let mut angles = Angle3::new(0.0, 0.0, 0.0);
        let mut prev = InputButtons::NONE;

        let body = PlayerBody::default();

        for cmd in cmds {
            step(
                &mut position,
                &mut velocity,
                &mut angles,
                cmd,
                prev,
                dt,
                24.0,
                brushes,
                voxels,
                None,
                &body,
            );
            prev = cmd.buttons;
        }

        (position, velocity)
    }

    fn command(tick: u64, buttons: InputButtons, wish_x: f64) -> UserCommand {
        UserCommand {
            tick,
            buttons,
            wish: Vector3::new(wish_x, 0.0, 0.0),
            view: Angle3::new(0.0, 0.0, 0.0),
        }
    }

    #[test]
    fn gravity_lands_on_the_floor() {
        let (brushes, voxels) = arena();
        let mut cmds = Vec::new();
        let mut tick = 1u64;

        while tick <= 90 {
            cmds.push(command(tick, InputButtons::NONE, 0.0));
            tick += 1;
        }

        let (position, velocity) = run(&cmds, &brushes, &voxels);

        assert!(position.z > -0.02, "z {}", position.z);
        assert!(position.z < 0.08, "z {}", position.z);
        assert!(velocity.z.abs() < 0.5, "vz {}", velocity.z);
    }

    #[test]
    fn walk_stops_at_a_wall() {
        let (mut brushes, voxels) = arena();
        assert!(brushes.add_box(
            Vector3::new(1.0, -2.0, -1.0),
            Vector3::new(2.0, 2.0, 3.0),
            1
        ));
        let mut cmds = Vec::new();
        let mut tick = 1u64;

        while tick <= 90 {
            cmds.push(command(tick, InputButtons::NONE, 1.0));
            tick += 1;
        }

        let (position, velocity) = run(&cmds, &brushes, &voxels);

        assert!(position.x > 0.4, "x {}", position.x);
        assert!(position.x < 0.8, "x {}", position.x);
        assert!(velocity.x.abs() < 1.0, "vx {}", velocity.x);
    }

    #[test]
    fn held_jump_is_a_single_hop() {
        let (brushes, voxels) = arena();
        let dt = 1.0 / 60.0;
        let mut position = Vector3::new(0.0, 0.0, 0.05);
        let mut velocity = Vector3::new(0.0, 0.0, 0.0);
        let mut angles = Angle3::new(0.0, 0.0, 0.0);
        let mut prev = InputButtons::NONE;
        let mut peak = position.z;
        let mut tick = 1u64;

        while tick <= 40 {
            let cmd = command(tick, InputButtons::IN_JUMP, 0.0);
            step(
                &mut position,
                &mut velocity,
                &mut angles,
                &cmd,
                prev,
                dt,
                24.0,
                &brushes,
                &voxels,
                None,
                &PlayerBody::default(),
            );
            prev = cmd.buttons;

            if position.z > peak {
                peak = position.z;
            }

            tick += 1;
        }

        assert!(near(peak, 1.2), "peak {peak}");
        assert!(position.z < 0.2, "landed {}", position.z);
    }

    #[test]
    fn replay_matches_the_full_simulation() {
        let (brushes, voxels) = arena();
        let mut cmds = Vec::new();
        let mut tick = 1u64;

        while tick <= 40 {
            let buttons = if tick == 8 || (tick >= 21 && tick <= 27) {
                InputButtons::IN_JUMP
            } else {
                InputButtons::NONE
            };
            let wish_x = if tick < 30 { 1.0 } else { 0.0 };
            cmds.push(command(tick, buttons, wish_x));
            tick += 1;
        }

        let (server_pos, server_vel) = run(&cmds, &brushes, &voxels);
        let dt = 1.0 / 60.0;
        let mut predicted = Prediction::new();
        let mut position = Vector3::new(0.0, 0.0, 0.05);
        let mut velocity = Vector3::new(0.0, 0.0, 0.0);
        let mut angles = Angle3::new(0.0, 0.0, 0.0);
        let mut idx = 0;

        while idx < 12 {
            let prev = predicted.previous();
            step(
                &mut position,
                &mut velocity,
                &mut angles,
                &cmds[idx],
                prev,
                dt,
                24.0,
                &brushes,
                &voxels,
                None,
                &PlayerBody::default(),
            );
            predicted.push(cmds[idx]);
            idx += 1;
        }

        assert!(predicted.take_ack(12));
        predicted.replay(
            &mut position,
            &mut velocity,
            &mut angles,
            dt,
            24.0,
            &brushes,
            &voxels,
        );

        idx = 12;

        while idx < cmds.len() {
            predicted.push(cmds[idx]);
            idx += 1;
        }

        let mut replay_pos = position;
        let mut replay_vel = velocity;
        let mut replay_ang = angles;
        predicted.replay(
            &mut replay_pos,
            &mut replay_vel,
            &mut replay_ang,
            dt,
            24.0,
            &brushes,
            &voxels,
        );

        assert!(
            near(replay_pos.x, server_pos.x),
            "x {} {}",
            replay_pos.x,
            server_pos.x
        );
        assert!(
            near(replay_pos.y, server_pos.y),
            "y {} {}",
            replay_pos.y,
            server_pos.y
        );
        assert!(
            near(replay_pos.z, server_pos.z),
            "z {} {}",
            replay_pos.z,
            server_pos.z
        );
        assert!(
            near(replay_vel.x, server_vel.x),
            "vx {} {}",
            replay_vel.x,
            server_vel.x
        );
        assert!(
            near(replay_vel.z, server_vel.z),
            "vz {} {}",
            replay_vel.z,
            server_vel.z
        );
    }

    #[test]
    fn view_origin_blends_the_last_predicted_step() {
        let mut prediction = Prediction::new();
        prediction.note_step(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        let mid = prediction.view_origin(0.5).unwrap();

        assert!(near(mid.x, 5.0));
        assert!(prediction.view_origin(-1.0).unwrap().x.abs() < 1e-6);
        assert!(near(prediction.view_origin(2.0).unwrap().x, 10.0));
    }

    #[test]
    fn take_ack_ignores_duplicates_and_correct_view_keeps_span() {
        let mut prediction = Prediction::new();
        prediction.push(UserCommand {
            tick: 1,
            buttons: InputButtons::NONE,
            wish: Vector3::new(0.0, 0.0, 0.0),
            view: Angle3::new(0.0, 0.0, 0.0),
        });
        prediction.note_step(Vector3::new(0.0, 0.0, 0.0), Vector3::new(4.0, 0.0, 0.0));

        assert!(prediction.take_ack(1));
        assert!(!prediction.take_ack(1));

        prediction.correct_view(Vector3::new(4.0, 0.0, 0.0));
        let mid = prediction.view_origin(0.5).unwrap();
        assert!(near(mid.x, 2.0));

        prediction.correct_view(Vector3::new(6.0, 0.0, 0.0));
        let mid = prediction.view_origin(0.5).unwrap();
        assert!(near(mid.x, 3.0));
    }

    #[test]
    fn remote_poses_blend_across_the_gap_and_wrap_yaw() {
        let mut samples = VecDeque::new();
        remember_pose(
            &mut samples,
            NetPose {
                tick: 1,
                time: 1.0,
                position: Vector3::new(0.0, 0.0, 0.0),
                angles: Angle3::new(0.0, 350.0, 0.0),
                velocity: Vector3::new(0.0, 0.0, 0.0),
            },
            0.1,
        );
        remember_pose(
            &mut samples,
            NetPose {
                tick: 2,
                time: 1.0,
                position: Vector3::new(10.0, 0.0, 0.0),
                angles: Angle3::new(0.0, 10.0, 0.0),
                velocity: Vector3::new(4.0, 0.0, 0.0),
            },
            0.1,
        );

        assert!((samples[1].time - 1.1).abs() < 1e-6);
        let mid = blend_poses(&samples, 1.05, 0.1).unwrap();
        let yaw = (mid.angles.y + 180.0).rem_euclid(360.0) - 180.0;

        assert!(near(mid.position.x, 5.0));
        assert!(yaw.abs() < 0.01, "yaw {}", mid.angles.y);

        let ahead = blend_poses(&samples, 1.4, 0.05).unwrap();

        assert!(near(ahead.position.x, 10.2));
        assert!(blend_poses(&samples, 0.0, 0.1).unwrap().position.x.abs() < 1e-6);
    }
}
