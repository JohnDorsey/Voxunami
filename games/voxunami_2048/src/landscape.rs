//! The landscape of Voxunami 2048: rolling hills made entirely of numbered blocks.
//!
//! Every solid voxel is a numbered block. Small numbers are common (half are 2s, a quarter 4s, and
//! so on up to 64s), so there are always equal neighbours to merge. The material comes in cubes of
//! 8 voxels: mostly stone, with patches of ice and of spring ice to slide blocks on.
//!
//! The generator `voxunami_2048:numbered_hills_v1` is part of every world's file format: its output
//! for a seed must never change. A better landscape is a new generator with a new name.

use terrain::{
    DensityField, Dimensionality, FractalNoise, Heightfield, TerrainGenerator, lattice_hash,
};
use voxel_grid::{Brick, ChannelIndex, ChannelStorage, GridLayout};
use voxel_space::{
    BRICK_EDGE_LENGTH, BRICK_VOXEL_COUNT, BrickCoordinate, LevelOfDetail, LocalVoxelIndex,
    VoxelBox, VoxelCoordinate,
};

use crate::material::Material;
use crate::numbered_block::{AIR, NumberedBlock};

/// The largest exponent the landscape holds: 2^6 = 64.
const LARGEST_GENERATED_EXPONENT: u32 = 6;

/// Materials are chosen per cube of this many voxels along each side, given as a power of two.
const MATERIAL_CUBE_EDGE_LOG2: u32 = 3;

const NUMBER_SALT: u64 = 0x2048_0000_4E55_4D42;
const MATERIAL_SALT: u64 = 0x2048_0000_4D41_5445;

/// The block at a voxel, if the landscape is solid there. Depends on nothing but the seed and the
/// voxel, so bricks come out the same however and whenever they are generated.
pub fn numbered_block_at(seed: u64, voxel: VoxelCoordinate) -> NumberedBlock {
    let number = lattice_hash(seed ^ NUMBER_SALT, voxel.x(), voxel.y(), voxel.z());
    // Each further zero bit halves the chance: 1 in 2 for the 2s, 1 in 4 for the 4s, ...
    let exponent = 1 + number.trailing_zeros().min(LARGEST_GENERATED_EXPONENT - 1);
    let cube = lattice_hash(
        seed ^ MATERIAL_SALT,
        voxel.x() >> MATERIAL_CUBE_EDGE_LOG2,
        voxel.y() >> MATERIAL_CUBE_EDGE_LOG2,
        voxel.z() >> MATERIAL_CUBE_EDGE_LOG2,
    );
    let material = match cube % 10 {
        0..=6 => Material::Stone,
        7 | 8 => Material::Ice,
        _ => Material::SpringIce,
    };
    NumberedBlock::new(material, exponent as u8)
}

/// Hills of numbered blocks, with solid ground all the way down.
#[derive(Debug)]
pub struct NumberedHills {
    seed: u64,
    ground: Heightfield,
    block_channel: ChannelIndex,
}

impl NumberedHills {
    pub fn new(seed: u64, block_channel: ChannelIndex) -> Self {
        let hills = FractalNoise::standard(seed, Dimensionality::Planar, 6, 4, 14.0, 0.5);
        Self {
            seed,
            ground: Heightfield::new(0.0, hills),
            block_channel,
        }
    }

    /// Whether the landscape is solid at a voxel.
    pub fn is_solid(&self, voxel: VoxelCoordinate) -> bool {
        self.ground.sample(voxel, LevelOfDetail::FULL_DETAIL) > 0.0
    }
}

