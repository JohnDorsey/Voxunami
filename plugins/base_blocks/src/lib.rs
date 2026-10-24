//! Plugin: the first blocks (air, stone, dirt, grass, lamp, wood, fire) and the rolling hills
//! terrain made of them.
//!
//! The solid blocks come in several shapes, chosen by the voxel's *state* (the channel
//! `base_blocks:block_state`): a whole cube, slabs, and stairs, so shapes do not take block types
//! of their own.
//!
//! The terrain generator `base_blocks:rolling_hills_v1` is part of every world's file format: its
//! output for a seed must never change. Better terrain is a new generator with a new name.

use std::sync::Arc;

use block_models::{
    BlockDefinition, BlockTable, PropertyDeclaration, PropertyKey, PropertySchema, Shape,
};
use content_registry::{Registry, RuntimeId};
use namespaced_id::NamespacedId;
use terrain::{
    PlanetSettings, RollingHillsSettings, TerrainGenerator, TerrainMaterials, planet, rolling_hills,
};
use voxel_grid::{ChannelDefinition, ChannelIndex};
use voxel_space::FaceDirection;

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

/// The grid channel holding each voxel's state: which shape its block has.
pub fn state_channel_identifier() -> NamespacedId {
    identifier("base_blocks:block_state")
}

/// The grid channel holding each voxel's orientation: the number of a turn of the lattice (see `voxel_space::LatticeRotation::index`), 0 for none. It is
/// the same for every block, whatever the block looks like turned, so a ship can be turned by turning these numbers, and the blocks of a plugin that is not
/// loaded are turned too.
pub fn orientation_channel_identifier() -> NamespacedId {
    identifier("base_blocks:block_orientation")
}

/// The grid channel holding the block light of each voxel: 0 to 255, in the voxels light can be in
/// (air, and the empty parts of blocks) and 0 in the others.
pub fn light_channel_identifier() -> NamespacedId {
    identifier("base_blocks:block_light")
}

/// The light a block gives off, 0 (none) to 255. The light rules scale it to their own levels.
pub const EMISSION: PropertyKey<u8> = PropertyKey::new(
    "base_blocks:emission",
    0,
    "the light a block gives off, 0 (none) to 255",
);

/// How readily a block catches fire next to a fire, 0 (it does not) to 255. The fire rule decides what the
/// number means.
pub const FLAMMABILITY: PropertyKey<u8> = PropertyKey::new(
    "base_blocks:flammability",
    0,
    "how readily a block catches fire next to a fire, 0 (it does not) to 255",
);

/// Whether bodies move through a block as if it were not there (flames), though it can still be seen and
/// aimed at.
pub const PASSABLE: PropertyKey<bool> = PropertyKey::new(
    "base_blocks:passable",
    false,
    "whether bodies move through the block as if it were not there",
);

/// The properties of this plugin as builders on a definition, so that a block reads as what it is.
pub trait BlockPropertiesExt {
    /// The light the block gives off, 0 (none) to 255.
    fn with_emission(self, emission: u8) -> Self;
    /// How readily the block catches fire next to a fire, 0 (it does not) to 255.
    fn with_flammability(self, flammability: u8) -> Self;
    /// Whether bodies move through the block as if it were not there.
    fn with_passable(self, passable: bool) -> Self;
}

impl BlockPropertiesExt for BlockDefinition {
    fn with_emission(self, emission: u8) -> Self {
        self.with_property(&EMISSION, emission)
    }

    fn with_flammability(self, flammability: u8) -> Self {
        self.with_property(&FLAMMABILITY, flammability)
    }

    fn with_passable(self, passable: bool) -> Self {
        self.with_property(&PASSABLE, passable)
    }
}

/// The properties this plugin declares, for a game to add to its package.
pub fn property_declarations() -> Vec<PropertyDeclaration> {
    vec![
        EMISSION.declaration(),
        FLAMMABILITY.declaration(),
        PASSABLE.declaration(),
    ]
}

/// The engine's properties and this plugin's.
pub fn property_schema() -> PropertySchema {
    let mut schema = PropertySchema::new();
    for declaration in property_declarations() {
        schema
            .declare(declaration)
            .expect("the plugin's properties are distinct from the engine's");
    }
    schema
}

/// The colour of the ends of a log, as sRGB bytes.
pub const LOG_END_COLOR: [u8; 3] = [191, 148, 92];

