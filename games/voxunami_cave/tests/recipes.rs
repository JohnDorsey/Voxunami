//! A planet that is data: a recipe of the planet family (a seed, a radius, hills and caves as numbers) is added to a world, a ship is given the generator it makes,
//! and that is a second planet of another size. The world keeps the recipe, a client that joins makes the same generator from the description of the world, a player
//! falls onto it and stands on its ground, and a restart finds all of it again.

mod common;

use common::{SEED, Scene, player_pose, ship_id, tick_and_follow};
use game_shell::GameContent;
use terrain::TerrainGenerator;
use voxel_space::{
    BrickBox, BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, WorldPosition,
};
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::CaveContent;
use voxunami_cave::generators::PlanetSettings;
use voxunami_cave::generators::planet_family_identifier;
use world_storage::{GeneratorRecipe, GridTerrain, MovingGrid, PilotLogout, WorldData};

/// The moon: a ball of 300 voxels with low hills, a seed of its own.
fn moon_settings() -> PlanetSettings {
    PlanetSettings {
        seed: 99,
        radius: 300.0,
        hill_height: 6.0,
        largest_hill_wavelength_log2: 5,
        hill_octave_count: 3,
        cave_wavelength_log2: 4,
        cave_half_thickness: 0.05,
    }
}

fn moon_recipe() -> GeneratorRecipe {
    GeneratorRecipe {
        instance: "voxunami_cave:test_moon".parse().unwrap(),
        family: planet_family_identifier(),
        parameters: moon_settings().to_bytes(),
    }
}

/// How many bricks the moon's generator makes either way of its centre: its radius and its hills, and some.
const MOON_REACH_BRICKS: u32 = 12;

/// Where the moon is: 400 kilometres along x from the planet of the new world, in space.
fn moon_pose() -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::new(12_500, 0, 0), [0.0; 3]),
        Orientation::IDENTITY,
    )
}

fn moon() -> GridId {
    // (The planet of a new world is the first ship.)
    ship_id(1)
}

/// A new world of Cave's with the moon in it, made as data.
fn world_with_a_moon(content: &CaveContent) -> WorldData {
    let mut world = content.new_world(SEED);
    let generator = content
        .add_generator_recipe(&mut world, moon_recipe())
        .expect("the planet family is Cave's");
    // Twice the least a ship can weigh and still pull (10^19 kilograms: a moon this small has to be very dense to pull at all, some 15,000 metres a second
    // a second at its surface; a pull of ten at the surface wants a radius of some eight kilometres).
    let mass = voxunami_cave::game::PULLING_MASS * 2.0;
    let first = world.main();
    let ship = MovingGrid {
        level: LevelIndex::MAIN,
        pose: moon_pose(),
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
        mass,
        centre_of_mass: [0.0; 3],
        held: true,
        frozen: false,
        on_pilot_logout: PilotLogout::Stay,
        voxels: voxel_grid::Grid::new(std::sync::Arc::clone(first.overrides.layout())),
        terrain: Some(GridTerrain {
            generator,
            extent: BrickBox::around(BrickCoordinate::ORIGIN, MOON_REACH_BRICKS),
            mass,
        }),
        rail: None,
        sections: Vec::new(),
    };
    let id = world.grids_mut().create(ship);
    assert_eq!(id, moon());
    world
}

fn a_scene_with_a_moon() -> Scene {
    let content = CaveContent::new();
    let world = world_with_a_moon(&content);
    Scene::in_this_world(content, world)
}

/// The moon's generator in the server's table, and what the table calls it.
fn moon_generator_name(scene: &Scene) -> (String, bool) {
    let world = scene.rig.server.world();
    let generator = world
        .grids()
        .get(moon())
        .unwrap()
        .terrain
        .unwrap()
        .generator;
    let entry = world.main().terrain.generators().entry(generator).unwrap();
    (entry.identifier().to_string(), entry.is_missing())
}

#[test]
fn a_planet_made_from_a_recipe_is_a_ship_whose_generator_the_world_names_and_keeps() {
    let scene = a_scene_with_a_moon();
    assert_eq!(
        moon_generator_name(&scene),
        ("voxunami_cave:test_moon".to_owned(), false)
    );
    assert_eq!(
        &scene.rig.server.world().generator_recipes()[1..],
        &[moon_recipe()]
    );
    // It is the generator of the settings: the brick at the pole of the moon is the brick a planet with those settings makes.
    let generator = scene
        .rig
        .server
        .world()
        .grids()
        .get(moon())
        .unwrap()
        .terrain
        .unwrap()
        .generator;
    let layout = std::sync::Arc::clone(scene.rig.server.world().main().overrides.layout());
    let made = scene
        .rig
        .server
        .world()
        .main()
        .terrain
        .generators()
        .entry(generator)
        .unwrap()
        .content()
        .unwrap()
        .generate_brick(&layout, BrickCoordinate::new(0, 9, 0));
    let blocks = &scene.rig.content.blocks;
    let direct = voxunami_cave::generators::planet(
        moon_settings(),
        scene.rig.content.block_channel(),
        voxunami_cave::generators::surface_layers(blocks),
    )
    .generate_brick(&layout, BrickCoordinate::new(0, 9, 0));
    assert_eq!(made, direct);
    assert_ne!(
        made.channel(scene.rig.content.block_channel())
            .uniform_value(),
        Some(blocks.air),
        "the top of the moon is not sky"
    );
}

