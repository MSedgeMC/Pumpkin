use pumpkin_data::Block;
use pumpkin_gametest::{GameTestHelper, GameTestResult};
use pumpkin_util::math::position::BlockPos;

use super::build_stone_floor;

/// Vanilla `BlockBehaviour#onExplosionHit` (`BlockBehaviour.java:170-193`) calls
/// `spawnAfterBreak`, so the TNT has to leave a silverfish behind.
pub(super) async fn test_tnt_blast_spawns_silverfish_from_infested_stone(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 5, 5).await?;

    let infested_pos = BlockPos::new(2, 1, 2);
    helper
        .set_block_default(&infested_pos, &Block::INFESTED_STONE)
        .await?;
    helper
        .spawn_entity("minecraft:tnt", &BlockPos::new(3, 1, 2))
        .await?;

    helper.succeed_when(move |helper| async move {
        helper
            .assert_block_not_present(&infested_pos, &Block::INFESTED_STONE)
            .await?;
        helper.assert_entity_present("minecraft:silverfish").await
    })?;
    Ok(())
}
