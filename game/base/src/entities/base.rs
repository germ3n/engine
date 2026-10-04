use crate::anim::AnimPlayback;
use crate::entities::context::TickContext;
use crate::entities::handle::EntityHandle;
use crate::movement::PlayerBody;
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;

pub trait Networkable {
    fn handle(&self) -> EntityHandle;
    fn sync_network_vars(&self);
}

pub struct BaseEntityData {
    pub handle: EntityHandle,
    pub owner: EntityHandle,
    pub position: Vector3,
    pub angles: Angle3,
    pub velocity: Vector3,
    pub anim: AnimPlayback,
}

impl Default for BaseEntityData {
    fn default() -> Self {
        Self {
            handle: EntityHandle::NULL,
            owner: EntityHandle::NULL,
            position: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            angles: Angle3 {
                p: 0.0,
                y: 0.0,
                r: 0.0,
            },
            velocity: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            anim: AnimPlayback::default(),
        }
    }
}

pub type DynEntity = dyn BaseEntity + Send;

pub trait BaseEntity: Networkable {
    fn base(&self) -> &BaseEntityData;
    fn base_mut(&mut self) -> &mut BaseEntityData;
    fn on_spawn(&mut self, ctx: &mut TickContext);
    fn tick(&mut self, ctx: &mut TickContext);
    fn wants_think(&self) -> bool;

    fn class_hash(&self) -> u32 {
        0
    }

    fn net_health(&self) -> i32 {
        0
    }

    fn is_spawned(&self) -> bool {
        true
    }

    fn set_spawned(&mut self, _spawned: bool) {}

    fn player_body(&self) -> Option<&PlayerBody> {
        None
    }

    fn player_body_mut(&mut self) -> Option<&mut PlayerBody> {
        None
    }
}
