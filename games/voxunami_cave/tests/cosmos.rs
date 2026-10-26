//! The world a new game starts in (Phase 39f): a level of space with a planet in it, first, and the overworld second. And the world the tests of the overworld start in.

mod common;

use base_blocks::{empty_generator_identifier, rolling_hills_generator_identifier};
use std::sync::{Arc, Mutex};

use common::{
    PlayerExt, SEED, Scene, make_ship, player_pose, pose_at, say, ship_id, tick_and_follow,
    top_of_the_column,
};
use game_shell::{GameContent, GameDefinition};
use server::GridView;
use terrain::PlanetSettings;
use voxel_space::{
    BrickCoordinate, GridId, GridPose, LevelIndex, Orientation, VoxelCoordinate, WorldPosition,
};
use voxunami_cave::Cave;
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::{CaveContent, mini_cosmos_id, overworld_id};
use voxunami_cave::game::{break_block, place_block};
use voxunami_cave::landing::{on_a_planet, spawn_in};
use voxunami_cave::plugins::world_spawn;
use world_storage::ShipFiles;

/// The levels of a world, each with its identifier and the generator that is the default of its regions.
fn levels_of(world: &world_storage::WorldData) -> Vec<(String, String)> {
    world
        .levels()
        .map(|(_, id, level)| {
            (
                id.to_string(),
                level.preferred_default_generator().to_string(),
            )
        })
        .collect()
}

#[test]
fn a_new_world_is_space_first_and_the_overworld_second_with_a_planet_in_space() {
    let content = CaveContent::new();
    let world = content.new_world(SEED);
    assert_eq!(
        levels_of(&world),
        [
            (
                mini_cosmos_id().to_string(),
                empty_generator_identifier().to_string()
            ),
            (
                overworld_id().to_string(),
                rolling_hills_generator_identifier().to_string()
            ),
        ]
    );
    // Players start in the first.
    assert_eq!(world.level_id(LevelIndex::MAIN), Some(&mini_cosmos_id()));

    // One ship, the planet: in space, at its origin, not moving, with nothing stored, made by the planet's generator, and as heavy as it must be to pull ten metres a
    // second a second at its surface.
    assert_eq!(world.grids().len(), 1);
    let planet = world
        .grids()
        .get(ship_id(0))
        .expect("the planet is the first ship");
    assert_eq!(planet.level, LevelIndex::MAIN);
    assert_eq!(planet.pose, GridPose::identity());
    assert!(planet.held && !planet.frozen);
    assert_eq!(planet.centre_of_mass, [0.0; 3]);
    assert_eq!(planet.voxels.brick_count(), 0);
    let terrain = planet.terrain.expect("the planet has terrain");
    let generators = world.main().terrain.generators();
    assert_eq!(
        generators.entry(terrain.generator).unwrap().identifier(),
        &"voxunami_cave:planet".parse().unwrap()
    );
    let radius = PlanetSettings::one_percent_of_earth(SEED).radius;
    let mass_for_ten = 10.0 * radius * radius / 6.674e-11;
    assert!((f64::from(planet.mass) / mass_for_ten - 1.0).abs() < 1.0e-6);
    assert_eq!(terrain.mass, planet.mass);
    // The extent holds the ground, hills and all: the hills of a face are 320 voxels at the most and the blend of three can be 1.8 times that.
    let extent = terrain.extent;
    let reach = |brick: i64| brick.abs() as f64 * 32.0;
    for coordinate in [extent.min(), extent.max()] {
        for axis in [coordinate.x(), coordinate.y(), coordinate.z()] {
            assert!(reach(axis) > radius + 600.0, "{extent:?}");
        }
    }
    // Space itself has nothing in it: not a brick stored, and the overworld none either.
    assert_eq!(world.main().overrides.brick_count(), 0);
    assert_eq!(
        world
            .level(LevelIndex::new(1))
            .expect("the overworld is the second level")
            .overrides
            .brick_count(),
        0
    );
}