/// How much light a lamp gives off. Light loses one level for every voxel it spreads, so this is
/// also how many voxels away a lamp lights things.
pub const LAMP_EMISSION: u8 = 64;

/// How readily grass and wood catch fire next to a fire: out of 1024 each tick, for each fire
/// beside them (see the fire rule in Voxunami Cave). Grass goes up fast, wood slowly.
pub const GRASS_FLAMMABILITY: u8 = 100;
pub const WOOD_FLAMMABILITY: u8 = 40;

/// How many steps a fire burns through before it goes out: its state is its age, counting up from
/// 0, and a fire of this age is gone. (The rule that ages it is the game's.)
pub const FIRE_LIFETIME: u32 = 12;

/// How much light a fire gives off.
pub const FIRE_EMISSION: u8 = 40;

/// The colours of a fire as it ages, hottest first, one for each age.
fn fire_colors() -> Vec<[f32; 3]> {
    (0..FIRE_LIFETIME)
        .map(|age| {
            let heat = 1.0 - age as f32 / FIRE_LIFETIME as f32;
            [0.95, 0.25 + 0.65 * heat, 0.05 + 0.2 * heat]
        })
        .collect()
}

/// The terrain generator version this plugin provides.
pub fn rolling_hills_generator_identifier() -> NamespacedId {
    identifier("base_blocks:rolling_hills_v1")
}

/// The terrain generator version of a planet with one hundredth of the radius of the Earth (63,710 voxels, `terrain::PlanetSettings::one_percent_of_earth`): a ball
/// with hills a kilometre wide, grass, dirt and stone, and the caves of the rolling hills. It is for a ship to be generated by (see `docs/planet-spec.md`).
pub fn small_planet_generator_identifier() -> NamespacedId {
    identifier("base_blocks:small_planet_v1")
}

/// What each state of a solid block of this plugin looks like, by name; the position is the state.
pub const SHAPE_NAMES: [&str; 7] = [
    "whole",
    "bottom slab",
    "top slab",
    "stair rising east",
    "stair rising west",
    "stair rising north",
    "stair rising south",
];

/// The shape of each state of a solid block: a whole cube, the two slabs, and a stair rising toward
/// each of the four sides (east is +x, north is -z).
pub fn standard_shapes() -> Vec<Shape> {
    vec![
        Shape::full(),
        Shape::half(FaceDirection::NegativeY),
        Shape::half(FaceDirection::PositiveY),
        Shape::stair(FaceDirection::PositiveX),
        Shape::stair(FaceDirection::NegativeX),
        Shape::stair(FaceDirection::NegativeZ),
        Shape::stair(FaceDirection::PositiveZ),
    ]
}

/// The name of a state of a solid block, as for [`SHAPE_NAMES`].
pub fn shape_name(state: u32) -> &'static str {
    SHAPE_NAMES.get(state as usize).copied().unwrap_or("whole")
}

/// The ids of the blocks, which are fixed: air first, so a world whose voxels are all zero is sky.
/// Everything a game needs to use the blocks and their channels and terrain once the registry is
/// the game's.
#[derive(Clone, Copy, Debug)]
pub struct BaseBlockIds {
    pub air: u32,
    pub stone: u32,
    pub dirt: u32,
    pub grass: u32,
    /// A solid block that gives off light. Not part of the terrain.
    pub lamp: u32,
    /// A solid block that burns.
    pub wood: u32,
    /// Flames: a low, lit, non-solid-looking block that spreads to what burns and goes out. Its state
    /// is its age, which any number can be (see [`FIRE_LIFETIME`]).
    pub fire: u32,
}

impl BaseBlockIds {
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

    /// The channel definition for the state of each voxel's block; new voxels are in state 0, the
    /// whole cube.
    pub fn state_channel(&self) -> ChannelDefinition {
        ChannelDefinition::number(state_channel_identifier(), 0)
    }

    /// The channel definition for the orientation of each voxel; new voxels are not turned.
    pub fn orientation_channel(&self) -> ChannelDefinition {
        ChannelDefinition::number(orientation_channel_identifier(), 0)
    }

