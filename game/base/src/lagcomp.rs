use crate::anim::{AnimAssets, BoneXform, NONE_ASSET};
use crate::entities::handle::EntityHandle;
use crate::entities::list::EntityList;
use crate::movement::{lerp_angles, lerp_vec};
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

pub const MAX_REWIND_SECONDS: f64 = 1.0;

pub type LagCompAccess = Arc<AtomicPtr<LagComp>>;

pub struct LagCompScope<'a> {
    access: &'a AtomicPtr<LagComp>,
    previous: *mut LagComp,
}

impl<'a> LagCompScope<'a> {
    pub fn new(access: &'a AtomicPtr<LagComp>, lagcomp: *mut LagComp) -> Self {
        let previous = access.swap(lagcomp, Ordering::Relaxed);

        Self { access, previous }
    }
}

impl Drop for LagCompScope<'_> {
    fn drop(&mut self) {
        self.access.store(self.previous, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug)]
struct Frame {
    tick: u64,
    position: Vector3,
    angles: Angle3,
    bones: Vec<BoneXform>,
}

#[derive(Clone, Copy, Debug)]
struct Command {
    player: EntityHandle,
    tick: u64,
    frac: f32,
}

struct Saved {
    handle: EntityHandle,
    position: Vector3,
    angles: Angle3,
    frozen: bool,
}

pub struct LagComp {
    tracks: HashMap<EntityHandle, VecDeque<Frame>>,
    command: Option<Command>,
    saved: Vec<Saved>,
    active: bool,
    newest: u64,
    max_frames: usize,
}

