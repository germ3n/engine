mod format;
mod gltf;
mod pose;
mod rig;

use crate::anim::format::{read_clips, read_mesh, Mesh};
use crate::anim::pose::{
    elapsed, events_between, lerp3, locals_from_tracks, mul_mat, nlerp, palette, root_delta, sees,
    strip_root, trs, wrap_time, ClipSet, FADE_SECONDS,
};
use crate::movement::RootStep;
use crate::network::events::{AnimSnapshot, BoneOverrideNet, EntityBones};
use crate::ui::skin::{SkinBatch, SkinGroup};
use std::collections::HashMap;
use std::sync::Arc;

pub use format::{write_clips, write_mesh};
pub use pose::{angles_from_pose, pose_matrix, yaw_of};

pub const NONE_ASSET: u32 = u32::MAX;
pub const NONE_SEQ: u16 = u16::MAX;
pub const TEST_MESH: &str = "models/test.mdl";
pub const TEST_CLIPS: &str = "models/test.anm";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoneXform {
    pub pos: [f32; 3],
    pub rot: [f32; 4],
}

impl BoneXform {
    pub fn lerp(self, to: BoneXform, weight: f32) -> BoneXform {
        BoneXform {
            pos: lerp3(self.pos, to.pos, weight),
            rot: nlerp(self.rot, to.rot, weight),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BoneOverride {
    pub pos: Option<[f32; 3]>,
    pub angles: Option<[f32; 3]>,
}

#[derive(Clone, Copy, Debug)]
pub struct AnimPlayback {
    pub mesh: u32,
    pub clips: u32,
    pub sequence: u16,
    pub gesture: u16,
    pub sequence_tick: u64,
    pub gesture_tick: u64,
    pub sequence_rate: f32,
    pub gesture_rate: f32,
    pub gesture_weight: f32,
    pub fade_sequence: u16,
    pub fade_tick: u64,
    pub fade_rate: f32,
    pub fade_begin: u64,
    pub event_tick: u64,
    pub draw_tick: u64,
    pub draw_frac: f32,
}

impl Default for AnimPlayback {
    fn default() -> Self {
        Self {
            mesh: NONE_ASSET,
            clips: NONE_ASSET,
            sequence: NONE_SEQ,
            gesture: NONE_SEQ,
            sequence_tick: 0,
            gesture_tick: 0,
            sequence_rate: 1.0,
            gesture_rate: 1.0,
            gesture_weight: 1.0,
            fade_sequence: NONE_SEQ,
            fade_tick: 0,
            fade_rate: 1.0,
            fade_begin: 0,
            event_tick: 0,
            draw_tick: 0,
            draw_frac: 0.0,
        }
    }
}

impl AnimPlayback {
    pub fn snapshot(&self) -> AnimSnapshot {
        AnimSnapshot {
            sequence: self.sequence,
            gesture: self.gesture,
            sequence_tick: self.sequence_tick,
            gesture_tick: self.gesture_tick,
            sequence_rate: self.sequence_rate,
            gesture_rate: self.gesture_rate,
            gesture_weight: self.gesture_weight,
        }
    }

    pub fn set_sequence(&mut self, sequence: u16, tick: u64, rate: f32) {
        let rate = sanitize_rate(rate);

        if self.sequence == sequence && (self.sequence_rate - rate).abs() < 1e-4 {
            return;
        }

        if self.sequence != NONE_SEQ {
            self.fade_sequence = self.sequence;
            self.fade_tick = self.sequence_tick;
            self.fade_rate = self.sequence_rate;
            self.fade_begin = tick;
        }

        self.sequence = sequence;
        self.sequence_tick = tick;
        self.sequence_rate = rate;
    }

    pub fn set_gesture(&mut self, gesture: u16, tick: u64, rate: f32, weight: f32) {
        let rate = sanitize_rate(rate);
        let weight = if weight.is_finite() {
            weight.clamp(0.0, 1.0)
        } else {
            1.0
        };

        if self.gesture == gesture
            && (self.gesture_rate - rate).abs() < 1e-4
            && (self.gesture_weight - weight).abs() < 1e-4
        {
            return;
        }

        self.gesture = gesture;
        self.gesture_tick = tick;
        self.gesture_rate = rate;
        self.gesture_weight = weight;
    }

    pub fn clear_gesture(&mut self) {
        self.gesture = NONE_SEQ;
        self.gesture_weight = 0.0;
    }

    pub fn reset_sequence(&mut self, tick: u64) {
        self.sequence_tick = tick;
        self.fade_sequence = NONE_SEQ;
    }

    pub fn set_cycle(&mut self, duration: f32, loops: bool, cycle: f32, tick: u64, dt: f64) {
        let cycle = if cycle.is_finite() {
            if loops {
                cycle.rem_euclid(1.0)
            } else {
                cycle.clamp(0.0, 1.0)
            }
        } else {
            0.0
        };
        let duration = duration.max(0.0);
        let time = cycle * duration;
        let rate = sanitize_rate(self.sequence_rate);
        let steps = if rate <= 1e-6 || dt <= 1e-12 {
            0.0
        } else {
            f64::from(time) / (dt * f64::from(rate))
        };
        let start = (tick as f64 - steps).max(0.0).round() as u64;
        self.sequence_tick = start;
        self.fade_sequence = NONE_SEQ;
    }

    pub fn finish_gesture(&mut self, clips: &crate::anim::pose::ClipSet, tick: u64, dt: f64) {
        let Some(sequence) = clips.sequences.get(self.gesture as usize) else {
            return;
        };

        if sequence.loops() {
            return;
        }

        let time = elapsed(tick as f64, self.gesture_tick, self.gesture_rate, dt);

        if time >= sequence.duration {
            self.clear_gesture();
        }
    }

    pub fn apply_remote(&mut self, snap: &AnimSnapshot, tick: u64) {
        let rate = sanitize_rate(snap.sequence_rate);
        let gesture_rate = sanitize_rate(snap.gesture_rate);
        let weight = if snap.gesture_weight.is_finite() {
            snap.gesture_weight.clamp(0.0, 1.0)
        } else {
            1.0
        };

        if self.sequence != snap.sequence {
            if self.sequence != NONE_SEQ {
                self.fade_sequence = self.sequence;
                self.fade_tick = self.sequence_tick;
                self.fade_rate = self.sequence_rate;
                self.fade_begin = tick;
            }

            self.sequence = snap.sequence;
            self.sequence_tick = snap.sequence_tick;
            self.sequence_rate = rate;
        } else {
            self.sequence_tick = snap.sequence_tick;
            self.sequence_rate = rate;
        }

        self.gesture = snap.gesture;
        self.gesture_tick = snap.gesture_tick;
        self.gesture_rate = gesture_rate;
        self.gesture_weight = weight;

        if self.event_tick == 0 {
            self.event_tick = tick;
        }
    }
}

#[derive(Clone, Copy)]
pub struct DrawInput {
    pub entity: u32,
    pub mesh: u32,
    pub clips: u32,
    pub playback: AnimPlayback,
    pub position: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub time: f64,
}

#[derive(Clone, Copy)]
pub struct Cull {
    pub eye: [f32; 3],
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub tan_y: f32,
    pub aspect: f32,
    pub far: f32,
}

pub fn cull_from(
    eye: [f32; 3],
    forward: [f32; 3],
    up: [f32; 3],
    fov_y: f32,
    aspect: f32,
    far: f32,
) -> Cull {
    let right = normalize3(cross(forward, up));
    let up = normalize3(cross(right, forward));

    Cull {
        eye,
        forward: normalize3(forward),
        right,
        up,
        tan_y: (fov_y * 0.5).tan().max(0.05),
        aspect,
        far,
    }
}

const VERTEX_STRIDE: usize = 16;

fn vertex_bounds(vertices: &[f32]) -> Option<([f32; 3], [f32; 3])> {
    let mut min = [f32::MAX, f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN, f32::MIN];
    let mut found = false;
    let mut idx = 0;

    while idx + 2 < vertices.len() {
        let x = vertices[idx];
        let y = vertices[idx + 1];
        let z = vertices[idx + 2];

        if x.is_finite() && y.is_finite() && z.is_finite() {
            found = true;

            if x < min[0] {
                min[0] = x;
            }

            if y < min[1] {
                min[1] = y;
            }

            if z < min[2] {
                min[2] = z;
            }

            if x > max[0] {
                max[0] = x;
            }

            if y > max[1] {
                max[1] = y;
            }

            if z > max[2] {
                max[2] = z;
            }
        }

        idx += VERTEX_STRIDE;
    }

    if !found {
        return None;
    }

    Some((min, max))
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let len = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();

    if len <= 1e-6 {
        return [0.0, 0.0, 1.0];
    }

    [value[0] / len, value[1] / len, value[2] / len]
}

struct StoredMesh {
    mesh: Mesh,
    vertices: Arc<[f32]>,
    indices: Arc<[u32]>,
    albedo: Arc<[u8]>,
    bind_pos: Vec<[f32; 3]>,
    bind_rot: Vec<[f32; 4]>,
    parents: Vec<i16>,
    inverse_bind: Vec<[f32; 16]>,
}

pub struct AnimAssets {
    pub replicate: bool,
    pub dirty: Vec<u32>,
    meshes: Vec<StoredMesh>,
    clips: Vec<ClipSet>,
    mesh_names: HashMap<String, u32>,
    clip_names: HashMap<String, u32>,
    bone_maps: HashMap<(u32, u32), Vec<u16>>,
    bone_overrides: HashMap<u32, Vec<(u16, BoneOverride)>>,
    frozen_bones: HashMap<u32, Vec<BoneXform>>,
    dirty_bones: Vec<u32>,
    scratch_pos: Vec<[f32; 3]>,
    scratch_rot: Vec<[f32; 4]>,
    scratch_pos_b: Vec<[f32; 3]>,
    scratch_rot_b: Vec<[f32; 4]>,
    scratch_pos_c: Vec<[f32; 3]>,
    scratch_rot_c: Vec<[f32; 4]>,
    scratch_world: Vec<[f32; 16]>,
    scratch_palette: Vec<[f32; 12]>,
}

impl AnimAssets {
    pub fn new(replicate: bool) -> Self {
        Self {
            replicate,
            dirty: Vec::new(),
            meshes: Vec::new(),
            clips: Vec::new(),
            mesh_names: HashMap::new(),
            clip_names: HashMap::new(),
            bone_maps: HashMap::new(),
            bone_overrides: HashMap::new(),
            frozen_bones: HashMap::new(),
            dirty_bones: Vec::new(),
            scratch_pos: Vec::new(),
            scratch_rot: Vec::new(),
            scratch_pos_b: Vec::new(),
            scratch_rot_b: Vec::new(),
            scratch_pos_c: Vec::new(),
            scratch_rot_c: Vec::new(),
            scratch_world: Vec::new(),
            scratch_palette: Vec::new(),
        }
    }

    pub fn mesh_bounds(&self, id: u32) -> Option<([f32; 3], [f32; 3])> {
        let stored = self.meshes.get(id as usize)?;

        vertex_bounds(&stored.vertices)
    }

    pub fn mesh_path(&self, id: u32) -> Option<&str> {
        self.mesh_names
            .iter()
            .find(|(_, stored)| **stored == id)
            .map(|(path, _)| path.as_str())
    }

    pub fn model_paths(&self, playback: &AnimPlayback) -> Option<(String, String)> {
        if playback.mesh == NONE_ASSET || playback.clips == NONE_ASSET {
            return None;
        }

        let mesh = self.mesh_path(playback.mesh)?.to_string();
        let clips = self.clip_path(playback.clips)?.to_string();

        Some((mesh, clips))
    }

    pub fn clip_path(&self, id: u32) -> Option<&str> {
        self.clip_names
            .iter()
            .find(|(_, stored)| **stored == id)
            .map(|(path, _)| path.as_str())
    }

    pub fn load_mesh(&mut self, path: &str) -> Result<u32, String> {
        if let Some(id) = self.mesh_names.get(path) {
            return Ok(*id);
        }

        if gltf::is_gltf(path) {
            let loaded = gltf::load_path(path)?;
            let id = self.meshes.len() as u32;
            self.meshes.push(store_mesh(loaded.mesh));
            self.mesh_names.insert(path.to_string(), id);

            return Ok(id);
        }

        let bytes = match crate::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) if path == TEST_MESH => rig::mesh_bytes(),
            Err(err) => return Err(err),
        };
        let mesh = read_mesh(&bytes)?;
        let id = self.meshes.len() as u32;
        self.meshes.push(store_mesh(mesh));
        self.mesh_names.insert(path.to_string(), id);

        Ok(id)
    }

    pub fn load_clips(&mut self, path: &str) -> Result<u32, String> {
        if let Some(id) = self.clip_names.get(path) {
            return Ok(*id);
        }

        if gltf::is_gltf(path) {
            let loaded = gltf::load_path(path)?;
            let id = self.clips.len() as u32;
            self.clips.push(loaded.clips);
            self.clip_names.insert(path.to_string(), id);

            return Ok(id);
        }

        let bytes = match crate::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) if path == TEST_CLIPS => rig::clip_bytes(),
            Err(err) => return Err(err),
        };
        let clips = read_clips(&bytes)?;
        let id = self.clips.len() as u32;
        self.clips.push(clips);
        self.clip_names.insert(path.to_string(), id);

