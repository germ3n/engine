use std::collections::VecDeque;
use crate::entities::{DynEntity, EntityHandle, EntityCommand, TickContext, FrameInfo};

const MIN_FREE_SLOTS: usize = 64;
const MAX_COMMAND_PASSES: usize = 8;
const NOT_THINKING: u32 = u32::MAX;

pub const DEFAULT_MAX_ENTITIES: usize = 102400;

struct Slot {
    generation: u16,
    think_idx: u32,
    entity: Option<Box<DynEntity>>,
}

pub struct EntityList {
    slots: Vec<Slot>,
    free: VecDeque<u32>,
    think_list: Vec<EntityHandle>,
    commands: Vec<EntityCommand>,
    frame: FrameInfo,
    max_entities: usize,
    count: usize,
}

impl Default for EntityList {
    fn default() -> Self {
        Self::with_max_entities(DEFAULT_MAX_ENTITIES)
    }
}

impl EntityList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_entities(max_entities: usize) -> Self {
        Self {
            slots: Vec::new(),
            free: VecDeque::new(),
            think_list: Vec::new(),
            commands: Vec::new(),
            frame: FrameInfo::default(),
            max_entities: max_entities.clamp(1, EntityHandle::MAX_ENTITIES),
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn think_count(&self) -> usize {
        self.think_list.len()
    }

    pub fn max_entities(&self) -> usize {
        self.max_entities
    }

    pub fn frame(&self) -> FrameInfo {
        self.frame
    }

    pub fn set_frame(&mut self, frame: FrameInfo) {
        self.frame = frame;
    }

    pub fn queue(&mut self, command: EntityCommand) {
        self.commands.push(command);
    }

    pub fn is_valid(&self, handle: EntityHandle) -> bool {
        if handle.is_null() {
            return false;
        }

        match self.slots.get(handle.index() as usize) {
            Some(slot) => slot.generation == handle.generation() && slot.entity.is_some(),
            None => false,
        }
    }

    pub fn get(&self, handle: EntityHandle) -> Option<&DynEntity> {
        let slot = self.slots.get(handle.index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }

        slot.entity.as_deref()
    }

    pub fn get_mut(&mut self, handle: EntityHandle) -> Option<&mut DynEntity> {
        let slot = self.slots.get_mut(handle.index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }

        slot.entity.as_deref_mut()
    }

    fn allocate_index(&mut self) -> Option<u32> {
        let at_capacity = self.slots.len() >= self.max_entities;

        if at_capacity || self.free.len() > MIN_FREE_SLOTS {
            while let Some(idx) = self.free.pop_front() {
                if self.slots[idx as usize].entity.is_none() {
                    return Some(idx);
                }
            }
        }

        if at_capacity {
            return None;
        }

        let idx = self.slots.len() as u32;
        self.slots.push(Slot { generation: 0, think_idx: NOT_THINKING, entity: None });

        Some(idx)
    }

    pub fn spawn(&mut self, mut entity: Box<DynEntity>) -> Option<EntityHandle> {
        let idx = self.allocate_index()?;
        let wants_think = entity.wants_think();
        let generation = EntityHandle::next_generation(self.slots[idx as usize].generation);
        let handle = EntityHandle::new(idx, generation);
        entity.base_mut().handle = handle;

        let slot = &mut self.slots[idx as usize];
        slot.generation = generation;
        slot.entity = Some(entity);
        self.count += 1;

        if wants_think {
            self.add_think(handle);
        }

        self.dispatch_on_spawn(handle);

        Some(handle)
    }

    pub fn insert_at(&mut self, handle: EntityHandle, mut entity: Box<DynEntity>) -> bool {
        if handle.is_null() {
            return false;
        }

        let idx = handle.index() as usize;
        if idx >= self.max_entities {
            return false;
        }

        if idx >= self.slots.len() {
            self.slots.resize_with(idx + 1, || Slot { generation: 0, think_idx: NOT_THINKING, entity: None });
        }

        if self.slots[idx].entity.is_some() {
            return false;
        }

        let wants_think = entity.wants_think();
        entity.base_mut().handle = handle;

        let slot = &mut self.slots[idx];
        slot.generation = handle.generation();
        slot.entity = Some(entity);
        self.count += 1;

        if wants_think {
            self.add_think(handle);
        }

        self.dispatch_on_spawn(handle);

        true
    }

    pub fn remove(&mut self, handle: EntityHandle) -> bool {
        if handle.is_null() {
            return false;
        }

        let idx = handle.index() as usize;
        match self.slots.get(idx) {
            Some(slot) if slot.generation == handle.generation() => {}
            _ => return false,
        }

        self.remove_think(handle);

        let slot = &mut self.slots[idx];
        slot.entity = None;
        slot.generation = EntityHandle::next_generation(slot.generation);
        self.free.push_back(handle.index());
        self.count -= 1;

        true
    }

    fn add_think(&mut self, handle: EntityHandle) {
        let idx = handle.index() as usize;
        let think_idx = self.think_list.len() as u32;

        match self.slots.get_mut(idx) {
            Some(slot) if slot.think_idx == NOT_THINKING => slot.think_idx = think_idx,
            _ => return,
        }

        self.think_list.push(handle);
    }

    fn remove_think(&mut self, handle: EntityHandle) {
        let idx = handle.index() as usize;
        let think_idx = match self.slots.get_mut(idx) {
            Some(slot) if slot.think_idx != NOT_THINKING => {
                let found = slot.think_idx;
                slot.think_idx = NOT_THINKING;

                found
            }
            _ => return,
        };

        if think_idx as usize >= self.think_list.len() {
            return;
        }

        let last = self.think_list.len() - 1;
        self.think_list.swap_remove(think_idx as usize);

        if (think_idx as usize) < last {
            let moved = self.think_list[think_idx as usize];
            if let Some(slot) = self.slots.get_mut(moved.index() as usize) {
                slot.think_idx = think_idx;
            }
        }
    }

    fn take(&mut self, handle: EntityHandle) -> Option<Box<DynEntity>> {
        let slot = self.slots.get_mut(handle.index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }

        slot.entity.take()
    }

    fn put_back(&mut self, handle: EntityHandle, entity: Box<DynEntity>) {
        if let Some(slot) = self.slots.get_mut(handle.index() as usize) {
            if slot.generation == handle.generation() && slot.entity.is_none() {
                slot.entity = Some(entity);
            }
        }
    }

    fn dispatch_on_spawn(&mut self, handle: EntityHandle) {
        let mut commands = std::mem::take(&mut self.commands);
        let frame = self.frame;

        if let Some(mut entity) = self.take(handle) {
            {
                let mut ctx = TickContext::new(frame, self, &mut commands);
                entity.on_spawn(&mut ctx);
            }

            self.put_back(handle, entity);
        }

        self.commands = commands;
    }

    pub fn tick_all(&mut self) {
        let mut commands = std::mem::take(&mut self.commands);
        let frame = self.frame;
        let count = self.think_list.len();

        for think_idx in 0..count {
            let handle = self.think_list[think_idx];
            let Some(mut entity) = self.take(handle) else { continue; };

            {
                let mut ctx = TickContext::new(frame, self, &mut commands);
                entity.tick(&mut ctx);
            }

            self.put_back(handle, entity);
        }

        self.commands = commands;
        self.apply_commands();
    }

    pub fn apply_commands(&mut self) {
        for _ in 0..MAX_COMMAND_PASSES {
            if self.commands.is_empty() {
                return;
            }

            let batch = std::mem::take(&mut self.commands);

            for command in batch {
                match command {
                    EntityCommand::Spawn { entity } => {
                        self.spawn(entity);
                    }
                    EntityCommand::SpawnAt { handle, entity } => {
                        self.insert_at(handle, entity);
                    }
                    EntityCommand::Remove { handle } => {
                        self.remove(handle);
                    }
                    EntityCommand::SetThink { handle, enabled } => {
                        if enabled {
                            self.add_think(handle);
                        } else {
                            self.remove_think(handle);
                        }
                    }
                }
            }
        }
    }

    pub fn handles(&self) -> Vec<EntityHandle> {
        let mut out = Vec::with_capacity(self.count);

        for (idx, slot) in self.slots.iter().enumerate() {
            if slot.entity.is_some() {
                out.push(EntityHandle::new(idx as u32, slot.generation));
            }
        }

        out
    }

    pub fn iter(&self) -> impl Iterator<Item = (EntityHandle, &DynEntity)> {
        self.slots.iter().enumerate().filter_map(|(idx, slot)| {
            let entity = slot.entity.as_deref()?;

            Some((EntityHandle::new(idx as u32, slot.generation), entity))
        })
    }
}