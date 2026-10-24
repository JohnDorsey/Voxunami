//! What the tests of the views share: flat ground to look at.

// Each test file uses some of it.
#![allow(dead_code)]

use std::sync::Arc;

use block_models::BlockTable;
use region_octree::BrickCube;
use voxel_grid::{Brick, ChannelIndex, ChannelStorage};
use voxel_space::{BrickBox, BrickCoordinate, VoxelCoordinate};
use world_storage::WorldData;
use world_storage::test_support::{Game, Plugin, id};
use world_view::{BrickLight, BulkLight, ViewChannels, ViewSettings, WorldMesher};

/// A hook that lights one brick as bright as `brightness` and leaves the rest dark.
#[derive(Debug)]
pub struct LitBrick {
    pub lit: BrickCoordinate,
    pub brightness: u32,
}

impl BrickLight for LitBrick {
    fn brightness(&self, _brick: &Brick, coordinate: BrickCoordinate) -> Option<ChannelStorage> {
        Some(ChannelStorage::uniform(if coordinate == self.lit {
            self.brightness
        } else {
            0
        }))
    }
}

pub struct Scene {
    pub game: Game,
    pub world: WorldData,
    pub material: ChannelIndex,
    pub air: u32,
}

/// Flat ground: stone below, three layers of dirt, grass at height 10 (the top voxel is y = 9).
pub fn scene() -> Scene {
    let game = Game::new(&[Plugin::BaseBlocks]);
    let world = game.new_world();
    let material = world
        .main()
        .overrides
        .layout()
        .channel_index(&id("core:block"))
        .unwrap();
    let air = game.raw_block("base_blocks:air");
    Scene {
        game,
        world,
        material,
        air,
    }
}

impl Scene {
    /// What the server does when a region is watched: gives its terrain a generator.
    pub fn assign_around(&mut self, center: BrickCoordinate, radius: u32) {
        let level = self.world.main().terrain.assignment_cube_level();
        for cube in BrickCube::covering(BrickBox::around(center, radius), level) {
            self.world
                .main_mut()
                .terrain
                .assign_unassigned(cube.origin());
        }
    }

    pub fn mesher(&self, radius: i64) -> WorldMesher {
        self.mesher_seeing(radius, ViewChannels::new(self.material, None, None))
    }

    pub fn mesher_seeing(&self, radius: i64, channels: ViewChannels) -> WorldMesher {
        // Air is see-through; every other block of the fixtures is an opaque cube.
        let mut opaque = vec![true; 16];
        opaque[self.air as usize] = false;
        WorldMesher::new(
            ViewSettings {
                view_radius_bricks: radius,
                worker_count: 3,
            },
            channels,
            Arc::new(BlockTable::of_cubes(&opaque)),
        )
    }

    /// What the view draws from when a game's hook says how bright `lit` is and everything else is dark.
    pub fn channels_lit_by<H: BrickLight + 'static>(&self, hook: &Arc<H>) -> ViewChannels {
        let mut channels = ViewChannels::new(self.material, None, None);
        channels.bulk_light = Some(BulkLight(Arc::clone(hook) as Arc<dyn BrickLight>));
        channels
    }

    pub fn edit(&mut self, at: [i64; 3], block: &str) -> BrickCoordinate {
        let coordinate = VoxelCoordinate::new(at[0], at[1], at[2]);
        let (brick_coordinate, local) = coordinate.split();
        if self
            .world
            .main()
            .overrides
            .brick(brick_coordinate)
            .is_none()
        {
            let world_storage::BrickSource::Generated(generated) =
                self.world.main().effective_brick(brick_coordinate)
            else {
                panic!("the region must be assigned first");
            };
            self.world
                .main_mut()
                .overrides
                .insert_brick(brick_coordinate, std::sync::Arc::new(generated))
                .unwrap();
        }
        self.world
            .main_mut()
            .overrides
            .brick_mut(brick_coordinate)
            .unwrap()
            .set(self.material, local, self.game.raw_block(block));
        brick_coordinate
    }
}
