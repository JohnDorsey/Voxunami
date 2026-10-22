//! How far players are from the voxels of any grid, terrain or ship: one answer for the rules that need it.

use std::collections::BTreeMap;

use voxel_space::{GridId, LevelIndex, VoxelCoordinate};
use world_storage::{GridFrame, WorldData};

use crate::messages::PlayerPose;
use crate::presence::Presence;

impl PlayerPose {
    /// The distance in voxels from a player who is in `level` to the middle of a voxel of the grid that sits in the world as `frame` says, a
    /// ship's voxel being where its pose puts it. `None` if the grid is in another level (it is not near, whatever the numbers say) or its
    /// voxel is not a place the world can hold.
    pub fn distance_to_voxel_of(
        &self,
        level: LevelIndex,
        frame: &GridFrame,
        voxel: VoxelCoordinate,
    ) -> Option<f64> {
        if frame.level() != level {
            return None;
        }
        match frame {
            GridFrame::Level(_) => Some(self.distance_to_voxel(voxel)),
            GridFrame::Ship { .. } => frame
                .centre_in_world(voxel)
                .map(|place| self.distance_to_position(place)),
        }
    }
}

/// Where the players and the ships are at one moment, to ask whether a voxel of any grid is near a player. It owns what it needs, so a rule can
/// hold it while it changes the world.
#[derive(Clone, Debug)]
pub struct PlayerReach {
    players: Vec<(PlayerPose, LevelIndex)>,
    ships: BTreeMap<GridId, GridFrame>,
}

impl PlayerReach {
    /// The players who are connected and the ships that are in memory, as they are now.
    pub fn new(presence: &Presence, world: &WorldData) -> Self {
        Self {
            players: presence
                .players(world)
                .into_iter()
                .map(|(_, pose, level)| (pose, level))
                .collect(),
            ships: world
                .grids()
                .iter()
                .map(|(id, ship)| {
                    (
                        id,
                        GridFrame::Ship {
                            level: ship.level,
                            pose: ship.pose,
                        },
                    )
                })
                .collect(),
        }
    }

    /// Whether some player is within `distance` voxels of the middle of this voxel of this grid. A grid that is not known (a ship that is not in
    /// memory) is near nobody.
    pub fn near(&self, grid: GridId, voxel: VoxelCoordinate, distance: f64) -> bool {
        let frame = match grid.root_level() {
            Some(level) => GridFrame::Level(level),
            None => match self.ships.get(&grid) {
                Some(frame) => *frame,
                None => return false,
            },
        };
        self.players.iter().any(|(pose, level)| {
            pose.distance_to_voxel_of(*level, &frame, voxel)
                .is_some_and(|found| found <= distance)
        })
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::{BrickCoordinate, GridPose, Orientation, WorldPosition};

    use super::*;

    fn player_at(x: f32, y: f32, z: f32) -> PlayerPose {
        PlayerPose {
            position: WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    #[test]
    fn the_distance_to_a_voxel_of_terrain_is_to_its_middle() {
        let player = player_at(10.0, 10.0, 10.0);
        let frame = GridFrame::Level(LevelIndex::MAIN);
        let found = player
            .distance_to_voxel_of(LevelIndex::MAIN, &frame, VoxelCoordinate::new(13, 14, 10))
            .unwrap();
        // (3.5, 4.5, 0.5) away.
        assert!((found - (3.5f64 * 3.5 + 4.5 * 4.5 + 0.5 * 0.5).sqrt()).abs() < 1e-9);
        assert_eq!(
            found,
            player.distance_to_voxel(VoxelCoordinate::new(13, 14, 10))
        );
    }

    #[test]
    fn the_distance_to_a_voxel_of_a_ship_is_to_where_the_ship_puts_it() {
        // A ship turned a quarter about the vertical axis, with its origin at (100, 50, 200): its voxel (2, 0, 0) is at (100.5, 50.5, 197.5).
        let frame = GridFrame::Ship {
            level: LevelIndex::MAIN,
            pose: GridPose::new(
                WorldPosition::new(BrickCoordinate::new(3, 1, 6), [4.0, 18.0, 8.0]),
                Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
            ),
        };
        let near_it = PlayerPose {
            position: WorldPosition::new(BrickCoordinate::new(3, 1, 6), [4.5, 18.5, 5.5]),
            yaw: 0.0,
            pitch: 0.0,
        };
        let found = near_it
            .distance_to_voxel_of(LevelIndex::MAIN, &frame, VoxelCoordinate::new(2, 0, 0))
            .unwrap();
        assert!(found < 1e-3, "{found}");
        // Read as if it were terrain, the same voxel would be a hundred voxels away: the pose is what counts.
        let as_terrain = near_it.distance_to_voxel(VoxelCoordinate::new(2, 0, 0));
        assert!(as_terrain > 100.0);
    }

    #[test]
    fn a_grid_in_another_level_is_not_near_whatever_the_numbers_say() {
        let player = player_at(0.0, 0.0, 0.0);
        let there = GridFrame::Level(LevelIndex::new(1));
        assert_eq!(
            player.distance_to_voxel_of(LevelIndex::MAIN, &there, VoxelCoordinate::new(0, 0, 0)),
            None
        );
    }
}
