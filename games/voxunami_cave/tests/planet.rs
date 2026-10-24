//! A planet (a ship with terrain) in Cave: a walker lands on it and is aboard it, walks on the ground its generator makes, falls toward its centre, digs into it, and the
//! player's block edits are the planet's. The planet here is the small planet of the base blocks (a hundredth of the Earth's radius), put in the overworld with the
//! overworld's own pull taken away, high enough that the rolling hills under it are no part of the picture.

mod common;

use std::sync::{Arc, Mutex};

use block_models::Shape;
use common::{PlayerExt, Scene, player_pose, ship_id, tick_and_follow};
use game_shell::{GameContent, GameDefinition, KeyCode};
use server::GridView;
use voxel_space::{
    BrickBox, BrickCoordinate, FaceDirection, GridId, GridPose, LevelIndex, Orientation,
    VoxelCoordinate, WorldPosition,
};
use voxunami_cave::Cave;
use voxunami_cave::aboard::in_voxels;
use voxunami_cave::content::{CaveContent, mini_cosmos_id};
use voxunami_cave::game::PULLING_MASS;
use world_storage::GridTerrain;

const SEED: u64 = common::SEED;
/// The radius of the planet, in voxels (metres).
const RADIUS: f64 = 63_710.0;
/// The mass that pulls 10 metres a second a second at that distance from the centre, with the real G.
const MASS: f32 = (10.0 * RADIUS * RADIUS / 6.674e-11) as f32;
/// How high the top of the planet is, in the world, at x = 0, z = 0.
const TOP_OF_THE_PLANET: f64 = 400.0;

fn planet() -> GridId {
    ship_id(0)
}

/// The height, in the planet's own coordinates, of the highest block that is not air in the column of voxels at `x` and `z`, as the planet's generator makes it
/// (caves and all).
fn top_of_the_column(x: i64, z: i64) -> i64 {
    let content = CaveContent::new();
    let world = content.new_world(SEED);
    let layout = Arc::clone(world.main().overrides.layout());
    let generator = content
        .blocks
        .terrain_generators(SEED, content.block_channel());
    let id = generator
        .runtime_id(&base_blocks::small_planet_generator_identifier())
        .unwrap();
    let generator = generator.entry(id).unwrap().content().unwrap();
    for brick_y in (1900..2010).rev() {
        let coordinate = BrickCoordinate::new(x.div_euclid(32), brick_y, z.div_euclid(32));
        let brick = generator.generate_brick(&layout, coordinate);
        for local_y in (0..32).rev() {
            let local = voxel_space::LocalVoxelIndex::new(
                x.rem_euclid(32) as u8,
                local_y,
                z.rem_euclid(32) as u8,
            );
            if brick.get(content.block_channel(), local) != content.blocks.air {
                return brick_y * 32 + i64::from(local_y);
            }
        }
    }
    panic!("no ground in the column");
}

/// The planet, made in the overworld with the overworld's pull taken away so that the planet's is all there is, with its centre so far down that the top of it is at
/// [`TOP_OF_THE_PLANET`] over the origin. Returns the height of the planet's centre.
fn make_the_planet(scene: &mut Scene) -> f64 {
    let top = top_of_the_column(0, 0) + 1;
    let centre = TOP_OF_THE_PLANET - top as f64;
    scene.run(move |world| {
        world.set_level_gravity(LevelIndex::MAIN, [0.0; 3]);
        let generator = world
            .data()
            .main()
            .terrain
            .generators()
            .runtime_id(&base_blocks::small_planet_generator_identifier())
            .expect("the world has the planet's generator");
        let pose = GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [0.0, centre as f32, 0.0]),
            Orientation::IDENTITY,
        );
        let grid = world.create_grid(LevelIndex::MAIN, pose).unwrap();
        assert_eq!(grid, planet());
        world
            .set_grid_terrain(
                grid,
                GridTerrain {
                    generator,
                    extent: BrickBox::around(BrickCoordinate::ORIGIN, 2000),
                    mass: MASS,
                },
            )
            .unwrap();
    });
    centre
}

