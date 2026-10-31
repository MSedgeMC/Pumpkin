//! Player inventory implementation.
//!
//! This module implements the player's inventory, which consists of:
//! - 36 main inventory slots (3 rows of 9 + hotbar)
//! - Equipment slots (armor + off-hand)
//!
//! The first 9 slots of the main inventory are the hotbar (accessible with number keys).
//! Slots 0-35 are the main inventory, with slot 40 being the off-hand slot.

use crate::entity_equipment::EntityEquipment;
use crate::screen_handler::InventoryPlayer;

use crate::inventory::{Clearable, Inventory};
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::java::client::play::CSetPlayerInventory;
use pumpkin_util::Hand;
use rustc_hash::FxHashMap;
use std::any::Any;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use tracing::warn;

/// The player's inventory.
///
/// Contains 36 main inventory slots (hotbar + main storage) plus
/// equipment slots accessed through [`EntityEquipment`].
pub struct PlayerInventory {
    /// The 36 main inventory slots (slots 0-35).
    ///
    /// The first 9 slots (0-8) are the hotbar, the remaining 27 (9-35) are the main storage.
    pub main_inventory: RwLock<[ItemStack; Self::MAIN_SIZE]>,
    /// Mapping of slot indices to equipment slot types.
    ///
    /// Used to identify which slots correspond to armor and off-hand equipment.
    pub equipment_slots: Arc<FxHashMap<usize, EquipmentSlot>>,
    /// The currently selected hotbar slot index (0-8).
    pub selected_slot: AtomicU8,
    /// The entity equipment storage for armor and off-hand items.
    ///
    /// This is separate from the main inventory and is rendered on the player model.
    pub entity_equipment: Arc<Mutex<EntityEquipment>>,
}

impl PlayerInventory {
    /// Size of the main inventory (36 slots: 27 storage + 9 hotbar).
    pub const MAIN_SIZE: usize = 36;
    /// Size of the hotbar (9 slots).
    pub const HOTBAR_SIZE: usize = 9;
    /// Slot index for the off-hand (40).
    pub const OFF_HAND_SLOT: usize = 40;

    /// Creates a new player inventory.
    ///
    /// # Arguments
    /// - `entity_equipment` - The entity equipment storage for armor/off-hand
    /// - `equipment_slots` - Mapping of slot indices to equipment slots
    // TODO: Add inventory load from nbt
    pub fn new(
        entity_equipment: Arc<Mutex<EntityEquipment>>,
        equipment_slots: Arc<FxHashMap<usize, EquipmentSlot>>,
    ) -> Self {
        Self {
            main_inventory: RwLock::new(std::array::from_fn(|_| ItemStack::EMPTY.clone())),
            equipment_slots,
            selected_slot: AtomicU8::new(0),
            entity_equipment,
        }
    }

    /// Fast non-blocking count of an item across all main inventory slots.
    pub fn count_item(&self, item: &'static Item) -> u32 {
        let mut total = 0u32;
        if let Ok(inv) = self.main_inventory.try_read() {
            for stack in inv.iter() {
                if stack.get_item().id == item.id {
                    total += u32::from(stack.item_count);
                }
            }
        }
        total
    }

    /// Fast non-blocking check if the main inventory contains a given item.
    pub fn contains_item(&self, item: &'static Item) -> bool {
        if let Ok(inv) = self.main_inventory.try_read() {
            for stack in inv.iter() {
                if !stack.is_empty() && stack.get_item().id == item.id {
                    return true;
                }
            }
        }
        false
    }

    /// Gets the item in the currently selected hotbar slot.
    ///
    /// This is the item the player is currently holding in their main hand.
    pub fn held_item(&self) -> ItemStack {
        let inv = self
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inv[self.get_selected_slot() as usize].clone()
    }

    /// Sets the item in the currently selected hotbar slot.
    pub fn set_held_item(&self, stack: ItemStack) {
        let selected = self.get_selected_slot() as usize;
        let mut inv = self
            .main_inventory
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inv[selected] = stack;
    }

