//! Items on the ground and in inventories, and the game mode that decides whether they matter.
//!
//! In creative nothing here does anything. In survival: a block a player breaks leaves its item on the ground
//! where it was ([`BlockBroken`]); items fall, join others they land with, and come into the inventory of a
//! player who gets close; and a block can be placed only by a player who has the item, which is used up
//! ([`BlockPlaced`]).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

use block_models::{BlockTable, PropertyHandle};
use entities::{ComponentBag, Inventory, ItemStack};
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use player_presence::{SharedPresence, lock};
use plugin_host::{
    Argument, CommandSpec, Context, HandlerSpec, Phase, PlayerJoined, Plugin, Registrar, SaveScope,
    SystemSpec, Verdict,
};
use protocol::{ClientId, EntityId, Message};
use server::{Audience, GridView};
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate};

use super::block_editing::{BlockBroken, BlockEditRequested, BlockPlaced};
use crate::aboard::{Extent, in_voxels};
use crate::block_properties;
use crate::content::CaveContent;
use crate::items::{
    GameMode, LOWEST_DROP_DISTANCE, LOWEST_DROP_VOXEL, MOST_GRAVITY_THAT_IS_NONE, PICKUP_RADIUS,
    STACK_SIZE, centre_of, creative_only_blocks, drop_pose, dropped_item_kind,
    dropped_item_kind_spec, falls_this_tick, game_mode_section, item_for_block, item_specs,
    placeable_blocks, read_game_mode, voxel_of,
};
use crate::messages::GameModeIs;

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The first slot of an inventory that holds this item.
fn slot_holding(inventory: &Inventory, item: &NamespacedId) -> Option<usize> {
    inventory
        .slots()
        .iter()
        .position(|slot| slot.as_ref().is_some_and(|stack| stack.item == *item))
}

#[derive(Debug)]
pub struct ItemsPlugin {
    /// The identifier of each block, by its number in this game's registry.
    block_names: HashMap<u32, NamespacedId>,
    /// The blocks that can be placed, and so have items.
    placeable: Arc<Vec<NamespacedId>>,
    /// The blocks that exist only in creative: breaking one leaves no item.
    creative_only: Arc<Vec<NamespacedId>>,
    air: u32,
    table: BlockTable,
    passable: PropertyHandle<bool>,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    orientation_channel: ChannelIndex,
    players: SharedPresence,
}

impl ItemsPlugin {
    pub fn new(content: &CaveContent, players: SharedPresence) -> Self {
        let registries = content.registries();
        let blocks = registries[0].1;
        let block_names: HashMap<u32, NamespacedId> = (0..blocks.entry_count() as u32)
            .filter_map(|raw| Some((raw, blocks.identifier_of_raw(raw)?.clone())))
            .collect();
        let table = content.block_table();
        Self {
            air: content.blocks.air,
            passable: block_properties::passable(&table),
            table,
            placeable: Arc::new(placeable_blocks(&content.package_data())),
            creative_only: Arc::new(creative_only_blocks(&content.package_data())),
            block_names,
            block_channel: content.block_channel(),
            state_channel: content.state_channel(),
            orientation_channel: content.orientation_channel(),
            players,
        }
    }
}

/// What the rules of the plugin need to know, shared by its handlers and its system.
#[derive(Debug)]
struct Rules {
    mode: GameMode,
}

impl Plugin for ItemsPlugin {
    fn id(&self) -> NamespacedId {
        "voxunami_cave:items"
            .parse()
            .expect("constant identifier is valid")
    }

