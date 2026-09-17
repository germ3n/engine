use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct StringTable {
    pub id_to_name: HashMap<u32, String>,
    pub name_to_id: HashMap<String, u32>,
    next_id: u32,
}

impl StringTable {
    pub fn register(&mut self, name: String) -> u32 {
        if let Some(&id) = self.name_to_id.get(&name) {
            return id;
        }
        
        let id = self.next_id;
        self.next_id += 1;
        
        self.id_to_name.insert(id, name.clone());
        self.name_to_id.insert(name, id);
        id
    }

    pub fn get_name(&self, id: u32) -> Option<&String> {
        self.id_to_name.get(&id)
    }
}