use std::cell::OnceCell;
use std::sync::Arc;

use pumpkin_data::game_event::GameEvent;
use pumpkin_util::math::{
    boundingbox::BoundingBox,
    position::{BlockPos, chunk_section_from_pos},
    vector3::Vector3,
};
use pumpkin_world::generation::section_coords::section_to_block;

use super::{World, entity_islands};
use crate::entity::EntityBase;
use crate::plugin::api::events::block::block_receive_game::BlockReceiveGameEvent;

pub struct GameEventContext<'a> {
    pub source_entity: Option<&'a dyn EntityBase>,
}

pub trait GameEventListener {
    fn listener_position(&self) -> Vector3<f64>;

    fn listener_radius(&self) -> i32;

    /// Whether this listener takes `event` right now.
    ///
    /// Vanilla `VibrationSystem.User.isValidVibration` (`VibrationSystem.java:219-220`) and
    /// the pending-vibration check before it (`VibrationSystem.java:217-218`). Bukkit fires
    /// `BlockReceiveGameEvent` from `VibrationSystem$Listener.handleGameEvent`, which is only
    /// reached once both pass, so the dispatcher fires the plugin event only for listeners
    /// that answer yes. It does not gate `handle_game_event`: a listener that answers no
    /// still handles the event, it just never produces the plugin event.
    fn accepts_game_event(&self, event: GameEvent, context: &GameEventContext<'_>) -> bool;

    fn handle_game_event(
        &self,
        world: &World,
        event: GameEvent,
        context: &GameEventContext<'_>,
        source_position: Vector3<f64>,
    ) -> bool;
}

fn notified_sections(center: BlockPos, radius: i32) -> (Vector3<i32>, Vector3<i32>) {
    (
        chunk_section_from_pos(&center.offset(Vector3::new(-radius, -radius, -radius))),
        chunk_section_from_pos(&center.offset(Vector3::new(radius, radius, radius))),
    )
}

/// The chunk sections whose listeners an event at `position` can notify.
#[must_use]
pub fn notified_area(position: Vector3<f64>, radius: i32) -> BoundingBox {
    let (section_min, section_max) = notified_sections(BlockPos::floored_v(position), radius);
    let block_min = |section: i32| f64::from(section_to_block(section));
    BoundingBox::new(
        Vector3::new(
            block_min(section_min.x),
            block_min(section_min.y),
            block_min(section_min.z),
        ),
        Vector3::new(
            block_min(section_max.x + 1),
            block_min(section_max.y + 1),
            block_min(section_max.z + 1),
        ),
    )
}

impl World {
    pub(super) fn post_game_event(
        &self,
        event: GameEvent,
        position: Vector3<f64>,
        context: &GameEventContext<'_>,
    ) {
        let radius = event.notification_radius();
        let center = BlockPos::floored_v(position);
        let (section_min, section_max) = notified_sections(center, radius);
        let area = notified_area(position, radius);
        // The plugin event owns its source entity while the dispatcher only borrows it.
        // Resolve it at most once, and only when a plugin listens: the lookup scans every
        // entity in the world.
        let source_entity: OnceCell<Option<Arc<dyn EntityBase>>> = OnceCell::new();
        for entity in self.entity_index.query(self.entities.load_full(), area) {
            let Some(mob) = entity.get_mob() else {
                continue;
            };
            if !self
                .level
                .is_chunk_loaded(&entity.get_entity().chunk_pos.load())
            {
                continue;
            }
            mob.for_each_game_event_listener(&mut |listener| {
                let listener_block = BlockPos::floored_v(listener.listener_position());
                let section = chunk_section_from_pos(&listener_block);
                let listener_radius = listener.listener_radius();
                if section.x < section_min.x
                    || section.y < section_min.y
                    || section.z < section_min.z
                    || section.x > section_max.x
                    || section.y > section_max.y
                    || section.z > section_max.z
                    || listener_block.squared_distance(&center) > listener_radius * listener_radius
                {
                    return;
                }
                entity_islands::check_reached(entity.get_entity().entity_id);
                // Bukkit fires this from `VibrationSystem$Listener.handleGameEvent`, once per
                // listener the event reaches, for the block the listener sits in. A listener
                // that answers no still handles the event below, it just produces no plugin
                // event: the allay's jukebox listener is not a `VibrationSystem.Listener`.
                if listener.accepts_game_event(event, context)
                    && let Some(server) = self.server.upgrade()
                    && server
                        .plugin_manager
                        .has_handlers::<BlockReceiveGameEvent>()
                {
                    let mut plugin_event = BlockReceiveGameEvent::new(
                        listener_block,
                        // The event owns an `Arc<World>` while the dispatcher only has
                        // `&self`; the listener entity holds the same allocation.
                        entity.get_entity().world.load_full(),
                        event.name().to_string(),
                        source_entity
                            .get_or_init(|| {
                                context.source_entity.and_then(|source| {
                                    self.get_entity_by_id(source.get_entity().entity_id)
                                })
                            })
                            .clone(),
                    );
                    server
                        .plugin_manager
                        .fire_blocking(&server, &mut plugin_event);
                    if plugin_event.cancelled {
                        return;
                    }
                }
                listener.handle_game_event(self, event, context, position);
            });
        }
    }
}