#[test]
fn a_walker_dropped_over_a_planet_lands_on_it_and_is_aboard_it_standing_in_its_coordinates() {
    let mut scene = Scene::new();
    let centre = make_the_planet(&mut scene);
    let top = top_of_the_column(0, 0) + 1;
    // Fifteen voxels over the ground, a little off the middle of a voxel column.
    let mut player = scene
        .rig
        .join_walking([0.5, TOP_OF_THE_PLANET + 15.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 80);

    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, planet(), "aboard the planet");
    // Standing on the ground: the eyes 1.62 over the top voxel, in the planet's own coordinates (whose origin is the centre of the planet).
    let [x, y, z] = in_voxels(pose.position);
    assert!((x - 0.5).abs() < 0.02 && (z - 0.5).abs() < 0.02, "{x} {z}");
    assert!(
        (y - (top as f64 + 1.62)).abs() < 0.05,
        "{y} against {}",
        top as f64 + 1.62
    );
    // And so in the world: the planet's centre is where it was put.
    let world_y = y + centre;
    assert!(
        (world_y - (TOP_OF_THE_PLANET + 1.62)).abs() < 0.05,
        "{world_y}"
    );
    // The bodies of the ships leave a planet alone: it weighs what its terrain was given, still, and stays where it was put.
    let planet_ship = scene.rig.server.world().grids().get(planet()).unwrap();
    assert_eq!(planet_ship.mass, MASS);
    assert_eq!(planet_ship.centre_of_mass, [0.0; 3]);
    assert!(planet_ship.held);
    // Standing quietly, to the bit.
    let standing = pose;
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(player_pose(&scene, 1), standing);
}

#[test]
fn what_a_walker_aboard_a_planet_falls_toward_is_its_centre_at_the_pull_the_mass_was_given_for() {
    let mut scene = Scene::new();
    make_the_planet(&mut scene);
    let mut player = scene
        .rig
        .join_walking([0.5, TOP_OF_THE_PLANET + 15.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 80);
    let eyes = player_pose(&scene, 1).position;

    // Gravity there, in the planet's axes: straight down at about ten (the ground is a few hundred metres either way of the radius, so the distance from the
    // centre is within a half of a percent of it, and the pull within one of ten).
    let found = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&found);
    scene.run(move |world| {
        *slot.lock().unwrap() = Some(world.gravity_in(planet(), eyes).unwrap());
    });
    let [gx, gy, gz] = found.lock().unwrap().unwrap();
    assert!(gx.abs() < 1e-3 && gz.abs() < 1e-3, "{gx} {gz}");
    assert!((-gy - 10.0).abs() < 0.1, "{gy}");
}

#[test]
fn a_walker_on_a_planet_walks_on_the_ground_it_generates_and_stays_aboard() {
    let mut scene = Scene::new();
    make_the_planet(&mut scene);
    // The walk is toward +z (a yaw of 180 degrees), down a slope that the hills make there and that has no step in it of more than one voxel and no hole (the caves
    // that reach the surface are some way off: the generator's ground is pinned, so if it moves the setup says so here and not in the walker).
    let slope: Vec<i64> = (0..=40).map(|z| top_of_the_column(0, z)).collect();
    assert!(
        slope
            .windows(2)
            .all(|pair| (0..=1).contains(&(pair[0] - pair[1]))),
        "the slope is not a slope: {slope:?}"
    );
    let mut player = scene
        .rig
        .join_walking([0.5, TOP_OF_THE_PLANET + 15.0, 0.5], 180.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 80);
    let start = in_voxels(player_pose(&scene, 1).position);

    // Forward for seven seconds: the walker covers the better part of thirty metres, and the controller keeps its feet on the ground.
    player.hold(&[KeyCode::KeyW]);
    let mut lowest_above_the_ground = f64::MAX;
    let mut highest_above_the_ground = f64::MIN;
    for _ in 0..14 {
        tick_and_follow(&mut scene, &mut player, 10);
        let pose = player_pose(&scene, 1);
        assert_eq!(pose.grid, planet(), "still aboard");
        let [x, y, z] = in_voxels(pose.position);
        let ground = top_of_the_column(x.floor() as i64, z.floor() as i64) + 1;
        lowest_above_the_ground = lowest_above_the_ground.min(y - ground as f64);
        highest_above_the_ground = highest_above_the_ground.max(y - ground as f64);
    }
    let end = in_voxels(player_pose(&scene, 1).position);
    let (across, along) = (end[0] - start[0], end[2] - start[2]);
    let covered = (across * across + along * along).sqrt();
    assert!(covered > 25.0, "walked {covered}");
    // The eyes are 1.62 over the ground the whole way (a step up or down a voxel shows as a moment of difference).
    assert!(
        lowest_above_the_ground > 1.4 && highest_above_the_ground < 2.8,
        "{lowest_above_the_ground} to {highest_above_the_ground}"
    );
}