    /// Sets the item in the specified hand.
    pub fn set_stack_in_hand(&self, hand: Hand, stack: ItemStack) {
        match hand {
            Hand::Right => self.set_held_item(stack),
            Hand::Left => {
                let Some(slot) = self.equipment_slots.get(&Self::OFF_HAND_SLOT) else {
                    return;
                };
                self.entity_equipment
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .put(slot, stack);
            }
        }
    }

    /// Gets the item in the specified hand.
    ///
    /// # Arguments
    /// - `hand` - Which hand to get the item from
    pub fn get_stack_in_hand(&self, hand: Hand) -> ItemStack {
        match hand {
            Hand::Left => self.off_hand_item(),
            Hand::Right => self.held_item(),
        }
    }

    /// Gets the item in the off-hand.
    ///
    /// Mojang name: `getOffHandStack`
    pub fn off_hand_item(&self) -> ItemStack {
        let Some(slot) = self.equipment_slots.get(&Self::OFF_HAND_SLOT) else {
            return ItemStack::EMPTY.clone();
        };
        self.entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(slot)
    }

    /// Swaps the items between main hand and off-hand.
    ///
    /// # Returns
    /// The new main hand item and new off-hand item.
    pub fn swap_item(&self) -> (ItemStack, ItemStack) {
        let Some(slot) = self.equipment_slots.get(&Self::OFF_HAND_SLOT) else {
            return (ItemStack::EMPTY.clone(), ItemStack::EMPTY.clone());
        };
        let mut equipment = self
            .entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let selected = self.get_selected_slot() as usize;
        let mut main_inv = self
            .main_inventory
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let main_hand_item = main_inv[selected].clone();
        let new_main = equipment.put(slot, main_hand_item.clone());
        main_inv[selected] = new_main.clone();
        (new_main, main_hand_item)
    }

    /// Checks if a slot index is a valid hotbar slot.
    #[must_use]
    pub const fn is_valid_hotbar_index(slot: usize) -> bool {
        slot < Self::HOTBAR_SIZE
    }

    /// Adds a stack to any available slot, prioritizing stacking with existing items.
    fn add_stack(&self, stack: ItemStack) -> usize {
        let mut slot_index = self.get_occupied_slot_with_room_for_stack(&stack);

        if slot_index == -1 {
            slot_index = self.get_empty_slot();
        }

        if slot_index == -1 {
            stack.item_count as usize
        } else {
            self.add_stack_to_slot(slot_index as usize, stack)
        }
    }

    /// Adds a stack to a specific slot.
    ///
    /// Returns the number of items that couldn't fit.
    fn add_stack_to_slot(&self, slot: usize, stack: ItemStack) -> usize {
        if slot >= Self::MAIN_SIZE {
            if let Some(slot_type) = self.equipment_slots.get(&slot) {
                let mut equipment = self
                    .entity_equipment
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut current = equipment.get(slot_type);
                if current.is_empty() {
                    let max_stack = stack.get_max_stack_size();
                    if stack.item_count <= max_stack {
                        equipment.put(slot_type, stack);
                        return 0;
                    }

                    equipment.put(slot_type, stack.copy_with_count(max_stack));

                    return (stack.item_count - max_stack) as usize;
                } else if matches!(slot_type, EquipmentSlot::OffHand(_))
                    && current.are_items_and_components_equal(&stack)
                {
                    let count_left = current
                        .get_max_stack_size()
                        .saturating_sub(current.item_count);
                    let count_min = stack.item_count.min(count_left);
                    if count_min != 0 {
                        current.increment(count_min);
                        equipment.put(slot_type, current);
                    }
                    return (stack.item_count - count_min) as usize;
                }
            }
            return stack.item_count as usize;
        }

        let mut inv = self
            .main_inventory
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stack_count = stack.item_count;
        let self_stack = &mut inv[slot];

        if self_stack.is_empty() {
            *self_stack = stack.copy_with_count(0);
        }

        let count_left = self_stack.get_max_stack_size() - self_stack.item_count;
        let count_min = stack_count.min(count_left);

        if count_min != 0 {
            stack_count -= count_min;
            self_stack.increment(count_min);
        }
        stack_count as usize
    }

