
use r#macro::Networkable;
use crate::entities::{base::Networkable, base::BaseEntity, base::BaseEntityData};

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
    pub fn new(networked_id: Option<i32>) -> Self {
        let mut player = Self::default();
        player.base.entity_id = networked_id.unwrap_or(0);
        player
    }
}

impl BaseEntity for Player {
    fn base(&self) -> &BaseEntityData {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseEntityData {
        &mut self.base
    }

    fn on_spawn(&mut self) {

    }

    fn tick(&mut self) {

    }
}