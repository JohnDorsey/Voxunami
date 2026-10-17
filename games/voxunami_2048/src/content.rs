//! What the world of 2048 is made of: air and a block for each material, whose number is the state
//! of the voxel, and how worlds are made and loaded.

use std::sync::Arc;

use block_models::{BlockDefinition, BlockTable};
use content_registry::{IdentifierRegistry, Registry};
use game_shell::{ContentPackage, GameContent, PackageSpec};
use namespaced_id::NamespacedId;
use terrain::TerrainGenerator;
use voxel_grid::{ChannelDefinition, ChannelIndex};
use world_storage::{LoadContext, WorldData};
use world_view::ViewChannels;

use crate::landscape::NumberedHills;
use crate::material::Material;
use crate::numbered_block::AIR;

/// Terrain generators are assigned to cubes of 8 bricks per side, as in Voxunami Cave.
const ASSIGNMENT_CUBE_LEVEL: u8 = 3;

fn identifier(text: &str) -> NamespacedId {
    text.parse().expect("constant identifiers are valid")
}

pub fn game_id() -> NamespacedId {
    identifier("voxunami_2048:game")
}

/// The level players start in, and the only one a new world has.
pub fn landscape_id() -> NamespacedId {
    identifier("voxunami_2048:landscape")
}

pub fn block_registry_kind() -> NamespacedId {
    identifier("voxunami_2048:block")
}

/// The grid channel holding each voxel's block: air or a material.
pub fn block_channel_identifier() -> NamespacedId {
    identifier("voxunami_2048:block")
}

/// The grid channel holding each voxel's number, as the state of its block.
pub fn number_channel_identifier() -> NamespacedId {
    identifier("voxunami_2048:number")
}

pub fn numbered_hills_generator_identifier() -> NamespacedId {
    identifier("voxunami_2048:numbered_hills_v2")
}

/// How many colours the numbers of a material are drawn in. Every number has one, however large:
/// the colour of a number is that of its residue modulo this prime. Because 2 is a primitive root
/// modulo 61, the first 60 powers of two (2, 4, 8, ...) have 60 different residues, so each of them
/// has a colour of its own.
const NUMBER_COLOR_COUNT: usize = 61;

/// A colour (sRGB) for the number 2^`exponent`, stepping around the hues so that neighbouring
/// powers differ clearly; the brightness climbs and starts over every 20 powers.
fn power_color(exponent: u32) -> [f32; 3] {
    // Hue in sixths, converted without trigonometry.
    let hue = ((exponent - 1) as f32 * 0.37).fract() * 6.0;
    let (saturation, value) = (0.75, 0.55 + 0.4 * (((exponent - 1) % 20) as f32 / 19.0));
    let channel = |offset: f32| {
        let position = (hue + offset).rem_euclid(6.0);
        let shape = (position - 3.0).abs() - 1.0;
        value * (1.0 - saturation * shape.clamp(0.0, 1.0))
    };
    [channel(0.0), channel(4.0), channel(2.0)]
}

/// A colour washed toward the colour of a material. Stone shows the colour as it is; ice is paler
/// and bluer; spring ice is paler and warmer.
fn in_material(material: Material, color: [f32; 3]) -> [f32; 3] {
    let (wash, amount) = match material {
        Material::Stone => return color,
        Material::Ice => ([0.85, 0.95, 1.0], 0.5),
        Material::SpringIce => ([1.0, 0.92, 0.65], 0.5),
    };
    std::array::from_fn(|channel| color[channel] + (wash[channel] - color[channel]) * amount)
}

/// The colours of the numbers of a material, by residue modulo [`NUMBER_COLOR_COUNT`]. A residue is
/// that of exactly one of the powers of two 2^1 to 2^60, and takes its colour; the multiples of the
/// modulus are white.
fn number_colors(material: Material) -> Vec<[f32; 3]> {
    let mut colors = vec![in_material(material, [0.95; 3]); NUMBER_COLOR_COUNT];
    let mut residue = 1;
    for exponent in 1..NUMBER_COLOR_COUNT as u32 {
        residue = residue * 2 % NUMBER_COLOR_COUNT;
        colors[residue] = in_material(material, power_color(exponent));
    }
    colors
}

/// The colour a block of this material and number is drawn in.
pub fn number_color(material: Material, number: u32) -> [f32; 3] {
    number_colors(material)[number as usize % NUMBER_COLOR_COUNT]
}

