//! What the Voxunami Cave server does: lets players change blocks within reach, and tells everyone
//! where everyone is.

use std::collections::HashMap;

use block_models::BlockTable;
use game_shell::{GameContent, PoseBook};
use lattice::ActiveVoxels;
use namespaced_id::NamespacedId;
use player_presence::pose_kind;
use protocol::{ClientId, Tick};
use server::{Audience, CommandLine, GameLogic, GridView, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, VoxelCoordinate, WorldPosition};
use world_storage::{Section, WorldData};

use crate::content::CaveContent;
use crate::fire::{Fire, FireWorld};
use crate::lighting::{self, LightWorld, Lighting};
use crate::messages::{
    decode_move_request, decode_set_block, decode_spawn, encode_light_scheme, light_scheme_kind,
    move_input_kind, set_block_kind, spawn_kind,
};
use crate::movement::Movement;
use crate::multiscale_light::{
    self, LightModel, LightScheme, MAXIMUM_SCALES, MultiScaleLighting, ScaledLightWorld,
};

/// How far from a player, in voxels, a block can be changed.
pub const REACH: f64 = 10.0;

/// How many voxels the light rule looks at in a tick at most. A lamp's light is some thousands of
/// voxels a tick as it spreads; more than this at once (many lamps) takes a few ticks longer.
pub const LIGHT_VOXELS_PER_TICK: usize = 20_000;

/// How far from a player, in voxels, fires burn: farther away they are frozen.
pub const FIRE_RANGE: f64 = 64.0;

/// The block and state of a voxel of a grid, or `None` where there is nothing to read.
fn block_and_state_of(
    world: &mut ServerWorld,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    grid: GridId,
    at: VoxelCoordinate,
) -> Option<(u32, u32)> {
    let mut grid = world.grid(grid).ok()?;
    let block = grid.get(block_channel, at).ok()?;
    let state = grid.get(state_channel, at).ok()?;
    Some((block, state))
}

/// The server's world as the light rule sees it. Terrain this server cannot generate has nothing
/// to read, and lets no light through.
struct WorldLight<'a> {
    world: &'a mut ServerWorld,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    light_channel: ChannelIndex,
}

impl LightWorld for WorldLight<'_> {
    fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
        block_and_state_of(self.world, self.block_channel, self.state_channel, grid, at)
    }

    fn light(&mut self, grid: GridId, at: VoxelCoordinate) -> u8 {
        self.world
            .grid(grid)
            .and_then(|mut grid| grid.get(self.light_channel, at))
            .map_or(0, |light| light.min(255) as u8)
    }

    fn set_light(&mut self, grid: GridId, at: VoxelCoordinate, value: u8) {
        // The voxel was just read, so the grid and the terrain there are available.
        let _ = self
            .world
            .grid(grid)
            .and_then(|mut grid| grid.set(self.light_channel, at, u32::from(value)));
    }
}

/// The server's world as the multiscale light sees it.
struct WorldScaledLight<'a> {
    world: &'a mut ServerWorld,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    scale_channels: &'a [ChannelIndex],
    scheme: LightScheme,
}

impl ScaledLightWorld for WorldScaledLight<'_> {
    fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
        block_and_state_of(self.world, self.block_channel, self.state_channel, grid, at)
    }

    fn word(&mut self, scale: usize, grid: GridId, anchor: VoxelCoordinate) -> u32 {
        self.world
            .grid(grid)
            .and_then(|mut grid| grid.get(self.scale_channels[scale], anchor))
            .unwrap_or(0)
    }

    fn set_word(&mut self, scale: usize, grid: GridId, anchor: VoxelCoordinate, value: u32) {
        // The cell's anchor was just read, so the grid and the terrain there are available. A
        // cell in more than one brick has its word in each (see `word_homes`); a brick that cannot be
        // written is not drawn from it either.
        let Ok(mut grid) = self.world.grid(grid) else {
            return;
        };
        for home in multiscale_light::word_homes(self.scheme.width(scale), anchor) {
            let _ = grid.set(self.scale_channels[scale], home, value);
        }
    }
}

