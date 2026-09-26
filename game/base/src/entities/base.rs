use crate::script::libs::vector3::Vector3;
use crate::script::libs::angle3::Angle3;
use crate::entities::handle::EntityHandle;
use crate::entities::context::TickContext;

pub trait Networkable {
    fn handle(&self) -> EntityHandle;
    fn sync_network_vars(&self);
}

pub struct BaseEntityData {
    pub handle: EntityHandle,
    pub position: Vector3,
    pub angles: Angle3,
    pub velocity: Vector3,
}

impl Default for BaseEntityData {
    fn default() -> Self {
        Self {
            handle: EntityHandle::NULL,
            position: Vector3 { x: 0.0, y: 0.0, z: 0.0 },
            angles: Angle3 { p: 0.0, y: 0.0, r: 0.0 },
            velocity: Vector3 { x: 0.0, y: 0.0, z: 0.0 },
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
}