#[test]
fn a_hole_dug_under_a_walker_on_a_planet_has_them_fall_into_it() {
    let mut scene = Scene::new();
    make_the_planet(&mut scene);
    let top = top_of_the_column(0, 0);
    let mut player = scene
        .rig
        .join_walking([0.5, TOP_OF_THE_PLANET + 15.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 80);
    let standing = in_voxels(player_pose(&scene, 1).position)[1];

    // The server digs out a column of 3 x 3 voxels, 6 deep, under their feet (edits of the planet: the bricks it stores).
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut planet_view = world.grid(planet()).unwrap();
        for x in -1..=1 {
            for z in -1..=1 {
                for y in (top - 5)..=top {
                    planet_view
                        .set(
                            content.block_channel(),
                            VoxelCoordinate::new(x, y, z),
                            content.blocks.air,
                        )
                        .unwrap();
                }
            }
        }
    });
    tick_and_follow(&mut scene, &mut player, 40);
    let fallen = in_voxels(player_pose(&scene, 1).position)[1];
    assert_eq!(player_pose(&scene, 1).grid, planet());
    // They are at the bottom of the hole now: six voxels lower, give or take what the controller leaves between the feet and the floor.
    assert!(
        (standing - fallen - 6.0).abs() < 0.2,
        "stood at {standing}, now at {fallen}"
    );
    // The bricks the dig stored are the planet's, and the server has them in its voxels.
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(planet())
            .unwrap()
            .voxels
            .brick_count()
            > 0
    );
}

#[test]
fn the_game_lets_a_planet_pull_and_not_a_ship_that_someone_built_and_space_has_no_pull_of_its_own()
{
    let gravity = Cave::gravity();
    assert_eq!(gravity.base, [0.0, -5.0, 0.0]);
    assert_eq!(gravity.pull_mass_threshold, Some(PULLING_MASS));
    const { assert!(MASS > PULLING_MASS, "the planet pulls") };
    // A ship of a thousand tonnes does not pull, nor does one of a hundred thousand.
    const { assert!(1.0e11 < PULLING_MASS) };
    assert_eq!(Cave::level_gravity(), vec![(mini_cosmos_id(), [0.0; 3])]);
}