    fn depends_on(&self) -> Vec<NamespacedId> {
        ["player_presence:players", "voxunami_cave:block_editing"]
            .into_iter()
            .map(|id| id.parse().expect("constant identifier is valid"))
            .collect()
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        for spec in item_specs(&self.placeable) {
            registrar.item(spec);
        }
        let default_item = self.placeable.first().cloned().unwrap_or_else(|| {
            "base_blocks:stone"
                .parse()
                .expect("constant identifier is valid")
        });
        registrar.entity_kind(dropped_item_kind_spec(&default_item));
        registrar.declares_message::<GameModeIs>();
        let mode = read_game_mode(registrar.world().opaque_sections()).unwrap_or_default();
        let rules = Arc::new(Mutex::new(Rules { mode }));

        // /gamemode [creative|survival]
        let shared = Arc::clone(&rules);
        registrar.command(
            CommandSpec::new(
                "gamemode",
                "which game mode this world is in; /gamemode creative; /gamemode survival",
            )
            .argument(Argument::rest("mode").optional()),
            move |context, client, arguments| {
                let mut rules = locked(&shared);
                let reply = match arguments.rest(0) {
                    [] => format!("this world is in {} mode", rules.mode.name()),
                    [wanted] => match GameMode::parse(wanted) {
                        Some(mode) => {
                            rules.mode = mode;
                            context.world().send(
                                Audience::Everyone,
                                GameModeIs::kind(),
                                GameModeIs(mode).encode(),
                            );
                            format!("this world is now in {} mode", mode.name())
                        }
                        None => "usage: /gamemode [creative|survival]".to_owned(),
                    },
                    _ => "usage: /gamemode [creative|survival]".to_owned(),
                };
                context.reply(client, &reply);
            },
        );
        // A player who joins is told the mode.
        let shared = Arc::clone(&rules);
        registrar.on::<PlayerJoined>(
            HandlerSpec::new("voxunami_cave:tell_the_newcomer_the_game_mode"),
            move |event, context| {
                context.world().send(
                    Audience::Client(event.client),
                    GameModeIs::kind(),
                    GameModeIs(locked(&shared).mode).encode(),
                );
            },
        );
        let shared = Arc::clone(&rules);
        registrar.save_section(SaveScope::Global, move || {
            // A world in the mode every world starts in saves nothing.
            match locked(&shared).mode {
                GameMode::Creative => Vec::new(),
                mode => vec![game_mode_section(mode)],
            }
        });

        // In survival, a block is placed only by a player who has the item.
        let (shared, players, names, placeable, air) = (
            Arc::clone(&rules),
            Arc::clone(&self.players),
            self.block_names.clone(),
            Arc::clone(&self.placeable),
            self.air,
        );
        registrar.on_ask(
            HandlerSpec::new("voxunami_cave:placing_needs_the_item"),
            move |event: &BlockEditRequested, context| {
                if locked(&shared).mode != GameMode::Survival || event.block == air {
                    return Verdict::Allow;
                }
                let Some(item) = names
                    .get(&event.block)
                    .and_then(|block| item_for_block(block, &placeable))
                else {
                    return Verdict::Allow;
                };
                let held = lock(&players)
                    .entity_of(event.client)
                    .and_then(|id| context.component::<Inventory>(id).ok().flatten())
                    .is_some_and(|inventory| slot_holding(&inventory, &item).is_some());
                if held {
                    Verdict::Allow
                } else {
                    Verdict::Refuse(format!("you have no {}", item.name().replace('_', " ")))
                }
            },
        );

        // ...and uses one up.
        let (shared, players, names, placeable) = (
            Arc::clone(&rules),
            Arc::clone(&self.players),
            self.block_names.clone(),
            Arc::clone(&self.placeable),
        );
        registrar.on::<BlockPlaced>(
            HandlerSpec::new("voxunami_cave:placing_uses_the_item_up"),
            move |event, context| {
                if locked(&shared).mode != GameMode::Survival {
                    return;
                }
                let Some(item) = names
                    .get(&event.block)
                    .and_then(|block| item_for_block(block, &placeable))
                else {
                    return;
                };
                let Some(entity) = lock(&players).entity_of(event.client) else {
                    return;
                };
                let Ok(Some(mut inventory)) = context.component::<Inventory>(entity) else {
                    return;
                };
                if let Some(slot) = slot_holding(&inventory, &item)
                    && inventory.take(slot, 1).is_ok()
                {
                    let _ = context.set_component(entity, &inventory);
                }
            },
        );

        // A broken block leaves its item where it was (a block that exists only in creative leaves none, so that playing never gives one).
        let (shared, names, placeable, creative_only) = (
            Arc::clone(&rules),
            self.block_names.clone(),
            Arc::clone(&self.placeable),
            Arc::clone(&self.creative_only),
        );
        registrar.on::<BlockBroken>(
            HandlerSpec::new("voxunami_cave:breaking_leaves_the_item"),
            move |event, context| {
                if locked(&shared).mode != GameMode::Survival {
                    return;
                }
                let Some(item) = names
                    .get(&event.block)
                    .filter(|block| !creative_only.contains(block))
                    .and_then(|block| item_for_block(block, &placeable))
                else {
                    return;
                };
                let mut components = ComponentBag::new();
                components.set(&ItemStack::new(item, 1));
                let _ = context.spawn(
                    &dropped_item_kind(),
                    drop_pose(event.grid, event.voxel),
                    components,
                );
            },
        );

        // Items fall, join and are picked up.
        let this = ItemsRun {
            air: self.air,
            table: self.table.clone(),
            passable: self.passable,
            block_channel: self.block_channel,
            state_channel: self.state_channel,
            orientation_channel: self.orientation_channel,
            players: Arc::clone(&self.players),
        };
        registrar.system(
            SystemSpec::new(
                "voxunami_cave:items_fall_join_and_are_picked_up",
                Phase::Rules,
            )
            // (After the fire has burnt: items see the ground as it is left.)
            .after("voxunami_cave:burn")
            .reads(&[
                self.block_channel,
                self.state_channel,
                self.orientation_channel,
            ])
            .reads_entities()
            .writes_entities()
            .writes_component::<ItemStack>()
            .writes_component::<Inventory>(),
            move |context| this.run(context),
        );
    }
}

