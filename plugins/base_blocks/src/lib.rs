//! Plugin: the first blocks (air, stone, dirt, grass) and the rolling hills terrain made of them.
//!
//! The terrain generator `base_blocks:rolling_hills_v1` is part of every world's file format: its
//! output for a seed must never change. Better terrain is a new generator with a new name.

use std::sync::Arc;

use content_registry::{Registry, RuntimeId};
use namespaced_id::NamespacedId;
use terrain::{RollingHillsSettings, TerrainGenerator, TerrainMaterials, rolling_hills};
use voxel_grid::{ChannelDefinition, ChannelIndex};

/// How a block looks and behaves, as far as drawing is concerned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockAppearance {
    /// sRGB colour.
    pub color: [f32; 3],
    /// Whether the block hides the faces behind it.
    pub opaque: bool,
}

/// Shown for block types whose plugin is not loaded.
pub const MISSING_BLOCK_COLOR: [f32; 3] = [1.0, 0.0, 1.0];

fn identifier(text: &str) -> NamespacedId {
    text.parse().expect("constant identifiers are valid")
}

/// The registry kind block ids belong to.
pub fn block_registry_kind() -> NamespacedId {
    identifier("base_blocks:block")
}

/// The grid channel holding each voxel's block.
pub fn block_channel_identifier() -> NamespacedId {
    identifier("base_blocks:block")
}

/// The terrain generator version this plugin provides.
pub fn rolling_hills_generator_identifier() -> NamespacedId {
    identifier("base_blocks:rolling_hills_v1")
}

/// The blocks, registered in a fixed order: air first, so a world whose voxels are all zero is sky.
#[derive(Debug)]
pub struct BaseBlocks {
    pub blocks: Registry<BlockAppearance>,
    pub air: u32,
    pub stone: u32,
    pub dirt: u32,
    pub grass: u32,
}

impl Default for BaseBlocks {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseBlocks {
    pub fn new() -> Self {
        let mut blocks = Registry::new();
        let mut register = |name: &str, color: [f32; 3], opaque: bool| -> u32 {
            blocks
                .register(
                    identifier(&format!("base_blocks:{name}")),
                    BlockAppearance { color, opaque },
                )
                .expect("the block names are distinct")
                .raw()
        };
        let air = register("air", [0.0, 0.0, 0.0], false);
        let stone = register("stone", [0.50, 0.50, 0.53], true);
        let dirt = register("dirt", [0.45, 0.30, 0.18], true);
        let grass = register("grass", [0.30, 0.62, 0.20], true);
        Self {
            blocks,
            air,
            stone,
            dirt,
            grass,
        }
    }

    pub fn terrain_materials(&self) -> TerrainMaterials {
        TerrainMaterials {
            air: self.air,
            stone: self.stone,
            dirt: self.dirt,
            grass: self.grass,
        }
    }

    /// The channel definition for the block of each voxel; new voxels are air.
    pub fn block_channel(&self) -> ChannelDefinition {
        ChannelDefinition::content(block_channel_identifier(), self.air, block_registry_kind())
    }

    /// The terrain generators this plugin provides for a world with this seed, writing blocks into
    /// `block_channel` of the world's grid layout.
    pub fn terrain_generators(
        &self,
        seed: u64,
        block_channel: ChannelIndex,
    ) -> Registry<Arc<dyn TerrainGenerator>> {
        let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
        let generator: Arc<dyn TerrainGenerator> = Arc::new(rolling_hills(
            RollingHillsSettings::for_one_metre_voxels(seed),
            block_channel,
            self.terrain_materials(),
        ));
        generators
            .register(rolling_hills_generator_identifier(), generator)
            .expect("the registry is empty");
        generators
    }

    /// Colours by block id, for the renderer's palette. Blocks from plugins that are not loaded are
    /// magenta.
    pub fn palette(&self) -> Vec<[f32; 3]> {
        self.blocks
            .iter()
            .map(|(_, entry)| {
                entry
                    .content()
                    .map_or(MISSING_BLOCK_COLOR, |appearance| appearance.color)
            })
            .collect()
    }

    /// Whether each block id hides the faces behind it. Blocks that are not loaded count as opaque,
    /// so unknown content is drawn rather than disappearing.
    pub fn opacity_table(&self) -> Vec<bool> {
        self.blocks
            .iter()
            .map(|(_, entry)| entry.content().is_none_or(|appearance| appearance.opaque))
            .collect()
    }

    pub fn block_id(&self, identifier: &NamespacedId) -> Option<RuntimeId<BlockAppearance>> {
        self.blocks.runtime_id(identifier)
    }
}

#[cfg(test)]
mod tests {
    use content_registry::IdentifierRegistry;
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::BrickCoordinate;

    use super::*;

    #[test]
    fn air_is_block_zero_and_the_blocks_have_their_names() {
        let blocks = BaseBlocks::new();
        assert_eq!(
            (blocks.air, blocks.stone, blocks.dirt, blocks.grass),
            (0, 1, 2, 3)
        );
        let names: Vec<String> = (0..4)
            .map(|raw| blocks.blocks.identifier_of_raw(raw).unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "base_blocks:air",
                "base_blocks:stone",
                "base_blocks:dirt",
                "base_blocks:grass"
            ]
        );
        assert_eq!(blocks.block_channel().default_value, 0);
    }

    #[test]
    fn palette_and_opacity_follow_the_registry_with_missing_blocks_magenta_and_opaque() {
        let mut blocks = BaseBlocks::new();
        assert_eq!(blocks.opacity_table(), [false, true, true, true]);
        assert_eq!(blocks.palette().len(), 4);
        assert_eq!(blocks.palette()[3], [0.30, 0.62, 0.20]);
        let missing = blocks
            .blocks
            .register_missing(identifier("elsewhere:marble"));
        assert_eq!(missing.raw(), 4);
        assert_eq!(blocks.palette()[4], MISSING_BLOCK_COLOR);
        assert!(blocks.opacity_table()[4]);
    }

    #[test]
    fn the_rolling_hills_generator_is_pinned() {
        // A released generator version must never change its output: this brick of the surface of
        // the seed 2024 world is its fingerprint. If this fails, add a new generator version instead.
        let blocks = BaseBlocks::new();
        let fixture = StandardTestLayout::default();
        let generators = blocks.terrain_generators(2024, fixture.material);
        let id = generators
            .runtime_id(&rolling_hills_generator_identifier())
            .unwrap();
        let generator = generators.entry(id).unwrap().content().unwrap();
        let surface = generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 2, 0));
        let underground = generator.generate_brick(&fixture.layout, BrickCoordinate::new(3, 1, -2));
        assert_eq!(
            (surface.content_digest(), underground.content_digest()),
            (PINNED_SURFACE_DIGEST, PINNED_UNDERGROUND_DIGEST)
        );
    }

    const PINNED_SURFACE_DIGEST: u64 = 7_737_110_308_396_193_837;
    const PINNED_UNDERGROUND_DIGEST: u64 = 14_443_270_889_545_709_645;
}
