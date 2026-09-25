
use r#macro::Networkable;
use crate::entities::{base::Networkable, base::BaseEntity, base::BaseEntityData};
use crate::entities::EntityHandle;
use crate::entities::list::EntityList;

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
    pub fn new(networked_id: Option<u32>) -> Self {
        let mut player = Self::default();
        player.base.handle = EntityHandle::new(networked_id.unwrap_or(0) as u32, 0);
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

    fn on_spawn(&mut self, _list: &mut EntityList) {

    }

    fn tick(&mut self, _list: &mut EntityList) {

    }
}