use std::sync::Arc;

use crate::block::{
    BlockBehaviour, CanPlaceAtArgs, GetStateForNeighborUpdateArgs, OnPlaceArgs,
    OnScheduledTickArgs, PathComputationType, push_entities_up,
};
use crate::world::World;
use pumpkin_data::{
    Block, BlockDirection, BlockState, BlockStateId,
    game_event::GameEvent,
    tag::{self, Taggable},
};
use pumpkin_macros::pumpkin_block;
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::tick::TickPriority;
use pumpkin_world::world::BlockAccessor;
use pumpkin_world::world::BlockFlags;

#[pumpkin_block("minecraft:dirt_path")]
pub struct DirtPathBlock;

impl BlockBehaviour for DirtPathBlock {
    fn on_scheduled_tick(&self, args: OnScheduledTickArgs<'_>) {
        let state = args.world.get_block_state(args.position);
        turn_to_base_block(args.world, args.position, state);
    }

    fn on_place(&self, args: OnPlaceArgs<'_>) -> BlockStateId {
        if !can_place_at(args.world, args.position) {
            return Block::DIRT.default_state.id;
        }

        args.block.default_state.id
    }

    fn get_state_for_neighbor_update(
        &self,
        args: GetStateForNeighborUpdateArgs<'_>,
    ) -> BlockStateId {
        if args.direction == BlockDirection::Up && !can_place_at(args.world, args.position) {
            args.world
                .schedule_block_tick(args.block, *args.position, 1, TickPriority::Normal);
        }
        args.state_id
    }

    fn can_place_at(&self, args: CanPlaceAtArgs<'_>) -> bool {
        can_place_at(args.block_accessor, args.position)
    }

    fn is_pathfindable(&self, _state: &BlockState, _computation_type: PathComputationType) -> bool {
        false
    }
}

/// Vanilla `PathBlock.turnToBaseBlock`.
fn turn_to_base_block(world: &Arc<World>, position: &BlockPos, state: &BlockState) {
    let new_state = Block::DIRT.default_state;
    push_entities_up(world, position, state, new_state);
    world.set_block_state(position, new_state.id, BlockFlags::NOTIFY_ALL);
    world.emit_game_event(GameEvent::BlockChange.name(), position.to_centered_f64());
}

fn can_place_at(world: &dyn BlockAccessor, block_pos: &BlockPos) -> bool {
    let above_pos = block_pos.up();
    let state = world.get_block_state(&above_pos);
    !state.is_solid()
        || world
            .get_block(&above_pos)
            .has_tag(&tag::Block::MINECRAFT_FENCE_GATES)
}
