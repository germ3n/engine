use std::collections::{HashMap, BinaryHeap};
use std::cmp::Reverse;
use crate::entities::BaseEntity;

pub struct EntityList {
    entities: HashMap<i32, Box<dyn BaseEntity + Send>>,
    next_id: i32,
    free_ids: BinaryHeap<Reverse<i32>>,
}

impl Default for EntityList {
    fn default() -> Self {
        Self {
            entities: HashMap::new(),
            next_id: 1,
            free_ids: BinaryHeap::new(),
        }
    }
}

impl EntityList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn_entity(&mut self, mut entity: Box<dyn BaseEntity + Send>) -> i32 {
        let id = if let Some(Reverse(reused_id)) = self.free_ids.pop() {
            reused_id
        } else {
            let new_id = self.next_id;
            self.next_id += 1;
            new_id
        };

        entity.base_mut().entity_id = id;
        entity.on_spawn(); 
        self.entities.insert(id, entity);

        id
    }

    pub fn remove_entity(&mut self, id: i32) -> Option<Box<dyn BaseEntity + Send>> {
        let entity = self.entities.remove(&id);
        
        if entity.is_some() {
            self.free_ids.push(Reverse(id));
        }
        
        entity
    }

    pub fn get_entity(&self, id: i32) -> Option<&Box<dyn BaseEntity + Send>> {
        self.entities.get(&id)
    }
}