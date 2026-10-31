use pumpkin_macros::{Event, cancellable};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

/// An event that occurs when an entity explodes.
#[cancellable]
#[derive(Event, Clone)]
pub struct EntityExplodeEvent {
    /// The ID of the exploding entity.
    pub entity_id: i32,

    /// The position where the explosion occurred.
    pub position: Vector3<f64>,

    /// The yield rate of blocks destroyed.
    pub yield_rate: f32,

    /// The blocks the explosion destroys. Vanilla hands this same list to every
    /// `BlockState.onExplosionHit` call, so a position a handler removes stays standing.
    pub blocks: Vec<BlockPos>,
}

impl EntityExplodeEvent {
    #[must_use]
    pub const fn new(
        entity_id: i32,
        position: Vector3<f64>,
        yield_rate: f32,
        blocks: Vec<BlockPos>,
    ) -> Self {
        Self {
            entity_id,
            position,
            yield_rate,
            blocks,
            cancelled: false,
        }
    }
}
