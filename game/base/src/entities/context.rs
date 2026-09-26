use crate::entities::{DynEntity, EntityHandle, EntityList};

#[derive(Default, Clone, Copy, Debug)]
pub struct FrameInfo {
    pub dt: f64,
    pub cur_time: f64,
    pub tick_count: u64,
}

pub enum EntityCommand {
    Spawn { entity: Box<DynEntity> },
    SpawnAt { handle: EntityHandle, entity: Box<DynEntity> },
    Remove { handle: EntityHandle },
    SetThink { handle: EntityHandle, enabled: bool },
}

pub struct TickContext<'a> {
    pub frame: FrameInfo,
    entities: &'a EntityList,
    commands: &'a mut Vec<EntityCommand>,
}

impl<'a> TickContext<'a> {
    pub fn new(
        frame: FrameInfo,
        entities: &'a EntityList,
        commands: &'a mut Vec<EntityCommand>,
    ) -> Self {
        Self { frame, entities, commands }
    }

    pub fn entities(&self) -> &EntityList {
        self.entities
    }

    pub fn get(&self, handle: EntityHandle) -> Option<&DynEntity> {
        self.entities.get(handle)
    }

    pub fn is_valid(&self, handle: EntityHandle) -> bool {
        self.entities.is_valid(handle)
    }

    pub fn spawn(&mut self, entity: Box<DynEntity>) {
        self.commands.push(EntityCommand::Spawn { entity });
    }

    pub fn spawn_at(&mut self, handle: EntityHandle, entity: Box<DynEntity>) {
        self.commands.push(EntityCommand::SpawnAt { handle, entity });
    }

    pub fn remove(&mut self, handle: EntityHandle) {
        self.commands.push(EntityCommand::Remove { handle });
    }

    pub fn set_think(&mut self, handle: EntityHandle, enabled: bool) {
        self.commands.push(EntityCommand::SetThink { handle, enabled });
    }
}