#[test]
fn the_world_the_tests_of_the_overworld_start_in_is_the_overworld_alone() {
    let content = CaveContent::new();
    let world = content.new_overworld_world(SEED);
    assert_eq!(
        levels_of(&world),
        [(
            overworld_id().to_string(),
            rolling_hills_generator_identifier().to_string()
        )]
    );
    assert!(world.grids().is_empty());
    assert_eq!(world.level_id(LevelIndex::MAIN), Some(&overworld_id()));
}

#[test]
fn the_planet_of_a_new_world_is_still_there_with_what_was_dug_in_it_after_a_restart() {
    let mut scene = Scene::in_a_new_world();
    // Dig out a block of voxels of the planet deep under its surface (stone, with a cave here and there), as a rule of a game would.
    let content = CaveContent::new();
    let (block_channel, air) = (content.block_channel(), content.blocks.air);
    scene.run(move |world| {
        let mut planet = world.grid(ship_id(0)).unwrap();
        for x in 0..3 {
            for y in 63_000..63_003 {
                for z in 0..3 {
                    planet
                        .set(block_channel, VoxelCoordinate::new(x, y, z), air)
                        .unwrap();
                }
            }
        }
    });
    let before = scene.rig.server.world().grids().get(ship_id(0)).unwrap();
    let (stored, digest, mass, terrain) = (
        before.voxels.brick_count(),
        before.voxels.content_digest(),
        before.mass,
        before.terrain,
    );
    assert!(stored > 0, "the dig is stored");

    // The world comes back with the planet in its file, as it does for every ship, and the first look at the ships brings it in, with no player anywhere near.
    let mut scene = scene.restarted();
    assert!(scene.rig.server.world().grids().get(ship_id(0)).is_none());
    scene.ticks(1);
    let after = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .expect("the planet came back");
    assert_eq!(after.mass, mass);
    assert_eq!(after.terrain, terrain);
    assert_eq!(after.voxels.brick_count(), stored);
    assert_eq!(after.voxels.content_digest(), digest);
    assert!(after.held);
    assert_eq!(after.level, LevelIndex::MAIN);
    assert_eq!(
        scene.rig.server.world().level_id(LevelIndex::MAIN),
        Some(&mini_cosmos_id())
    );
}

/// Where a player's eyes land on the first planet of `level`, and on that of the overworld, as the server works it out.
fn landings(scene: &mut Scene) -> (Option<[f64; 3]>, Option<[f64; 3]>) {
    let content = CaveContent::new();
    let (channel, air) = (content.block_channel(), content.blocks.air);
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        let place = |level| on_a_planet(world, level, channel, air).map(in_voxels);
        *slot.lock().unwrap() = Some((place(LevelIndex::MAIN), place(LevelIndex::new(1))));
    });
    let places = found.lock().unwrap().take();
    places.unwrap()
}

#[test]
fn a_player_lands_on_top_of_a_whole_brick_of_stone_that_was_built_over_the_pole_and_not_inside_it()
{
    let mut scene = Scene::in_a_new_world();
    let (ground, _) = landings(&mut scene);
    let ground = ground.unwrap();
    // A brick of stone, all of it, two bricks over the ground: not a block in the column but a brick that is one value all through, which the landing passes in one look
    // and must not pass by.
    let top = top_of_the_column(0, 0);
    let brick_y = top.div_euclid(32) + 2;
    let content = CaveContent::new();
    let (channel, stone) = (content.block_channel(), content.blocks.stone);
    scene.run(move |world| {
        let mut planet = world.grid(ship_id(0)).unwrap();
        for x in 0..32 {
            for y in brick_y * 32..(brick_y + 1) * 32 {
                for z in 0..32 {
                    planet
                        .set(channel, VoxelCoordinate::new(x, y, z), stone)
                        .unwrap();
                }
            }
        }
    });
    let (built, _) = landings(&mut scene);
    let [_, y, _] = built.expect("the planet has a pole");
    assert!(
        (y - (((brick_y + 1) * 32) as f64 + 1.64)).abs() < 1.0e-3,
        "{y} over the stone brick that ends at {} (the ground was at {})",
        (brick_y + 1) * 32,
        ground[1]
    );
}