/// What the per-tick system needs.
struct ItemsRun {
    air: u32,
    table: BlockTable,
    passable: PropertyHandle<bool>,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    orientation_channel: ChannelIndex,
    players: SharedPresence,
}

/// An item on the ground, as the system looks at it.
#[derive(Clone, Debug)]
struct Lying {
    id: EntityId,
    grid: GridId,
    voxel: VoxelCoordinate,
    stack: ItemStack,
}

impl ItemsRun {
    fn run(&self, context: &mut Context<'_>) {
        self.fall(context);
        self.join(context);
        self.pick_up(context);
    }

    /// Every item on the ground, in order of identifier.
    fn lying(&self, context: &mut Context<'_>) -> Vec<Lying> {
        let kind = dropped_item_kind();
        let ids: Vec<EntityId> = context
            .world()
            .entities()
            .iter()
            .filter(|entity| entity.kind == kind)
            .map(|entity| entity.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| {
                let entity = context.entity(id)?;
                let (grid, voxel) = (entity.pose.grid, voxel_of(entity.pose.position));
                let stack = context.component::<ItemStack>(id).ok().flatten()?;
                Some(Lying {
                    id,
                    grid,
                    voxel,
                    stack,
                })
            })
            .collect()
    }

    /// Whether something in this voxel holds an item up. A voxel that cannot be read does.
    fn holds_up(&self, context: &mut Context<'_>, grid: GridId, voxel: VoxelCoordinate) -> bool {
        let Ok(mut view) = context.grid(grid) else {
            return true;
        };
        let (Ok(block), Ok(state), Ok(orientation)) = (
            view.get(self.block_channel, voxel),
            view.get(self.state_channel, voxel),
            view.get(self.orientation_channel, voxel),
        ) else {
            return true;
        };
        if block == self.air {
            return false;
        }
        let model = self.table.model_of(block, state, orientation);
        !(self.table.get(self.passable, model) || self.table.shape(model).is_empty())
    }

    fn fall(&self, context: &mut Context<'_>) {
        if !falls_this_tick(context.tick().get()) {
            return;
        }
        for item in self.lying(context) {
            // Down is where gravity pulls at the item, in the axes of the grid it is in (the planet's pull is toward its centre, so an item on the side of a
            // planet falls along x or z); where there is none, nothing falls.
            let Some(down) = self.down_for(context, &item) else {
                continue;
            };
            if self.fell_out_of_the_world(context, &item, down) {
                context.despawn(item.id);
                continue;
            }
            let below = VoxelCoordinate::new(
                item.voxel.x() + down[0],
                item.voxel.y() + down[1],
                item.voxel.z() + down[2],
            );
            if !self.holds_up(context, item.grid, below) {
                let _ = context.set_pose(item.id, drop_pose(item.grid, below));
            }
        }
    }

