use std::collections::VecDeque;
use crate::entities::{EntityHandle, DynEntity};

const MIN_FREE_SLOTS: usize = 64;

struct Slot {
    generation: u16,
    entity: Option<Box<DynEntity>>,
}

#[derive(Default)]
pub struct EntityList {
    slots: Vec<Slot>,
    free: VecDeque<u32>,
    count: usize,
}

impl EntityList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn is_valid(&self, handle: EntityHandle) -> bool {
        match self.slots.get(handle.index() as usize) {
            Some(slot) => slot.generation == handle.generation() && !handle.is_null(),
            None => false,
        }
    }

    fn allocate_index(&mut self) -> Option<u32> {
        let at_capacity = self.slots.len() >= EntityHandle::MAX_ENTITIES;

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
        self.slots.push(Slot { generation: 0, entity: None });

        Some(idx)
    }

    pub fn spawn(&mut self, mut entity: Box<DynEntity>) -> Option<EntityHandle> {
        let idx = self.allocate_index()?;

        let slot = &mut self.slots[idx as usize];
        slot.generation = EntityHandle::next_generation(slot.generation);

        let handle = EntityHandle::new(idx, slot.generation);
        entity.base_mut().handle = handle;
        slot.entity = Some(entity);
        self.count += 1;

        self.with_entity(handle, |entity, list| entity.on_spawn(list));

        Some(handle)
    }

    pub fn insert_at(&mut self, handle: EntityHandle, mut entity: Box<DynEntity>) -> bool {
        if handle.is_null() {
            return false;
        }

        let idx = handle.index() as usize;
        if idx >= self.slots.len() {
            self.slots.resize_with(idx + 1, || Slot { generation: 0, entity: None });
        }

        let slot = &mut self.slots[idx];
        if slot.entity.is_some() {
            return false;
        }

        slot.generation = handle.generation();
        entity.base_mut().handle = handle;
        slot.entity = Some(entity);
        self.count += 1;

        self.with_entity(handle, |entity, list| entity.on_spawn(list));

        true
    }

    pub fn remove(&mut self, handle: EntityHandle) -> bool {
        let idx = handle.index() as usize;
        let Some(slot) = self.slots.get_mut(idx) else { return false; };

        if slot.generation != handle.generation() || handle.is_null() {
            return false;
        }

        slot.entity = None;
        slot.generation = EntityHandle::next_generation(slot.generation);
        self.free.push_back(handle.index());
        self.count -= 1;

        true
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

    pub fn with_entity<R>(
        &mut self,
        handle: EntityHandle,
        func: impl FnOnce(&mut DynEntity, &mut Self) -> R,
    ) -> Option<R> {
        let idx = handle.index() as usize;
        let slot = self.slots.get_mut(idx)?;

        if slot.generation != handle.generation() {
            return None;
        }

        let mut entity = slot.entity.take()?;
        let result = func(entity.as_mut(), self);

        let slot = &mut self.slots[idx];
        if slot.generation == handle.generation() {
            slot.entity = Some(entity);
        }

        Some(result)
    }

    pub fn tick_all(&mut self) {
        let len = self.slots.len();

        for idx in 0..len {
            let slot = &self.slots[idx];
            if slot.entity.is_none() {
                continue;
            }

            let handle = EntityHandle::new(idx as u32, slot.generation);
            self.with_entity(handle, |entity, list| entity.tick(list));
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