    /// The channel definition for the light of each voxel; new voxels have none.
    pub fn light_channel(&self) -> ChannelDefinition {
        ChannelDefinition::number(light_channel_identifier(), 0)
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
        // (After the hills, so that the numbers of the generators of the worlds made before it are as they were.)
        let small_planet: Arc<dyn TerrainGenerator> = Arc::new(planet(
            PlanetSettings::one_percent_of_earth(seed),
            block_channel,
            self.terrain_materials(),
        ));
        generators
            .register(small_planet_generator_identifier(), small_planet)
            .expect("the identifier is new");
        generators
    }
}

/// The blocks, registered in a fixed order (see [`BaseBlockIds`]), with their ids. A game takes
/// the registry for its world and keeps the ids.
#[derive(Debug)]
pub struct BaseBlocks {
    pub blocks: Registry<BlockDefinition>,
    ids: BaseBlockIds,
}

impl std::ops::Deref for BaseBlocks {
    type Target = BaseBlockIds;

    fn deref(&self) -> &BaseBlockIds {
        &self.ids
    }
}

impl Default for BaseBlocks {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseBlocks {
    pub fn new() -> Self {
        let mut blocks = Registry::new();
        let mut register = |name: &str, definition: BlockDefinition| -> u32 {
            blocks
                .register(identifier(&format!("base_blocks:{name}")), definition)
                .expect("the block names are distinct")
                .raw()
        };
        let solid = |color: [f32; 3]| BlockDefinition::cube(color).with_shapes(standard_shapes());
        let air = register("air", BlockDefinition::air());
        let stone = register("stone", solid([0.50, 0.50, 0.53]));
        let dirt = register("dirt", solid([0.45, 0.30, 0.18]));
        let grass = register(
            "grass",
            solid([0.30, 0.62, 0.20]).with_flammability(GRASS_FLAMMABILITY),
        );
        let lamp = register(
            "lamp",
            BlockDefinition::cube([1.0, 0.92, 0.65]).with_emission(LAMP_EMISSION),
        );
        // Wood is a log: it is drawn with lighter ends, and when a player places it the line between its ends runs along the face it is placed against.
        let wood = register(
            "wood",
            solid([0.55, 0.38, 0.20])
                .with_flammability(WOOD_FLAMMABILITY)
                .with_marked_faces(LOG_END_COLOR, &[Some(FaceDirection::PositiveY); 7])
                .with_both_ends_marked()
                .with_axis_face(FaceDirection::PositiveY),
        );
        let fire = register(
            "fire",
            BlockDefinition::cube([0.95, 0.5, 0.1])
                .with_shapes(vec![Shape::half(FaceDirection::NegativeY)])
                .with_passable(true)
                .with_emission(FIRE_EMISSION)
                .with_state_colors(fire_colors()),
        );
        Self {
            blocks,
            ids: BaseBlockIds {
                air,
                stone,
                dirt,
                grass,
                lamp,
                wood,
                fire,
            },
        }
    }

    /// The ids of the blocks, to keep when the registry goes to a game.
    pub fn ids(&self) -> BaseBlockIds {
        self.ids
    }

    /// The registry of the blocks, to give to a game's content.
    pub fn into_registry(self) -> Registry<BlockDefinition> {
        self.blocks
    }

    /// What every block looks like and how it fills its cell, by block id. Blocks from plugins that
    /// are not loaded are opaque magenta cubes.
    pub fn block_table(&self) -> BlockTable {
        let definitions: Vec<Option<&BlockDefinition>> = self
            .blocks
            .iter()
            .map(|(_, entry)| entry.content())
            .collect();
        BlockTable::try_from_definitions(&property_schema(), &definitions)
            .expect("the base blocks' properties are declared")
    }

    pub fn block_id(&self, identifier: &NamespacedId) -> Option<RuntimeId<BlockDefinition>> {
        self.blocks.runtime_id(identifier)
    }
}

#[cfg(test)]
mod tests {
    use block_models::MISSING_BLOCK_COLOR;
    use content_registry::IdentifierRegistry;
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::BrickCoordinate;

    use super::*;

