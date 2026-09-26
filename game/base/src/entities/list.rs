use std::cell::UnsafeCell;
use std::collections::VecDeque;
use crate::entities::{DynEntity, EntityHandle, EntityCommand, TickContext, FrameInfo};

const MIN_FREE_SLOTS: usize = 64;
const MAX_COMMAND_PASSES: usize = 8;
const NOT_THINKING: u32 = u32::MAX;

pub const DEFAULT_MAX_ENTITIES: usize = 102400;

fn slot_entity(slot: &Slot) -> &Option<Box<DynEntity>> {
    unsafe { &*slot.entity.get() }
}

struct Slot {
    generation: u16,
    think_idx: u32,
    entity: UnsafeCell<Option<Box<DynEntity>>>,
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
            Some(slot) => slot.generation == handle.generation() && slot_entity(slot).is_some(),
            None => false,
        }
    }

    pub fn get(&self, handle: EntityHandle) -> Option<&DynEntity> {
        let slot = self.slots.get(handle.index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }

        slot_entity(slot).as_deref()
    }

    pub fn get_mut(&mut self, handle: EntityHandle) -> Option<&mut DynEntity> {
        let slot = self.slots.get_mut(handle.index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }

        slot.entity.get_mut().as_deref_mut()
    }

    fn allocate_index(&mut self) -> Option<u32> {
        let at_capacity = self.slots.len() >= self.max_entities;

        if at_capacity || self.free.len() > MIN_FREE_SLOTS {
            while let Some(idx) = self.free.pop_front() {
                if self.slots[idx as usize].entity.get_mut().is_none() {
                    return Some(idx);
                }
            }
        }

        if at_capacity {
            return None;
        }

        let idx = self.slots.len() as u32;
        self.slots.push(Slot { generation: 0, think_idx: NOT_THINKING, entity: UnsafeCell::new(None) });

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
        *slot.entity.get_mut() = Some(entity);
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
            self.slots.resize_with(idx + 1, || Slot { generation: 0, think_idx: NOT_THINKING, entity: UnsafeCell::new(None) });
        }

        if self.slots[idx].entity.get_mut().is_some() {
            return false;
        }

        let wants_think = entity.wants_think();
        entity.base_mut().handle = handle;

        let slot = &mut self.slots[idx];
        slot.generation = handle.generation();
        *slot.entity.get_mut() = Some(entity);
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
        *slot.entity.get_mut() = None;
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

    fn with_entity(&mut self, handle: EntityHandle, commands: &mut Vec<EntityCommand>, f: impl FnOnce(&mut DynEntity, &mut TickContext)) {
        let idx = handle.index() as usize;
        let frame = self.frame;
        let entity_ptr = {
            let Some(slot) = self.slots.get(idx) else {
                return;
            };

            if slot.generation != handle.generation() {
                return;
            }

            unsafe {
                (*slot.entity.get()).as_mut().map(|entity| entity.as_mut() as *mut DynEntity)
            }
        };

        let Some(entity_ptr) = entity_ptr else {
            return;
        };

        let mut ctx = TickContext::new(frame, self, commands);
        unsafe {
            f(&mut *entity_ptr, &mut ctx);
        }
    }

    fn dispatch_on_spawn(&mut self, handle: EntityHandle) {
        let mut commands = std::mem::take(&mut self.commands);
        self.with_entity(handle, &mut commands, |entity, ctx| {
            entity.on_spawn(ctx);
        });
        self.commands = commands;
    }

    pub fn tick_all(&mut self) {
        let mut commands = std::mem::take(&mut self.commands);
        let count = self.think_list.len();

        for think_idx in 0..count {
            let handle = self.think_list[think_idx];
            self.with_entity(handle, &mut commands, |entity, ctx| {
                entity.tick(ctx);
            });
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
            if slot_entity(slot).is_some() {
                out.push(EntityHandle::new(idx as u32, slot.generation));
            }
        }

        out
    }

    pub fn iter(&self) -> impl Iterator<Item = (EntityHandle, &DynEntity)> {
        self.slots.iter().enumerate().filter_map(|(idx, slot)| {
            let entity = slot_entity(slot).as_deref()?;

            Some((EntityHandle::new(idx as u32, slot.generation), entity))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use crate::entities::base::{BaseEntity, BaseEntityData, Networkable};

    struct Probe {
        base: BaseEntityData,
        other: EntityHandle,
        saw_self_on_spawn: Arc<AtomicBool>,
        saw_self: Arc<AtomicBool>,
        saw_other: Arc<AtomicBool>,
    }

    impl Networkable for Probe {
        fn handle(&self) -> EntityHandle {
            self.base.handle
        }

        fn sync_network_vars(&self) {
        }
    }

    impl BaseEntity for Probe {
        fn base(&self) -> &BaseEntityData {
            &self.base
        }

        fn base_mut(&mut self) -> &mut BaseEntityData {
            &mut self.base
        }

        fn on_spawn(&mut self, ctx: &mut TickContext) {
            let handle = self.base.handle;
            if let Some(current) = ctx.get(handle) {
                self.saw_self_on_spawn.store(current.handle() == handle && ctx.is_valid(handle), Ordering::Relaxed);
            }
        }

        fn tick(&mut self, ctx: &mut TickContext) {
            let handle = self.base.handle;
            if let Some(current) = ctx.get(handle) {
                self.saw_self.store(current.handle() == handle && ctx.is_valid(handle), Ordering::Relaxed);
            }

            if self.other.is_null() {
                return;
            }

            if let Some(other) = ctx.get(self.other) {
                self.saw_other.store(other.handle() == self.other, Ordering::Relaxed);
            }
        }

        fn wants_think(&self) -> bool {
            true
        }
    }

    #[test]
    fn ticking_entity_stays_visible() {
        let mut list = EntityList::new();
        let other_saw_self_on_spawn = Arc::new(AtomicBool::new(false));
        let other_saw_self = Arc::new(AtomicBool::new(false));
        let other = list.spawn(Box::new(Probe {
            base: BaseEntityData::default(),
            other: EntityHandle::NULL,
            saw_self_on_spawn: other_saw_self_on_spawn.clone(),
            saw_self: other_saw_self.clone(),
            saw_other: Arc::new(AtomicBool::new(false)),
        })).unwrap();

        let saw_self_on_spawn = Arc::new(AtomicBool::new(false));
        let saw_self = Arc::new(AtomicBool::new(false));
        let saw_other = Arc::new(AtomicBool::new(false));
        let main = list.spawn(Box::new(Probe {
            base: BaseEntityData::default(),
            other,
            saw_self_on_spawn: saw_self_on_spawn.clone(),
            saw_self: saw_self.clone(),
            saw_other: saw_other.clone(),
        })).unwrap();

        assert!(other_saw_self_on_spawn.load(Ordering::Relaxed));
        assert!(saw_self_on_spawn.load(Ordering::Relaxed));
        assert!(list.is_valid(other));
        assert!(list.is_valid(main));

        list.tick_all();

        assert!(other_saw_self.load(Ordering::Relaxed));
        assert!(saw_self.load(Ordering::Relaxed));
        assert!(saw_other.load(Ordering::Relaxed));
        assert!(list.get(main).is_some());
        assert!(list.get(other).is_some());
    }
}