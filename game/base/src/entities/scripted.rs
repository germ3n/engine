use crate::entities::context::TickContext;
use crate::entities::EntityHandle;
use crate::entities::{base::BaseEntity, base::BaseEntityData, base::Networkable};

pub struct ScriptedEntity {
    pub base: BaseEntityData,
    pub class_hash: u32,
    pub spawned: bool,
}

impl ScriptedEntity {
    pub fn new(class_hash: u32) -> Self {
        Self {
            base: BaseEntityData::default(),
            class_hash,
            spawned: false,
        }
    }
}

impl Networkable for ScriptedEntity {
    fn handle(&self) -> EntityHandle {
        self.base.handle
    }

    fn sync_network_vars(&self) {}
}

impl BaseEntity for ScriptedEntity {
    fn base(&self) -> &BaseEntityData {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseEntityData {
        &mut self.base
    }

    fn on_spawn(&mut self, _ctx: &mut TickContext) {}

    fn tick(&mut self, _ctx: &mut TickContext) {}

    fn wants_think(&self) -> bool {
        false
    }

    fn class_hash(&self) -> u32 {
        self.class_hash
    }

    fn is_spawned(&self) -> bool {
        self.spawned
    }

    fn set_spawned(&mut self, spawned: bool) {
        self.spawned = spawned;
    }
}
