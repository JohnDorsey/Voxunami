//! Cave's terrain generators, made out of the first blocks: the file format of each is its output for a seed, pinned here.

use base_blocks::BaseBlocks;
use voxel_grid::test_support::StandardTestLayout;
use voxel_space::BrickCoordinate;
use voxunami_cave::generators::{
    PlanetSettings, empty_generator_identifier, generator_families,
    great_rivers_generator_identifier, planet_family_identifier,
    rolling_hills_generator_identifier, small_planet_generator_identifier, surface_layers,
    terrain_generators,
};

#[test]
fn the_rolling_hills_generator_is_pinned() {
    // A released generator version must never change its output: this brick of the surface of
    // the seed 2024 world is its fingerprint. If this fails, add a new generator version instead.
    let blocks = BaseBlocks::new();
    let fixture = StandardTestLayout::default();
    let generators = terrain_generators(&blocks.ids(), 2024, fixture.material);
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
fn the_empty_generator_is_third_in_the_table_and_makes_this_plugins_air() {
    let blocks = BaseBlocks::new();
    let fixture = StandardTestLayout::default();
    let generators = terrain_generators(&blocks.ids(), 2024, fixture.material);
    // The numbers of the generators of the worlds made before it do not move.
    let numbers: Vec<_> = [
        rolling_hills_generator_identifier(),
        small_planet_generator_identifier(),
        empty_generator_identifier(),
        great_rivers_generator_identifier(),
    ]
    .iter()
    .map(|identifier| generators.runtime_id(identifier).unwrap().raw())
    .collect();
    assert_eq!(numbers, [0, 1, 2, 3]);
    let id = generators
        .runtime_id(&empty_generator_identifier())
        .unwrap();
    let generator = generators.entry(id).unwrap().content().unwrap();
    let brick = generator.generate_brick(&fixture.layout, BrickCoordinate::new(5, -7, 9));
    assert_eq!(
        brick.channel(fixture.material).uniform_value(),
        Some(surface_layers(&blocks.ids()).air)
    );
}

#[test]
fn the_small_planet_generator_is_pinned_and_follows_the_hills_in_the_table() {
    // Like the hills: the bricks of the planet of the seed 2024 world, out of this plugin's blocks, are its fingerprint (the generator itself is pinned in `terrain`;
    // this pins what the plugin makes of it, which blocks the layers are).
    let blocks = BaseBlocks::new();
    let fixture = StandardTestLayout::default();
    let generators = terrain_generators(&blocks.ids(), 2024, fixture.material);
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
    let grass = surface_layers(&blocks.ids()).surface;
    let top = generator.generate_brick(&fixture.layout, BrickCoordinate::new(0, 1990, 0));
    assert!(
        top.channel(fixture.material)
            .iter()
            .any(|value| value == grass)
    );
    assert_eq!(digests, PINNED_PLANET_DIGESTS);
}

#[test]
fn a_planet_family_recipe_with_the_settings_of_the_small_planet_makes_the_small_planet_bit_for_bit()
{
    let blocks = BaseBlocks::new();
    let fixture = StandardTestLayout::default();
    let generators = terrain_generators(&blocks.ids(), 2024, fixture.material);
    let id = generators
        .runtime_id(&small_planet_generator_identifier())
        .unwrap();
    let small_planet = generators.entry(id).unwrap().content().unwrap();

    let families = generator_families(&blocks.ids(), fixture.material);
    assert_eq!(families.len(), 1);
    assert_eq!(families[0].0, planet_family_identifier());
    // (The name is in the recipes of every world that uses the family: a file format, so written out here.)
    assert_eq!(
        planet_family_identifier().as_str(),
        "voxunami_cave:planet_v1"
    );
    let made = families[0]
        .1
        .build(&PlanetSettings::one_percent_of_earth(2024).to_bytes())
        .unwrap();

    // The same bricks (the top, the side, a seam and the inside), the same coarse nodes, the same core.
    for coordinate in [
        BrickCoordinate::new(0, 1990, 0),
        BrickCoordinate::new(1989, 3, -2),
        BrickCoordinate::new(-1147, 1147, 1147),
        BrickCoordinate::new(40, 1000, -40),
    ] {
        assert_eq!(
            made.generate_brick(&fixture.layout, coordinate),
            small_planet.generate_brick(&fixture.layout, coordinate),
            "at {coordinate:?}"
        );
    }
    for cube in [
        region_octree::BrickCube::containing(BrickCoordinate::new(0, 1984, 0), 5),
        region_octree::BrickCube::containing(BrickCoordinate::new(0, 0, 0), 6),
    ] {
        assert_eq!(made.coarse_node(cube, 7), small_planet.coarse_node(cube, 7));
    }
    assert_eq!(made.solid_core(), small_planet.solid_core());
    // And a planet of another seed is another planet.
    let other = families[0]
        .1
        .build(&PlanetSettings::one_percent_of_earth(2025).to_bytes())
        .unwrap();
    let top = BrickCoordinate::new(0, 1990, 0);
    assert_ne!(
        other.generate_brick(&fixture.layout, top),
        small_planet.generate_brick(&fixture.layout, top)
    );
}

/// (The same as the bricks of the planet in `terrain`, which are pinned there: this plugin's air, stone, dirt and grass are the numbers 0 to 3.)
const PINNED_PLANET_DIGESTS: [u64; 3] = [
    15_048_038_413_391_879_901,
    13_937_117_706_274_179_919,
    9_257_172_923_548_608_195,
];

const PINNED_SURFACE_DIGEST: u64 = 7_737_110_308_396_193_837;
const PINNED_UNDERGROUND_DIGEST: u64 = 14_443_270_889_545_709_645;
