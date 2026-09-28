use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use crate::console::{ConVar, ConVarValue};
use crate::entities::EntityHandle;
use crate::r#enum::InputButtons;
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use crate::world::{BrushHit, BrushMap, Face, TraceHit, VoxelWorld};

const STAND_MINS: Vector3 = Vector3::new(-0.28, -0.28, 0.0);
const STAND_MAXS: Vector3 = Vector3::new(0.28, 0.28, 1.65);
const DUCK_MAXS: Vector3 = Vector3::new(0.28, 0.28, 0.9);
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

#[derive(Clone, Copy, Debug)]
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
        }
    }

    pub fn clear(&mut self) {
        self.local = EntityHandle::NULL;
        self.cmds.clear();
        self.ack = 0;
        self.prev_buttons = InputButtons::NONE;
        self.arm_look = false;
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
    }

    pub fn previous(&self) -> InputButtons {
        match self.cmds.back() {
            Some(cmd) => cmd.buttons,
            None => self.prev_buttons,
        }
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
        if ack < self.ack {
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

        for cmd in &self.cmds {
            step(position, velocity, angles, cmd, prev, dt, gravity, brushes, voxels);
            prev = cmd.buttons;
        }
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

pub fn eye_height(buttons: InputButtons) -> f64 {
    if buttons.contains(InputButtons::IN_DUCK) {
        0.78
    } else {
        1.45
    }
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
) {
    if dt <= 0.0 {
        return;
    }

    let mut cmd = *cmd;
    sanitize(&mut cmd);
    *angles = cmd.view;
    repair(velocity);
    *position = unstuck(*position, cmd.buttons, brushes, voxels);

    let (mins, maxs) = hull_for(cmd.buttons, *position, brushes, voxels);
    let mut on_ground = grounded(*position, *velocity, mins, maxs, brushes, voxels);

    if on_ground {
        friction(velocity, dt);
    }

    accelerate(velocity, &cmd, on_ground, dt);

    let jump = cmd.buttons.contains(InputButtons::IN_JUMP) && !prev.contains(InputButtons::IN_JUMP);

    if on_ground && jump {
        velocity.z = jump_impulse(gravity);
        on_ground = false;
    } else if !on_ground {
        velocity.z -= gravity * dt;
    }

    clamp_speed(velocity);

    if on_ground {
        let start = *position;
        let start_vel = *velocity;
        slide(position, velocity, dt, mins, maxs, brushes, voxels);
        try_step(start, start_vel, position, velocity, dt, mins, maxs, brushes, voxels);
    } else {
        slide(position, velocity, dt, mins, maxs, brushes, voxels);
    }

    if velocity.z <= 0.0 {
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

fn hull_for(buttons: InputButtons, position: Vector3, brushes: &BrushMap, voxels: &VoxelWorld) -> (Vector3, Vector3) {
    if buttons.contains(InputButtons::IN_DUCK) || overlapping(position, STAND_MINS, STAND_MAXS, brushes, voxels) {
        return (STAND_MINS, DUCK_MAXS);
    }

    (STAND_MINS, STAND_MAXS)
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

    Vector3::new(start.x + delta.x * scale, start.y + delta.y * scale, start.z + delta.z * scale)
}

fn unstuck(position: Vector3, buttons: InputButtons, brushes: &BrushMap, voxels: &VoxelWorld) -> Vector3 {
    let (mins, maxs) = hull_for(buttons, position, brushes, voxels);

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

fn overlapping(position: Vector3, mins: Vector3, maxs: Vector3, brushes: &BrushMap, voxels: &VoxelWorld) -> bool {
    match sweep(brushes, voxels, position, position, mins, maxs) {
        Some(hit) => hit.stuck,
        None => false,
    }
}

fn grounded(position: Vector3, velocity: Vector3, mins: Vector3, maxs: Vector3, brushes: &BrushMap, voxels: &VoxelWorld) -> bool {
    if velocity.z > 0.5 {
        return false;
    }

    let end = Vector3::new(position.x, position.y, position.z - GROUND_PROBE);

    match sweep(brushes, voxels, position, end, mins, maxs) {
        Some(hit) if !hit.stuck && hit.normal.z >= 0.7 => true,
        _ => false,
    }
}

fn snap_ground(position: &mut Vector3, velocity: &mut Vector3, mins: Vector3, maxs: Vector3, brushes: &BrushMap, voxels: &VoxelWorld) {
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
        let travel = Vector3::new(velocity.x * time_left, velocity.y * time_left, velocity.z * time_left);
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
    slide(&mut stepped_pos, &mut stepped_vel, dt, mins, maxs, brushes, voxels);
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

fn sweep(brushes: &BrushMap, voxels: &VoxelWorld, start: Vector3, end: Vector3, mins: Vector3, maxs: Vector3) -> Option<SweepHit> {
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
        Some(normal) if normal.len_sq() > 1e-8 => SweepHit { distance: hit.distance, normal, stuck: false },
        _ => SweepHit { distance: hit.distance, normal: Vector3::new(0.0, 0.0, 0.0), stuck: true },
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

    SweepHit { distance: hit.distance, normal, stuck }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 0.08
    }

    fn arena() -> (BrushMap, VoxelWorld) {
        let mut brushes = BrushMap::new();
        assert!(brushes.add_box(Vector3::new(-40.0, -40.0, -1.0), Vector3::new(40.0, 40.0, 0.0), 1));

        (brushes, VoxelWorld::new())
    }

    fn run(cmds: &[UserCommand], brushes: &BrushMap, voxels: &VoxelWorld) -> (Vector3, Vector3) {
        let dt = 1.0 / 60.0;
        let mut position = Vector3::new(0.0, 0.0, 0.05);
        let mut velocity = Vector3::new(0.0, 0.0, 0.0);
        let mut angles = Angle3::new(0.0, 0.0, 0.0);
        let mut prev = InputButtons::NONE;

        for cmd in cmds {
            step(&mut position, &mut velocity, &mut angles, cmd, prev, dt, 24.0, brushes, voxels);
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
        assert!(brushes.add_box(Vector3::new(1.0, -2.0, -1.0), Vector3::new(2.0, 2.0, 3.0), 1));
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
            step(&mut position, &mut velocity, &mut angles, &cmd, prev, dt, 24.0, &brushes, &voxels);
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
            step(&mut position, &mut velocity, &mut angles, &cmds[idx], prev, dt, 24.0, &brushes, &voxels);
            predicted.push(cmds[idx]);
            idx += 1;
        }

        assert!(predicted.take_ack(12));
        predicted.replay(&mut position, &mut velocity, &mut angles, dt, 24.0, &brushes, &voxels);

        idx = 12;

        while idx < cmds.len() {
            predicted.push(cmds[idx]);
            idx += 1;
        }

        let mut replay_pos = position;
        let mut replay_vel = velocity;
        let mut replay_ang = angles;
        predicted.replay(&mut replay_pos, &mut replay_vel, &mut replay_ang, dt, 24.0, &brushes, &voxels);

        assert!(near(replay_pos.x, server_pos.x), "x {} {}", replay_pos.x, server_pos.x);
        assert!(near(replay_pos.y, server_pos.y), "y {} {}", replay_pos.y, server_pos.y);
        assert!(near(replay_pos.z, server_pos.z), "z {} {}", replay_pos.z, server_pos.z);
        assert!(near(replay_vel.x, server_vel.x), "vx {} {}", replay_vel.x, server_vel.x);
        assert!(near(replay_vel.z, server_vel.z), "vz {} {}", replay_vel.z, server_vel.z);
    }
}