#[test]
fn a_player_lands_where_the_planet_is_and_on_a_planet_and_not_on_a_ship_that_was_there_first() {
    let mut scene = Scene::in_a_new_world();
    let (at_the_origin, _) = landings(&mut scene);
    let at_the_origin = at_the_origin.unwrap();
    // The planet is somewhere else in space, and an ordinary ship, made before it was put back, is the first ship of the level.
    scene.run(|world| {
        let terrain = world
            .data()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .terrain
            .unwrap();
        world.remove_grid(ship_id(0)).unwrap();
        world
            .create_grid(LevelIndex::MAIN, GridPose::identity())
            .unwrap();
        let moved = world
            .create_grid(
                LevelIndex::MAIN,
                GridPose::new(
                    WorldPosition::new(BrickCoordinate::ORIGIN, [100.0, 50.0, -20.0]),
                    Orientation::IDENTITY,
                ),
            )
            .unwrap();
        world.set_grid_terrain(moved, terrain).unwrap();
    });
    let (elsewhere, _) = landings(&mut scene);
    let elsewhere = elsewhere.expect("the second ship is a planet");
    // The same place on it, a hundred along, fifty up and twenty back.
    assert!(
        (elsewhere[0] - (at_the_origin[0] + 100.0)).abs() < 1.0e-3,
        "{elsewhere:?}"
    );
    assert!(
        (elsewhere[1] - (at_the_origin[1] + 50.0)).abs() < 1.0e-3,
        "{elsewhere:?}"
    );
    assert!(
        (elsewhere[2] - (at_the_origin[2] - 20.0)).abs() < 1.0e-3,
        "{elsewhere:?}"
    );
}

#[test]
fn a_player_lands_on_the_ground_at_the_top_of_the_planet_with_what_was_dug_there_counted_and_no_other_level_has_such_a_place()
 {
    let mut scene = Scene::in_a_new_world();
    let top = top_of_the_column(0, 0);
    let (planet, overworld) = landings(&mut scene);
    // In the middle of the column of voxels at the top of the planet, with the eyes over the top voxel.
    let [x, y, z] = planet.expect("the planet has a pole");
    assert!(
        (x - 0.5).abs() < 1.0e-4 && (z - 0.5).abs() < 1.0e-4,
        "{x} {z}"
    );
    assert!(
        (y - ((top + 1) as f64 + 1.64)).abs() < 1.0e-3,
        "{y} over {top}"
    );
    assert_eq!(overworld, None, "the overworld has no planet");

    // Dig the top voxel out, and the next two, and they land lower.
    let content = CaveContent::new();
    let (channel, air) = (content.block_channel(), content.blocks.air);
    scene.run(move |world| {
        let mut planet = world.grid(ship_id(0)).unwrap();
        for y in top - 2..=top {
            planet
                .set(channel, VoxelCoordinate::new(0, y, 0), air)
                .unwrap();
        }
    });
    let (dug, _) = landings(&mut scene);
    let [_, dug_y, _] = dug.unwrap();
    assert!((dug_y - (y - 3.0)).abs() < 1.0e-3, "{dug_y} against {y}");
}

#[test]
fn the_first_player_of_a_new_world_arrives_standing_on_the_planet_wherever_their_client_said_it_started()
 {
    let mut scene = Scene::in_a_new_world();
    // The client starts where the game says (16, 110, 16), which is under the ground of the planet, near its middle.
    let start = Cave::default_camera().position;
    let mut player = scene.rig.join_walking(start, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 60);

    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, ship_id(0), "aboard the planet");
    let [x, y, z] = in_voxels(pose.position);
    let top = top_of_the_column(0, 0);
    assert!((x - 0.5).abs() < 0.05 && (z - 0.5).abs() < 0.05, "{x} {z}");
    assert!(
        (y - ((top + 1) as f64 + 1.62)).abs() < 0.05,
        "{y} over the ground at {top}"
    );
}

