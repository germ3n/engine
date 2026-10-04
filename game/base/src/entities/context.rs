use crate::entities::{DynEntity, EntityHandle, EntityList};

#[derive(Default, Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct FrameInfo {
    pub dt: f64,
    pub cur_time: f64,
    pub tick_count: u64,
}

#[allow(dead_code)]
pub enum EntityCommand {
    Spawn {
        entity: Box<DynEntity>,
    },
    SpawnAt {
        handle: EntityHandle,
        entity: Box<DynEntity>,
    },
    Remove {
        handle: EntityHandle,
    },
    SetThink {
        handle: EntityHandle,
        enabled: bool,
    },
}

#[allow(dead_code)]
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
        Self {
            frame,
            entities,
            commands,
        }
    }

    #[allow(dead_code)]
    pub fn entities(&self) -> &EntityList {
        self.entities
    }

    #[allow(dead_code)]
    pub fn get(&self, handle: EntityHandle) -> Option<&DynEntity> {
        self.entities.get(handle)
    }

    #[allow(dead_code)]
    pub fn is_valid(&self, handle: EntityHandle) -> bool {
        self.entities.is_valid(handle)
    }

    #[allow(dead_code)]
    pub fn spawn(&mut self, entity: Box<DynEntity>) {
        self.commands.push(EntityCommand::Spawn { entity });
    }

    #[allow(dead_code)]
    pub fn spawn_at(&mut self, handle: EntityHandle, entity: Box<DynEntity>) {
        self.commands
            .push(EntityCommand::SpawnAt { handle, entity });
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, handle: EntityHandle) {
        self.commands.push(EntityCommand::Remove { handle });
    }

    #[allow(dead_code)]
    pub fn set_think(&mut self, handle: EntityHandle, enabled: bool) {
        self.commands
            .push(EntityCommand::SetThink { handle, enabled });
    }
}
