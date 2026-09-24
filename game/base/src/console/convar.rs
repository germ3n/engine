use std::sync::Mutex;
use mlua::RegistryKey;

#[derive(Debug, Clone)]
pub enum ConVarValue {
    Integer(i64),
    Float(f64),
    String(String),
    Bool(bool),
}

#[derive(Debug)]
pub struct ConVar {
    pub name: String,
    pub description: String,
    pub value: Mutex<ConVarValue>,
    pub default_value: ConVarValue,
    pub has_cheat_flag: bool,
    pub is_replicated_to_clients: bool,
    pub callbacks: Mutex<Vec<RegistryKey>>,
}

impl ConVar {
    pub fn new(name: &str, default: ConVarValue, description: &str, is_cheat: Option<bool>, is_replicated: Option<bool>) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            value: Mutex::new(default.clone()),
            default_value: default,
            has_cheat_flag: is_cheat.unwrap_or(false),
            is_replicated_to_clients: is_replicated.unwrap_or(false),
            callbacks: Mutex::new(Vec::new())
        }
    }

    pub fn set_value(&self, new_value: ConVarValue) {
        let mut val = self.value.lock().unwrap();
        *val = new_value;
    }
    
    pub fn reset(&self) {
        self.set_value(self.default_value.clone());
    }
}