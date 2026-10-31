use pumpkin_macros::{Event, cancellable};
use pumpkin_util::math::position::BlockPos;

/// An event that occurs when a block explodes.
#[cancellable]
#[derive(Event, Clone)]
pub struct BlockExplodeEvent {
    pub block_pos: BlockPos,
    pub yield_rate: f32,
    /// The blocks the explosion destroys. Vanilla hands this same list to every
    /// `BlockState.onExplosionHit` call, so a position a handler removes stays standing.
    pub blocks: Vec<BlockPos>,
}

impl BlockExplodeEvent {
    #[must_use]
    pub const fn new(block_pos: BlockPos, yield_rate: f32, blocks: Vec<BlockPos>) -> Self {
        Self {
            block_pos,
            yield_rate,
            blocks,
            cancelled: false,
        }
    }
}
