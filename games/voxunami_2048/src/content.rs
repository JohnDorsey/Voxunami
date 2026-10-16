//! What the world of 2048 is made of: air and the numbered blocks, and how worlds are made and
//! loaded.

use std::sync::Arc;

use content_registry::{IdentifierRegistry, Registry};
use game_shell::GameContent;
use namespaced_id::NamespacedId;
use terrain::{TerrainGenerator, WorldTerrain};
use voxel_grid::{ChannelDefinition, ChannelIndex, Grid, GridLayout};
use world_storage::{LoadContext, WorldData};

use crate::landscape::NumberedHills;
use crate::material::Material;
use crate::numbered_block::{AIR, MAXIMUM_EXPONENT, NumberedBlock};

/// Terrain generators are assigned to cubes of 8 bricks per side, as in Voxunami Cave.
const ASSIGNMENT_CUBE_LEVEL: u8 = 3;

fn identifier(text: &str) -> NamespacedId {
    text.parse().expect("constant identifiers are valid")
}

pub fn game_id() -> NamespacedId {
    identifier("voxunami_2048:game")
}

pub fn block_registry_kind() -> NamespacedId {
    identifier("voxunami_2048:block")
}

pub fn block_channel_identifier() -> NamespacedId {
    identifier("voxunami_2048:block")
}

pub fn numbered_hills_generator_identifier() -> NamespacedId {
    identifier("voxunami_2048:numbered_hills_v1")
}

/// How a block looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockAppearance {
    pub color: [f32; 3],
    pub opaque: bool,
}

/// A colour (sRGB) for a number, stepping around the hues so neighbouring numbers differ clearly.
fn number_color(exponent: u8) -> [f32; 3] {
    // Hue in sixths, converted without trigonometry.
    let hue = (f32::from(exponent - 1) * 0.37).fract() * 6.0;
    let (saturation, value) = (
        0.75,
        0.55 + 0.4 * (f32::from(exponent) / f32::from(MAXIMUM_EXPONENT)),
    );
    let channel = |offset: f32| {
        let position = (hue + offset).rem_euclid(6.0);
        let shape = (position - 3.0).abs() - 1.0;
        value * (1.0 - saturation * shape.clamp(0.0, 1.0))
    };
    [channel(0.0), channel(4.0), channel(2.0)]
}

/// The colour of a block: its number's colour, washed toward the colour of its material. Stone
/// shows the number's colour as it is; ice is paler and bluer; spring ice is paler and warmer.
fn block_color(block: NumberedBlock) -> [f32; 3] {
    let number = number_color(block.exponent());
    let (wash, amount) = match block.material {
        Material::Stone => return number,
        Material::Ice => ([0.85, 0.95, 1.0], 0.5),
        Material::SpringIce => ([1.0, 0.92, 0.65], 0.5),
    };
    std::array::from_fn(|channel| number[channel] + (wash[channel] - number[channel]) * amount)
}

/// The blocks, registered in a fixed order: air, then every numbered block in id order (see
/// [`NumberedBlock::id`]).
#[derive(Debug)]
pub struct Content2048 {
    blocks: Registry<BlockAppearance>,
}

impl Default for Content2048 {
    fn default() -> Self {
        Self::new()
    }
}

impl Content2048 {
    pub fn new() -> Self {
        let mut blocks = Registry::new();
        let air = blocks
            .register(
                identifier("voxunami_2048:air"),
                BlockAppearance {
                    color: [0.0; 3],
                    opaque: false,
                },
            )
            .expect("the registry is empty");
        assert_eq!(air.raw(), AIR);
        for block in NumberedBlock::all() {
            let registered = blocks
                .register(
                    identifier(&format!("voxunami_2048:{}", block.name())),
                    BlockAppearance {
                        color: block_color(block),
                        opaque: true,
                    },
                )
                .expect("the block names are distinct");
            assert_eq!(registered.raw(), block.id());
        }
        Self { blocks }
    }

    fn layout(&self) -> Arc<GridLayout> {
        Arc::new(
            GridLayout::new(vec![ChannelDefinition::content(
                block_channel_identifier(),
                AIR,
                block_registry_kind(),
            )])
            .expect("one channel is a valid layout"),
        )
    }

    /// The channel holding each voxel's block.
    pub fn block_channel(&self) -> ChannelIndex {
        self.layout()
            .channel_index(&block_channel_identifier())
            .expect("the layout has the block channel")
    }

    fn generators(&self, seed: u64) -> Registry<Arc<dyn TerrainGenerator>> {
        let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
        let generator: Arc<dyn TerrainGenerator> =
            Arc::new(NumberedHills::new(seed, self.block_channel()));
        generators
            .register(numbered_hills_generator_identifier(), generator)
            .expect("the registry is empty");
        generators
    }
}

impl GameContent for Content2048 {
    fn material_channel(&self) -> ChannelIndex {
        self.block_channel()
    }