    /// Finds an empty slot in the inventory.
    ///
    /// # Returns
    /// The slot index or -1 if inventory is full.
    fn get_empty_slot(&self) -> i16 {
        let inv = self
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (i, stack) in inv.iter().enumerate() {
            if stack.is_empty() {
                return i as i16;
            }
        }
        -1
    }

    /// Checks if a stack can be added to an existing stack.
    fn can_stack_add_more(existing_stack: &ItemStack, stack: &ItemStack) -> bool {
        !existing_stack.is_empty()
            && existing_stack.are_items_and_components_equal(stack)
            && existing_stack.is_stackable()
            && existing_stack.item_count < existing_stack.get_max_stack_size()
    }

    /// Finds a slot with the same item type that has room for more items.
    ///
    /// Checks selected slot, off-hand, then other slots.
    fn get_occupied_slot_with_room_for_stack(&self, stack: &ItemStack) -> i16 {
        let selected = self.get_selected_slot() as usize;
        let inv = self
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if Self::can_stack_add_more(&inv[selected], stack) {
            return selected as i16;
        }

        let off_hand = self.off_hand_item();
        if Self::can_stack_add_more(&off_hand, stack) {
            return Self::OFF_HAND_SLOT as i16;
        }

        for (i, item) in inv.iter().enumerate() {
            if Self::can_stack_add_more(item, stack) {
                return i as i16;
            }
        }

        -1
    }

    /// Inserts a stack into any available slot.
    ///
    /// # Arguments
    /// - `stack` - The stack to insert (modified in place)
    ///
    /// # Returns
    /// `true` if any items were inserted, `false` otherwise.
    pub fn insert_stack_anywhere(&self, stack: &mut ItemStack) -> bool {
        self.insert_stack(-1, stack)
    }

    /// Inserts a stack into a specific slot or any slot.
    ///
    /// # Arguments
    /// - `slot` - The slot index, or -1 for any slot
    /// - `stack` - The stack to insert (modified in place)
    ///
    /// # Returns
    /// `true` if any items were inserted, `false` otherwise.
    pub fn insert_stack(&self, slot: i16, stack: &mut ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }

        let mut i;
        let start_count = stack.item_count;

        loop {
            i = stack.item_count;
            if slot == -1 {
                stack.set_count(self.add_stack(stack.clone()) as u8);
            } else {
                stack.set_count(self.add_stack_to_slot(slot as usize, stack.clone()) as u8);
            }

            if stack.is_empty() || stack.item_count >= i {
                break;
            }
        }