/// The server's world as the fire rule sees it.
struct WorldFire<'a> {
    world: &'a mut ServerWorld,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
}

impl FireWorld for WorldFire<'_> {
    fn block_and_state(&mut self, grid: GridId, at: VoxelCoordinate) -> Option<(u32, u32)> {
        block_and_state_of(self.world, self.block_channel, self.state_channel, grid, at)
    }

    fn set_block_and_state(&mut self, grid: GridId, at: VoxelCoordinate, block: u32, state: u32) {
        // The voxel was just read, so the grid and the terrain there are available.
        let Ok(mut grid) = self.world.grid(grid) else {
            return;
        };
        let _ = grid.set(self.state_channel, at, state);
        let _ = grid.set(self.block_channel, at, block);
    }
}

#[derive(Debug)]
pub struct CaveLogic {
    block_ids: HashMap<NamespacedId, u32>,
    /// How many states each block has, which is what makes a request for a state valid.
    table: BlockTable,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    light_channel: ChannelIndex,
    /// The world's levels by identifier, in index order, for naming them in the saved light.
    level_ids: Vec<NamespacedId>,
    lighting: Lighting,
    /// Which of the two lights the world has.
    light_model: LightModel,
    scale_channels: Vec<ChannelIndex>,
    multiscale: MultiScaleLighting,
    fire: Fire,
    fire_block: u32,
    movement: Movement,
    poses: PoseBook,
}

impl CaveLogic {
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
    fn light_command(&mut self, arguments: &[&str], world: &mut ServerWorld) -> String {
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
        self.change_light_model(model, world);
        format!("the light is now {}", model.name())
    }