        Ok(id)
    }

    pub fn finish_playback(&self, playback: &mut AnimPlayback, tick: u64, dt: f64) {
        let Some(clips) = self.clips.get(playback.clips as usize) else {
            return;
        };

        playback.finish_gesture(clips, tick, dt);
    }

    pub fn sequence_id(&self, clips: u32, name: &str) -> Option<u16> {
        let clips = self.clips.get(clips as usize)?;
        let mut idx = 0;

        while idx < clips.sequences.len() {
            if clips.sequences[idx].name == name {
                return Some(idx as u16);
            }

            idx += 1;
        }

        None
    }

    pub fn sequence_count(&self, clips: u32) -> u16 {
        self.clips
            .get(clips as usize)
            .map(|clips| clips.sequences.len() as u16)
            .unwrap_or(0)
    }

    pub fn sequence_name(&self, clips: u32, sequence: u16) -> Option<&str> {
        Some(self.sequence(clips, sequence)?.name.as_str())
    }

    pub fn sequence_duration(&self, clips: u32, sequence: u16) -> Option<f32> {
        Some(self.sequence(clips, sequence)?.duration)
    }

    pub fn sequence_loops(&self, clips: u32, sequence: u16) -> Option<bool> {
        Some(self.sequence(clips, sequence)?.loops())
    }

    pub fn sequence_cycle(&self, playback: &AnimPlayback, time: f64, dt: f64) -> f32 {
        let Some(sequence) = self.sequence(playback.clips, playback.sequence) else {
            return 0.0;
        };

        if sequence.duration <= 1e-5 {
            return 0.0;
        }

        let sample = wrap_time(
            elapsed(time, playback.sequence_tick, playback.sequence_rate, dt),
            sequence.duration,
            sequence.loops(),
        );

        (sample / sequence.duration).clamp(0.0, 1.0)
    }

    pub fn bone_count(&self, mesh: u32) -> u16 {
        self.meshes
            .get(mesh as usize)
            .map(|mesh| mesh.parents.len() as u16)
            .unwrap_or(0)
    }

    pub fn bone_name(&self, mesh: u32, bone: u16) -> Option<&str> {
        self.meshes
            .get(mesh as usize)?
            .mesh
            .bones
            .get(bone as usize)
            .map(|bone| bone.name.as_str())
    }

    pub fn bone_parent(&self, mesh: u32, bone: u16) -> Option<i16> {
        self.meshes
            .get(mesh as usize)?
            .parents
            .get(bone as usize)
            .copied()
    }

    pub fn bone_id(&self, mesh: u32, name: &str) -> Option<u16> {
        let bones = &self.meshes.get(mesh as usize)?.mesh.bones;
        let mut idx = 0;

        while idx < bones.len() {
            if bones[idx].name == name {
                return Some(idx as u16);
            }

            idx += 1;
        }

        None
    }

    pub fn manipulate_bone_position(&mut self, entity: u32, bone: u16, pos: [f32; 3]) {
        self.bone_slot(entity, bone).pos = Some(pos);
        self.mark_bones_dirty(entity);
    }

    pub fn manipulate_bone_angles(&mut self, entity: u32, bone: u16, angles: [f32; 3]) {
        self.bone_slot(entity, bone).angles = Some(angles);
        self.mark_bones_dirty(entity);
    }

    pub fn clear_bone_manipulations(&mut self, entity: u32) {
        self.bone_overrides.remove(&entity);
        self.mark_bones_dirty(entity);
    }

    pub fn entity_bones(&self, entity: u32) -> EntityBones {
        EntityBones {
            handle: crate::entities::EntityHandle(entity),
            bones: self.bone_net_list(entity),
        }
    }

    pub fn take_dirty_bones(&mut self) -> Vec<EntityBones> {
        let raw = std::mem::take(&mut self.dirty_bones);
        let mut out: Vec<EntityBones> = Vec::new();
        let mut idx = 0;

        while idx < raw.len() {
            let entity = raw[idx];
            let mut seen = false;
            let mut check = 0;

            while check < out.len() {
                if out[check].handle.0 == entity {
                    seen = true;

                    break;
                }

                check += 1;
            }

            if !seen {
                out.push(self.entity_bones(entity));
            }

            idx += 1;
        }

        out
    }

    pub fn all_entity_bones(&self) -> Vec<EntityBones> {
        let mut out = Vec::new();

        for entity in self.bone_overrides.keys() {
            let bones = self.entity_bones(*entity);

            if !bones.bones.is_empty() {
                out.push(bones);
            }
        }

        out
    }

    pub fn apply_bone_net(&mut self, entity: u32, bones: &[BoneOverrideNet]) {
        self.bone_overrides.remove(&entity);

        if bones.is_empty() {
            return;
        }

        let mut list = Vec::with_capacity(bones.len());
        let mut idx = 0;

        while idx < bones.len() {
            let entry = bones[idx];
            let mut over = BoneOverride::default();

            if entry.has_pos() {
                over.pos = Some(entry.pos);
            }

            if entry.has_angles() {
                over.angles = Some(entry.angles);
            }

            list.push((entry.bone, over));
            idx += 1;
        }

        self.bone_overrides.insert(entity, list);
    }

    fn bone_net_list(&self, entity: u32) -> Vec<BoneOverrideNet> {
        let Some(list) = self.bone_overrides.get(&entity) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(list.len());
        let mut idx = 0;

        while idx < list.len() {
            let (bone, over) = &list[idx];
            let mut flags = 0u8;
            let mut pos = [0.0, 0.0, 0.0];
            let mut angles = [0.0, 0.0, 0.0];

            if let Some(value) = over.pos {
                flags |= BoneOverrideNet::HAS_POS;
                pos = value;
            }

            if let Some(value) = over.angles {
                flags |= BoneOverrideNet::HAS_ANGLES;
                angles = value;
            }

            if flags != 0 {
                out.push(BoneOverrideNet {
                    bone: *bone,
                    flags,
                    pos,
                    angles,
                });
            }

            idx += 1;
        }

        out
    }

    fn mark_bones_dirty(&mut self, entity: u32) {
        if self.replicate {
            self.dirty_bones.push(entity);
        }
    }

    pub fn bone_pose(
        &mut self,
        entity: u32,
        playback: &AnimPlayback,
        bone: u16,
        position: [f32; 3],
        pitch: f32,
        yaw: f32,
        roll: f32,
        time: f64,
        dt: f64,
    ) -> Option<([f32; 3], [f32; 3])> {
        let local = if let Some(frozen) = self.frozen_bones.get(&entity) {
            let xform = frozen.get(bone as usize)?;

            trs(xform.pos, xform.rot)
        } else {
            if !self.sample_palette(
                playback.mesh,
                playback.clips,
                playback,
                time,
                dt,
                entity,
            ) {
                return None;
            }

            *self.scratch_world.get(bone as usize)?
        };
        let world = mul_mat(pose_matrix(position, pitch, yaw, roll), local);
        let pos = [world[12], world[13], world[14]];
        let angles = angles_from_pose(world);

        Some((pos, angles))
    }

    pub fn sample_bones(
        &mut self,
        entity: u32,
        playback: &AnimPlayback,
        time: f64,
        dt: f64,
        out: &mut Vec<BoneXform>,
    ) -> bool {
        out.clear();

        if !self.sample_palette(playback.mesh, playback.clips, playback, time, dt, entity) {
            return false;
        }

        let count = self.scratch_world.len().min(usize::from(u16::MAX));
        let mut idx = 0;

        while idx < count {
            let mat = self.scratch_world[idx];

            out.push(BoneXform {
                pos: [mat[12], mat[13], mat[14]],
                rot: quat_from_mat(mat),
            });
            idx += 1;
        }

        true
    }

    pub fn freeze_bones(&mut self, entity: u32, bones: Vec<BoneXform>) {
        self.frozen_bones.insert(entity, bones);
    }

    pub fn unfreeze_bones(&mut self, entity: u32) {
        self.frozen_bones.remove(&entity);
    }

    fn bone_slot(&mut self, entity: u32, bone: u16) -> &mut BoneOverride {
        let list = self.bone_overrides.entry(entity).or_default();
        let mut idx = 0;

        while idx < list.len() {
            if list[idx].0 == bone {
                return &mut list[idx].1;
            }

            idx += 1;
        }

        list.push((bone, BoneOverride::default()));

        &mut list.last_mut().unwrap().1
    }

    pub fn assign(
        &mut self,
        raw: u32,
        playback: &mut AnimPlayback,
        mesh: &str,
        clips: &str,
    ) -> Result<(), String> {
        let mesh_id = self.load_mesh(mesh)?;
        let clip_path = if clips.is_empty() && gltf::is_gltf(mesh) {
            mesh
        } else {
            clips
        };
        let clip_id = self.load_clips(clip_path)?;
        playback.mesh = mesh_id;
        playback.clips = clip_id;
        playback.sequence = NONE_SEQ;
        playback.gesture = NONE_SEQ;
        playback.fade_sequence = NONE_SEQ;
        self.bone_overrides.remove(&raw);
        self.mark_bones_dirty(raw);
        self.bone_map(mesh_id, clip_id);

        if self.replicate {
            self.dirty.push(raw);
        }

        Ok(())
    }

    pub fn root_motion(
        &self,
        playback: &AnimPlayback,
        yaw: f32,
        tick: u64,
        dt: f64,
    ) -> Option<RootStep> {
        if tick == 0 {
            return None;
        }

        let sequence = self.sequence(playback.clips, playback.sequence)?;

        if !sequence.root_motion() {
            return None;
        }

        let track = sequence.tracks.first()?;
        let from = elapsed(
            (tick - 1) as f64,
            playback.sequence_tick,
            playback.sequence_rate,
            dt,
        );
        let to = elapsed(
            tick as f64,
            playback.sequence_tick,
            playback.sequence_rate,
            dt,
        );
        let from_s = wrap_time(from, sequence.duration, sequence.loops());
        let to_s = wrap_time(to, sequence.duration, sequence.loops());
        let (dx, dy, dyaw) = if sequence.loops() && to_s + 1e-4 < from_s {
            let (dx0, dy0, yaw0) = root_delta(track, from_s, sequence.duration, yaw);
            let (dx1, dy1, yaw1) = root_delta(track, 0.0, to_s, yaw);
            (dx0 + dx1, dy0 + dy1, yaw0 + yaw1)
        } else {
            root_delta(track, from_s, to_s, yaw)
        };

        if dx.abs() < 1e-8 && dy.abs() < 1e-8 && dyaw.abs() < 1e-4 {
            return None;
        }

        Some(RootStep { dx, dy, dyaw })
    }

    pub fn events(
        &self,
        playback: &AnimPlayback,
        from_tick: f64,
        to_tick: f64,
        dt: f64,
    ) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_events(playback, from_tick, to_tick, dt, &mut names);

        names
    }

    pub fn build_batch(&mut self, inputs: &[DrawInput], cull: Option<&Cull>, dt: f64) -> SkinBatch {
        let mut order: Vec<usize> = Vec::new();
        let mut idx = 0;

        while idx < inputs.len() {
            let input = inputs[idx];
            let visible = match cull {
                Some(cull) => sees(
                    cull.eye,
                    cull.forward,
                    cull.right,
                    cull.up,
                    cull.tan_y,
                    cull.aspect,
                    cull.far,
                    input.position,
                    2.0,
                ),
                None => true,
            };

            if input.mesh != NONE_ASSET && visible {
                order.push(idx);
            }

            idx += 1;
        }

        order.sort_by_key(|input| inputs[*input].mesh);
        let mut batch = SkinBatch::default();
        let mut cursor = 0;

        while cursor < order.len() {
            let mesh_id = inputs[order[cursor]].mesh;
            let Some(stored) = self.meshes.get(mesh_id as usize) else {
                cursor += 1;

                continue;
            };
            let bones = stored.parents.len() as u32;
            let mut instances = Vec::new();
            let mut palette_texels = Vec::new();
            let mut count = 0u32;

            while cursor < order.len() && inputs[order[cursor]].mesh == mesh_id {
                let input = inputs[order[cursor]];
                let world = pose_matrix(input.position, input.pitch, input.yaw, input.roll);
                instances.extend_from_slice(&world);

                if self.sample_palette(
                    input.mesh,
                    input.clips,
                    &input.playback,
                    input.time,
                    dt,
                    input.entity,
                ) {
                    let mut bone_idx = 0;

                    while bone_idx < self.scratch_palette.len() {
                        palette_texels.extend_from_slice(&self.scratch_palette[bone_idx]);
                        bone_idx += 1;
                    }
                } else {
                    palette_texels.resize(palette_texels.len() + bones as usize * 12, 0.0);
                }

                count += 1;
                cursor += 1;
            }

            if count == 0 {
                continue;
            }

            let stored = &self.meshes[mesh_id as usize];
            batch.groups.push(SkinGroup {
                key: mesh_id as u64,
                vertices: Arc::clone(&stored.vertices),
                indices: Arc::clone(&stored.indices),
                albedo: Arc::clone(&stored.albedo),
                albedo_w: stored.mesh.albedo_w,
                albedo_h: stored.mesh.albedo_h,
                bones,
                instances: Arc::from(instances),
                palette: Arc::from(palette_texels),
                palette_w: bones * 3,
                palette_h: count,
            });
        }

        batch
    }

    fn collect_events(
        &self,
        playback: &AnimPlayback,
        from_tick: f64,
        to_tick: f64,
        dt: f64,
        out: &mut Vec<String>,
    ) {
        if let Some(sequence) = self.sequence(playback.clips, playback.sequence) {
            let from = elapsed(
                from_tick,
                playback.sequence_tick,
                playback.sequence_rate,
                dt,
            );
            let to = elapsed(to_tick, playback.sequence_tick, playback.sequence_rate, dt);
            events_between(sequence, from, to, out);
        }

        if let Some(sequence) = self.sequence(playback.clips, playback.gesture) {
            let from = elapsed(from_tick, playback.gesture_tick, playback.gesture_rate, dt);
            let to = elapsed(to_tick, playback.gesture_tick, playback.gesture_rate, dt);
            events_between(sequence, from, to, out);
        }
    }

    fn sequence(&self, clips: u32, sequence: u16) -> Option<&pose::Sequence> {
        self.clips
            .get(clips as usize)?
            .sequences
            .get(sequence as usize)
    }

    fn bone_map(&mut self, mesh: u32, clips: u32) -> Option<&[u16]> {
        if self.bone_maps.contains_key(&(mesh, clips)) {
            return self.bone_maps.get(&(mesh, clips)).map(Vec::as_slice);
        }

        let stored = self.meshes.get(mesh as usize)?;
        let clip_set = self.clips.get(clips as usize)?;
        let mut map = Vec::with_capacity(clip_set.bones.len());
        let mut idx = 0;

        while idx < clip_set.bones.len() {
            let mut bone_idx = 0;
            let mut found = u16::MAX;

            while bone_idx < stored.mesh.bones.len() {
                if stored.mesh.bones[bone_idx].name == clip_set.bones[idx] {
                    found = bone_idx as u16;

                    break;
                }

                bone_idx += 1;
            }

            map.push(found);
            idx += 1;
        }

        self.bone_maps.insert((mesh, clips), map);

        self.bone_maps.get(&(mesh, clips)).map(Vec::as_slice)
    }

    fn sample_palette(
        &mut self,
        mesh: u32,
        clips: u32,
        playback: &AnimPlayback,
        time: f64,
        dt: f64,
        entity: u32,
    ) -> bool {
        let bone_count = match self.meshes.get(mesh as usize) {
            Some(stored) => stored.parents.len(),
            None => return false,
        };

        if bone_count == 0 || self.bone_map(mesh, clips).is_none() {
            return false;
        }

        self.scratch_pos.resize(bone_count, [0.0, 0.0, 0.0]);
        self.scratch_rot.resize(bone_count, [0.0, 0.0, 0.0, 1.0]);
        self.scratch_pos_b.resize(bone_count, [0.0, 0.0, 0.0]);
        self.scratch_rot_b.resize(bone_count, [0.0, 0.0, 0.0, 1.0]);
        self.scratch_pos_c.resize(bone_count, [0.0, 0.0, 0.0]);
        self.scratch_rot_c.resize(bone_count, [0.0, 0.0, 0.0, 1.0]);
        self.scratch_world.resize(bone_count, [0.0; 16]);
        self.scratch_palette.resize(bone_count, [0.0; 12]);
        self.write_locals(
            mesh,
            clips,
            playback.sequence,
            playback.sequence_tick,
            playback.sequence_rate,
            time,
            dt,
            true,
        );
        let fade = self.fade_weight(playback, time, dt);

        if fade < 1.0 && playback.fade_sequence != NONE_SEQ {
            std::mem::swap(&mut self.scratch_pos, &mut self.scratch_pos_b);
            std::mem::swap(&mut self.scratch_rot, &mut self.scratch_rot_b);
            self.write_locals(
                mesh,
                clips,
                playback.fade_sequence,
                playback.fade_tick,
                playback.fade_rate,
                time,
                dt,
                true,
            );
            self.scratch_pos_c.clone_from(&self.scratch_pos_b);
            self.scratch_rot_c.clone_from(&self.scratch_rot_b);
            self.blend_from_c(fade);
        }

        if playback.gesture != NONE_SEQ && playback.gesture_weight > 0.0 {
            self.scratch_pos_b.clone_from(&self.scratch_pos);
            self.scratch_rot_b.clone_from(&self.scratch_rot);
            self.write_locals(
                mesh,
                clips,
                playback.gesture,
                playback.gesture_tick,
                playback.gesture_rate,
                time,
                dt,
                false,
            );
            let ramp = gesture_ramp(playback, time, dt) * playback.gesture_weight;
            self.scratch_pos_c.clone_from(&self.scratch_pos);
            self.scratch_rot_c.clone_from(&self.scratch_rot);
            self.scratch_pos.clone_from(&self.scratch_pos_b);
            self.scratch_rot.clone_from(&self.scratch_rot_b);
            self.blend_from_c(ramp);
        }

        self.apply_bone_overrides(entity);
        let mut worlds = std::mem::take(&mut self.scratch_world);
        let mut posed = std::mem::take(&mut self.scratch_palette);
        let pos = std::mem::take(&mut self.scratch_pos);
        let rot = std::mem::take(&mut self.scratch_rot);
        let stored = &self.meshes[mesh as usize];
        palette(
            &stored.parents,
            &pos,
            &rot,
            &stored.inverse_bind,
            &mut worlds,
            &mut posed,
        );
        self.scratch_world = worlds;
        self.scratch_palette = posed;
        self.scratch_pos = pos;
        self.scratch_rot = rot;

        true
    }

    fn apply_bone_overrides(&mut self, entity: u32) {
        let Some(overrides) = self.bone_overrides.get(&entity).cloned() else {
            return;
        };
        let mut idx = 0;

        while idx < overrides.len() {
            let bone = overrides[idx].0 as usize;

            if bone < self.scratch_pos.len() {
                if let Some(pos) = overrides[idx].1.pos {
                    self.scratch_pos[bone] = pos;
                }

                if let Some(angles) = overrides[idx].1.angles {
                    self.scratch_rot[bone] = quat_from_angles(angles[0], angles[1], angles[2]);
                }
            }

            idx += 1;
        }
    }

    fn write_locals(
        &mut self,
        mesh: u32,
        clips: u32,
        sequence: u16,
        start: u64,
        rate: f32,
        time: f64,
        dt: f64,
        strip: bool,
    ) {
        let Some(map) = self.bone_maps.get(&(mesh, clips)).cloned() else {
            return;
        };
        let mut pos = std::mem::take(&mut self.scratch_pos);
        let mut rot = std::mem::take(&mut self.scratch_rot);
        let stored = self.meshes.get(mesh as usize);
        let sequence_ref = self.sequence(clips, sequence);

        if let (Some(stored), Some(sequence_ref)) = (stored, sequence_ref) {
            let sample = wrap_time(
                elapsed(time, start, rate, dt),
                sequence_ref.duration,
                sequence_ref.loops(),
            );
            let strip_root_motion = strip && sequence_ref.root_motion();
            locals_from_tracks(
                &sequence_ref.tracks,
                &map,
                sample,
                &stored.bind_pos,
                &stored.bind_rot,
                &mut pos,
                &mut rot,
            );

            if strip_root_motion {
                if let (Some(bone_pos), Some(bone_rot)) = (pos.first_mut(), rot.first_mut()) {
                    let bind = stored.bind_pos.first().copied().unwrap_or([0.0, 0.0, 0.0]);
                    strip_root(bone_pos, bone_rot, bind);
                }
            }
        } else if let Some(stored) = self.meshes.get(mesh as usize) {
            pos.clone_from(&stored.bind_pos);
            rot.clone_from(&stored.bind_rot);
        }

        self.scratch_pos = pos;
        self.scratch_rot = rot;
    }

    fn blend_from_c(&mut self, weight: f32) {
        let from_pos = std::mem::take(&mut self.scratch_pos);
        let from_rot = std::mem::take(&mut self.scratch_rot);
        let mut out_pos = from_pos.clone();
        let mut out_rot = from_rot.clone();
        pose::blend_locals(
            &from_pos,
            &from_rot,
            &self.scratch_pos_c,
            &self.scratch_rot_c,
            weight,
            &mut out_pos,
            &mut out_rot,
        );
        self.scratch_pos = out_pos;
        self.scratch_rot = out_rot;
    }

    fn fade_weight(&self, playback: &AnimPlayback, time: f64, dt: f64) -> f32 {
        if playback.fade_sequence == NONE_SEQ || FADE_SECONDS <= 0.0 {
            return 1.0;
        }

        let elapsed = ((time - playback.fade_begin as f64).max(0.0) * dt) as f32;

        (elapsed / FADE_SECONDS).clamp(0.0, 1.0)
    }
}

