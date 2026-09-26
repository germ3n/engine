use r#macro::Networkable;
use crate::entities::{base::Networkable, base::BaseEntity, base::BaseEntityData};
use crate::entities::EntityHandle;
use crate::entities::context::TickContext;

#[Networkable]
pub struct Player {
    pub base: BaseEntityData,
    #[Networked]
    pub health: i32,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            base: BaseEntityData::default(),
            health: 100
        }
    }
}

impl Player {
    pub fn new() -> Self {
        Self::default()
    }
}

impl BaseEntity for Player {
    fn base(&self) -> &BaseEntityData {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseEntityData {
        &mut self.base
    }

    fn on_spawn(&mut self, _ctx: &mut TickContext) {

    }

    fn tick(&mut self, _ctx: &mut TickContext) {

    }

    fn wants_think(&self) -> bool {
        true
    }
}