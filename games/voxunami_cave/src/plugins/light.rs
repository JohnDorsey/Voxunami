//! Light: the classic light, the multiscale light, `/light` to choose between them, and telling players
//! how the light is cut into cells so that they can draw it.

use std::sync::{Arc, Mutex, MutexGuard};

use block_models::{BlockTable, PropertyHandle};
use game_shell::GameContent;
use lattice::ActiveVoxels;
use namespaced_id::NamespacedId;
use plugin_host::{
    Argument, CommandSpec, Context, HandlerSpec, Phase, PlayerJoined, Plugin, Registrar, SaveScope,
    SystemSpec,
};
use protocol::{ClientId, Message};
use server::{Audience, ServerWorld};
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
    table: BlockTable,
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
        // Light is only in edited bricks (generated terrain has no lamps), so those are all to clear,
        // and all that can hold a lamp.
        let mut channels = vec![self.channels.light];
        channels.extend(self.channels.scales.iter().copied());
        let mut emitters = Vec::new();
        let mut lit = Vec::new();
        for (level, _, data) in context.world().data().levels() {
            for (brick, contents) in data.overrides.bricks() {
                for local in LocalVoxelIndex::all() {
                    let voxel = brick.voxel_at(local);
                    let block = contents.get(self.channels.block, local);
                    let state = contents.get(self.channels.state, local);
                    if self.table.get_of(self.emission, block, state) > 0 {
                        emitters.push((GridId::root(level), voxel));
                    }
                    for &channel in &channels {
                        if contents.get(channel, local) != 0 {
                            lit.push((level, channel, voxel));
                        }
                    }
                }
            }
        }
        for (level, channel, voxel) in lit {
            let _ = context.world().set_voxel(level, channel, voxel, 0);
        }
        self.lighting = Lighting::new(self.table.clone());
        let scheme = match model {
            LightModel::MultiScale(scheme) => scheme,
            LightModel::Classic => LightScheme::LEVELS_16,
        };
        self.multiscale = MultiScaleLighting::new(self.table.clone(), scheme);
        self.light_model = model;
        context.send(
            Audience::Everyone,
            LightSchemeChanged::kind(),
            LightSchemeChanged(self.scheme_in_use()).encode(),
        );
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
    table: BlockTable,
}

impl LightPlugin {
    pub fn new(content: &CaveContent) -> Self {
        Self {
            channels: CaveChannels::of(content),
            table: content.block_table(),
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
        let multiscale = MultiScaleLighting::with_pending(
            self.table.clone(),
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
            emission: crate::block_properties::emission(&self.table),
            table: self.table.clone(),
            channels: self.channels.clone(),
            level_ids,
            lighting: Lighting::with_pending(self.table.clone(), pending),
            light_model,
            multiscale,
        }));

        // Blocks that changed are looked at, and their neighbours.
        let hearing = Arc::clone(&state);
        registrar.on_channels(
            HandlerSpec::new("voxunami_cave:light_notices_changed_blocks"),
            &[self.channels.block, self.channels.state],
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
                .reads(&[self.channels.block, self.channels.state])
                .writes(&written),
            move |context| locked(&spreading).spread(context),
        );

        registrar.save_section(SaveScope::PerGrid { movable: true }, move || {
            locked(&state).saved_sections()
        });
    }
}