#[test]
fn a_trench_across_a_stored_brick_of_a_planet_does_not_cut_the_planet_in_two() {
    let mut scene = Scene::new();
    make_the_planet(&mut scene);
    let content = CaveContent::new();
    // The brick that the surface at the middle of the trench is in, with ground in the whole of the trench's length.
    let bottom = top_of_the_column(16, 16).div_euclid(32) * 32;
    assert!(
        (0..32).all(|z| top_of_the_column(16, z) >= bottom + 4),
        "the setup: ground in the brick all along the trench"
    );
    // All the way across the brick, the whole of its height: what is stored of the planet is two pieces now.
    scene.run(move |world| {
        let mut planet_view = world.grid(planet()).unwrap();
        for z in 0..32 {
            for y in bottom..(bottom + 32) {
                planet_view
                    .set(
                        content.block_channel(),
                        VoxelCoordinate::new(16, y, z),
                        content.blocks.air,
                    )
                    .unwrap();
            }
        }
    });
    assert!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(planet())
            .unwrap()
            .voxels
            .brick_count()
            > 0,
        "the setup: the trench is stored"
    );
    // (The checks that a ship is whole are a few thousand voxels a tick; the brick has thirty-two thousand.)
    scene.ticks(100);
    let world = scene.rig.server.world();
    assert_eq!(world.grids().len(), 1, "one ship, the planet");
    let planet_ship = world.grids().get(planet()).unwrap();
    assert!(planet_ship.terrain.is_some());
    assert_eq!(planet_ship.mass, MASS);
    assert!(planet_ship.voxels.brick_count() > 0);
}

#[test]
fn a_block_put_in_the_sky_over_a_planet_and_taken_away_again_does_not_take_the_planet_with_it() {
    let mut scene = Scene::new();
    make_the_planet(&mut scene);
    let content = CaveContent::new();
    let (channel, stone, air) = (
        content.block_channel(),
        content.blocks.stone,
        content.blocks.air,
    );
    let up = top_of_the_column(0, 0) + 40;
    // The brick it is put in has nothing in it but air and then this block, and nothing but air after: a ship that holds only air has nothing to carry.
    scene.run(move |world| {
        world
            .grid(planet())
            .unwrap()
            .set(channel, VoxelCoordinate::new(0, up, 0), stone)
            .unwrap();
    });
    scene.ticks(10);
    assert!(scene.rig.server.world().grids().get(planet()).is_some());
    scene.run(move |world| {
        world
            .grid(planet())
            .unwrap()
            .set(channel, VoxelCoordinate::new(0, up, 0), air)
            .unwrap();
    });
    scene.ticks(100);
    let planet_ship = scene
        .rig
        .server
        .world()
        .grids()
        .get(planet())
        .expect("the planet is still there");
    assert!(planet_ship.terrain.is_some());
    assert_eq!(planet_ship.mass, MASS);
}

#[test]
fn a_slab_put_on_a_planet_upside_down_is_a_ceiling_to_a_walker_and_not_a_floor() {
    let mut scene = Scene::new();
    make_the_planet(&mut scene);
    let content = CaveContent::new();
    let table = content.block_table();
    let stone = content.blocks.stone;
    // The turn of a bottom slab of stone (state 1) that makes it a top slab.
    let turn = (1..24)
        .find(|&turn| {
            table
                .shape(table.model_of(stone, 1, turn))
                .same_cells(&Shape::half(FaceDirection::PositiveY))
        })
        .expect("some turn makes a bottom slab a top slab");
    let air_above = top_of_the_column(0, 0) + 1;
    let (block_channel, state_channel, orientation_channel) = (
        content.block_channel(),
        content.state_channel(),
        content.orientation_channel(),
    );
    scene.run(move |world| {
        let mut planet_view = world.grid(planet()).unwrap();
        let at = VoxelCoordinate::new(0, air_above, 0);
        planet_view.set(block_channel, at, stone).unwrap();
        planet_view.set(state_channel, at, 1).unwrap();
        planet_view.set(orientation_channel, at, turn).unwrap();
    });
    let mut player = scene
        .rig
        .join_walking([0.5, TOP_OF_THE_PLANET + 15.0, 0.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 80);

    // The walker stands on the top of the slab, which is the top of its voxel: the slab is in the top half of the voxel.
    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, planet());
    let eyes = in_voxels(pose.position)[1];
    assert!(
        (eyes - ((air_above + 1) as f64 + 1.62)).abs() < 0.05,
        "{eyes} against {}",
        (air_above + 1) as f64 + 1.62
    );
}
