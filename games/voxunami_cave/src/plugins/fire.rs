//! Fire: it burns near players, spreads to what burns, and goes out.

use std::sync::{Arc, Mutex, MutexGuard};

use game_shell::GameContent;
use namespaced_id::NamespacedId;
use player_presence::{SharedPoseBook, lock};
use plugin_host::{HandlerSpec, Phase, Plugin, Registrar, SaveScope, SystemSpec};
use server::GridView;

use super::adapters::{CaveChannels, WorldFire};
use crate::content::CaveContent;
use crate::fire::Fire;
use crate::logic::FIRE_RANGE;

/// The fire rule. A fire is looked at when a block is changed into one (by a player or by anything
/// else) and from then on by itself; fires farther than [`FIRE_RANGE`] from every player in their level
/// are frozen.
#[derive(Debug)]
pub struct FirePlugin {
    channels: CaveChannels,
    table: block_models::BlockTable,
    fire_block: u32,
    air_block: u32,
    players: SharedPoseBook,
}

impl FirePlugin {
    pub fn new(content: &CaveContent, players: SharedPoseBook) -> Self {
        Self {
            channels: CaveChannels::of(content),
            table: content.block_table(),
            fire_block: content.blocks.fire,
            air_block: content.blocks.air,
            players,
        }
    }
}

fn locked(fire: &Mutex<Fire>) -> MutexGuard<'_, Fire> {
    fire.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Plugin for FirePlugin {
    fn id(&self) -> NamespacedId {
        "voxunami_cave:fire"
            .parse()
            .expect("constant identifier is valid")
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        vec![
            "player_presence:players"
                .parse()
                .expect("constant identifier is valid"),
        ]
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        let world = registrar.world();
        let level_ids: Vec<NamespacedId> = world.levels().map(|(_, id, _)| id.clone()).collect();
        let (clock, active) =
            Fire::read_section(world.opaque_sections(), |level| world.level_index(level))
                .unwrap_or_default();
        let fire = Arc::new(Mutex::new(Fire::new(
            self.table.clone(),
            self.fire_block,
            self.air_block,
            world.world_seed,
            clock,
            active,
        )));

        // A block changed into fire is looked at from the next tick.
        let (watcher, fire_block, block_channel) =
            (Arc::clone(&fire), self.fire_block, self.channels.block);
        registrar.on_channels(
            HandlerSpec::new("voxunami_cave:fire_notices_new_fires"),
            &[block_channel],
            move |event, context| {
                let Ok(mut grid) = context.grid(event.grid) else {
                    return;
                };
                let mut fire = locked(&watcher);
                for &voxel in &event.voxels {
                    if grid.get(block_channel, voxel) == Ok(fire_block) {
                        fire.ignite(event.grid, voxel);
                    }
                }
            },
        );

        // Fires near a player burn; what they change is looked at by the light in the same tick.
        let (burner, players, channels) = (
            Arc::clone(&fire),
            Arc::clone(&self.players),
            self.channels.clone(),
        );
        registrar.system(
            SystemSpec::new("voxunami_cave:burn", Phase::Rules)
                .writes(&[self.channels.block, self.channels.state]),
            move |context| {
                let book = lock(&players);
                let _ = locked(&burner).step(
                    &mut WorldFire {
                        context,
                        channels: &channels,
                    },
                    |grid, at| {
                        book.players().any(|(_, pose, player_level)| {
                            Some(player_level) == grid.root_level()
                                && pose.distance_to_voxel(at) <= FIRE_RANGE
                        })
                    },
                );
            },
        );

        registrar.save_section(SaveScope::PerGrid { movable: true }, move || {
            vec![locked(&fire).to_section(&level_ids)]
        });
    }
}
