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
    pub value: ConVarValue,
    pub default_value: ConVarValue,
    pub has_cheat_flag: bool,
    pub is_replicated_to_clients: bool,
}

impl ConVar {
    pub fn new(name: &str, default: ConVarValue, description: &str, is_cheat: Option<bool>, is_replicated: Option<bool>) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            value: default.clone(),
            default_value: default,
            has_cheat_flag: is_cheat.unwrap_or(false),
            is_replicated_to_clients: is_replicated.unwrap_or(false),
        }
    }

    pub fn set_value(&mut self, new_value: ConVarValue) {
        self.value = new_value;
    }
    
    pub fn reset(&mut self) {
        self.value = self.default_value.clone();
    }
}