impl LagComp {
    pub fn new() -> Self {
        Self {
            tracks: HashMap::new(),
            command: None,
            saved: Vec::new(),
            active: false,
            newest: 0,
            max_frames: 2,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn record(&mut self, tick: u64, dt: f64, entities: &EntityList, anims: &mut AnimAssets) {
        if self.active || dt <= 0.0 {
            return;
        }

        self.max_frames = ((MAX_REWIND_SECONDS / dt).ceil() as usize + 1).max(2);
        self.newest = tick;
        self.tracks.retain(|handle, _| entities.is_valid(*handle));

        for (handle, entity) in entities.iter() {
            if !entity.is_spawned() {
                continue;
            }

            let base = entity.base();
            let track = self.tracks.entry(handle).or_default();

            if track.back().is_some_and(|frame| frame.tick >= tick) {
                continue;
            }

            let mut bones = Vec::new();

            if base.anim.mesh == NONE_ASSET
                || base.anim.clips == NONE_ASSET
                || !anims.sample_bones(handle.0, &base.anim, tick as f64, dt, &mut bones)
            {
                bones.clear();
            }

            track.push_back(Frame {
                tick,
                position: base.position,
                angles: base.angles,
                bones,
            });

            while track.len() > self.max_frames {
                track.pop_front();
            }
        }
    }

    pub fn begin_command(&mut self, player: EntityHandle, view_tick: u64, view_frac: f32) {
        self.command = Some(Command {
            player,
            tick: view_tick,
            frac: if view_frac.is_finite() {
                view_frac.clamp(0.0, 1.0)
            } else {
                0.0
            },
        });
    }

    pub fn end_command(&mut self, entities: &mut EntityList, anims: &mut AnimAssets) {
        self.finish(entities, anims);
        self.command = None;
    }

    pub fn start(
        &mut self,
        entities: &mut EntityList,
        anims: &mut AnimAssets,
    ) -> Result<usize, String> {
        let Some(command) = self.command else {
            return Err("lagcomp.start needs a running player command".to_string());
        };

        if self.active {
            return Err("lag compensation is already active".to_string());
        }

        self.active = true;

        if command.tick == 0 || self.newest == 0 {
            return Ok(0);
        }

        let oldest = self.newest.saturating_sub(self.max_frames as u64 - 1) as f64;
        let target = (command.tick as f64 + f64::from(command.frac)).max(oldest);

        if target >= self.newest as f64 {
            return Ok(0);
        }

        for (handle, track) in &self.tracks {
            if *handle == command.player {
                continue;
            }

            let Some(entity) = entities.get_mut(*handle) else {
                continue;
            };

            if entity.base().owner == command.player {
                continue;
            }

            let Some(pose) = pose_at(track, target) else {
                continue;
            };
            let base = entity.base_mut();

            self.saved.push(Saved {
                handle: *handle,
                position: base.position,
                angles: base.angles,
                frozen: !pose.bones.is_empty(),
            });
            base.position = pose.position;
            base.angles = pose.angles;

            if !pose.bones.is_empty() {
                anims.freeze_bones(handle.0, pose.bones);
            }
        }

        Ok(self.saved.len())
    }

    pub fn finish(&mut self, entities: &mut EntityList, anims: &mut AnimAssets) -> usize {
        let saved = std::mem::take(&mut self.saved);
        let count = saved.len();

        for item in saved {
            if let Some(entity) = entities.get_mut(item.handle) {
                let base = entity.base_mut();
                base.position = item.position;
                base.angles = item.angles;
            }

            if item.frozen {
                anims.unfreeze_bones(item.handle.0);
            }
        }

        self.active = false;

        count
    }
}

struct Pose {
    position: Vector3,
    angles: Angle3,
    bones: Vec<BoneXform>,
}

fn pose_at(track: &VecDeque<Frame>, target: f64) -> Option<Pose> {
    let first = track.front()?;
    let last = track.back()?;

    if target < first.tick as f64 - 1.0 {
        return None;
    }

    if target <= first.tick as f64 {
        return Some(pose_of(first));
    }

    if target >= last.tick as f64 {
        return Some(pose_of(last));
    }

    let mut idx = track.len() - 1;

    while idx > 0 && track[idx - 1].tick as f64 > target {
        idx -= 1;
    }

    let from = &track[idx - 1];
    let to = &track[idx];
    let span = (to.tick - from.tick) as f64;
    let alpha = ((target - from.tick as f64) / span).clamp(0.0, 1.0);
    let bones = if from.bones.len() == to.bones.len() {
        from.bones
            .iter()
            .zip(&to.bones)
            .map(|(a, b)| a.lerp(*b, alpha as f32))
            .collect()
    } else if alpha < 0.5 {
        from.bones.clone()
    } else {
        to.bones.clone()
    };

    Some(Pose {
        position: lerp_vec(from.position, to.position, alpha),
        angles: lerp_angles(from.angles, to.angles, alpha as f32),
        bones,
    })
}

fn pose_of(frame: &Frame) -> Pose {
    Pose {
        position: frame.position,
        angles: frame.angles,
        bones: frame.bones.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(tick: u64, x: f64, bone_x: f32) -> Frame {
        Frame {
            tick,
            position: Vector3::new(x, 0.0, 0.0),
            angles: Angle3::new(0.0, 0.0, 0.0),
            bones: vec![BoneXform {
                pos: [bone_x, 0.0, 0.0],
                rot: [0.0, 0.0, 0.0, 1.0],
            }],
        }
    }

    fn track() -> VecDeque<Frame> {
        VecDeque::from([frame(10, 0.0, 0.0), frame(11, 10.0, 1.0), frame(12, 30.0, 3.0)])
    }

    #[test]
    fn interpolates_origin_and_bones_between_frames() {
        let pose = pose_at(&track(), 10.5).unwrap();
        assert!((pose.position.x - 5.0).abs() < 1e-9);
        assert!((pose.bones[0].pos[0] - 0.5).abs() < 1e-6);

        let pose = pose_at(&track(), 11.25).unwrap();
        assert!((pose.position.x - 15.0).abs() < 1e-9);
        assert!((pose.bones[0].pos[0] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn clamps_to_the_ends_and_skips_entities_that_did_not_exist() {
        assert_eq!(pose_at(&track(), 10.0).unwrap().position.x, 0.0);
        assert_eq!(pose_at(&track(), 99.0).unwrap().position.x, 30.0);
        assert_eq!(pose_at(&track(), 9.5).unwrap().position.x, 0.0);
        assert!(pose_at(&track(), 8.0).is_none());
    }

    #[test]
    fn mismatched_bone_counts_snap_to_the_nearer_frame() {
        let mut data = track();
        let extra = data[1].bones[0];
        data[1].bones.push(extra);
        assert_eq!(pose_at(&data, 10.25).unwrap().bones.len(), 1);
        assert_eq!(pose_at(&data, 10.75).unwrap().bones.len(), 2);
    }
}
