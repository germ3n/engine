use mlua::MetaMethod;
use mlua::{Error, FromLua, Lua, Result, UserData, UserDataMethods, Value};
use r#macro::document;
use wincode::{SchemaRead, SchemaWrite};

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

impl FromLua for EntityHandle {
    fn from_lua(value: Value, _lua: &Lua) -> Result<Self> {
        match value {
            Value::Nil => Ok(Self::NULL),
            Value::UserData(ud) => Ok(*ud.borrow::<Self>()?),
            Value::Integer(raw) => Ok(Self(raw as u32)),
            Value::Number(raw) => Ok(Self(raw as u32)),
            other => Err(Error::FromLuaConversionError {
                from: other.type_name(),
                to: "EntityHandle".to_string(),
                message: Some("expected an EntityHandle, integer, or nil".to_string()),
            }),
        }
    }
}

#[document(
    kind = "class",
    name = "EntityHandle",
    realm = "shared",
    summary = "Packed entity id passed to hooks. ents.get accepts it.",
    note = "Two handles compare equal when their packed ids match.",
    see_also = "ents.get",
)]
fn entity_handle_class() {}

#[document(
    parent = "EntityHandle",
    name = "index",
    kind = "method",
    realm = "shared",
    summary = "Slot index inside the packed id.",
    returns = { ty = "number", desc = "The index bits." },
)]
fn entity_handle_index() {}

#[document(
    parent = "EntityHandle",
    name = "generation",
    kind = "method",
    realm = "shared",
    summary = "Generation stored in the packed id. It changes when the slot is reused.",
    returns = { ty = "number", desc = "The generation bits." },
)]
fn entity_handle_generation() {}

#[document(
    parent = "EntityHandle",
    name = "raw",
    kind = "method",
    realm = "shared",
    summary = "Packed id as an integer.",
    returns = { ty = "number", desc = "The raw handle." },
    see_also = "ents.get",
)]
fn entity_handle_raw() {}

#[document(
    parent = "EntityHandle",
    name = "is_null",
    kind = "method",
    realm = "shared",
    summary = "True when the handle is empty.",
    returns = { ty = "boolean", desc = "True for the zero handle." },
)]
fn entity_handle_is_null() {}

impl UserData for EntityHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("index", |_, this, ()| Ok(this.index()));
        methods.add_method("generation", |_, this, ()| Ok(this.generation()));
        methods.add_method("raw", |_, this, ()| Ok(this.0));
        methods.add_method("is_null", |_, this, ()| Ok(this.is_null()));
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: Self| Ok(this.0 == other.0));
        methods.add_meta_method(MetaMethod::Lt, |_, this, other: Self| Ok(this.0 < other.0));
        methods.add_meta_method(MetaMethod::Le, |_, this, other: Self| Ok(this.0 <= other.0));
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            if this.is_null() {
                return Ok("EntityHandle[null]".to_string());
            }
            Ok(format!(
                "EntityHandle[{}:{}]",
                this.index(),
                this.generation()
            ))
        });
    }
}
