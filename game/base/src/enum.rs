use std::ops::{BitAnd, BitOr, BitOrAssign};
use wincode::{SchemaRead, SchemaWrite};

#[repr(transparent)]
#[derive(SchemaRead, SchemaWrite, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct InputButtons(pub u64);

impl InputButtons {
    pub const NONE: Self       = Self(0);
    pub const IN_ATTACK: Self  = Self(1 << 0);
    pub const IN_ATTACK2: Self = Self(1 << 1);
    pub const IN_USE: Self     = Self(1 << 2);
    pub const IN_SPRINT: Self  = Self(1 << 3);
    pub const IN_WALK: Self    = Self(1 << 4);
    pub const IN_DUCK: Self    = Self(1 << 5);
    pub const IN_JUMP: Self    = Self(1 << 6);
    pub const IN_RELOAD: Self  = Self(1 << 7);

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub const fn has_buttons(self, other: Self) -> bool {
        self.contains(other)
    }

    #[inline]
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

#[repr(transparent)]
#[derive(SchemaRead, SchemaWrite, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EntityFlags(pub u64);

impl EntityFlags {
    pub const NONE: Self        = Self(0);
    pub const FL_ONGROUND: Self = Self(1 << 0);
    pub const FL_ONFIRE: Self   = Self(1 << 1);
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub const fn has_buttons(self, other: Self) -> bool {
        self.contains(other)
    }

    #[inline]
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

impl BitOr for InputButtons {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self { Self(self.0 | rhs.0) }
}

impl BitAnd for InputButtons {
    type Output = Self;
    #[inline]
    fn bitand(self, rhs: Self) -> Self { Self(self.0 & rhs.0) }
}

impl BitOrAssign for InputButtons {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) { self.0 |= rhs.0; }
}

impl BitOr for EntityFlags {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self { Self(self.0 | rhs.0) }
}

impl BitAnd for EntityFlags {
    type Output = Self;
    #[inline]
    fn bitand(self, rhs: Self) -> Self { Self(self.0 & rhs.0) }
}

impl BitOrAssign for EntityFlags {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) { self.0 |= rhs.0; }
}