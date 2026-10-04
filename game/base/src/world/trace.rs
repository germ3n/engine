use super::{Block, BlockPos};

/// Decides which world geometry a trace or sweep may hit. A rejected target is passed through
/// as if it were not there. Every method defaults to hitting.
pub trait TraceFilter {
    /// `brush` is the index into the brush map.
    fn should_hit_brush(&self, brush: usize) -> bool {
        let _ = brush;

        true
    }

    /// Only called for solid blocks.
    fn should_hit_voxel(&self, pos: BlockPos, block: Block) -> bool {
        let _ = (pos, block);

        true
    }
}

/// Hits everything. What the unfiltered `trace` and `sweep` use.
pub struct HitAll;

impl TraceFilter for HitAll {}

impl<T: TraceFilter + ?Sized> TraceFilter for &T {
    fn should_hit_brush(&self, brush: usize) -> bool {
        (**self).should_hit_brush(brush)
    }

    fn should_hit_voxel(&self, pos: BlockPos, block: Block) -> bool {
        (**self).should_hit_voxel(pos, block)
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
}