fn gesture_ramp(playback: &AnimPlayback, time: f64, dt: f64) -> f32 {
    let elapsed = ((time - playback.gesture_tick as f64).max(0.0) * dt) as f32;

    (elapsed / 0.1).clamp(0.0, 1.0)
}

fn sanitize_rate(rate: f32) -> f32 {
    if rate.is_finite() && rate >= 0.0 {
        rate.min(8.0)
    } else {
        1.0
    }
}

fn quat_from_angles(pitch_deg: f32, yaw_deg: f32, roll_deg: f32) -> [f32; 4] {
    quat_from_mat(pose_matrix([0.0, 0.0, 0.0], pitch_deg, yaw_deg, roll_deg))
}

fn quat_from_mat(mat: [f32; 16]) -> [f32; 4] {
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

    if trace > 0.0 {
        let s = 2.0 * (trace + 1.0).sqrt();

        return norm_quat([(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]);
    }

    if m00 > m11 && m00 > m22 {
        let s = 2.0 * (1.0 + m00 - m11 - m22).sqrt();

        return norm_quat([0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]);
    }

    if m11 > m22 {
        let s = 2.0 * (1.0 + m11 - m00 - m22).sqrt();

        return norm_quat([(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]);
    }

    let s = 2.0 * (1.0 + m22 - m00 - m11).sqrt();

    norm_quat([(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s])
}

fn norm_quat(quat: [f32; 4]) -> [f32; 4] {
    let len =
        (quat[0] * quat[0] + quat[1] * quat[1] + quat[2] * quat[2] + quat[3] * quat[3]).sqrt();

    if len <= 1e-8 {
        return [0.0, 0.0, 0.0, 1.0];
    }

    let inv = 1.0 / len;

    [quat[0] * inv, quat[1] * inv, quat[2] * inv, quat[3] * inv]
}

fn store_mesh(mesh: Mesh) -> StoredMesh {
    let mut bind_pos = Vec::with_capacity(mesh.bones.len());
    let mut bind_rot = Vec::with_capacity(mesh.bones.len());
    let mut parents = Vec::with_capacity(mesh.bones.len());
    let mut inverse_bind = Vec::with_capacity(mesh.bones.len());

    for bone in &mesh.bones {
        bind_pos.push(bone.local_pos);
        bind_rot.push(bone.local_rot);
        parents.push(bone.parent);
        inverse_bind.push(bone.inverse_bind);
    }

    StoredMesh {
        vertices: Arc::from(mesh.vertices.as_slice()),
        indices: Arc::from(mesh.indices.as_slice()),
        albedo: Arc::from(mesh.albedo.as_slice()),
        bind_pos,
        bind_rot,
        parents,
        inverse_bind,
        mesh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::format::{read_clips, read_mesh};
    use crate::anim::pose::{quat_rotate, quat_z};

    #[test]
    fn yaw_turns_forward_onto_y() {
        let turned = quat_rotate(quat_z(std::f32::consts::FRAC_PI_2), [1.0, 0.0, 0.0]);

        assert!(turned[0].abs() < 1e-4);
        assert!((turned[1] - 1.0).abs() < 1e-4);
        assert!(
            (yaw_of(quat_z(std::f32::consts::FRAC_PI_2)) - std::f32::consts::FRAC_PI_2).abs()
                < 1e-4
        );
    }

    #[test]
    fn rig_roundtrip_samples_root_and_events() {
        let mesh_bytes = write_mesh(&rig::test_mesh()).unwrap();
        let clip_bytes = write_clips(&rig::test_clips()).unwrap();
        let mesh = read_mesh(&mesh_bytes).unwrap();
        let clips = read_clips(&clip_bytes).unwrap();
        assert_eq!(mesh.bones.len(), 12);
        assert_eq!(clips.sequences.len(), 4);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test.mdl"), &mesh_bytes).unwrap();
        std::fs::write(dir.join("test.anm"), &clip_bytes).unwrap();

        let mut assets = AnimAssets::new(false);
        let mesh_id = assets.load_mesh(TEST_MESH).unwrap();
        let clip_id = assets.load_clips(TEST_CLIPS).unwrap();
        let mut playback = AnimPlayback::default();
        playback.mesh = mesh_id;
        playback.clips = clip_id;
        playback.set_sequence(assets.sequence_id(clip_id, "lunge").unwrap(), 1, 1.0);
        let dt = 1.0 / 60.0;
        let mut dx = 0.0;
        let mut dyaw = 0.0;
        let mut tick = 2u64;

        while tick <= 37 {
            if let Some(step) = assets.root_motion(&playback, 0.0, tick, dt) {
                dx += step.dx;
                dyaw += step.dyaw;
            }

            tick += 1;
        }

        assert!(dx > 1.0);
        assert!(dyaw > 20.0);
        let names = assets.events(&playback, 18.0, 19.0, dt);
        assert_eq!(names, vec!["hit".to_string()]);
        let again = assets.events(&playback, 19.0, 20.0, dt);
        assert!(again.is_empty());

        let input = DrawInput {
            entity: 0,
            mesh: mesh_id,
            clips: clip_id,
            playback,
            position: [0.0, 0.0, 0.0],
            pitch: 0.0,
            yaw: 0.0,
            roll: 0.0,
            time: 20.0,
        };
        let cull = Cull {
            eye: [0.0, -4.0, 2.0],
            forward: [0.0, 1.0, 0.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            tan_y: 1.0,
            aspect: 1.0,
            far: 100.0,
        };
        let batch = assets.build_batch(&[input], Some(&cull), dt);
        assert_eq!(batch.groups.len(), 1);
        assert_eq!(batch.groups[0].palette_h, 1);
        assert!(batch.groups[0].indices.len() > 30);
    }
}
