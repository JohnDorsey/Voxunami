//! Light: the classic light, the multiscale light, `/light` to choose between them, and telling players
//! how the light is cut into cells so that they can draw it.

use std::sync::{Arc, Mutex, MutexGuard};

use block_models::{BlockTable, PropertyHandle, SharedBlockTable};
use lattice::ActiveVoxels;
use namespaced_id::NamespacedId;
use plugin_host::{
    Argument, CommandClass, CommandSpec, Context, HandlerSpec, Phase, PlayerJoined, Plugin,
    Registrar, SaveScope, SystemSpec,
};
use protocol::{ClientId, Message};
use server::{Audience, GridView, ServerWorld};
use voxel_grid::{Brick, ChannelIndex};
use voxel_space::{GridId, LocalVoxelIndex, VoxelCoordinate};

use super::adapters::{CaveChannels, WorldLight, WorldScaledLight};
use crate::content::CaveContent;
use crate::lighting::{self, Lighting};
use crate::logic::LIGHT_VOXELS_PER_TICK;
use crate::messages::LightSchemeChanged;
use crate::multiscale_light::{self, LightModel, LightScheme, MultiScaleLighting};

/// Which light a world has, and the rule that spreads it.
#[derive(Debug)]
struct LightState {
    table: Arc<BlockTable>,
    /// The table of the server, which may be replaced, and the version of it that `table` is: the light is made again when they differ.
    shared: SharedBlockTable,
    table_version: u64,
    emission: PropertyHandle<u8>,
    channels: CaveChannels,
    /// The world's levels by identifier, in index order, for naming them in the saved light.
    level_ids: Vec<NamespacedId>,
    lighting: Lighting,
    /// Which of the two lights the world has.
    light_model: LightModel,
    multiscale: MultiScaleLighting,
}

impl LightState {
    /// The scheme of the multiscale light, if that is the world's light.
    fn scheme_in_use(&self) -> Option<LightScheme> {
        match self.light_model {
            LightModel::Classic => None,
            LightModel::MultiScale(scheme) => Some(scheme),
        }
    }

    /// Tells whichever light the world has that a voxel changed.
    fn light_changed(&mut self, grid: GridId, at: VoxelCoordinate) {
        match self.light_model {
            LightModel::Classic => self.lighting.voxel_changed(grid, at),
            LightModel::MultiScale(_) => self.multiscale.voxel_changed(grid, at),
        }
    }

    /// `/light` says which light the world has; `/light classic`, `/light multiscale [32|16|8|4]`
    /// (the number of levels, 16 if not said) and `/light multiscale LEVELS WIDTH...` (the widths of
    /// the cells, finest first) change it. Changing clears every light there is, and has the new one
    /// look at every lamp and fire in the world, so it spreads from them again.
    fn light_command(&mut self, arguments: &[&str], context: &mut Context<'_>) -> String {
        const USAGE: &str = "usage: /light, /light classic, /light multiscale [32|16|8|4], /light multiscale LEVELS WIDTH... (widths 1 to 32, growing)";
        if arguments.is_empty() {
            return format!("the light is {}", self.light_model.name());
        }
        let Some(model) = LightModel::from_arguments(arguments) else {
            return USAGE.to_owned();
        };
        if model == self.light_model {
            return format!("the light is already {}", model.name());
        }
        self.change_light_model(model, context);
        format!("the light is now {}", model.name())
    }

    fn change_light_model(&mut self, model: LightModel, context: &mut Context<'_>) {
        self.restart_light(model, context, true);
    }

    /// If the table of the blocks was replaced since the light was made, has the world's light made again by what the blocks are now (what gives light, what lets it
    /// through). A table whose blocks all behave as they did (it only repaints them) changes no light, and the light is left as it is: lighting the world again is a visit
    /// to every voxel of every brick.
    fn follow_the_table(&mut self, context: &mut Context<'_>) {
        if self.shared.version() == self.table_version {
            return;
        }
        let (table, version) = self.shared.snapshot();
        let earlier = std::mem::replace(&mut self.table, table);
        self.table_version = version;
        if earlier
            .blocks_that_behave_otherwise_in(&self.table)
            .is_empty()
        {
            return;
        }
        self.restart_light(self.light_model, context, false);
    }

