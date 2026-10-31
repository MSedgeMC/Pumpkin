use crate::entity::player::Player;
use pumpkin_macros::{Event, cancellable};
use pumpkin_util::math::position::BlockPos;
use std::sync::Arc;

/// An event that occurs when a player opens an inventory.
#[cancellable]
#[derive(Event, Clone)]
pub struct InventoryOpenEvent {
    /// The player opening the inventory.
    pub player: Arc<Player>,
    /// The position of the block that owns the opened container, if the screen has one.
    pub block_pos: Option<BlockPos>,
}

impl InventoryOpenEvent {
    #[must_use]
    pub const fn new(player: Arc<Player>, block_pos: Option<BlockPos>) -> Self {
        Self {
            player,
            block_pos,
            cancelled: false,
        }
    }
}