        stack.item_count < start_count
    }

    /// Finds the first slot containing a matching stack.
    ///
    /// # Returns
    /// The slot index or -1 if not found.
    pub fn get_slot_with_stack(&self, stack: &ItemStack) -> i16 {
        let inv = self
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (i, item) in inv.iter().enumerate() {
            if !item.is_empty() && item.are_items_and_components_equal(stack) {
                return i as i16;
            }
        }
        -1
    }

    /// Finds an empty hotbar slot to swap an item to.
    ///
    /// Only the first `max_hotbar_slots` hotbar slots are considered. Vanilla always
    /// passes all nine; the Bedrock `BlockPickRequest` carries the limit in `max_slots`.
    ///
    /// First looks for empty slots, then slots without enchantments.
    fn get_swappable_hotbar_slot(&self, max_hotbar_slots: usize) -> usize {
        let hotbar_slots = max_hotbar_slots.clamp(1, Self::HOTBAR_SIZE);
        let inv = self
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let selected_slot = self.get_selected_slot() as usize;
        for i in 0..hotbar_slots {
            let check_index = (i + selected_slot) % hotbar_slots;
            if inv[check_index].is_empty() {
                return check_index;
            }
        }

        selected_slot % hotbar_slots
    }

    /// Swaps an item stack with an item on the hotbar.
    ///
    /// Finds an empty hotbar slot out of the first `max_hotbar_slots` hotbar slots and
    /// places the stack there.
    pub fn swap_stack_with_hotbar(&self, stack: ItemStack, max_hotbar_slots: usize) {
        let swappable = self.get_swappable_hotbar_slot(max_hotbar_slots);
        self.set_selected_slot(swappable as u8);
        let selected = self.get_selected_slot() as usize;
        let mut inv = self
            .main_inventory
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(empty_slot) = inv.iter().position(ItemStack::is_empty)
            && !inv[selected].is_empty()
        {
            inv[empty_slot] = inv[selected].clone();
        }

        inv[selected] = stack;
    }

    /// Swaps the items at slot `slot` and a hotbar slot out of the first
    /// `max_hotbar_slots` hotbar slots.
    pub fn swap_slot_with_hotbar(&self, slot: usize, max_hotbar_slots: usize) {
        let swappable = self.get_swappable_hotbar_slot(max_hotbar_slots);
        self.set_selected_slot(swappable as u8);
        let selected = self.get_selected_slot() as usize;
        let mut inv = self
            .main_inventory
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inv.swap(selected, slot);
    }

    /// Picks a stack into the hotbar, mirroring vanilla
    /// `ServerGamePacketListenerImpl#tryPickItem`.
    ///
    /// A matching stack already in a hotbar slot gets that slot selected, a matching
    /// stack elsewhere in the main inventory is swapped into the hotbar, and otherwise
    /// the stack is added to the hotbar when `add_if_missing` is set (creative).
    ///
    /// `max_hotbar_slots` is the number of hotbar slots the server may fill, not a slot
    /// index. Vanilla always allows [`Self::HOTBAR_SIZE`]; the Bedrock
    /// `BlockPickRequest` sends it in `max_slots` and the official client sends `9`.
    ///
    /// Returns `true` if the selection changed or the inventory was modified.
    pub fn pick_item_into_hotbar(
        &self,
        stack: ItemStack,
        add_if_missing: bool,
        max_hotbar_slots: usize,
    ) -> bool {
        let slot_with_stack = self.get_slot_with_stack(&stack);

        if slot_with_stack != -1 {
            if Self::is_valid_hotbar_index(slot_with_stack as usize) {
                self.set_selected_slot(slot_with_stack as u8);
            } else {
                self.swap_slot_with_hotbar(slot_with_stack as usize, max_hotbar_slots);
            }
            true
        } else if add_if_missing {
            self.swap_stack_with_hotbar(stack, max_hotbar_slots);
            true
        } else {
            false
        }
    }

    /// Gets the item in the specified slot (synchronously).
    pub fn get_slot(&self, slot: usize) -> ItemStack {
        if slot < Self::MAIN_SIZE {
            let inv = self
                .main_inventory
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inv[slot].clone()
        } else if let Some(slot_type) = self.equipment_slots.get(&slot) {
            self.entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(slot_type)
        } else {
            ItemStack::EMPTY.clone()
        }
    }

    /// Sets the item in the specified slot (synchronously).
    pub fn set_slot(&self, slot: usize, stack: ItemStack) {
        if slot < Self::MAIN_SIZE {
            let mut inv = self
                .main_inventory
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inv[slot] = stack;
        } else if let Some(slot_type) = self.equipment_slots.get(&slot) {
            self.entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .put(slot_type, stack);
        }
    }

    /// Gives a stack to the player or drops it if inventory is full.
    pub fn offer_or_drop_stack(&self, stack: ItemStack, player: &dyn InventoryPlayer) {
        self.offer(stack, true, player);
    }

    /// Gives a stack to the player, optionally notifying the client.
    ///
    /// # Arguments
    /// - `stack` - The stack to give
    /// - `notify_client` - Whether to send inventory update packets
    /// - `player` - The player to give the stack to
    pub fn offer(&self, stack: ItemStack, notify_client: bool, player: &dyn InventoryPlayer) {
        let mut stack = stack;
        while !stack.is_empty() {
            let mut room_for_stack = self.get_occupied_slot_with_room_for_stack(&stack);
            if room_for_stack == -1 {
                room_for_stack = self.get_empty_slot();
            }

            if room_for_stack == -1 {
                player.drop_item(stack, false);
                break;
            }

            let items_fit =
                stack.get_max_stack_size() - self.get_stack(room_for_stack as usize).item_count;
            if self.insert_stack(room_for_stack, &mut stack.split(items_fit)) && notify_client {
                player.enqueue_slot_set_packet(&CSetPlayerInventory::new(
                    i32::from(room_for_stack).into(),
                    &self.get_stack(room_for_stack as usize).into(),
                ));
            }
        }
    }
}