    fn change_light_model(&mut self, model: LightModel, world: &mut ServerWorld) {
        // Light is only in edited bricks (generated terrain has no lamps), so those are all to clear,
        // and all that can hold a lamp.
        let mut channels = vec![self.light_channel];
        channels.extend(self.scale_channels.iter().copied());
        let mut emitters = Vec::new();
        let mut lit = Vec::new();
        for (level, _, data) in world.data().levels() {
            for (brick, contents) in data.overrides.bricks() {
                for local in voxel_space::LocalVoxelIndex::all() {
                    let voxel = brick.voxel_at(local);
                    if self.table.emission(contents.get(self.block_channel, local)) > 0 {
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
            let _ = world.set_voxel(level, channel, voxel, 0);
        }
        self.lighting = Lighting::new(self.table.clone());
        let scheme = match model {
            LightModel::MultiScale(scheme) => scheme,
            LightModel::Classic => LightScheme::LEVELS_16,
        };
        self.multiscale = MultiScaleLighting::new(self.table.clone(), scheme);
        self.light_model = model;
        world.send(
            Audience::Everyone,
            light_scheme_kind(),
            encode_light_scheme(self.scheme_in_use()),
        );
        for (grid, voxel) in emitters {
            self.light_changed(grid, voxel);
        }
    }

    /// The rules for the game in `world`, carrying on spreading the light that was spreading when
    /// the world was saved.
    pub fn new(content: &CaveContent, world: &WorldData) -> Self {
        let level_ids: Vec<NamespacedId> = world.levels().map(|(_, id, _)| id.clone()).collect();
        // Voxels in a level this world no longer has are not looked at.
        let pending = ActiveVoxels::from_sections(
            world.opaque_sections(),
            &lighting::section_id(),
            |level| world.level_index(level),
        )
        .unwrap_or_default();
        let registries = content.registries();
        let blocks = registries[0].1;
        let block_ids = (0..blocks.entry_count() as u32)
            .filter_map(|raw| Some((blocks.identifier_of_raw(raw)?.clone(), raw)))
            .collect();
        let light_model = LightModel::read_section(world.opaque_sections());
        let scheme = match light_model {
            LightModel::MultiScale(scheme) => scheme,
            LightModel::Classic => LightScheme::LEVELS_16,
        };
        let (fire_clock, fire_active) =
            Fire::read_section(world.opaque_sections(), |level| world.level_index(level))
                .unwrap_or_default();
        Self {
            block_ids,
            table: content.block_table(),
            block_channel: content.block_channel(),
            state_channel: content.state_channel(),
            light_channel: content.light_channel(),
            level_ids,
            lighting: Lighting::with_pending(content.block_table(), pending),
            light_model,
            scale_channels: (0..MAXIMUM_SCALES)
                .map(|scale| content.scale_channel(scale))
                .collect(),
            multiscale: MultiScaleLighting::with_pending(
                content.block_table(),
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
            ),
            fire: Fire::new(
                content.block_table(),
                content.blocks.fire,
                content.blocks.air,
                world.world_seed,
                fire_clock,
                fire_active,
            ),
            fire_block: content.blocks.fire,
            movement: Movement::new(
                content.block_table(),
                content.block_channel(),
                content.state_channel(),
                content.blocks.air,
            ),
            poses: PoseBook::new(),
        }
    }

    fn set_block(
        &mut self,
        client: ClientId,
        at: VoxelCoordinate,
        block: &NamespacedId,
        state: u32,
        world: &mut ServerWorld,
    ) {
        let (Some(pose), Some(&raw)) = (self.poses.pose_of(client), self.block_ids.get(block))
        else {
            return; // a player who has not said where they are, or a block this server lacks
        };
        // Air has one state, so breaking a block always resets its state too.
        if pose.distance_to_voxel(at) > REACH || state >= self.table.state_count(raw) {
            return;
        }
        // Terrain this server cannot generate cannot be changed; the request is dropped.
        // Players change the level they are in.
        let level = self.poses.level_of(client);
        if world
            .set_voxel(level, self.state_channel, at, state)
            .is_ok()
        {
            let _ = world.set_voxel(level, self.block_channel, at, raw);
            self.light_changed(GridId::root(level), at);
            self.movement.brick_changed(level, at.split().0);
            if raw == self.fire_block {
                self.fire.ignite(GridId::root(level), at);
            }
        }
    }
}

impl CaveLogic {
    /// `/tp X Y Z`: puts the player's eyes at a position, in voxels. Returns what to tell them.
    fn teleport(&mut self, client: ClientId, arguments: &[&str]) -> String {
        let [x, y, z] = arguments else {
            return "usage: /tp X Y Z".to_owned();
        };
        let parsed =
            [x, y, z].map(|text| text.parse::<f64>().ok().filter(|value| value.is_finite()));
        let [Some(x), Some(y), Some(z)] = parsed else {
            return "usage: /tp X Y Z, with numbers".to_owned();
        };
        // The position must be somewhere in the world.
        let Ok(voxel) =
            VoxelCoordinate::try_new(x.floor() as i64, y.floor() as i64, z.floor() as i64)
        else {
            return "that is outside the world".to_owned();
        };
        let (brick, local) = voxel.split();
        let fraction = |value: f64| (value - value.floor()) as f32;
        let position = WorldPosition::new(
            brick,
            [
                f32::from(local.x()) + fraction(x),
                f32::from(local.y()) + fraction(y),
                f32::from(local.z()) + fraction(z),
            ],
        );
        if self.movement.teleport(client, position) {
            format!("teleported to {x} {y} {z}")
        } else {
            "you have not joined yet".to_owned()
        }
    }
}

impl GameLogic for CaveLogic {
    fn client_joined(&mut self, client: ClientId, world: &mut ServerWorld) {
        // The view needs to know how the light is cut into cells to draw it.
        world.send(
            Audience::Client(client),
            light_scheme_kind(),
            encode_light_scheme(self.scheme_in_use()),
        );
    }

    fn client_left(&mut self, client: ClientId, _world: &mut ServerWorld) {
        self.movement.remove(client);
        self.poses.client_left(client);
    }

    fn message_received(
        &mut self,
        client: ClientId,
        kind: &NamespacedId,
        payload: &[u8],
        world: &mut ServerWorld,
    ) {
        // Players do not say where they are: the server moves them (see `Movement`).
        if *kind == pose_kind() {
            return;
        }
        if *kind == spawn_kind() {
            if !self.movement.has_body(client)
                && let Ok(pose) = decode_spawn(payload)
            {
                self.movement
                    .spawn(client, pose, self.poses.level_of(client));
                self.poses.set_pose(client, pose);
            }
            return;
        }
        if *kind == move_input_kind() {
            if let Ok(request) = decode_move_request(payload) {
                self.movement.ask(client, request);
            }
            return;
        }
        if *kind == set_block_kind()
            && let Ok(request) = decode_set_block(payload)
        {
            self.set_block(client, request.at, &request.block, request.state, world);
        }
        // Anything else is another game's, or malformed: ignored.
    }

    fn command_received(&mut self, client: ClientId, command: &str, world: &mut ServerWorld) {
        if self.poses.handle_command(client, command, world) {
            return;
        }
        match CommandLine::parse(command) {
            Some(line) if line.name == "fly" => match self.movement.toggle_flying(client) {
                Some(true) => world.reply(client, "you are flying"),
                Some(false) => world.reply(client, "you are walking"),
                None => world.reply(client, "you have not joined yet"),
            },
            Some(line) if line.name == "light" => {
                let reply = self.light_command(&line.arguments, world);
                world.reply(client, &reply);
            }
            Some(line) if line.name == "tp" => {
                let reply = self.teleport(client, &line.arguments);
                world.reply(client, &reply);
            }
            _ => world.reply(
                client,
                "unknown command; try /level, /fly, /tp X Y Z, /light",
            ),
        }
    }

    fn tick(&mut self, _tick: Tick, world: &mut ServerWorld) {
        // The bodies move first, so that what the players do this tick is from where they are.
        let poses = &self.poses;
        let moved = self.movement.step(world, |client| poses.level_of(client));
        for (client, pose) in moved {
            self.poses.set_pose(client, pose);
        }
        self.poses.tell_everyone(world);
        // Fires near a player burn; what they change is looked at by the light in the same tick.
        let poses = &self.poses;
        let changed = self.fire.step(
            &mut WorldFire {
                world,
                block_channel: self.block_channel,
                state_channel: self.state_channel,
            },
            |grid, at| {
                poses.players().any(|(_, pose, player_level)| {
                    Some(player_level) == grid.root_level()
                        && pose.distance_to_voxel(at) <= FIRE_RANGE
                })
            },
        );
        for (grid, at) in changed {
            self.light_changed(grid, at);
            if let Some(level) = grid.root_level() {
                self.movement.brick_changed(level, at.split().0);
            }
        }
        match self.light_model {
            LightModel::Classic => {
                self.lighting.step_within(
                    &mut WorldLight {
                        world,
                        block_channel: self.block_channel,
                        state_channel: self.state_channel,
                        light_channel: self.light_channel,
                    },
                    LIGHT_VOXELS_PER_TICK,
                );
            }
            LightModel::MultiScale(scheme) => {
                self.multiscale.step_within(
                    &mut WorldScaledLight {
                        world,
                        block_channel: self.block_channel,
                        state_channel: self.state_channel,
                        scale_channels: &self.scale_channels,
                        scheme,
                    },
                    &multiscale_light::default_budgets(scheme),
                );
            }
        }
    }

    fn save_sections(&self) -> Vec<Section> {
        let mut sections = vec![
            self.lighting
                .pending()
                .to_section(&lighting::section_id(), &self.level_ids),
            self.fire.to_section(&self.level_ids),
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
