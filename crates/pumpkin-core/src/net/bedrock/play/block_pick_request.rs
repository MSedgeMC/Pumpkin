#[allow(clippy::wildcard_imports)]
use super::*;

impl BedrockClient {
    pub fn handle_block_pick_request(&self, player: &Arc<Player>, packet: &SBlockPickRequest) {
        if !player.can_interact_with_block_at(&packet.position, 1.0) {
            return;
        }

        let world = player.world();
        let block = world.get_block(&packet.position);

        if block.item_id == 0 {
            return;
        }

        let Some(item) = pumpkin_data::item::Item::from_id(block.item_id) else {
            return;
        };
        let stack = ItemStack::new(1, item);

        // Same pick as the Java `ServerboundPickItemFromBlockPacket` handler. `max_slots`
        // is the number of hotbar slots the server may fill, not a slot index, and the
        // official client always sends 9. Block entity NBT (`with_data`) is not copied,
        // matching the Java path.
        let picked = player.inventory.pick_item_into_hotbar(
            stack,
            player.gamemode.load() == GameMode::Creative,
            packet.max_slots as usize,
        );
        if !picked {
            return;
        }

        // Send hotbar updates
        player.client.try_enqueue_packet_editioned(
            &CSetSelectedSlot::new(player.inventory.get_selected_slot() as i8),
            &CPlayerHotbar {
                selected_slot: VarUInt(player.inventory.get_selected_slot() as u32),
                container_id: 0,
                should_select_slot: true,
            },
        );

        // Send screen handler / Java inventory updates
        player
            .player_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send_content_updates();

        // Sync main hand equipment to other players
        let stack_in_hand = player.inventory().held_item();
        let equipment = &[(EquipmentSlot::MAIN_HAND, stack_in_hand)];
        player.living_entity.send_equipment_changes(equipment);

        // Sync bedrock inventory updates
        let slots = player
            .inventory()
            .main_inventory
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(NetworkItemStackDescriptor::from)
            .collect();
        self.try_enqueue_client_packet(&CInventoryContent {
            container_id: VarUInt(0),
            slots,
            full_container_name: FullContainerName {
                container_name: ContainerName::Inventory,
                dynamic_id: None,
            },
            storage_item: NetworkItemStackDescriptor::default(),
        });
    }
}