impl TerrainGenerator for NumberedHills {
    fn generate_brick(&self, layout: &GridLayout, coordinate: BrickCoordinate) -> Brick {
        let region = VoxelBox::of_brick(coordinate);
        let bounds = self.ground.bounds(&region, LevelOfDetail::FULL_DETAIL);
        let mut brick = Brick::new(layout);
        if bounds.upper() <= 0.0 {
            return brick; // all sky
        }
        let all_solid = bounds.lower() > 0.0;
        let origin = coordinate.origin();
        let mut blocks = vec![AIR; BRICK_VOXEL_COUNT];
        for z in 0..BRICK_EDGE_LENGTH {
            for y in 0..BRICK_EDGE_LENGTH {
                for x in 0..BRICK_EDGE_LENGTH {
                    let voxel = origin
                        .checked_offset(x as i64, y as i64, z as i64)
                        .expect("the voxels of a brick are within range");
                    if all_solid || self.is_solid(voxel) {
                        let local = LocalVoxelIndex::new(x as u8, y as u8, z as u8);
                        blocks[local.index()] = numbered_block_at(self.seed, voxel).id();
                    }
                }
            }
        }
        *brick.channel_mut(self.block_channel) = ChannelStorage::from_values(&blocks);
        brick
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use voxel_grid::test_support::StandardTestLayout;

    use super::*;

    fn generate(seed: u64, brick: BrickCoordinate) -> Brick {
        let fixture = StandardTestLayout::default();
        NumberedHills::new(seed, fixture.material).generate_brick(&fixture.layout, brick)
    }

    fn block_channel() -> ChannelIndex {
        StandardTestLayout::default().material
    }

    #[test]
    fn the_numbered_hills_generator_is_pinned() {
        // A released generator version must never change its output. These bricks of the seed 2048
        // world are its fingerprint: the ground's surface, deep ground, and sky. If this fails, add
        // a new generator version instead of changing this one.
        let surface = generate(2048, BrickCoordinate::new(0, 0, 0));
        let underground = generate(2048, BrickCoordinate::new(3, -4, -2));
        let sky = generate(2048, BrickCoordinate::new(1, 9, 1));
        assert_eq!(
            (
                surface.content_digest(),
                underground.content_digest(),
                sky.content_digest()
            ),
            (
                PINNED_SURFACE_DIGEST,
                PINNED_UNDERGROUND_DIGEST,
                PINNED_SKY_DIGEST
            )
        );
    }

    const PINNED_SURFACE_DIGEST: u64 = 4_541_207_031_239_676_783;
    const PINNED_UNDERGROUND_DIGEST: u64 = 15_234_628_967_280_726_699;
    const PINNED_SKY_DIGEST: u64 = 12_280_675_352_798_744_816;

    #[test]
    fn a_brick_is_the_same_every_time_and_depends_on_the_seed() {
        let first = generate(7, BrickCoordinate::new(0, 0, 0));
        assert_eq!(
            first.content_digest(),
            generate(7, BrickCoordinate::new(0, 0, 0)).content_digest()
        );
        assert_ne!(
            first.content_digest(),
            generate(8, BrickCoordinate::new(0, 0, 0)).content_digest()
        );
    }

    #[test]
    fn the_sky_is_empty_the_depths_are_solid_and_the_surface_is_both() {
        let channel = block_channel();
        let count_solid = |brick: &Brick| {
            LocalVoxelIndex::all()
                .filter(|&local| brick.get(channel, local) != AIR)
                .count()
        };
        assert_eq!(count_solid(&generate(5, BrickCoordinate::new(2, 9, 2))), 0);
        assert_eq!(
            count_solid(&generate(5, BrickCoordinate::new(2, -9, 2))),
            BRICK_VOXEL_COUNT
        );
        let solid = count_solid(&generate(5, BrickCoordinate::new(0, 0, 0)));
        assert!(0 < solid && solid < BRICK_VOXEL_COUNT, "{solid}");
    }

    #[test]
    fn generated_voxels_are_the_numbered_blocks_of_the_solid_ground() {
        let channel = block_channel();
        let seed = 31;
        let hills = NumberedHills::new(seed, channel);
        let coordinate = BrickCoordinate::new(-1, 0, 1);
        let brick = generate(seed, coordinate);
        for local in LocalVoxelIndex::all() {
            let voxel = coordinate.voxel_at(local);
            let expected = if hills.is_solid(voxel) {
                numbered_block_at(seed, voxel).id()
            } else {
                AIR
            };
            assert_eq!(brick.get(channel, local), expected, "{voxel}");
        }
    }

    #[test]
    fn numbers_are_mostly_small_and_every_material_appears_in_cubes() {
        let seed = 99;
        let mut counts = [0usize; 8];
        let mut materials = HashSet::new();
        let mut cube_materials = std::collections::HashMap::new();
        for x in -20..20 {
            for y in -20..20 {
                for z in -20..20 {
                    let voxel = VoxelCoordinate::new(x, y, z);
                    let block = numbered_block_at(seed, voxel);
                    counts[usize::from(block.exponent())] += 1;
                    materials.insert(block.material);
                    let cube = (x >> 3, y >> 3, z >> 3);
                    assert_eq!(
                        *cube_materials.entry(cube).or_insert(block.material),
                        block.material,
                        "{voxel} is in the same cube as voxels of another material"
                    );
                }
            }
        }
        let total = 40usize * 40 * 40;
        assert_eq!(counts[0], 0);
        assert_eq!(counts[7], 0, "nothing above 2^6 is generated");
        let share = |exponent: usize| counts[exponent] as f64 / total as f64;
        for exponent in 1..=5 {
            let expected = 1.0 / f64::from(1u32 << exponent);
            assert!(
                (share(exponent) - expected).abs() < 0.02,
                "{exponent}: {}",
                share(exponent)
            );
        }
        assert!(share(6) > 0.02, "64s are rare but exist");
        assert_eq!(materials.len(), 3);
    }
}