#[test]
fn a_player_who_asked_for_another_place_than_where_players_start_is_put_there_even_in_a_level_with_a_planet()
 {
    let mut scene = Scene::in_a_new_world();
    // Flying, over the planet and farther from its middle than any ground of it: where a client that was started with `--camera` is put.
    let asked = [100.5, 70_000.0, 200.5];
    let mut player = scene.rig.join(asked, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 20);
    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, GridId::root(LevelIndex::MAIN), "in space");
    let [x, y, z] = in_voxels(pose.position);
    assert!(
        (x - asked[0]).abs() < 0.01 && (y - asked[1]).abs() < 0.01 && (z - asked[2]).abs() < 0.01,
        "{x} {y} {z}"
    );
}

#[test]
fn level_go_takes_a_player_to_the_overworld_at_where_players_start_there_and_back_to_the_pole_of_the_planet()
 {
    let mut scene = Scene::in_a_new_world();
    let start = Cave::default_camera().position;
    let mut player = scene.rig.join_walking(start, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 60);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));

    // To the overworld: at the place players start in it (not 64 kilometres over it, where the pole of the planet is).
    let reply = say(&mut scene, &mut player, "/level go overworld");
    assert!(
        reply.contains("you are now in voxunami_cave:overworld"),
        "{reply}"
    );
    let overworld = GridId::root(LevelIndex::new(1));
    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, overworld);
    let [x, y, z] = in_voxels(pose.position);
    assert!(
        (x - start[0]).abs() < 0.5 && (z - start[2]).abs() < 0.5,
        "{x} {z}"
    );
    assert!((y - start[1]).abs() < 5.0, "{y}");

    // And back, on the planet.
    let reply = say(&mut scene, &mut player, "/level go mini_cosmos");
    assert!(
        reply.contains("you are now in voxunami_cave:mini_cosmos"),
        "{reply}"
    );
    tick_and_follow(&mut scene, &mut player, 60);
    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, ship_id(0));
    let [x, y, z] = in_voxels(pose.position);
    let top = top_of_the_column(0, 0);
    assert!((x - 0.5).abs() < 0.05 && (z - 0.5).abs() < 0.05, "{x} {z}");
    assert!((y - ((top + 1) as f64 + 1.62)).abs() < 0.05, "{y}");
}

#[test]
fn a_player_standing_on_the_planet_breaks_the_ground_under_them_and_the_planet_stores_the_hole_and_they_stand_lower()
 {
    let mut scene = Scene::in_a_new_world();
    let start = Cave::default_camera().position;
    let mut player = scene.rig.join_walking(start, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 60);
    let top = top_of_the_column(0, 0);

    // Looking straight down at the voxel they stand on.
    player.camera.pitch = (-89.0f32).to_radians();
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 3);
    scene.rig.settle(&mut [&mut player]);
    let hit = player.target().expect("the ground is within reach");
    assert_eq!(
        hit.grid,
        ship_id(0),
        "the ground they look at is the planet's"
    );
    assert_eq!(hit.voxel, VoxelCoordinate::new(0, top, 0));
    assert_eq!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(0))
            .unwrap()
            .voxels
            .brick_count(),
        0,
        "nothing is stored of the planet before the edit"
    );

    break_block(&mut player);
    tick_and_follow(&mut scene, &mut player, 3);
    // The planet stores the brick with the hole in it, and the player has it.
    let planet = scene.rig.server.world().grids().get(ship_id(0)).unwrap();
    assert_eq!(planet.voxels.brick_count(), 1);
    // They fall into the hole and stand on what is under it, a voxel lower.
    tick_and_follow(&mut scene, &mut player, 40);
    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, ship_id(0));
    let [_, y, _] = in_voxels(pose.position);
    assert!((y - (top as f64 + 1.62)).abs() < 0.05, "{y}");
}