    /// Clears every light there is and has the light of `model` look at every lamp and fire in the world (see [`LightState::light_command`]), and tells everyone how the
    /// light is cut into cells if `announce`.
    fn restart_light(&mut self, model: LightModel, context: &mut Context<'_>, announce: bool) {
        // Light is only in edited bricks (generated terrain has no lamps), so those are all to clear,
        // and all that can hold a lamp.
        let mut channels = vec![self.channels.light];
        channels.extend(self.channels.scales.iter().copied());
        let mut emitters = Vec::new();
        let mut lit = Vec::new();
        // The terrain of every level, and every ship that is in memory (a ship in a file has its light made again when it comes out).
        let data = context.world().data();
        let grids = data
            .levels()
            .map(|(level, _, data)| (GridId::root(level), &data.overrides))
            .chain(data.grids().iter().map(|(id, ship)| (id, &ship.voxels)));
        for (grid, voxels) in grids {
            for (brick, contents) in voxels.bricks() {
                let found = light_in_brick(
                    contents,
                    self.channels.block,
                    self.channels.state,
                    &channels,
                    &self.table,
                    self.emission,
                );
                emitters.extend(
                    found
                        .emitters
                        .into_iter()
                        .map(|local| (grid, brick.voxel_at(local))),
                );
                lit.extend(
                    found
                        .lit
                        .into_iter()
                        .map(|(local, channel)| (grid, channel, brick.voxel_at(local))),
                );
            }
        }
        for (grid, channel, voxel) in lit {
            if let Ok(mut view) = context.world().grid(grid) {
                let _ = view.set(channel, voxel, 0);
            }
        }
        self.lighting = Lighting::new(self.table.clone());
        let scheme = match model {
            LightModel::MultiScale(scheme) => scheme,
            LightModel::Classic => LightScheme::LEVELS_16,
        };
        self.multiscale = MultiScaleLighting::new(self.table.clone(), scheme);
        self.light_model = model;
        if announce {
            context.send(
                Audience::Everyone,
                LightSchemeChanged::kind(),
                LightSchemeChanged(self.scheme_in_use()).encode(),
            );
        }
        for (grid, voxel) in emitters {
            self.light_changed(grid, voxel);
        }
    }

    /// Tells a player who has joined how the light is cut into cells, to draw it.
    fn welcome(&self, client: ClientId, world: &mut ServerWorld) {
        world.send(
            Audience::Client(client),
            LightSchemeChanged::kind(),
            LightSchemeChanged(self.scheme_in_use()).encode(),
        );
    }

    /// One tick of the light rule that the world has.
    fn spread(&mut self, context: &mut Context<'_>) {
        self.follow_the_table(context);
        match self.light_model {
            LightModel::Classic => {
                self.lighting.step_within(
                    &mut WorldLight {
                        context,
                        channels: &self.channels,
                    },
                    LIGHT_VOXELS_PER_TICK,
                );
            }
            LightModel::MultiScale(scheme) => {
                self.multiscale.step_within(
                    &mut WorldScaledLight {
                        context,
                        channels: &self.channels,
                        scheme,
                    },
                    &multiscale_light::default_budgets(scheme),
                );
            }
        }
    }

    fn saved_sections(&self) -> Vec<world_storage::Section> {
        let mut sections = vec![
            self.lighting
                .pending()
                .to_section(&lighting::section_id(), &self.level_ids),
            self.light_model.to_section(),
        ];
        for scale in 0..self.multiscale.scheme().scales() {
            sections.push(
                self.multiscale
                    .pending(scale)
                    .to_section(&multiscale_light::section_id(scale), &self.level_ids),
            );
        }
        sections
    }
}

fn locked(state: &Mutex<LightState>) -> MutexGuard<'_, LightState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The light of the world, carrying on spreading what was spreading when the world was saved.
#[derive(Debug)]
pub struct LightPlugin {
    channels: CaveChannels,
    table: SharedBlockTable,
}

impl LightPlugin {
    pub fn new(content: &CaveContent, table: &SharedBlockTable) -> Self {
        Self {
            channels: CaveChannels::of(content),
            table: table.clone(),
        }
    }
}

impl Plugin for LightPlugin {
    fn id(&self) -> NamespacedId {
        "voxunami_cave:light"
            .parse()
            .expect("constant identifier is valid")
    }

