use super::{Block, BlockPos};

pub trait TraceFilter {
    fn should_hit_brush(&self, brush: usize) -> bool {
        let _ = brush;

        true
    }

    fn should_hit_voxel(&self, pos: BlockPos, block: Block) -> bool {
        let _ = (pos, block);

        true
    }

    fn should_hit_entity(&self, entity: u32) -> bool {
        let _ = entity;

        true
    }

    fn should_hit_bone(&self, entity: u32, bone: u16, group: u8) -> bool {
        let _ = (entity, bone, group);

        true
    }
}

pub struct HitAll;

impl TraceFilter for HitAll {}

impl<T: TraceFilter + ?Sized> TraceFilter for &T {
    fn should_hit_brush(&self, brush: usize) -> bool {
        (**self).should_hit_brush(brush)
    }

    fn should_hit_voxel(&self, pos: BlockPos, block: Block) -> bool {
        (**self).should_hit_voxel(pos, block)
    }

    fn should_hit_entity(&self, entity: u32) -> bool {
        (**self).should_hit_entity(entity)
    }

    fn should_hit_bone(&self, entity: u32, bone: u16, group: u8) -> bool {
        (**self).should_hit_bone(entity, bone, group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SkipBrush(usize);

    impl TraceFilter for SkipBrush {
        fn should_hit_brush(&self, brush: usize) -> bool {
            brush != self.0
        }
    }

    #[test]
    fn defaults_hit_and_overrides_apply() {
        let pos = BlockPos { x: 0, y: 0, z: 0 };

        assert!(HitAll.should_hit_brush(3));
        assert!(HitAll.should_hit_voxel(pos, Block::STONE));
        assert!(!SkipBrush(3).should_hit_brush(3));
        assert!(SkipBrush(3).should_hit_voxel(pos, Block::STONE));
        assert!(!(&SkipBrush(1)).should_hit_brush(1));
    }

    struct SkipEntity(u32);

    impl TraceFilter for SkipEntity {
        fn should_hit_entity(&self, entity: u32) -> bool {
            entity != self.0
        }

        fn should_hit_bone(&self, entity: u32, bone: u16, group: u8) -> bool {
            !(entity == self.0 && bone == 2 && group == 9)
        }
    }

    #[test]
    fn entity_and_bone_defaults_hit() {
        assert!(HitAll.should_hit_entity(4));
        assert!(HitAll.should_hit_bone(4, 1, 0));
        assert!(SkipBrush(1).should_hit_entity(4));
        assert!(SkipBrush(1).should_hit_bone(4, 1, 0));
    }

    #[test]
    fn entity_and_bone_overrides_apply_and_forward_through_refs() {
        let filter = SkipEntity(4);
        let by_ref = &filter;
        let dynamic: &dyn TraceFilter = &filter;

        assert!(!filter.should_hit_entity(4));
        assert!(filter.should_hit_entity(5));
        assert!(!by_ref.should_hit_entity(4));
        assert!(!dynamic.should_hit_entity(4));
        assert!(!(&&filter).should_hit_entity(4));
        assert!(!by_ref.should_hit_bone(4, 2, 9));
        assert!(by_ref.should_hit_bone(4, 2, 8));
        assert!(!dynamic.should_hit_bone(4, 2, 9));
        assert!(dynamic.should_hit_bone(5, 2, 9));
    }
}
