use wincode::{SchemaRead, SchemaWrite};
use mlua::{Error, FromLua, IntoLua, Lua, Result, Value};

const INDEX_BITS: u32 = 21;
const GENERATION_BITS: u32 = 11;

const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;
const GENERATION_MASK: u32 = (1 << GENERATION_BITS) - 1;

#[repr(transparent)]
#[derive(SchemaRead, SchemaWrite, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EntityHandle(pub u32);

impl EntityHandle {
    pub const NULL: Self = Self(0);
    pub const MAX_ENTITIES: usize = 1 << INDEX_BITS;
    pub const MAX_GENERATION: u16 = GENERATION_MASK as u16;

    #[inline]
    pub const fn new(idx: u32, generation: u16) -> Self {
        Self((((generation as u32) & GENERATION_MASK) << INDEX_BITS) | (idx & INDEX_MASK))
    }

    #[inline]
    pub const fn index(self) -> u32 {
        self.0 & INDEX_MASK
    }

    #[inline]
    pub const fn generation(self) -> u16 {
        ((self.0 >> INDEX_BITS) & GENERATION_MASK) as u16
    }

    #[inline]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn next_generation(generation: u16) -> u16 {
        match ((generation & Self::MAX_GENERATION) + 1) & Self::MAX_GENERATION {
            0 => 1,
            next => next,
        }
    }
}

impl IntoLua for EntityHandle {
    fn into_lua(self, _lua: &Lua) -> Result<Value> {
        Ok(Value::Integer(self.0 as mlua::Integer))
    }
}

impl FromLua for EntityHandle {
    fn from_lua(value: Value, _lua: &Lua) -> Result<Self> {
        match value {
            Value::Nil => Ok(Self::NULL),
            Value::Integer(raw) => Ok(Self(raw as u32)),
            Value::Number(raw) => Ok(Self(raw as u32)),
            other => Err(Error::FromLuaConversionError {
                from: other.type_name(),
                to: "EntityHandle".to_string(),
                message: Some("expected an entity handle integer or nil".to_string()),
            }),
        }
    }
}