/// The blocks, registered in a fixed order: air, then each material (see [`Material::block_id`]),
/// declared as a package.
#[derive(Debug)]
pub struct Content2048 {
    package: ContentPackage,
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
            .register(identifier("voxunami_2048:air"), BlockDefinition::air())
            .expect("the registry is empty");
        assert_eq!(air.raw(), AIR);
        for material in Material::ALL {
            let block = BlockDefinition::cube(in_material(material, [0.7; 3]))
                .with_state_colors(number_colors(material));
            let registered = blocks
                .register(
                    identifier(&format!("voxunami_2048:{}", material.name())),
                    block,
                )
                .expect("the material names are distinct");
            assert_eq!(registered.raw(), material.block_id());
        }
        let spec = PackageSpec {
            level: landscape_id(),
            block_registry_kind: block_registry_kind(),
            channels: Self::channels(),
            block_channel: block_channel_identifier(),
            state_channel: Some(number_channel_identifier()),
            light_channel: None,
            scaled_lights: Vec::new(),
            generators: Arc::new(|seed, layout| {
                let channel = |id: &NamespacedId| {
                    layout
                        .channel_index(id)
                        .expect("the layout has the channel")
                };
                let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
                generators
                    .register(
                        numbered_hills_generator_identifier(),
                        Arc::new(NumberedHills::new(
                            seed,
                            channel(&block_channel_identifier()),
                            channel(&number_channel_identifier()),
                        )),
                    )
                    .expect("the registry is empty");
                generators
            }),
            default_generator: numbered_hills_generator_identifier(),
            assignment_cube_level: ASSIGNMENT_CUBE_LEVEL,
            ambient_light: 0,
        };
        Self {
            package: ContentPackage::new(blocks, spec),
        }
    }

    fn channels() -> Vec<ChannelDefinition> {
        vec![
            ChannelDefinition::content(block_channel_identifier(), AIR, block_registry_kind()),
            ChannelDefinition::number(number_channel_identifier(), 0),
        ]
    }

    /// The channel holding each voxel's block.
    pub fn block_channel(&self) -> ChannelIndex {
        self.package.block_channel()
    }

    /// The channel holding each voxel's number.
    pub fn number_channel(&self) -> ChannelIndex {
        self.package.state_channel()
    }
}

impl GameContent for Content2048 {
    fn view_channels(&self) -> ViewChannels {
        self.package.view_channels()
    }

    fn block_table(&self) -> BlockTable {
        self.package.block_table()
    }

    /// A new world: numbered hills as far as anyone goes, with nothing changed yet.
    fn new_world(&self, seed: u64) -> WorldData {
        self.package.new_world(seed)
    }

