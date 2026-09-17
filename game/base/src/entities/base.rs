pub trait Networkable {
    fn entity_id(&self) -> i32;
    fn sync_network_vars(&self);
}

pub struct BaseEntityData {
    pub entity_id: i32,
    pub position: [f64; 3]
}

impl Default for BaseEntityData {
    fn default() -> Self {
        Self {
            entity_id: 0,
            position: [0.0; 3]
        }
    }   
}

pub trait BaseEntity: Networkable {
    fn base(&self) -> &BaseEntityData;
    fn base_mut(&mut self) -> &mut BaseEntityData;
    fn on_spawn(&mut self);
    fn tick(&mut self);
}