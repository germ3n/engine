use crate::entities::context::TickContext;
use crate::entities::EntityHandle;
use crate::entities::{base::BaseEntity, base::BaseEntityData, base::Networkable};
use crate::movement::PlayerBody;
use r#macro::Networkable;

#[Networkable]
pub struct Player {
    pub base: BaseEntityData,
    #[Networked]
    pub health: i32,
    pub body: PlayerBody,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            base: BaseEntityData::default(),
            health: 100,
            body: PlayerBody::default(),
        }
    }
}

impl Player {
    pub const CLASS_HASH: u32 = fnv1a(b"Player");

    pub fn new() -> Self {
        Self::default()
    }
}

const fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 2166136261;
    let mut idx = 0;
    while idx < bytes.len() {
        hash ^= bytes[idx] as u32;
        hash = hash.wrapping_mul(16777619);
        idx += 1;
    }

    hash
}

impl BaseEntity for Player {
    fn base(&self) -> &BaseEntityData {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseEntityData {
        &mut self.base
    }

    fn on_spawn(&mut self, _ctx: &mut TickContext) {}

    fn tick(&mut self, _ctx: &mut TickContext) {}

    fn wants_think(&self) -> bool {
        true
    }

    fn class_hash(&self) -> u32 {
        Self::CLASS_HASH
    }

    fn net_health(&self) -> i32 {
        self.health
    }

    fn player_body(&self) -> Option<&PlayerBody> {
        Some(&self.body)
    }

    fn player_body_mut(&mut self) -> Option<&mut PlayerBody> {
        Some(&mut self.body)
    }
}