    #[test]
    fn air_is_block_zero_and_the_blocks_have_their_names() {
        let blocks = BaseBlocks::new();
        assert_eq!(
            (
                blocks.air,
                blocks.stone,
                blocks.dirt,
                blocks.grass,
                blocks.lamp,
                blocks.wood,
                blocks.fire
            ),
            (0, 1, 2, 3, 4, 5, 6)
        );
        let names: Vec<String> = (0..7)
            .map(|raw| blocks.blocks.identifier_of_raw(raw).unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "base_blocks:air",
                "base_blocks:stone",
                "base_blocks:dirt",
                "base_blocks:grass",
                "base_blocks:lamp",
                "base_blocks:wood",
                "base_blocks:fire"
            ]
        );
        assert_eq!(blocks.light_channel().default_value, 0);
        assert_eq!(
            blocks.light_channel().identifier.to_string(),
            "base_blocks:block_light"
        );
        assert_eq!(blocks.block_channel().default_value, 0);
        assert_eq!(blocks.state_channel().default_value, 0);
        assert_eq!(
            blocks.state_channel().identifier.to_string(),
            "base_blocks:block_state"
        );
    }

    #[test]
    fn solid_blocks_have_the_seven_shapes_and_air_the_lamp_and_fire_have_one() {
        let table = BaseBlocks::new().block_table();
        assert_eq!(table.state_count(0), 1);
        assert_eq!(table.state_count(4), 1);
        assert_eq!(
            table.state_count(6),
            1,
            "a fire has one shape, and ages are not shapes"
        );
        // The whole cube, and the slabs and stairs: how they stand is their orientation, which every block has, not a state.
        for block in [1, 2, 3, 5] {
            assert_eq!(table.state_count(block), SHAPE_NAMES.len() as u32);
        }
        // State 0 is the whole cube and the others are not; the names follow the shapes.
        let stone = |state| table.model_of(1, state, 0);
        assert!(table.is_opaque_cube(stone(0)));
        for state in 1..7 {
            assert!(!table.is_opaque_cube(stone(state)), "{}", shape_name(state));
            assert!(table.is_solid(stone(state)));
        }
        assert!(table.shape(stone(1)).covers_face(FaceDirection::NegativeY));
        assert!(table.shape(stone(2)).covers_face(FaceDirection::PositiveY));
        assert!(table.shape(stone(3)).covers_face(FaceDirection::PositiveX));
        assert!(table.shape(stone(4)).covers_face(FaceDirection::NegativeX));
        assert!(table.shape(stone(5)).covers_face(FaceDirection::NegativeZ));
        assert!(table.shape(stone(6)).covers_face(FaceDirection::PositiveZ));
        assert_eq!(shape_name(0), "whole");
        assert_eq!(shape_name(2), "top slab");
        assert_eq!(shape_name(99), "whole");
    }

    #[test]
    fn wood_is_a_log_with_its_line_up_and_down_and_both_ends_marked_and_nothing_else_is_turned_by_the_player()
     {
        let table = BaseBlocks::new().block_table();
        let wood = BaseBlocks::new().wood;
        assert_eq!(
            table.placement(wood, 0),
            block_models::Placement::AlongPlacedFace(FaceDirection::PositiveY)
        );
        // Marked at the top and at the bottom as it is made (the ends of the trunk), and the other faces are the bark.
        let ends: Vec<FaceDirection> = FaceDirection::ALL
            .into_iter()
            .filter(|face| table.material_of_face(wood, 0, 0, *face) != table.material_of(wood, 0))
            .collect();
        assert_eq!(ends, [FaceDirection::NegativeY, FaceDirection::PositiveY]);
        for block in [0, 1, 2, 3, 4, 6] {
            assert_eq!(
                table.placement(block, 0),
                block_models::Placement::Fixed,
                "{block}"
            );
        }
    }

