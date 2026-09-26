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

    pub fn clear(&mut self) {
        self.slots.clear();
        self.free.clear();
        self.think_list.clear();
        self.commands.clear();
        self.count = 0;
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
        let Some(slot) = self.slots.get_mut(idx) else {
            return;
        };

        if slot.generation != handle.generation() {
            return;
        }

        if slot.entity.get_mut().is_none() {
            return;
        }

        if slot.think_idx != NOT_THINKING {
            return;
        }

        let think_idx = self.think_list.len() as u32;
        slot.think_idx = think_idx;
        self.think_list.push(handle);
    }

    fn remove_think(&mut self, handle: EntityHandle) {
        let idx = handle.index() as usize;
        let Some(slot) = self.slots.get(idx) else {
            return;
        };

        if slot.generation != handle.generation() {
            return;
        }

        let think_idx = slot.think_idx;

        if think_idx == NOT_THINKING {
            return;
        }

        if think_idx as usize >= self.think_list.len() || self.think_list[think_idx as usize] != handle {
            return;
        }

        self.slots[idx].think_idx = NOT_THINKING;
        let last = self.think_list.len() - 1;
        self.think_list.swap_remove(think_idx as usize);

        if (think_idx as usize) >= last {
            return;
        }

        let moved = self.think_list[think_idx as usize];
        let Some(slot) = self.slots.get_mut(moved.index() as usize) else {
            return;
        };

        if slot.generation != moved.generation() {
            return;
        }

        slot.think_idx = think_idx;
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
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
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

    struct Actor {
        base: BaseEntityData,
        ticks: Arc<AtomicUsize>,
        spawns: Arc<AtomicUsize>,
        think: bool,
        victim: Arc<AtomicU32>,
        chain: u32,
        spawn_on_tick: bool,
        child_ticks: Option<Arc<AtomicUsize>>,
    }

    impl Actor {
        fn new(think: bool) -> (Self, Arc<AtomicUsize>) {
            let ticks = Arc::new(AtomicUsize::new(0));
            let actor = Self {
                base: BaseEntityData::default(),
                ticks: ticks.clone(),
                spawns: Arc::new(AtomicUsize::new(0)),
                think,
                victim: Arc::new(AtomicU32::new(0)),
                chain: 0,
                spawn_on_tick: false,
                child_ticks: None,
            };

            (actor, ticks)
        }

        fn child(&self, chain: u32) -> Self {
            Self {
                base: BaseEntityData::default(),
                ticks: Arc::new(AtomicUsize::new(0)),
                spawns: self.spawns.clone(),
                think: self.think,
                victim: Arc::new(AtomicU32::new(0)),
                chain,
                spawn_on_tick: false,
                child_ticks: None,
            }
        }
    }

    impl Networkable for Actor {
        fn handle(&self) -> EntityHandle {
            self.base.handle
        }

        fn sync_network_vars(&self) {
        }
    }

    impl BaseEntity for Actor {
        fn base(&self) -> &BaseEntityData {
            &self.base
        }

        fn base_mut(&mut self) -> &mut BaseEntityData {
            &mut self.base
        }

        fn on_spawn(&mut self, ctx: &mut TickContext) {
            self.spawns.fetch_add(1, Ordering::Relaxed);

            if self.chain == 0 {
                return;
            }

            ctx.spawn(Box::new(self.child(self.chain - 1)));
        }

        fn tick(&mut self, ctx: &mut TickContext) {
            self.ticks.fetch_add(1, Ordering::Relaxed);
            let victim = EntityHandle(self.victim.load(Ordering::Relaxed));

            if !victim.is_null() {
                ctx.remove(victim);
            }

            if !self.spawn_on_tick {
                return;
            }

            self.spawn_on_tick = false;
            let Some(ticks) = self.child_ticks.clone() else {
                return;
            };

            ctx.spawn(Box::new(Actor {
                base: BaseEntityData::default(),
                ticks,
                spawns: Arc::new(AtomicUsize::new(0)),
                think: true,
                victim: Arc::new(AtomicU32::new(0)),
                chain: 0,
                spawn_on_tick: false,
                child_ticks: None,
            }));
        }

        fn wants_think(&self) -> bool {
            self.think
        }
    }

    #[test]
    fn removed_handle_stays_dead_after_reuse() {
        let mut list = EntityList::with_max_entities(1);
        let (actor, _) = Actor::new(true);
        let old = list.spawn(Box::new(actor)).unwrap();

        assert!(list.remove(old));
        assert!(!list.is_valid(old));
        assert!(list.get(old).is_none());
        assert!(!list.remove(old));
        assert_eq!(list.len(), 0);

        let (actor, _) = Actor::new(true);
        let renewed = list.spawn(Box::new(actor)).unwrap();

        assert_eq!(renewed.index(), old.index());
        assert_ne!(renewed.generation(), old.generation());
        assert!(!list.is_valid(old));
        assert!(list.is_valid(renewed));
        assert!(list.get(old).is_none());
        assert!(list.get(renewed).is_some());
    }

    #[test]
    fn removing_a_thinker_ticks_each_survivor_once() {
        let mut list = EntityList::new();
        let (first, first_ticks) = Actor::new(true);
        let (middle, middle_ticks) = Actor::new(true);
        let (last, last_ticks) = Actor::new(true);
        let first = list.spawn(Box::new(first)).unwrap();
        let middle = list.spawn(Box::new(middle)).unwrap();
        let last = list.spawn(Box::new(last)).unwrap();

        assert!(list.remove(middle));
        assert!(!list.remove(middle));
        list.tick_all();

        assert_eq!(first_ticks.load(Ordering::Relaxed), 1);
        assert_eq!(middle_ticks.load(Ordering::Relaxed), 0);
        assert_eq!(last_ticks.load(Ordering::Relaxed), 1);
        assert_eq!(list.len(), 2);
        assert_eq!(list.think_count(), 2);
        assert!(list.is_valid(first));
        assert!(!list.is_valid(middle));
        assert!(list.is_valid(last));

        let mut list = EntityList::new();
        let (first, first_ticks) = Actor::new(true);
        let (middle, middle_ticks) = Actor::new(true);
        let (last, last_ticks) = Actor::new(true);
        let _first = list.spawn(Box::new(first)).unwrap();
        let _middle = list.spawn(Box::new(middle)).unwrap();
        let _last = list.spawn(Box::new(last)).unwrap();

        assert!(list.remove(_first));
        list.tick_all();

        assert_eq!(first_ticks.load(Ordering::Relaxed), 0);
        assert_eq!(middle_ticks.load(Ordering::Relaxed), 1);
        assert_eq!(last_ticks.load(Ordering::Relaxed), 1);
        assert_eq!(list.think_count(), 2);
    }

    #[test]
    fn queued_remove_waits_until_after_the_think() {
        let mut list = EntityList::new();
        let (killer, killer_ticks) = Actor::new(true);
        let victim_slot = killer.victim.clone();
        let (victim, victim_ticks) = Actor::new(true);
        let _killer = list.spawn(Box::new(killer)).unwrap();
        let victim = list.spawn(Box::new(victim)).unwrap();
        victim_slot.store(victim.0, Ordering::Relaxed);

        list.tick_all();

        assert_eq!(killer_ticks.load(Ordering::Relaxed), 1);
        assert_eq!(victim_ticks.load(Ordering::Relaxed), 1);
        assert!(!list.is_valid(victim));
        assert!(list.is_valid(_killer));
        assert_eq!(list.len(), 1);
        assert_eq!(list.think_count(), 1);
    }

    #[test]
    fn spawn_during_tick_thinks_next_tick() {
        let mut list = EntityList::new();
        let (mut parent, parent_ticks) = Actor::new(true);
        let child_ticks = Arc::new(AtomicUsize::new(0));
        parent.spawn_on_tick = true;
        parent.child_ticks = Some(child_ticks.clone());
        list.spawn(Box::new(parent)).unwrap();

        list.tick_all();

        assert_eq!(parent_ticks.load(Ordering::Relaxed), 1);
        assert_eq!(child_ticks.load(Ordering::Relaxed), 0);
        assert_eq!(list.len(), 2);
        assert_eq!(list.think_count(), 2);

        list.tick_all();

        assert_eq!(parent_ticks.load(Ordering::Relaxed), 2);
        assert_eq!(child_ticks.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn spawn_chain_stops_after_max_command_passes() {
        let mut list = EntityList::new();
        let (mut root, _) = Actor::new(false);
        let spawns = root.spawns.clone();
        root.chain = MAX_COMMAND_PASSES as u32;
        list.spawn(Box::new(root)).unwrap();
        list.apply_commands();

        assert_eq!(list.len(), 1 + MAX_COMMAND_PASSES);
        assert_eq!(spawns.load(Ordering::Relaxed), 1 + MAX_COMMAND_PASSES);

        list.tick_all();

        assert_eq!(list.len(), 1 + MAX_COMMAND_PASSES);

        let mut list = EntityList::new();
        let (mut root, _) = Actor::new(false);
        let spawns = root.spawns.clone();
        root.chain = MAX_COMMAND_PASSES as u32 + 1;
        list.spawn(Box::new(root)).unwrap();
        list.apply_commands();

        assert_eq!(list.len(), 1 + MAX_COMMAND_PASSES);
        assert_eq!(spawns.load(Ordering::Relaxed), 1 + MAX_COMMAND_PASSES);

        list.tick_all();

        assert_eq!(list.len(), 2 + MAX_COMMAND_PASSES);
        assert_eq!(spawns.load(Ordering::Relaxed), 2 + MAX_COMMAND_PASSES);
    }

    #[test]
    fn set_think_enables_a_quiet_entity() {
        let mut list = EntityList::new();
        let (actor, ticks) = Actor::new(false);
        let handle = list.spawn(Box::new(actor)).unwrap();

        list.tick_all();

        assert_eq!(ticks.load(Ordering::Relaxed), 0);
        assert_eq!(list.think_count(), 0);

        list.queue(EntityCommand::SetThink { handle, enabled: true });
        list.apply_commands();
        list.tick_all();

        assert_eq!(list.think_count(), 1);
        assert_eq!(ticks.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn stale_set_think_does_not_claim_the_slot() {
        let mut list = EntityList::with_max_entities(1);
        let (actor, _) = Actor::new(true);
        let stale = list.spawn(Box::new(actor)).unwrap();

        assert!(list.remove(stale));
        list.queue(EntityCommand::SetThink { handle: stale, enabled: true });
        list.apply_commands();

        let (actor, ticks) = Actor::new(true);
        let renewed = list.spawn(Box::new(actor)).unwrap();
        list.tick_all();

        assert_eq!(renewed.index(), stale.index());
        assert_eq!(ticks.load(Ordering::Relaxed), 1);
        assert_eq!(list.think_count(), 1);
        assert!(list.is_valid(renewed));
        assert!(!list.is_valid(stale));

        let mut list = EntityList::with_max_entities(1);
        let (actor, _) = Actor::new(true);
        let stale = list.spawn(Box::new(actor)).unwrap();

        assert!(list.remove(stale));

        let (actor, ticks) = Actor::new(true);
        let renewed = list.spawn(Box::new(actor)).unwrap();
        list.queue(EntityCommand::SetThink { handle: stale, enabled: false });
        list.apply_commands();
        list.tick_all();

        assert_eq!(ticks.load(Ordering::Relaxed), 1);
        assert_eq!(list.think_count(), 1);
        assert!(list.is_valid(renewed));
    }
}