#[test]
fn a_player_standing_on_the_planet_places_a_block_on_the_ground_ahead_of_them_and_the_planet_has_it()
 {
    let mut scene = Scene::in_a_new_world();
    let start = Cave::default_camera().position;
    let mut player = scene.rig.join_walking(start, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 60);

    // Looking down at the ground a voxel or two ahead.
    player.camera.pitch = (-45.0f32).to_radians();
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 3);
    scene.rig.settle(&mut [&mut player]);
    let hit = player.target().expect("the ground is within reach");
    assert_eq!(hit.grid, ship_id(0));
    let above = VoxelCoordinate::new(hit.voxel.x(), hit.voxel.y() + 1, hit.voxel.z());

    place_block(&mut player, "base_blocks:stone");
    tick_and_follow(&mut scene, &mut player, 3);
    let content = CaveContent::new();
    let planet = scene.rig.server.world().grids().get(ship_id(0)).unwrap();
    assert_eq!(planet.voxels.brick_count(), 1, "the brick the block is in");
    let (brick, local) = above.split();
    assert_eq!(
        planet
            .voxels
            .brick(brick)
            .expect("the brick is stored")
            .get(content.block_channel(), local),
        content.blocks.stone
    );
}

#[test]
fn a_pilot_whose_ship_cannot_be_found_and_who_goes_to_the_world_spawn_of_a_level_with_a_planet_stands_on_its_ground()
 {
    let mut scene = Scene::in_a_new_world();
    let ground = (top_of_the_column(0, 0) + 1) as f64;
    // A deck of stone three over the ground at the pole, held; a pilot who lands on it, takes the controls and tells it to go with them.
    make_ship(
        &mut scene,
        pose_at(-10.0, (ground + 3.0) as f32, -4.0),
        true,
        [30, 2, 8],
        |_| true,
    );
    let mut pilot = scene
        .rig
        .join_walking([0.5, ground + 5.0 + 8.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut pilot]);
    tick_and_follow(&mut scene, &mut pilot, 60);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(1), "aboard the deck");
    assert!(say(&mut scene, &mut pilot, "/ship pilot").starts_with("you pilot ship 1"));
    assert!(say(&mut scene, &mut pilot, "/ship vanish on").contains("will go away with its pilot"));
    drop(pilot);
    scene.ticks(3);
    // The file of the ship is lost.
    let mut files = scene.rig.files.clone();
    for name in files.names().unwrap() {
        files.remove(&name).unwrap();
    }
    assert_eq!(scene.rig.files.len(), 0);
    // The pilot comes back, is asked, and chooses the first: to go to the world spawn.
    let mut back = scene
        .rig
        .join_walking_as(1, [0.5, ground + 100.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut back]);
    tick_and_follow(&mut scene, &mut back, 3);
    assert!(
        back.hud_text()
            .starts_with("The ship you were aboard cannot be found."),
        "{}",
        back.hud_text()
    );
    back.client_command(".choose 1");
    tick_and_follow(&mut scene, &mut back, 60);
    // They stand on the ground of the planet, at its pole: aboard the planet, with their eyes over the ground.
    let pose = player_pose(&scene, 2);
    assert_eq!(pose.grid, ship_id(0));
    let [x, y, z] = in_voxels(pose.position);
    assert!(
        (x - 0.5).abs() < 0.1 && (z - 0.5).abs() < 0.1 && (y - (ground + 1.62)).abs() < 0.1,
        "{x} {y} {z} over {ground}"
    );
}

