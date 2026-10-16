//! What the Voxunami Cave server does: lets players change blocks within reach, and tells everyone
//! where everyone is.

use std::collections::HashMap;

use block_models::BlockTable;
use game_shell::PoseBook;
use lattice::ActiveVoxels;
use namespaced_id::NamespacedId;
use player_presence::pose_kind;
use protocol::{ClientId, Tick};
use server::{CommandLine, GameLogic, ServerWorld};
use voxel_grid::ChannelIndex;
use voxel_space::{LevelIndex, VoxelCoordinate, WorldPosition};
use world_storage::{Section, WorldData};

use crate::content::CaveContent;
use crate::fire::{Fire, FireWorld};
use crate::lighting::{self, LightWorld, Lighting};
use crate::messages::{
    decode_move_request, decode_set_block, decode_spawn, move_input_kind, set_block_kind,
    spawn_kind,
};
use crate::movement::Movement;

/// How far from a player, in voxels, a block can be changed.
pub const REACH: f64 = 10.0;

/// How many voxels the light rule looks at in a tick at most. A lamp's light is some thousands of
/// voxels a tick as it spreads; more than this at once (many lamps) takes a few ticks longer.
pub const LIGHT_VOXELS_PER_TICK: usize = 20_000;

/// How far from a player, in voxels, fires burn: farther away they are frozen.
pub const FIRE_RANGE: f64 = 64.0;

/// The server's world as the light rule sees it. Terrain this server cannot generate has nothing
/// to read, and lets no light through.
struct WorldLight<'a> {
    world: &'a mut ServerWorld,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    light_channel: ChannelIndex,
}

impl LightWorld for WorldLight<'_> {
    fn block_and_state(&mut self, level: LevelIndex, at: VoxelCoordinate) -> Option<(u32, u32)> {
        let block = self.world.voxel(level, self.block_channel, at).ok()?;
        let state = self.world.voxel(level, self.state_channel, at).ok()?;
        Some((block, state))
    }

    fn light(&mut self, level: LevelIndex, at: VoxelCoordinate) -> u8 {
        self.world
            .voxel(level, self.light_channel, at)
            .map_or(0, |light| light.min(255) as u8)
    }

    fn set_light(&mut self, level: LevelIndex, at: VoxelCoordinate, value: u8) {
        // The voxel was just read, so the level and the terrain there are available.
        let _ = self
            .world
            .set_voxel(level, self.light_channel, at, u32::from(value));
    }
}

/// The server's world as the fire rule sees it.
struct WorldFire<'a> {
    world: &'a mut ServerWorld,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
}

impl FireWorld for WorldFire<'_> {
    fn block_and_state(&mut self, level: LevelIndex, at: VoxelCoordinate) -> Option<(u32, u32)> {
        let block = self.world.voxel(level, self.block_channel, at).ok()?;
        let state = self.world.voxel(level, self.state_channel, at).ok()?;
        Some((block, state))
    }

    fn set_block_and_state(
        &mut self,
        level: LevelIndex,
        at: VoxelCoordinate,
        block: u32,
        state: u32,
    ) {
        // The voxel was just read, so the level and the terrain there are available.
        let _ = self.world.set_voxel(level, self.state_channel, at, state);
        let _ = self.world.set_voxel(level, self.block_channel, at, block);
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
    fire: Fire,
    fire_block: u32,
    movement: Movement,
    poses: PoseBook,
}

impl CaveLogic {
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
        let block_ids = content
            .blocks
            .blocks
            .iter()
            .map(|(id, entry)| (entry.identifier().clone(), id.raw()))
            .collect();
        let (fire_clock, fire_active) =
            Fire::read_section(world.opaque_sections(), |level| world.level_index(level))
                .unwrap_or_default();
        Self {
            block_ids,
            table: content.blocks.block_table(),
            block_channel: content.block_channel(),
            state_channel: content.state_channel(),
            light_channel: content.light_channel(),
            level_ids,
            lighting: Lighting::with_pending(content.blocks.block_table(), pending),
            fire: Fire::new(
                content.blocks.block_table(),
                content.blocks.fire,
                content.blocks.air,
                world.world_seed,
                fire_clock,
                fire_active,
            ),
            fire_block: content.blocks.fire,
            movement: Movement::new(
                content.blocks.block_table(),
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
            self.lighting.voxel_changed(level, at);
            self.movement.brick_changed(level, at.split().0);
            if raw == self.fire_block {
                self.fire.ignite(level, at);
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
    fn client_joined(&mut self, _client: ClientId, _world: &mut ServerWorld) {}

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
            Some(line) if line.name == "tp" => {
                let reply = self.teleport(client, &line.arguments);
                world.reply(client, &reply);
            }
            _ => world.reply(client, "unknown command; try /level, /fly, /tp X Y Z"),
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
            |level, at| {
                poses.players().any(|(_, pose, player_level)| {
                    player_level == level && pose.distance_to_voxel(at) <= FIRE_RANGE
                })
            },
        );
        for (level, at) in changed {
            self.lighting.voxel_changed(level, at);
            self.movement.brick_changed(level, at.split().0);
        }
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

    fn save_sections(&self) -> Vec<Section> {
        vec![
            self.lighting
                .pending()
                .to_section(&lighting::section_id(), &self.level_ids),
            self.fire.to_section(&self.level_ids),
        ]
    }
}
