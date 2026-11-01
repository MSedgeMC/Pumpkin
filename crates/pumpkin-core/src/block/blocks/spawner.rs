use std::sync::Arc;

use crate::block::entities::mob_spawner::MobSpawnerBlockEntity;
use crate::entity::experience_orb::ExperienceOrbEntity;
use pumpkin_macros::pumpkin_block;

use crate::block::{BlockBehaviour, OnSyncedBlockEventArgs, PlacedArgs, SpawnAfterBreakArgs};

#[pumpkin_block("minecraft:spawner")]
pub struct SpawnerBlock;

impl BlockBehaviour for SpawnerBlock {
    fn on_synced_block_event(&self, _args: OnSyncedBlockEventArgs<'_>) -> bool {
        true
    }

    fn placed(&self, args: PlacedArgs<'_>) {
        {
            let spawner_block_entity = MobSpawnerBlockEntity::new(*args.position, None);
            args.world.add_block_entity(Arc::new(spawner_block_entity));
        }
    }

    fn spawn_after_break(&self, args: SpawnAfterBreakArgs<'_>) {
        // Vanilla `SpawnerBlock#spawnAfterBreak` and the `popExperience` block drops check.
        if args.drop_experience && args.world.level_info.load().game_rules.block_drops {
            let xp_count = 15 + rand::random_range(0..15) + rand::random_range(0..15);
            ExperienceOrbEntity::spawn(args.world, args.position.to_centered_f64(), xp_count);
        }
    }
}