#[test]
fn the_world_spawn_of_a_level_is_the_ground_at_the_pole_of_its_planet_or_where_players_start_if_it_has_none()
 {
    let mut scene = Scene::in_a_new_world();
    let content = CaveContent::new();
    let (channel, air) = (content.block_channel(), content.blocks.air);
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        let landing = on_a_planet(world, LevelIndex::MAIN, channel, air);
        *slot.lock().unwrap() = Some((
            landing,
            spawn_in(world, LevelIndex::MAIN, channel, air),
            spawn_in(world, LevelIndex::new(1), channel, air),
        ));
    });
    let (landing, space, overworld) = found.lock().unwrap().take().unwrap();
    assert!(landing.is_some());
    assert_eq!(Some(space), landing, "the level with a planet");
    assert_eq!(overworld, world_spawn(), "the level without one");
    assert_ne!(space, world_spawn());
}

#[test]
fn in_survival_a_block_broken_on_the_planet_leaves_its_item_and_the_player_standing_there_picks_it_up()
 {
    let mut scene = Scene::in_a_new_world();
    let start = Cave::default_camera().position;
    let mut player = scene.rig.join_walking(start, 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 60);
    assert!(say(&mut scene, &mut player, "/gamemode survival").contains("survival"));
    // Looking straight down at the voxel they stand on, they break it.
    player.camera.pitch = (-89.0f32).to_radians();
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 3);
    scene.rig.settle(&mut [&mut player]);
    break_block(&mut player);
    tick_and_follow(&mut scene, &mut player, 40);
    // The item is theirs: none is lying in the planet, and the inventory has a block of the planet's ground.
    let lying = scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == voxunami_cave::items::dropped_item_kind())
        .count();
    assert_eq!(lying, 0);
    let inventory = scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .find_map(|entity| {
            entity
                .components
                .get::<entities::Inventory>()
                .ok()
                .flatten()
        })
        .expect("a player has an inventory");
    let held: u32 = inventory
        .slots()
        .iter()
        .flatten()
        .map(|stack| stack.count)
        .sum();
    assert_eq!(held, 1);
}

/// A dropped item of stone in this voxel of this grid.
fn spawn_an_item(scene: &mut Scene, grid: GridId, at: VoxelCoordinate) {
    let stack = entities::ItemStack::new("base_blocks:stone".parse().unwrap(), 1);
    scene.run(move |world| {
        let mut components = entities::ComponentBag::new();
        components.set(&stack);
        world
            .spawn_entity(
                &voxunami_cave::items::dropped_item_kind(),
                voxunami_cave::items::drop_pose(grid, at),
                components,
            )
            .unwrap();
    });
}

/// Where the dropped items are, as (grid, voxel).
fn items_lying(scene: &Scene) -> Vec<(GridId, VoxelCoordinate)> {
    scene
        .rig
        .server
        .world()
        .entities()
        .iter()
        .filter(|entity| entity.kind == voxunami_cave::items::dropped_item_kind())
        .map(|entity| {
            (
                entity.pose.grid,
                voxunami_cave::items::voxel_of(entity.pose.position),
            )
        })
        .collect()
}

#[test]
fn an_item_in_space_far_from_every_pull_does_not_fall_and_one_far_off_to_the_side_of_the_planet_is_not_removed_for_being_low()
 {
    let mut scene = Scene::in_a_new_world();
    let space = GridId::root(LevelIndex::MAIN);
    // Thirty million voxels from the planet its pull is a twentieth of a thousandth: none. And nine hundred kilometres along -x and two thousand under the
    // origin, the pull is toward the planet along +x, and the item is not at the bottom of the world.
    let far = VoxelCoordinate::new(30_000_000, 0, 0);
    let low = VoxelCoordinate::new(-900_000, -2_000, 0);
    spawn_an_item(&mut scene, space, far);
    spawn_an_item(&mut scene, space, low);
    scene.ticks(80);
    let lying = items_lying(&scene);
    assert!(lying.contains(&(space, far)), "{lying:?}");
    assert_eq!(lying.len(), 2, "{lying:?}");
    let moved = lying.iter().find(|(_, voxel)| *voxel != far).unwrap().1;
    assert!(moved.x() > low.x() && moved.y() == low.y(), "{moved:?}");
}