    fn load_context(&mut self, seed: u64) -> LoadContext<'_> {
        self.package.load_context(seed)
    }

    fn registries(&self) -> Vec<(NamespacedId, &dyn IdentifierRegistry)> {
        self.package.registries()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use terrain::WorldTerrain;
    use voxel_grid::{Grid, GridLayout};
    use world_storage::{LevelData, ParsedWorld};

    use super::*;

    #[test]
    fn blocks_are_registered_as_air_then_the_materials() {
        let content = Content2048::new();
        let name = |raw: u32| {
            content.registries()[0]
                .1
                .identifier_of_raw(raw)
                .unwrap()
                .to_string()
        };
        assert_eq!(name(AIR), "voxunami_2048:air");
        assert_eq!(name(Material::Stone.block_id()), "voxunami_2048:stone");
        assert_eq!(name(Material::Ice.block_id()), "voxunami_2048:ice");
        assert_eq!(
            name(Material::SpringIce.block_id()),
            "voxunami_2048:spring_ice"
        );
        let table = content.block_table();
        assert_eq!(table.block_count(), 4);
        assert!(!table.is_solid(table.model_of(AIR, 0)));
        for material in Material::ALL {
            // Whatever the number, the block is a whole opaque cube.
            for number in [1, 2, 3, 2048, u32::MAX] {
                let model = table.model_of(material.block_id(), number);
                assert!(table.is_opaque_cube(model), "{material:?} {number}");
            }
        }
        // A palette entry for each block, then each material's colours for its numbers.
        assert_eq!(table.palette().len(), 4 + 3 * NUMBER_COLOR_COUNT);
    }

    #[test]
    fn two_is_a_primitive_root_of_the_colour_modulus_so_sixty_powers_of_two_differ() {
        let mut residues = HashSet::new();
        let mut residue = 1;
        for _ in 1..NUMBER_COLOR_COUNT {
            residue = residue * 2 % NUMBER_COLOR_COUNT;
            assert!(residues.insert(residue), "a power of two repeats a residue");
        }
        assert_eq!(residues.len(), NUMBER_COLOR_COUNT - 1);
        assert!(!residues.contains(&0));
    }

    fn distance(first: [f32; 3], second: [f32; 3]) -> f32 {
        (0..3)
            .map(|channel| (first[channel] - second[channel]).abs())
            .sum()
    }

    #[test]
    fn colours_are_valid_visible_and_tell_numbers_and_materials_apart() {
        for material in Material::ALL {
            let colors = number_colors(material);
            for (residue, color) in colors.iter().enumerate() {
                assert!(
                    color.iter().all(|c| (0.0..=1.0).contains(c)),
                    "{material:?} {residue}: {color:?}"
                );
                assert!(color.iter().any(|&c| c > 0.3), "{residue} is nearly black");
            }
            for first in 0..colors.len() {
                for second in first + 1..colors.len() {
                    assert_ne!(colors[first], colors[second], "{first} and {second}");
                }
            }
            // The first dozen powers of two, the ones a game reaches, are clearly different.
            for first in 1..=12 {
                for second in first + 1..=12 {
                    let gap = distance(
                        number_color(material, 1 << first),
                        number_color(material, 1 << second),
                    );
                    assert!(gap > 0.1, "{material:?} 2^{first} and 2^{second}: {gap}");
                }
            }
        }
        // The same number in different materials differs.
        for number in [1, 2, 3, 4, 1000, 65_536, 4_000_000_000] {
            for (first, second) in [
                (Material::Stone, Material::Ice),
                (Material::Stone, Material::SpringIce),
                (Material::Ice, Material::SpringIce),
            ] {
                let gap = distance(number_color(first, number), number_color(second, number));
                assert!(gap > 0.1, "{first:?} and {second:?} at {number}: {gap}");
            }
        }
    }

    #[test]
    fn any_number_however_large_has_a_colour_and_the_table_draws_that_one() {
        let table = Content2048::new().block_table();
        let palette = table.palette();
        for material in Material::ALL {
            for number in [1, 2, 61, 62, 4096, 1 << 31, u32::MAX] {
                let drawn = palette[table.material_of(material.block_id(), number) as usize];
                assert_eq!(
                    drawn,
                    number_color(material, number),
                    "{material:?} {number}"
                );
            }
        }
        // Numbers that differ by the modulus look alike; neighbouring numbers do not.
        assert_eq!(
            number_color(Material::Stone, 5),
            number_color(Material::Stone, 5 + NUMBER_COLOR_COUNT as u32)
        );
        assert_ne!(
            number_color(Material::Stone, 5),
            number_color(Material::Stone, 6)
        );
    }

    #[test]
    fn a_new_world_changes_nothing_and_generates_numbered_hills() {
        let content = Content2048::new();
        let world = content.new_world(5);
        assert_eq!(world.main().overrides.brick_count(), 0);
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

    #[test]
    fn a_world_made_with_the_retired_first_generator_still_loads_and_is_kept_as_it_is() {
        // Before numbers were states, the landscape was made by `numbered_hills_v1` out of a block
        // for every number. That generator is gone, and a world that names it keeps what it says
        // (the saved edits, the generator's name) without the terrain it made.
        let content = Content2048::new();
        let retired = identifier("voxunami_2048:numbered_hills_v1");
        let mut generators: Registry<Arc<dyn TerrainGenerator>> = Registry::new();
        let stand_in: Arc<dyn TerrainGenerator> = Arc::new(NumberedHills::new(
            3,
            content.block_channel(),
            content.number_channel(),
        ));
        let default = generators.register(retired.clone(), stand_in).unwrap();
        let terrain = WorldTerrain::new(generators, default, ASSIGNMENT_CUBE_LEVEL).unwrap();
        let mut old_world = WorldData::new(
            3,
            landscape_id(),
            LevelData::new(
                terrain,
                Grid::new(Arc::new(GridLayout::new(Content2048::channels()).unwrap())),
            ),
        );
        let edited = voxel_space::VoxelCoordinate::new(4, 5, 6);
        let stone = Material::Stone;
        old_world
            .main_mut()
            .overrides
            .set(content.block_channel(), edited, stone.block_id());
        old_world
            .main_mut()
            .overrides
            .set(content.number_channel(), edited, 6);
        let bytes = old_world.save(&content.registries()).unwrap();

        let mut later = Content2048::new();
        let parsed = ParsedWorld::parse(&bytes).unwrap();
        let (loaded, report) = parsed.load(later.load_context(3)).unwrap();
        assert_eq!(
            report.unavailable_default_generators,
            [(landscape_id(), retired.clone())]
        );
        assert_eq!(
            loaded.main().preferred_default_generator(),
            &retired,
            "the world still says which generator made it"
        );
        let overrides = &loaded.main().overrides;
        assert_eq!(
            (
                overrides.get(later.block_channel(), edited),
                overrides.get(later.number_channel(), edited)
            ),
            (stone.block_id(), 6),
            "the saved edits are kept"
        );
        // Saving lists the generator the game now has beside the retired one, and from then on
        // the file no longer changes by being loaded and saved.
        let resaved = loaded.save(&later.registries()).unwrap();
        let mut again = Content2048::new();
        let (reloaded, _) = ParsedWorld::parse(&resaved)
            .unwrap()
            .load(again.load_context(3))
            .unwrap();
        assert_eq!(reloaded.save(&again.registries()).unwrap(), resaved);
    }
}