    #[test]
    fn the_table_has_the_blocks_colours_air_is_see_through_and_missing_blocks_are_magenta_cubes() {
        let mut blocks = BaseBlocks::new();
        let table = blocks.block_table();
        // The blocks, then the colours a fire is drawn in as it ages, then the colour of the ends of a log.
        assert_eq!(table.palette().len(), 7 + FIRE_LIFETIME as usize + 1);
        assert_eq!(table.color(3), [0.30, 0.62, 0.20]);
        // Only the lamp and the fire give off light, only grass and wood burn, and bodies pass through flames and
        // nothing else of the blocks.
        let emission = table.handle(&EMISSION).unwrap();
        let flammability = table.handle(&FLAMMABILITY).unwrap();
        let passable = table.handle(&PASSABLE).unwrap();
        assert_eq!(
            (0..7)
                .map(|block| table.get_of(emission, block, 0))
                .collect::<Vec<_>>(),
            [0, 0, 0, 0, LAMP_EMISSION, 0, FIRE_EMISSION]
        );
        assert_eq!(
            (0..7)
                .map(|block| table.get_of(flammability, block, 0))
                .collect::<Vec<_>>(),
            [0, 0, 0, GRASS_FLAMMABILITY, 0, WOOD_FLAMMABILITY, 0],
            "grass and wood burn, and fire does not burn"
        );
        // The lamp is a whole opaque block.
        assert!(table.is_opaque_cube(table.model_of(4, 0, 0)));
        assert_eq!(
            (0..7)
                .map(|block| table.get_of(passable, block, 0))
                .collect::<Vec<_>>(),
            [false, false, false, false, false, false, true]
        );
        assert!(!table.is_solid(table.model_of(0, 0, 0)));
        assert!(table.is_see_through(table.model_of(0, 0, 0)));
        let missing = blocks
            .blocks
            .register_missing(identifier("elsewhere:marble"));
        assert_eq!(missing.raw(), 7);
        let table = blocks.block_table();
        assert_eq!(table.palette()[7], MISSING_BLOCK_COLOR);
        assert!(table.is_opaque_cube(table.model_of(7, 0, 0)));
    }

    #[test]
    fn the_plugins_properties_are_declared_once_beside_the_engines_and_an_unknown_block_has_none_of_them()
     {
        let declared: Vec<String> = property_schema().iter().map(|d| d.id.to_string()).collect();
        assert_eq!(
            declared,
            [
                "base_blocks:emission",
                "base_blocks:flammability",
                "base_blocks:passable",
                "engine:axis_face",
                "engine:density",
                "engine:docking_face",
                "engine:front_face",
                "engine:mark_both_ends",
                "engine:mark_color",
                "engine:marked_face",
                "engine:placeable_on_moving_grids"
            ]
        );
        let table = BaseBlocks::new().block_table();
        let emission = table.handle(&EMISSION).unwrap();
        let passable = table.handle(&PASSABLE).unwrap();
        // A block the table has no definition for: no light, and a wall.
        assert_eq!(table.get_of(emission, 9999, 0), 0);
        assert!(!table.get_of(passable, 9999, 0));
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

    #[test]
    fn the_small_planet_generator_is_pinned_and_follows_the_hills_in_the_table() {
        // Like the hills: the bricks of the planet of the seed 2024 world, out of this plugin's blocks, are its fingerprint (the generator itself is pinned in `terrain`;
        // this pins what the plugin makes of it, which blocks the layers are).
        let blocks = BaseBlocks::new();
        let fixture = StandardTestLayout::default();
        let generators = blocks.terrain_generators(2024, fixture.material);
        assert_eq!(
            generators.runtime_id(&rolling_hills_generator_identifier()),
            Some(content_registry::RuntimeId::from_raw(0)),
            "worlds made before the planet have the hills at 0"
        );
        let id = generators
            .runtime_id(&small_planet_generator_identifier())
            .unwrap();
        assert_eq!(id.raw(), 1);
        let generator = generators.entry(id).unwrap().content().unwrap();
        let digests: Vec<u64> = [
            BrickCoordinate::new(0, 1990, 0),
            BrickCoordinate::new(1989, 3, -2),
            BrickCoordinate::new(-1147, 1147, 1147),
        ]
        .into_iter()
        .map(|coordinate| {
            let brick = generator.generate_brick(&fixture.layout, coordinate);
            assert_eq!(brick.channel(fixture.material).uniform_value(), None);
            brick.content_digest()
        })
        .collect();
        // The top of the planet is made of this plugin's grass.
        let grass = blocks.terrain_materials().grass;
        let top = generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 1990, 0));
        assert!(
            top.channel(fixture.material)
                .iter()
                .any(|value| value == grass)
        );
        assert_eq!(digests, PINNED_PLANET_DIGESTS);
    }

    /// (The same as the bricks of the planet in `terrain`, which are pinned there: this plugin's air, stone, dirt and grass are the numbers 0 to 3.)
    const PINNED_PLANET_DIGESTS: [u64; 3] = [
        15_048_038_413_391_879_901,
        13_937_117_706_274_179_919,
        9_257_172_923_548_608_195,
    ];

    const PINNED_SURFACE_DIGEST: u64 = 7_737_110_308_396_193_837;
    const PINNED_UNDERGROUND_DIGEST: u64 = 14_443_270_889_545_709_645;
}
