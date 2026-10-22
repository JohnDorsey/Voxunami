//! A place for a rule's scenario to happen in, either the terrain of the main level or a ship, with the same blocks at the same offsets and the
//! same world coordinates, so that a rule written for one can be checked to behave the same in the other.
//!
//! The promise of ships is that any rule a game or a mod adds works aboard them with no effort from its author. A rule is checked against the
//! promise by writing its scenario once, over a [`Site`], and running it with `for place in BOTH`.

use std::sync::{Arc, Mutex};

use server::GridView;
use voxel_grid::ChannelIndex;
use voxel_space::{GridId, LevelIndex, VoxelCoordinate};

use super::{Scene, pose_at};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Terrain,
    Ship,
}

pub const BOTH: [Place; 2] = [Place::Terrain, Place::Ship];

/// A box of the world that a scenario uses: its offsets are from `origin`, which is a voxel of the world. The ship of a [`Place::Ship`] site is held
/// there, not turned, with its own origin at `origin`, so an offset is the same voxel of the world in both places.
#[derive(Clone, Copy, Debug)]
pub struct Site {
    pub place: Place,
    pub origin: [i64; 3],
    pub grid: GridId,
}

impl Site {
    pub fn new(scene: &mut Scene, place: Place, origin: [i64; 3]) -> Self {
        let grid = match place {
            Place::Terrain => GridId::MAIN,
            Place::Ship => {
                scene.run(move |world| {
                    let ship = world
                        .create_grid(
                            LevelIndex::MAIN,
                            pose_at(origin[0] as f32, origin[1] as f32, origin[2] as f32),
                        )
                        .unwrap();
                    world.set_grid_held(ship, true).unwrap();
                });
                scene
                    .rig
                    .server
                    .world()
                    .grids()
                    .iter()
                    .map(|(id, _)| id)
                    .max()
                    .expect("the ship was made")
            }
        };
        Self {
            place,
            origin,
            grid,
        }
    }

    /// The voxel of the site's grid at this offset: a ship's own coordinates start at its origin.
    pub fn voxel(&self, offset: [i64; 3]) -> VoxelCoordinate {
        match self.place {
            Place::Terrain => VoxelCoordinate::new(
                self.origin[0] + offset[0],
                self.origin[1] + offset[1],
                self.origin[2] + offset[2],
            ),
            Place::Ship => VoxelCoordinate::new(offset[0], offset[1], offset[2]),
        }
    }

    /// Where the offset is in the world, as a point in voxels.
    pub fn world_at(&self, offset: [i64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|axis| (self.origin[axis] + offset[axis]) as f64)
    }

    /// Sets these voxels in one tick.
    pub fn set(&self, scene: &mut Scene, channel: ChannelIndex, voxels: Vec<([i64; 3], u32)>) {
        let (grid, site) = (self.grid, *self);
        scene.run(move |world| {
            let mut view = world.grid(grid).unwrap();
            for (offset, value) in voxels {
                view.set(channel, site.voxel(offset), value).unwrap();
            }
        });
    }

    /// What these voxels hold in a channel (this takes a tick).
    pub fn read(&self, scene: &mut Scene, channel: ChannelIndex, offsets: &[[i64; 3]]) -> Vec<u32> {
        let (grid, site, offsets) = (self.grid, *self, offsets.to_vec());
        let found = Arc::new(Mutex::new(Vec::new()));
        let slot = Arc::clone(&found);
        scene.run(move |world| {
            // A ship that was cut apart (a fire can burn one into pieces) is in the parts it became, which keep its coordinates: a voxel is
            // what the part that has something there says.
            let parts: Vec<GridId> = if grid.root_level().is_some() {
                vec![grid]
            } else {
                world
                    .data()
                    .grids()
                    .resolve(grid)
                    .into_iter()
                    .map(|(part, _)| part)
                    .collect()
            };
            let default = world
                .data()
                .main()
                .overrides
                .layout()
                .default_value(channel);
            *slot.lock().unwrap() = offsets
                .iter()
                .map(|offset| {
                    parts
                        .iter()
                        .map(|part| {
                            world
                                .grid(*part)
                                .unwrap()
                                .get(channel, site.voxel(*offset))
                                .unwrap()
                        })
                        .find(|found| *found != default)
                        .unwrap_or(default)
                })
                .collect();
        });
        found.lock().unwrap().clone()
    }

    /// How many voxels of the box from `low` to just before `high` (offsets) hold this value in a channel (this takes a tick).
    pub fn count(
        &self,
        scene: &mut Scene,
        channel: ChannelIndex,
        value: u32,
        low: [i64; 3],
        high: [i64; 3],
    ) -> usize {
        let mut offsets = Vec::new();
        for x in low[0]..high[0] {
            for y in low[1]..high[1] {
                for z in low[2]..high[2] {
                    offsets.push([x, y, z]);
                }
            }
        }
        self.read(scene, channel, &offsets)
            .into_iter()
            .filter(|found| *found == value)
            .count()
    }
}
