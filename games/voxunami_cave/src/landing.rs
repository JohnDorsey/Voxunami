//! Where a player arrives on a planet: standing on the ground at its pole, where the first player of a new world starts.

use server::ServerWorld;
use voxel_grid::ChannelIndex;
use voxel_space::{BRICK_EDGE_LENGTH, BrickCoordinate, LevelIndex, VoxelCoordinate, WorldPosition};

/// How far over the ground, in voxels, the eyes of a player standing are (`physics::CharacterSettings::eye_height`), and a little more so that the body is not put
/// in the ground by the rounding of an `f32`.
const EYES_OVER_THE_GROUND: f32 = 1.62 + 0.02;

/// The place of the eyes of a player standing on the ground at the top of the first planet in `level` (the highest of the voxels of the column of the planet's
/// +y axis that are not air, with what has been dug or built there counted), in the world's axes; `None` if the level has no planet, or its column is air all
/// through, or cannot be read.
pub fn on_a_planet(
    world: &ServerWorld,
    level: LevelIndex,
    block_channel: ChannelIndex,
    air: u32,
) -> Option<WorldPosition> {
    let (id, planet) = world
        .data()
        .grids()
        .iter()
        .find(|(_, ship)| ship.level == level && ship.terrain.is_some())?;
    let extent = planet.terrain?.extent;
    let edge = BRICK_EDGE_LENGTH as i64;
    for brick_y in (extent.min().y()..=extent.max().y()).rev() {
        // A brick of air all through is passed in one look: the sky over a planet is most of the column.
        let brick = BrickCoordinate::new(0, brick_y, 0);
        match world.moving_uniform_value(id, block_channel, brick) {
            Ok(Some(block)) if block == air => continue,
            Ok(_) => {}
            Err(_) => return None,
        }
        for y in (brick_y * edge..(brick_y + 1) * edge).rev() {
            let voxel = VoxelCoordinate::try_new(0, y, 0).ok()?;
            match world.moving_voxel(id, block_channel, voxel) {
                Ok(block) if block != air => {
                    // The middle of the voxel column, and the eyes over its top.
                    let (above, local) = VoxelCoordinate::try_new(0, y + 1, 0).ok()?.split();
                    let eyes = WorldPosition::new(above, [0.5, f32::from(local.y()), 0.5])
                        .translated([0.0, EYES_OVER_THE_GROUND, 0.0]);
                    return planet.pose.local_to_world(eyes);
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }
    None
}

/// Where a player goes who chooses the world's spawn in `level`: the ground at the top of the planet that is in it, else where players start (which in a level with a
/// planet would be inside the rock of it).
pub fn spawn_in(
    world: &ServerWorld,
    level: LevelIndex,
    block_channel: ChannelIndex,
    air: u32,
) -> WorldPosition {
    on_a_planet(world, level, block_channel, air).unwrap_or_else(crate::plugins::world_spawn)
}
