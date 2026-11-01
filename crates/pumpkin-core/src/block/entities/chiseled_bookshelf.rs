use pumpkin_data::Block;
use pumpkin_data::block_properties::ChiseledBookshelfLikeProperties;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::position::BlockPos;
use std::any::Any;
use std::sync::{OnceLock, RwLock, Weak};
use std::{
    array::from_fn,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI8, Ordering},
    },
};
use tracing::warn;

use crate::{
    block::entities::BlockEntity,
    world::{BlockFlags, World},
};
use pumpkin_inventory::{Clearable, Inventory, sync_write_items_to_nbt};

pub struct ChiseledBookshelfBlockEntity {
    pub position: BlockPos,
    pub items: RwLock<[ItemStack; Self::INVENTORY_SIZE]>,
    pub last_interacted_slot: AtomicI8,
    pub dirty: AtomicBool,
    pub comparator_dirty: AtomicBool,
    /// Vanilla reads the level off `BlockEntity.level`. Pumpkin only hands it to `tick`.
    world: OnceLock<Weak<World>>,
    /// Slot changed by an inventory update before `world` was known, applied by the next tick.
    pending_state_slot: AtomicI8,
}

const LAST_INTERACTED_SLOT: &str = "last_interacted_slot";

impl BlockEntity for ChiseledBookshelfBlockEntity {
    fn resource_location(&self) -> &'static str {
        Self::ID
    }

    fn get_position(&self) -> BlockPos {
        self.position
    }

    fn tick(&self, world: &Arc<World>) {
        self.world.get_or_init(|| Arc::downgrade(world));
        let slot = self.pending_state_slot.swap(-1, Ordering::Relaxed);
        if slot >= 0 {
            self.update_state_after_inventory_change(slot as usize);
        }
    }

    fn from_nbt(nbt: &NbtCompound, position: BlockPos) -> Self
    where
        Self: Sized,
    {
        let mut bookshelf = Self {
            position,
            items: RwLock::new(from_fn(|_| ItemStack::EMPTY.clone())),
            last_interacted_slot: AtomicI8::new(-1),
            dirty: AtomicBool::new(false),
            comparator_dirty: AtomicBool::new(false),
            world: OnceLock::new(),
            pending_state_slot: AtomicI8::new(-1),
        };
        pumpkin_inventory::sync_read_items_from_nbt(
            nbt,
            bookshelf
                .items
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if let Some(slot) = nbt.get_int(LAST_INTERACTED_SLOT) {
            bookshelf
                .last_interacted_slot
                .store(slot as i8, Ordering::Relaxed);
        }

        bookshelf
    }

    fn write_nbt(&self, nbt: &mut NbtCompound) {
        let items = self
            .items
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sync_write_items_to_nbt(items.as_slice(), nbt);
        nbt.put_int(
            LAST_INTERACTED_SLOT,
            i32::from(self.last_interacted_slot.load(Ordering::Relaxed)),
        );
    }

    fn get_inventory(self: Arc<Self>) -> Option<Arc<dyn Inventory>> {
        Some(self as Arc<dyn Inventory>)
    }

    fn is_comparator_dirty(&self) -> bool {
        self.comparator_dirty.load(Ordering::Relaxed)
    }

    fn clear_comparator_dirty(&self) {
        self.comparator_dirty.store(false, Ordering::Relaxed);
    }

    fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    fn clear_dirty(&self) {
        self.dirty.store(false, Ordering::Relaxed);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ChiseledBookshelfBlockEntity {
    pub const INVENTORY_SIZE: usize = 6;
    pub const ID: &'static str = "minecraft:chiseled_bookshelf";

    #[must_use]
    pub fn new(position: BlockPos) -> Self {
        Self {
            position,
            items: RwLock::new(from_fn(|_| ItemStack::EMPTY.clone())),
            last_interacted_slot: AtomicI8::new(-1),
            dirty: AtomicBool::new(false),
            comparator_dirty: AtomicBool::new(false),
            world: OnceLock::new(),
            pending_state_slot: AtomicI8::new(-1),
        }
    }

    /// Vanilla `ChiseledBookShelfBlockEntity.updateState` for the paths that only have an
    /// inventory: `setItem`/`removeItem` end in a state update (server jar :80-101).
    fn update_state_after_inventory_change(&self, slot: usize) {
        let Some(world) = self.world.get().and_then(Weak::upgrade) else {
            self.pending_state_slot.store(slot as i8, Ordering::Relaxed);
            return;
        };
        let (block, state) = world.get_block_and_state(&self.position);
        // A block entity outlives its block for a tick when another worker replaces it.
        if block.id != Block::CHISELED_BOOKSHELF.id {
            return;
        }
        self.update_state(
            ChiseledBookshelfLikeProperties::from_state_id(state.id),
            &world,
            slot,
        );
    }

    pub fn update_state(
        &self,
        mut properties: ChiseledBookshelfLikeProperties,
        world: &Arc<World>,
        slot: usize,
    ) {
        if (0..Self::INVENTORY_SIZE).contains(&slot) {
            self.last_interacted_slot
                .store(slot as i8, Ordering::Relaxed);
            self.mark_dirty();

            // Vanilla recomputes every slot, not just the interacted one (server jar :41-45).
            // Read under the lock and write the state after releasing it.
            {
                let items = self
                    .items
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                properties.slot_0_occupied = !items[0].is_empty();
                properties.slot_1_occupied = !items[1].is_empty();
                properties.slot_2_occupied = !items[2].is_empty();
                properties.slot_3_occupied = !items[3].is_empty();
                properties.slot_4_occupied = !items[4].is_empty();
                properties.slot_5_occupied = !items[5].is_empty();
            }

            world.set_block_state(
                &self.position,
                properties.to_state_id(&Block::CHISELED_BOOKSHELF),
                BlockFlags::NOTIFY_LISTENERS,
            );
        } else {
            warn!(
                "Invalid interacted slot: {} for chiseled bookshelf at position {:?}",
                slot, self.position
            );
        }
    }

    pub fn set_book(&self, slot: usize, stack: ItemStack) {
        let mut items = self
            .items
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        items[slot] = stack;
        self.mark_dirty();
    }

    pub fn remove_book(&self, slot: usize, amount: u8) -> ItemStack {
        let mut items = self
            .items
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let res = if !items[slot].is_empty() && amount > 0 {
            items[slot].split(amount)
        } else {
            ItemStack::EMPTY.clone()
        };
        self.mark_dirty();
        res
    }
}

impl Inventory for ChiseledBookshelfBlockEntity {
    fn size(&self) -> usize {
        Self::INVENTORY_SIZE
    }

    fn is_empty(&self) -> bool {
        let items = self
            .items
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        items.iter().all(ItemStack::is_empty)
    }

    fn get_stack(&self, slot: usize) -> ItemStack {
        let items = self
            .items
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        items[slot].clone()
    }

    fn remove_stack(&self, slot: usize) -> ItemStack {
        let removed = {
            let mut items = self
                .items
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::replace(&mut items[slot], ItemStack::EMPTY.clone())
        };
        self.mark_dirty();
        if !removed.is_empty() {
            self.update_state_after_inventory_change(slot);
        }
        removed
    }

    fn remove_stack_specific(&self, slot: usize, amount: u8) -> ItemStack {
        let removed = {
            let mut items = self
                .items
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !items[slot].is_empty() && amount > 0 {
                items[slot].split(amount)
            } else {
                ItemStack::EMPTY.clone()
            }
        };
        self.mark_dirty();
        if !removed.is_empty() {
            self.update_state_after_inventory_change(slot);
        }
        removed
    }

    fn set_stack(&self, slot: usize, stack: ItemStack) {
        let (was_empty, is_empty) = {
            let mut items = self
                .items
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let was_empty = items[slot].is_empty();
            let is_empty = stack.is_empty();
            items[slot] = stack;
            (was_empty, is_empty)
        };
        self.mark_dirty();
        // Vanilla `setItem` updates the state for a stored book and for clearing an occupied
        // slot, but not for an empty stack over an empty slot (server jar :91-101).
        if !is_empty || !was_empty {
            self.update_state_after_inventory_change(slot);
        }
    }

    fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Relaxed);
        self.comparator_dirty.store(true, Ordering::Relaxed);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Clearable for ChiseledBookshelfBlockEntity {
    fn clear(&self) {
        let mut items = self
            .items
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        items.fill_with(|| ItemStack::EMPTY.clone());
        self.mark_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ai::test_support::world;
    use pumpkin_data::item::Item;
    use pumpkin_util::math::vector2::Vector2;
    use pumpkin_world::chunk::ChunkData;

    fn shelf(world: &Arc<World>, position: &BlockPos) -> Arc<ChiseledBookshelfBlockEntity> {
        world
            .level
            .loaded_chunks
            .insert(Vector2::new(0, 0), ChunkData::empty_sync(0, 0));
        world.set_block_state(
            position,
            Block::CHISELED_BOOKSHELF.default_state.id,
            BlockFlags::NOTIFY_LISTENERS | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
        );
        let bookshelf = Arc::new(ChiseledBookshelfBlockEntity::new(*position));
        world.add_block_entity(bookshelf.clone());
        bookshelf
    }

    fn occupied(world: &Arc<World>, position: &BlockPos) -> ChiseledBookshelfLikeProperties {
        ChiseledBookshelfLikeProperties::from_state_id(world.get_block_state(position).id)
    }

    #[tokio::test]
    async fn inventory_change_updates_the_occupied_slot() -> Result<(), Box<dyn std::error::Error>>
    {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let position = BlockPos::new(0, 64, 0);
        let bookshelf = shelf(&world, &position);
        bookshelf.tick(&world);

        // What a hopper moving a book in and back out does (server jar :80-101).
        bookshelf.set_stack(2, ItemStack::new(1, &Item::BOOK));
        assert!(occupied(&world, &position).slot_2_occupied);
        assert!(!occupied(&world, &position).slot_0_occupied);
        assert_eq!(bookshelf.last_interacted_slot.load(Ordering::Relaxed), 2);

        bookshelf.remove_stack_specific(2, 1);
        assert!(!occupied(&world, &position).slot_2_occupied);

        world.level.shutdown().await;
        Ok(())
    }

    #[tokio::test]
    async fn inventory_change_before_the_first_tick_reaches_the_block_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let position = BlockPos::new(0, 64, 0);
        let bookshelf = shelf(&world, &position);

        // A hopper can reach a freshly loaded shelf before its block entity ticks.
        bookshelf.set_stack(5, ItemStack::new(1, &Item::BOOK));
        bookshelf.tick(&world);
        assert!(occupied(&world, &position).slot_5_occupied);

        world.level.shutdown().await;
        Ok(())
    }
}