    /// The step an item takes when it falls: one voxel along the axis gravity is strongest on, the way it pulls; `None` where it is not pulled.
    fn down_for(&self, context: &mut Context<'_>, item: &Lying) -> Option<[i128; 3]> {
        let gravity = context
            .world()
            .gravity_in(item.grid, centre_of(item.voxel))
            .ok()?;
        let axis = (0..3).max_by(|a, b| gravity[*a].abs().total_cmp(&gravity[*b].abs()))?;
        if gravity[axis].abs() <= MOST_GRAVITY_THAT_IS_NONE {
            return None;
        }
        let mut step = [0; 3];
        step[axis] = if gravity[axis] < 0.0 { -1 } else { 1 };
        Some(step)
    }

    /// Whether an item has fallen out of the world: below the lowest voxel in a level, or a long way from the ship it was on when it fell off it.
    fn fell_out_of_the_world(
        &self,
        context: &mut Context<'_>,
        item: &Lying,
        down: [i128; 3],
    ) -> bool {
        match item.grid.root_level() {
            Some(_) => item.voxel.y() < LOWEST_DROP_VOXEL && down == [0, -1, 0],
            None => context
                .world()
                .data()
                .grids()
                .get(item.grid)
                .and_then(Extent::of)
                .is_none_or(|extent| {
                    extent.distance_to(in_voxels(centre_of(item.voxel))) > LOWEST_DROP_DISTANCE
                }),
        }
    }

    /// Items of one kind in one voxel become one stack, as far as a stack goes.
    fn join(&self, context: &mut Context<'_>) {
        let mut voxels: BTreeMap<(GridId, [i128; 3]), Vec<Lying>> = BTreeMap::new();
        for item in self.lying(context) {
            let key = (item.grid, [item.voxel.x(), item.voxel.y(), item.voxel.z()]);
            voxels.entry(key).or_default().push(item);
        }
        for items in voxels.into_values().filter(|items| items.len() > 1) {
            let mut kept: Vec<Lying> = Vec::new();
            for mut item in items {
                for earlier in &mut kept {
                    if item.stack.count == 0 {
                        break;
                    }
                    if earlier.stack.stacks_with(&item.stack) {
                        let moved = STACK_SIZE
                            .saturating_sub(earlier.stack.count)
                            .min(item.stack.count);
                        earlier.stack.count += moved;
                        item.stack.count -= moved;
                    }
                }
                if item.stack.count == 0 {
                    context.despawn(item.id);
                } else {
                    kept.push(item);
                }
            }
            // What changed, written back.
            for item in kept {
                let _ = context.set_component(item.id, &item.stack);
            }
        }
    }

    /// Items near a player go into their inventory, as far as it has room.
    fn pick_up(&self, context: &mut Context<'_>) {
        let kind = dropped_item_kind();
        let players = lock(&self.players).players(context.world().data());
        for (client, pose, level) in players {
            // Items lie in the grid of the block they were broken from: the level's terrain, a ship, a planet; the player picks up those near them in each.
            let mut places = vec![(GridId::root(level), pose.position)];
            places.extend(
                context
                    .world()
                    .data()
                    .grids()
                    .iter()
                    .filter(|(_, ship)| ship.level == level)
                    .filter_map(|(id, ship)| Some((id, ship.pose.world_to_local(pose.position)?))),
            );
            for (grid, position) in places {
                self.pick_up_for(context, client, position, grid, &kind);
            }
        }
    }

    fn pick_up_for(
        &self,
        context: &mut Context<'_>,
        client: ClientId,
        position: voxel_space::WorldPosition,
        grid: GridId,
        kind: &NamespacedId,
    ) {
        let Some(player) = lock(&self.players).entity_of(client) else {
            return;
        };
        let Ok(Some(mut inventory)) = context.component::<Inventory>(player) else {
            return;
        };
        let mut changed = false;
        for id in context.entities_near(grid, position, PICKUP_RADIUS) {
            let is_item = context
                .entity(id)
                .is_some_and(|entity| entity.kind == *kind);
            if !is_item {
                continue;
            }
            let Some(stack) = context.component::<ItemStack>(id).ok().flatten() else {
                continue;
            };
            let Ok(left_over) = inventory.insert(stack.clone(), &context.world().catalog().items)
            else {
                continue;
            };
            match left_over {
                None => {
                    context.despawn(id);
                    changed = true;
                }
                Some(rest) if rest.count < stack.count => {
                    let _ = context.set_component(id, &rest);
                    changed = true;
                }
                Some(_) => {}
            }
        }
        if changed {
            let _ = context.set_component(player, &inventory);
        }
    }
}