impl Clearable for PlayerInventory {
    fn clear(&self) {
        let mut inv = self
            .main_inventory
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inv.fill_with(|| ItemStack::EMPTY.clone());
        self.entity_equipment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

impl Inventory for PlayerInventory {
    fn size(&self) -> usize {
        Self::MAIN_SIZE + self.equipment_slots.len()
    }

    fn is_empty(&self) -> bool {
        let inv = self
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inv.iter().any(|s| !s.is_empty()) {
            return false;
        }

        for slot in self.equipment_slots.values() {
            let eq_item = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(slot);
            if !eq_item.is_empty() {
                return false;
            }
        }

        true
    }

    fn get_stack(&self, slot: usize) -> ItemStack {
        if slot < Self::MAIN_SIZE {
            let inv = self
                .main_inventory
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inv[slot].clone()
        } else if let Some(slot) = self.equipment_slots.get(&slot) {
            self.entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(slot)
        } else {
            ItemStack::EMPTY.clone()
        }
    }

    fn remove_stack(&self, slot: usize) -> ItemStack {
        if slot < Self::MAIN_SIZE {
            let mut inv = self
                .main_inventory
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::replace(&mut inv[slot], ItemStack::EMPTY.clone())
        } else if let Some(slot) = self.equipment_slots.get(&slot) {
            self.entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .put(slot, ItemStack::EMPTY.clone())
        } else {
            ItemStack::EMPTY.clone()
        }
    }

    fn remove_stack_specific(&self, slot: usize, amount: u8) -> ItemStack {
        if slot < Self::MAIN_SIZE {
            let mut inv = self
                .main_inventory
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !inv[slot].is_empty() && amount > 0 {
                inv[slot].split(amount)
            } else {
                ItemStack::EMPTY.clone()
            }
        } else if let Some(slot) = self.equipment_slots.get(&slot) {
            let mut equipment = self
                .entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut stack = equipment.get(slot);

            if !stack.is_empty() && amount > 0 {
                let split = stack.split(amount);
                equipment.put(slot, stack);
                split
            } else {
                ItemStack::EMPTY.clone()
            }
        } else {
            ItemStack::EMPTY.clone()
        }
    }

    fn set_stack(&self, slot: usize, stack: ItemStack) {
        if slot < Self::MAIN_SIZE {
            let mut inv = self
                .main_inventory
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inv[slot] = stack;
        } else if let Some(slot) = self.equipment_slots.get(&slot) {
            self.entity_equipment
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .put(slot, stack);
        } else {
            warn!("Failed to get Equipment Slot at {slot}");
        }
    }

    fn mark_dirty(&self) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl PlayerInventory {
    /// Sets the selected hotbar slot.
    ///
    /// # Panics
    /// Panics if the slot index is not a valid hotbar index.
    pub fn set_selected_slot(&self, slot: u8) {
        if Self::is_valid_hotbar_index(slot as usize) {
            self.selected_slot.store(slot, Ordering::Relaxed);
        }
    }

    /// Gets the currently selected hotbar slot index.
    pub fn get_selected_slot(&self) -> u8 {
        self.selected_slot.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_equipment_slots;
    use pumpkin_data::item::Item;

    fn create_test_inventory() -> PlayerInventory {
        let equipment_slots = Arc::new(build_equipment_slots());
        let entity_equipment = Arc::new(Mutex::new(EntityEquipment::new()));
        PlayerInventory::new(entity_equipment, equipment_slots)
    }

    #[test]
    fn add_stack_to_slot_offhand_when_empty() {
        let inv = create_test_inventory();
        let stack = ItemStack::new(10, &Item::ARROW);

        let leftover = inv.add_stack_to_slot(PlayerInventory::OFF_HAND_SLOT, stack);
        assert_eq!(leftover, 0);

        let off_hand = inv.off_hand_item();
        assert_eq!(off_hand.item.id, Item::ARROW.id);
        assert_eq!(off_hand.item_count, 10);
    }

    #[test]
    fn add_stack_to_slot_offhand_stacks_matching_item() {
        let inv = create_test_inventory();
        inv.set_stack_in_hand(Hand::Left, ItemStack::new(10, &Item::ARROW));

        let leftover = inv.add_stack_to_slot(
            PlayerInventory::OFF_HAND_SLOT,
            ItemStack::new(15, &Item::ARROW),
        );
        assert_eq!(leftover, 0);

        let off_hand = inv.off_hand_item();
        assert_eq!(off_hand.item.id, Item::ARROW.id);
        assert_eq!(off_hand.item_count, 25);
    }

    #[test]
    fn add_stack_to_slot_offhand_stacks_up_to_max() {
        let inv = create_test_inventory();
        inv.set_stack_in_hand(Hand::Left, ItemStack::new(60, &Item::ARROW));

        let leftover = inv.add_stack_to_slot(
            PlayerInventory::OFF_HAND_SLOT,
            ItemStack::new(10, &Item::ARROW),
        );
        assert_eq!(leftover, 6);

        let off_hand = inv.off_hand_item();
        assert_eq!(off_hand.item.id, Item::ARROW.id);
        assert_eq!(off_hand.item_count, 64);
    }

    #[test]
    fn add_stack_to_slot_offhand_rejects_different_item() {
        let inv = create_test_inventory();
        inv.set_stack_in_hand(Hand::Left, ItemStack::new(1, &Item::SHIELD));

        let leftover = inv.add_stack_to_slot(
            PlayerInventory::OFF_HAND_SLOT,
            ItemStack::new(10, &Item::ARROW),
        );
        assert_eq!(leftover, 10);

        let off_hand = inv.off_hand_item();
        assert_eq!(off_hand.item.id, Item::SHIELD.id);
        assert_eq!(off_hand.item_count, 1);
    }

    #[test]
    fn add_stack_to_slot_armor_does_not_stack() {
        let inv = create_test_inventory();
        let feet_slot = EquipmentSlot::FEET
            .get_offset_entity_slot_id(PlayerInventory::MAIN_SIZE as i32)
            as usize;
        let boots = ItemStack::new(1, &Item::IRON_BOOTS);

        let leftover = inv.add_stack_to_slot(feet_slot, boots.clone());
        assert_eq!(leftover, 0);

        let leftover_second = inv.add_stack_to_slot(feet_slot, boots);
        assert_eq!(leftover_second, 1);
    }

    #[test]
    fn pickup_stacks_into_offhand() {
        let inv = create_test_inventory();
        inv.set_stack_in_hand(Hand::Left, ItemStack::new(10, &Item::ARROW));

        let mut pickup = ItemStack::new(5, &Item::ARROW);
        let inserted = inv.insert_stack_anywhere(&mut pickup);

        assert!(inserted);
        assert!(pickup.is_empty());
        assert_eq!(inv.off_hand_item().item_count, 15);
    }

    #[test]
    fn pickup_prioritizes_held_main_hand_over_offhand() {
        let inv = create_test_inventory();
        inv.set_held_item(ItemStack::new(60, &Item::ARROW));
        inv.set_stack_in_hand(Hand::Left, ItemStack::new(10, &Item::ARROW));

        let mut pickup = ItemStack::new(10, &Item::ARROW);
        let inserted = inv.insert_stack_anywhere(&mut pickup);

        assert!(inserted);
        assert!(pickup.is_empty());
        assert_eq!(inv.held_item().item_count, 64);
        assert_eq!(inv.off_hand_item().item_count, 16);
    }

    #[test]
    fn pickup_offhand_full_overflows_to_main_inventory() {
        let inv = create_test_inventory();
        inv.set_stack_in_hand(Hand::Left, ItemStack::new(64, &Item::ARROW));

        let mut pickup = ItemStack::new(10, &Item::ARROW);
        let inserted = inv.insert_stack_anywhere(&mut pickup);

        assert!(inserted);
        assert!(pickup.is_empty());
        assert_eq!(inv.off_hand_item().item_count, 64);
        assert_eq!(inv.get_slot(0).item.id, Item::ARROW.id);
        assert_eq!(inv.get_slot(0).item_count, 10);
    }

    #[test]
    fn pickup_does_not_fill_empty_offhand() {
        let inv = create_test_inventory();

        let mut pickup = ItemStack::new(10, &Item::ARROW);
        let inserted = inv.insert_stack_anywhere(&mut pickup);

        assert!(inserted);
        assert!(pickup.is_empty());
        assert!(inv.off_hand_item().is_empty());
        assert_eq!(inv.get_slot(0).item.id, Item::ARROW.id);
        assert_eq!(inv.get_slot(0).item_count, 10);
    }

    /// When the main inventory is completely full and only the off-hand has
    /// partial room, `insert_stack` must still return `true` even though the
    /// second loop iteration makes no progress.
    #[test]
    fn insert_stack_returns_true_on_partial_offhand_with_full_main_inventory() {
        let inv = create_test_inventory();
        for i in 0..PlayerInventory::MAIN_SIZE {
            inv.set_slot(i, ItemStack::new(64, &Item::DIRT));
        }
        inv.set_stack_in_hand(Hand::Left, ItemStack::new(60, &Item::ARROW));

        let mut pickup = ItemStack::new(10, &Item::ARROW);
        let inserted = inv.insert_stack_anywhere(&mut pickup);

        assert!(
            inserted,
            "insert_stack must return true when some items were placed"
        );
        assert_eq!(
            pickup.item_count, 6,
            "6 arrows should remain after filling off-hand to 64"
        );
        assert_eq!(inv.off_hand_item().item_count, 64);
    }

    /// The Bedrock `BlockPickRequest` sends the number of usable hotbar slots in
    /// `max_slots` (the official client always sends 9), not a hotbar slot index, so a
    /// limit of 9 has to pick the item instead of being rejected.
    #[test]
    fn pick_item_into_hotbar_accepts_max_slots_of_nine() {
        let inv = create_test_inventory();

        let picked = inv.pick_item_into_hotbar(
            ItemStack::new(1, &Item::DIRT),
            true,
            PlayerInventory::HOTBAR_SIZE,
        );

        assert!(picked);
        assert_eq!(inv.get_selected_slot(), 0);
        assert_eq!(inv.get_slot(0).item.id, Item::DIRT.id);
    }

    #[test]
    fn pick_item_into_hotbar_selects_slot_of_existing_stack() {
        let inv = create_test_inventory();
        inv.set_slot(4, ItemStack::new(1, &Item::DIRT));

        let picked = inv.pick_item_into_hotbar(ItemStack::new(1, &Item::DIRT), true, 9);

        assert!(picked);
        assert_eq!(inv.get_selected_slot(), 4);
    }

    #[test]
    fn pick_item_into_hotbar_swaps_stack_out_of_main_inventory() {
        let inv = create_test_inventory();
        inv.set_slot(20, ItemStack::new(1, &Item::DIRT));

        let picked = inv.pick_item_into_hotbar(ItemStack::new(1, &Item::DIRT), true, 9);

        assert!(picked);
        assert_eq!(inv.get_selected_slot(), 0);
        assert_eq!(inv.get_slot(0).item.id, Item::DIRT.id);
        assert!(inv.get_slot(20).is_empty());
    }

    #[test]
    fn pick_item_into_hotbar_does_nothing_without_creative_or_existing_stack() {
        let inv = create_test_inventory();

        let picked = inv.pick_item_into_hotbar(ItemStack::new(1, &Item::DIRT), false, 9);

        assert!(!picked);
        assert!(inv.get_slot(0).is_empty());
    }

    /// Only the first `max_slots` hotbar slots may be filled.
    #[test]
    fn get_swappable_hotbar_slot_stays_within_max_slots() {
        let inv = create_test_inventory();
        for i in 0..3 {
            inv.set_slot(i, ItemStack::new(1, &Item::ARROW));
        }

        assert_eq!(inv.get_swappable_hotbar_slot(3), 0);
        assert_eq!(
            inv.get_swappable_hotbar_slot(PlayerInventory::HOTBAR_SIZE),
            3
        );
    }
}