#[test]
fn an_item_under_the_bottom_of_a_level_is_removed_and_one_far_from_the_ship_it_fell_off_is_too() {
    let mut scene = Scene::new();
    let level = GridId::root(LevelIndex::MAIN);
    spawn_an_item(&mut scene, level, VoxelCoordinate::new(0, -1_001, 0));
    // A ship of one block, held; an item a long way under it, and one near.
    common::make_ship(
        &mut scene,
        common::pose_at(0.0, 300.0, 0.0),
        true,
        [1; 3],
        |_| true,
    );
    let ship = ship_id(0);
    spawn_an_item(&mut scene, ship, VoxelCoordinate::new(0, -1_100, 0));
    spawn_an_item(&mut scene, ship, VoxelCoordinate::new(0, -500, 0));
    scene.ticks(12);
    let lying = items_lying(&scene);
    assert_eq!(lying.len(), 1, "{lying:?}");
    assert_eq!(lying[0].0, ship);
    assert!((-510..=-495).contains(&lying[0].1.y()), "{lying:?}");
}

#[test]
fn an_item_on_the_side_of_the_planet_falls_toward_its_centre_and_one_under_it_does_not_vanish() {
    let mut scene = Scene::in_a_new_world();
    let planet = ship_id(0);
    spawn_an_item(&mut scene, planet, VoxelCoordinate::new(63_900, 0, 0));
    spawn_an_item(&mut scene, planet, VoxelCoordinate::new(0, -63_900, 0));
    // (It falls a voxel every 4 ticks: three hundred voxels would be twelve hundred ticks. The ground is 190 voxels down at most.)
    scene.ticks(1_000);
    let content = CaveContent::new();
    let channel = content.block_channel();
    let found = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        let items: Vec<VoxelCoordinate> = world
            .entities()
            .iter()
            .filter(|entity| entity.kind == voxunami_cave::items::dropped_item_kind())
            .map(|entity| voxunami_cave::items::voxel_of(entity.pose.position))
            .collect();
        let mut view = world.grid(planet).unwrap();
        // What is under each (along the planet's down for it) and in its place.
        let mut rows = Vec::new();
        for voxel in items {
            let (down, here) = if voxel.x().abs() > voxel.y().abs() {
                (
                    VoxelCoordinate::new(voxel.x() - 1, voxel.y(), voxel.z()),
                    voxel,
                )
            } else {
                (
                    VoxelCoordinate::new(voxel.x(), voxel.y() + 1, voxel.z()),
                    voxel,
                )
            };
            rows.push((
                voxel,
                view.get(channel, down).unwrap(),
                view.get(channel, here).unwrap(),
            ));
        }
        *slot.lock().unwrap() = rows;
    });
    let rows = found.lock().unwrap().clone();
    assert_eq!(rows.len(), 2, "{rows:?}");
    for (voxel, under, here) in rows {
        assert!(under != content.blocks.air, "{voxel:?} hangs over air");
        assert_eq!(here, content.blocks.air, "{voxel:?} is in the ground");
        assert!(
            voxel.x().abs() < 63_900 && voxel.y().abs() < 63_900,
            "{voxel:?} has not fallen"
        );
    }
}

#[test]
fn the_bricks_where_players_arrive_on_a_planet_are_made_before_anyone_arrives() {
    let mut scene = Scene::in_a_new_world();
    // (The worker is a thread: the ticks are given the time of a server's.)
    for _ in 0..40 {
        scene.ticks(1);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let found = Arc::new(Mutex::new((0, 0)));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        *slot.lock().unwrap() = (world.bricks_prefetched(), world.bricks_generated())
    });
    let (prefetched, _) = *found.lock().unwrap();
    // The 5 x 5 x 5 bricks round the pole, those of them that are in the planet's extent (all are).
    assert!(prefetched >= 100, "{prefetched} made ahead");
}