    fn register(&self, registrar: &mut Registrar<'_>) {
        registrar.declares_message::<LightSchemeChanged>();
        let world = registrar.world();
        let level_ids: Vec<NamespacedId> = world.levels().map(|(_, id, _)| id.clone()).collect();
        // Voxels in a level this world no longer has are not looked at.
        let pending = ActiveVoxels::from_sections(
            world.opaque_sections(),
            &lighting::section_id(),
            |level| world.level_index(level),
        )
        .unwrap_or_default();
        let light_model = LightModel::read_section(world.opaque_sections());
        let scheme = match light_model {
            LightModel::MultiScale(scheme) => scheme,
            LightModel::Classic => LightScheme::LEVELS_16,
        };
        let (table, table_version) = self.table.snapshot();
        let multiscale = MultiScaleLighting::with_pending(
            Arc::clone(&table),
            scheme,
            (0..scheme.scales())
                .map(|scale| {
                    ActiveVoxels::from_sections(
                        world.opaque_sections(),
                        &multiscale_light::section_id(scale),
                        |level| world.level_index(level),
                    )
                    .unwrap_or_default()
                })
                .collect(),
        );
        let state = Arc::new(Mutex::new(LightState {
            emission: crate::block_properties::emission(&table),
            table: Arc::clone(&table),
            shared: self.table.clone(),
            table_version,
            channels: self.channels.clone(),
            level_ids,
            lighting: Lighting::with_pending(table, pending),
            light_model,
            multiscale,
        }));

        // Blocks that changed are looked at, and their neighbours.
        let hearing = Arc::clone(&state);
        registrar.on_channels(
            HandlerSpec::new("voxunami_cave:light_notices_changed_blocks"),
            &[
                self.channels.block,
                self.channels.state,
                self.channels.orientation,
            ],
            move |event, _| {
                let mut state = locked(&hearing);
                for &voxel in &event.voxels {
                    state.light_changed(event.grid, voxel);
                }
            },
        );

        let greeting = Arc::clone(&state);
        registrar.on::<PlayerJoined>(
            HandlerSpec::new("voxunami_cave:tell_the_newcomer_how_light_is_cut"),
            move |event, context| locked(&greeting).welcome(event.client, context.world()),
        );

        let commanded = Arc::clone(&state);
        registrar.command(
            CommandSpec::new(
                "light",
                "which light the world has; /light classic; /light multiscale [LEVELS [WIDTH...]]",
            )
            .class(CommandClass::Change)
            .argument(Argument::rest("subcommand").optional()),
            move |context, client, arguments| {
                let reply = locked(&commanded).light_command(arguments.rest(0), context);
                context.reply(client, &reply);
            },
        );

        // After fire, whose changes it has just been told of.
        let spreading = Arc::clone(&state);
        let mut written = vec![self.channels.light];
        written.extend(self.channels.scales.iter().copied());
        registrar.system(
            SystemSpec::new("voxunami_cave:spread_light", Phase::Rules)
                .after("voxunami_cave:burn")
                .reads(&[
                    self.channels.block,
                    self.channels.state,
                    self.channels.orientation,
                ])
                .writes(&written),
            move |context| locked(&spreading).spread(context),
        );

        registrar.save_section(SaveScope::PerGrid { movable: true }, move || {
            locked(&state).saved_sections()
        });
    }
}

/// What one brick holds that the light needs to know of when it is made again: the voxels whose block gives off light in their state, and the voxels that have any light in
/// a channel (with the channel), the voxels in index order and for one voxel the channels in the order given. Found from the palettes of the channels and not by looking at
/// each of the 32,768 voxels, so that a brick that is a few blocks in air costs a pass over its packed indices.
struct LightInBrick {
    emitters: Vec<LocalVoxelIndex>,
    lit: Vec<(LocalVoxelIndex, ChannelIndex)>,
}