    fn palette(&self) -> Vec<[f32; 3]> {
        self.blocks
            .iter()
            .map(|(_, entry)| entry.content().map_or([1.0, 0.0, 1.0], |block| block.color))
            .collect()
    }

    fn opacity_table(&self) -> Vec<bool> {
        self.blocks
            .iter()
            .map(|(_, entry)| entry.content().is_none_or(|block| block.opaque))
            .collect()
    }

    /// A new world: numbered hills as far as anyone goes, with nothing changed yet.
    fn new_world(&self, seed: u64) -> WorldData {
        let generators = self.generators(seed);
        let default = generators
            .runtime_id(&numbered_hills_generator_identifier())
            .expect("the generator was just registered");
        let terrain = WorldTerrain::new(generators, default, ASSIGNMENT_CUBE_LEVEL)
            .expect("the generator is available");
        WorldData::new(seed, terrain, Grid::new(self.layout()))
    }

    fn load_context(&mut self, seed: u64) -> LoadContext<'_> {
        let generators = self.generators(seed);
        let channel =
            ChannelDefinition::content(block_channel_identifier(), AIR, block_registry_kind());
        LoadContext {
            channels: vec![channel],
            registries: vec![(block_registry_kind(), &mut self.blocks)],
            generators,
            fallback_default_generator: numbered_hills_generator_identifier(),
            assignment_cube_level: ASSIGNMENT_CUBE_LEVEL,
        }
    }

    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        vec![(block_registry_kind(), &self.blocks)]
    }
}

#[cfg(test)]
mod tests {
    use world_storage::ParsedWorld;

    use super::*;

    #[test]
    fn blocks_are_registered_in_id_order_under_their_names() {
        let content = Content2048::new();
        let name = |raw: u32| content.blocks.identifier_of_raw(raw).unwrap().to_string();
        assert_eq!(name(AIR), "voxunami_2048:air");
        assert_eq!(name(1), "voxunami_2048:stone_2");
        assert_eq!(
            name(NumberedBlock::new(Material::Ice, 11).id()),
            "voxunami_2048:ice_2048"
        );
        assert_eq!(
            name(NumberedBlock::new(Material::SpringIce, MAXIMUM_EXPONENT).id()),
            "voxunami_2048:spring_ice_1048576"
        );
        assert_eq!(
            content.palette().len(),
            1 + 3 * usize::from(MAXIMUM_EXPONENT)
        );
        assert!(!content.opacity_table()[0]);
        assert!(content.opacity_table()[1..].iter().all(|&opaque| opaque));
    }

    fn distance(first: [f32; 3], second: [f32; 3]) -> f32 {
        (0..3)
            .map(|channel| (first[channel] - second[channel]).abs())
            .sum()
    }

    #[test]
    fn block_colours_are_valid_visible_and_tell_numbers_and_materials_apart() {
        for block in NumberedBlock::all() {
            let color = block_color(block);
            assert!(
                color.iter().all(|c| (0.0..=1.0).contains(c)),
                "{block:?}: {color:?}"
            );
            assert!(color.iter().any(|&c| c > 0.3), "{block:?} is nearly black");
        }
        for material in Material::ALL {
            // The first dozen numbers, the ones a game reaches, are clearly different.
            for first in 1..=12 {
                for second in first + 1..=12 {
                    let gap = distance(
                        block_color(NumberedBlock::new(material, first)),
                        block_color(NumberedBlock::new(material, second)),
                    );
                    assert!(gap > 0.1, "{material:?} {first} and {second}: {gap}");
                }
            }
        }
        // The same number in different materials differs.
        for exponent in 1..=MAXIMUM_EXPONENT {
            for (first, second) in [
                (Material::Stone, Material::Ice),
                (Material::Stone, Material::SpringIce),
                (Material::Ice, Material::SpringIce),
            ] {
                let gap = distance(
                    block_color(NumberedBlock::new(first, exponent)),
                    block_color(NumberedBlock::new(second, exponent)),
                );
                assert!(gap > 0.1, "{first:?} and {second:?} at {exponent}: {gap}");
            }
        }
    }

    #[test]
    fn a_new_world_changes_nothing_and_generates_numbered_hills() {
        let content = Content2048::new();
        let world = content.new_world(5);
        assert_eq!(world.overrides.brick_count(), 0);
        assert_eq!(world.world_seed, 5);
        assert_ne!(
            content.new_world(5).save(&content.registries()).unwrap(),
            content.new_world(6).save(&content.registries()).unwrap()
        );
    }

    #[test]
    fn a_world_saves_and_loads_back_the_same() {
        let content = Content2048::new();
        let world = content.new_world(11);
        let bytes = world.save(&content.registries()).unwrap();
        let mut later = Content2048::new();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let seed = parsed.world_seed();
        let (loaded, report) = parsed.load(later.load_context(seed)).unwrap();
        assert!(report.missing_content.is_empty() && report.extra_channels.is_empty());
        assert_eq!(loaded.save(&later.registries()).unwrap(), bytes);
    }
}
