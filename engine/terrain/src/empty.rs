//! The terrain of a level in space: nothing but air everywhere, whatever is put in it later.

use voxel_grid::{Brick, ChannelIndex, GridLayout};
use voxel_space::BrickCoordinate;

use region_octree::BrickCube;

use crate::generator::TerrainGenerator;

/// Makes bricks of air and nothing else. Its version is part of the format of a world that uses it, like every generator's: air is air for ever.
#[derive(Clone, Copy, Debug)]
pub struct EmptyTerrain {
    material_channel: ChannelIndex,
    air: u32,
}

/// The generator of nothing: every voxel is `air` in `material_channel`.
pub fn empty(material_channel: ChannelIndex, air: u32) -> EmptyTerrain {
    EmptyTerrain {
        material_channel,
        air,
    }
}

impl TerrainGenerator for EmptyTerrain {
    fn generate_brick(&self, layout: &GridLayout, _: BrickCoordinate) -> Brick {
        let mut brick = Brick::new(layout);
        brick.channel_mut(self.material_channel).fill(self.air);
        brick
    }

    /// Nothing of it is ever drawn, from any distance.
    fn may_show(&self, _: BrickCube) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::{BRICK_VOXEL_COUNT, LocalVoxelIndex};

    use super::*;

    const AIR: u32 = 7;

    #[test]
    fn every_brick_is_air_all_through_in_the_block_channel_and_the_default_in_the_others() {
        let fixture = StandardTestLayout::default();
        let generator = empty(fixture.material, AIR);
        let reference = Brick::new(&fixture.layout);
        for coordinate in [
            BrickCoordinate::ORIGIN,
            BrickCoordinate::new(-3, 400, 9),
            BrickCoordinate::new(1_000_000, -1_000_000, 5),
        ] {
            let brick = generator.generate_brick(&fixture.layout, coordinate);
            assert_eq!(brick.channel(fixture.material).uniform_value(), Some(AIR));
            for local in LocalVoxelIndex::all() {
                assert_eq!(brick.get(fixture.material, local), AIR);
            }
            // The other channels are as a new brick has them.
            for channel in fixture.layout.channel_indices() {
                if channel != fixture.material {
                    assert_eq!(brick.channel(channel), reference.channel(channel));
                }
            }
        }
        assert_eq!(LocalVoxelIndex::all().len(), BRICK_VOXEL_COUNT);
    }

    #[test]
    fn nothing_of_it_may_show_at_any_size_and_it_has_no_coarse_cells_and_no_core() {
        let fixture = StandardTestLayout::default();
        let generator = empty(fixture.material, AIR);
        for level in [0, 1, 5, 20, 40] {
            for origin in [BrickCoordinate::ORIGIN, BrickCoordinate::new(-64, 128, 0)] {
                let cube = BrickCube::containing(origin, level);
                assert!(!generator.may_show(cube), "{cube:?}");
                if level > 0 {
                    assert!(generator.coarse_node(cube, 3).is_none());
                }
            }
        }
        assert_eq!(generator.solid_core(), None);
    }

    #[test]
    fn the_same_brick_every_time() {
        let fixture = StandardTestLayout::default();
        let generator = empty(fixture.material, AIR);
        let first = generator.generate_brick(&fixture.layout, BrickCoordinate::new(2, 3, 4));
        let second = generator.generate_brick(&fixture.layout, BrickCoordinate::new(-9, 0, 1));
        assert_eq!(first.content_digest(), second.content_digest());
    }
}