fn light_in_brick(
    contents: &Brick,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    light_channels: &[ChannelIndex],
    table: &BlockTable,
    emission: PropertyHandle<u8>,
) -> LightInBrick {
    // A block can only be where light comes from if it gives off some in a state it has (a state it lacks is its first).
    let may_emit = |block: u32| {
        (0..table.state_count(block)).any(|state| table.get_of(emission, block, state) > 0)
    };
    let emitters = contents
        .channel(block_channel)
        .indices_where(may_emit)
        .into_iter()
        .filter(|&local| {
            let block = contents.get(block_channel, local);
            let state = contents.get(state_channel, local);
            table.get_of(emission, block, state) > 0
        })
        .collect();
    let mut lit: Vec<(LocalVoxelIndex, usize, ChannelIndex)> = light_channels
        .iter()
        .enumerate()
        .flat_map(|(position, &channel)| {
            contents
                .channel(channel)
                .indices_where(|value| value != 0)
                .into_iter()
                .map(move |local| (local, position, channel))
        })
        .collect();
    lit.sort_by_key(|&(local, position, _)| (local.index(), position));
    LightInBrick {
        emitters,
        lit: lit
            .into_iter()
            .map(|(local, _, channel)| (local, channel))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use voxel_grid::{ChannelDefinition, GridLayout};

    use super::*;

    /// What it was before the palettes were asked: every voxel of the brick, one after another.
    fn every_voxel(
        contents: &Brick,
        block_channel: ChannelIndex,
        state_channel: ChannelIndex,
        light_channels: &[ChannelIndex],
        table: &BlockTable,
        emission: PropertyHandle<u8>,
    ) -> (Vec<LocalVoxelIndex>, Vec<(LocalVoxelIndex, ChannelIndex)>) {
        let (mut emitters, mut lit) = (Vec::new(), Vec::new());
        for local in LocalVoxelIndex::all() {
            let block = contents.get(block_channel, local);
            let state = contents.get(state_channel, local);
            if table.get_of(emission, block, state) > 0 {
                emitters.push(local);
            }
            for &channel in light_channels {
                if contents.get(channel, local) != 0 {
                    lit.push((local, channel));
                }
            }
        }
        (emitters, lit)
    }

    #[test]
    fn the_emitters_and_the_lit_voxels_found_from_the_palettes_are_those_every_voxel_gives_in_the_same_order()
     {
        let content = CaveContent::new();
        let table = game_shell::GameContent::block_table(&content);
        let emission = table
            .handle(&base_blocks::EMISSION)
            .expect("Cave declares what a block gives off");
        compare_with_every_voxel(&table, emission);
    }

    #[test]
    fn a_block_that_gives_off_light_in_one_of_its_states_is_an_emitter_only_in_that_state() {
        let mut schema = block_models::PropertySchema::new();
        schema.declare(base_blocks::EMISSION.declaration()).unwrap();
        let air = block_models::BlockDefinition::air();
        // A lamp that is lit in its second state and dark in its first, and a block that never gives any.
        let lamp = block_models::BlockDefinition::cube([0.5; 3])
            .with_shapes(vec![
                block_models::Shape::full(),
                block_models::Shape::full(),
            ])
            .with_state_property(1, &base_blocks::EMISSION, 10);
        let stone = block_models::BlockDefinition::cube([0.5; 3]);
        let table =
            BlockTable::try_from_definitions(&schema, &[Some(&air), Some(&lamp), Some(&stone)])
                .unwrap();
        let emission = table.handle(&base_blocks::EMISSION).unwrap();
        compare_with_every_voxel(&table, emission);
    }

    /// Bricks of every sparseness, of the blocks of `table` in its states, with light here and there: what is found is what a look at every voxel finds.
    fn compare_with_every_voxel(table: &BlockTable, emission: PropertyHandle<u8>) {
        let layout = GridLayout::new(vec![
            ChannelDefinition::number("test:block".parse().unwrap(), 0),
            ChannelDefinition::number("test:state".parse().unwrap(), 0),
            ChannelDefinition::number("test:light".parse().unwrap(), 0),
            ChannelDefinition::number("test:scale_one".parse().unwrap(), 0),
            ChannelDefinition::number("test:scale_two".parse().unwrap(), 0),
        ])
        .unwrap();
        let channels: Vec<ChannelIndex> = layout.channel_indices().collect();
        let block_count = table.block_count() as u32;
        for (count, seed) in [
            (0usize, 1u64),
            (1, 2),
            (27, 3),
            (500, 4),
            (9_000, 5),
            (30_000, 6),
        ] {
            let mut brick = Brick::new(&layout);
            let mut random = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let mut next = move || {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                random
            };
            for _ in 0..count {
                let local = LocalVoxelIndex::from_index((next() % 32_768) as usize).unwrap();
                brick.set(channels[0], local, (next() % u64::from(block_count)) as u32);
                brick.set(channels[1], local, (next() % 8) as u32);
                for &channel in &channels[2..] {
                    if next() % 3 == 0 {
                        brick.set(channel, local, 1 + (next() % 15) as u32);
                    }
                }
            }
            let found = light_in_brick(
                &brick,
                channels[0],
                channels[1],
                &channels[2..],
                table,
                emission,
            );
            let (emitters, lit) = every_voxel(
                &brick,
                channels[0],
                channels[1],
                &channels[2..],
                table,
                emission,
            );
            assert_eq!(found.emitters, emitters, "{count} voxels: the emitters");
            assert_eq!(found.lit, lit, "{count} voxels: the lit voxels");
            if count >= 500 {
                assert!(
                    !emitters.is_empty() && !lit.is_empty(),
                    "{count}: the brick has both"
                );
            }
        }
    }
}