#[test]
fn a_client_that_joins_makes_the_same_generator_from_the_description_of_the_world() {
    let mut scene = a_scene_with_a_moon();
    let mut player = scene.rig.join([0.5, 70_000.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 10);

    let client = player.client().world();
    let summary = client.summary();
    let told: Vec<_> = summary
        .grids
        .iter()
        .filter_map(|grid| grid.terrain.clone())
        .collect();
    assert!(
        told.iter().any(|(generator, extent)| {
            generator.as_str() == "voxunami_cave:test_moon"
                && *extent == BrickBox::around(BrickCoordinate::ORIGIN, MOON_REACH_BRICKS)
        }),
        "the client was told of a planet whose generator is the recipe's: {told:?}"
    );
    // The client's table has it, made, and it makes what the server's makes.
    let generators = client.world().main().terrain.generators();
    let number = generators
        .runtime_id(&"voxunami_cave:test_moon".parse().unwrap())
        .expect("the client has the generator");
    assert!(!generators.entry(number).unwrap().is_missing());
    // (The planet of the world is the first recipe, from Cave's own data; the moon is the second.)
    assert_eq!(
        client.world().generator_recipes()[0].instance.as_str(),
        "voxunami_cave:planet"
    );
    assert_eq!(&client.world().generator_recipes()[1..], &[moon_recipe()]);
    let layout = std::sync::Arc::clone(client.world().main().overrides.layout());
    let at = BrickCoordinate::new(2, 9, -1);
    let on_the_client = generators
        .entry(number)
        .unwrap()
        .content()
        .unwrap()
        .generate_brick(&layout, at);
    let server_generator = scene
        .rig
        .server
        .world()
        .grids()
        .get(moon())
        .unwrap()
        .terrain
        .unwrap()
        .generator;
    let on_the_server = scene
        .rig
        .server
        .world()
        .main()
        .terrain
        .generators()
        .entry(server_generator)
        .unwrap()
        .content()
        .unwrap()
        .generate_brick(&layout, at);
    assert_eq!(on_the_client, on_the_server);
}

#[test]
fn a_walker_dropped_over_the_moon_lands_on_it_and_stands_on_ground_a_recipe_made() {
    let mut scene = a_scene_with_a_moon();
    // Thirty voxels over where the top of the moon is, to the nearest ten (the hills are six voxels high), at the moon's pole.
    let mut player = scene.rig.join_walking([400_000.5, 330.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 150);

    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, moon(), "aboard the moon");
    // In the moon's own coordinates, whose origin is its centre: standing on its ground, 1.62 over the top voxel (300 voxels and the hills, at the pole).
    let [x, y, z] = in_voxels(pose.position);
    assert!((x - 0.5).abs() < 0.5 && (z - 0.5).abs() < 0.5, "{x} {z}");
    assert!(
        (290.0..=320.0).contains(&y),
        "the eyes are at the surface of a moon of 300 voxels: {y}"
    );
    // Still, to the bit, after more ticks.
    let standing = pose;
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(player_pose(&scene, 1), standing);
}

#[test]
fn a_restart_finds_the_recipe_the_generator_and_the_moon_where_they_were() {
    let mut scene = a_scene_with_a_moon();
    scene.ticks(5);
    let before = scene.rig.server.world().grids().get(moon()).unwrap().pose;
    let mut scene = scene.restarted();
    // The ships of the file come into memory at the first look.
    scene.ticks(1);

    assert_eq!(
        &scene.rig.server.world().generator_recipes()[1..],
        &[moon_recipe()]
    );
    assert_eq!(
        moon_generator_name(&scene),
        ("voxunami_cave:test_moon".to_owned(), false)
    );
    let after = scene.rig.server.world().grids().get(moon()).unwrap();
    assert_eq!(after.pose, before);
    assert_eq!(
        after.terrain.unwrap().extent,
        BrickBox::around(BrickCoordinate::ORIGIN, MOON_REACH_BRICKS)
    );
}
