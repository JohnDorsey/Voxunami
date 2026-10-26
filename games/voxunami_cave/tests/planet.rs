//! A planet (a ship with terrain) in Cave: a walker lands on it and is aboard it, walks on the ground its generator makes, falls toward its centre, digs into it, and the
//! player's block edits are the planet's. The planet here is the small planet of the base blocks (a hundredth of the Earth's radius), put in the overworld with the
//! overworld's own pull taken away, high enough that the rolling hills under it are no part of the picture.

mod common;

use std::sync::{Arc, Mutex};

use block_models::Shape;
use common::{
    PlayerExt, Scene, make_ship, player_pose, pose_at, ship_id, ship_pose, tick_and_follow,
    top_of_the_column,
};
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

/// The radius of the planet, in voxels (metres).
const RADIUS: f64 = 63_710.0;
/// The mass that pulls 10 metres a second a second at that distance from the centre, with the real G.
const MASS: f32 = (10.0 * RADIUS * RADIUS / 6.674e-11) as f32;
/// How high the top of the planet is, in the world, at x = 0, z = 0.
const TOP_OF_THE_PLANET: f64 = 400.0;

fn planet() -> GridId {
    ship_id(0)
}

/// The planet, made in the overworld with the overworld's pull taken away so that the planet's is all there is, with its centre so far down that the top of it is at
/// [`TOP_OF_THE_PLANET`] over the origin. Returns the height of the planet's centre.
fn make_the_planet(scene: &mut Scene) -> f64 {
    let top = top_of_the_column(0, 0) + 1;
    let centre = TOP_OF_THE_PLANET - top as f64;
    make_a_planet(
        scene,
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [0.0, centre as f32, 0.0]),
            Orientation::IDENTITY,
        ),
    );
    centre
}

/// The planet, made in the overworld at `pose`, with the overworld's pull taken away so that the planet's is all there is.
fn make_a_planet(scene: &mut Scene, pose: GridPose) {
    make_a_planet_that_reaches(scene, pose, 2000);
}

/// The same, with a planet whose generator makes the bricks this many bricks either way of its origin (and nothing past them but what is built there).
fn make_a_planet_that_reaches(scene: &mut Scene, pose: GridPose, reach: u32) {
    scene.run(move |world| {
        world.set_level_gravity(LevelIndex::MAIN, [0.0; 3]);
        let generator = world
            .data()
            .main()
            .terrain
            .generators()
            .runtime_id(&base_blocks::small_planet_generator_identifier())
            .expect("the world has the planet's generator");
        let grid = world.create_grid(LevelIndex::MAIN, pose).unwrap();
        assert_eq!(grid, planet());
        world
            .set_grid_terrain(
                grid,
                GridTerrain {
                    generator,
                    extent: BrickBox::around(BrickCoordinate::ORIGIN, reach),
                    mass: MASS,
                },
            )
            .unwrap();
    });
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
fn the_game_draws_a_planet_out_to_where_it_is_a_few_pixels_across_and_the_terrain_of_a_level_not_beyond_its_bricks()
 {
    let detail = Cave::grid_detail().expect("a planet is drawn in detail");
    // Far enough that the planet is still drawn from a hundred of its radii off, and not so far that the nodes of the far side of a star are asked for.
    assert!(detail.maximum_distance >= 100.0 * RADIUS);
    assert!(detail.maximum_distance <= 1.0e9);
    assert_eq!(detail.unlit, 0);
    assert!(detail.cell_pixels > 0.0);
    assert_eq!(Cave::coarse_detail(), None);
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

/// The side of the cubes the ships below are, and the columns they stand over: a patch of the planet's pole where the ground is flat for a few voxels (its top voxel
/// is 63,679 in the planet's coordinates from x = -2 to 5 and z = 5 to 9, and one higher at x = 2 and 3 just before z = 5, which is why the cubes are put half a voxel
/// from the step: a cube flush against a step and pulled sideways into it keeps moving a hundredth of a metre a second and does not go to sleep).
const CUBE: i64 = 4;
const PLACE: [f32; 2] = [0.5, 5.5];
/// The columns, in the planet's coordinates, that a cube put at [`PLACE`] has under it.
const COLUMNS_X: std::ops::Range<i64> = 0..5;
const COLUMNS_Z: std::ops::Range<i64> = 5..10;

/// The height in the planet's coordinates of the ground (the top of the highest voxel that is not air) under the cube's columns.
fn ground_under_the_cube() -> i64 {
    COLUMNS_X
        .flat_map(|x| COLUMNS_Z.map(move |z| (x, z)))
        .map(|(x, z)| top_of_the_column(x, z) + 1)
        .max()
        .unwrap()
}

/// Where the origin of a ship is, in the world, in voxels.
fn place_of_the_ship(scene: &Scene, number: u64) -> [f64; 3] {
    let position = ship_pose(scene, number).position;
    let brick = position.brick();
    let offset = position.offset();
    [brick.x(), brick.y(), brick.z()]
        .into_iter()
        .zip(offset)
        .map(|(index, offset)| index as f64 * 32.0 + f64::from(offset))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

/// A cube of stone, at rest, that has the planet's ground under it: it is put this many voxels over the ground at [`PLACE`].
fn drop_a_cube_over_the_ground(scene: &mut Scene, ground: f64, height: f64) {
    make_ship(
        scene,
        pose_at(PLACE[0], (ground + height) as f32, PLACE[1]),
        false,
        [CUBE; 3],
        |_| true,
    );
}

#[test]
fn a_ship_released_over_a_planet_falls_onto_the_ground_it_generates_and_rests_on_it() {
    let mut scene = Scene::new();
    let centre = make_the_planet(&mut scene);
    let ground = ground_under_the_cube() as f64 + centre;
    // Ten voxels over the ground, where nothing of the planet has ever been stored: all of it is made by the generator.
    drop_a_cube_over_the_ground(&mut scene, ground, 10.0);
    assert_eq!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(planet())
            .unwrap()
            .voxels
            .brick_count(),
        0
    );
    scene.ticks(120);

    let [x, height, z] = place_of_the_ship(&scene, 1);
    assert!(
        (height - ground).abs() < 0.05,
        "the ship is at {height}, the ground at {ground}"
    );
    assert!(
        (x - f64::from(PLACE[0])).abs() < 0.1 && (z - f64::from(PLACE[1])).abs() < 0.1,
        "it stayed where it fell: {x} {z}"
    );
    // At rest, and so it stays, to the bit.
    let resting = ship_pose(&scene, 1);
    assert_eq!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(ship_id(1))
            .unwrap()
            .linear_velocity,
        [0.0; 3]
    );
    scene.ticks(40);
    assert_eq!(ship_pose(&scene, 1), resting);
    // Nothing of the planet was stored by any of it.
    assert_eq!(
        scene
            .rig
            .server
            .world()
            .grids()
            .get(planet())
            .unwrap()
            .voxels
            .brick_count(),
        0
    );
}

#[test]
fn a_ship_resting_on_a_planet_falls_into_a_hole_dug_under_it_and_rests_on_the_bottom_of_it() {
    let mut scene = Scene::new();
    let centre = make_the_planet(&mut scene);
    let ground = ground_under_the_cube();
    drop_a_cube_over_the_ground(&mut scene, ground as f64 + centre, 2.0);
    scene.ticks(80);
    let resting = place_of_the_ship(&scene, 1)[1];
    assert!(
        (resting - (ground as f64 + centre)).abs() < 0.05,
        "{resting}"
    );

    // The server digs out the ground under it and around it, five voxels deep: edits of the planet, which the ship had colliders of before.
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut planet_view = world.grid(planet()).unwrap();
        for x in (COLUMNS_X.start - 1)..(COLUMNS_X.end + 1) {
            for z in (COLUMNS_Z.start - 1)..(COLUMNS_Z.end + 1) {
                for y in (ground - 5)..(ground + 3) {
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
    scene.ticks(120);
    let [x, fallen, z] = place_of_the_ship(&scene, 1);
    assert!(
        (fallen - (resting - 5.0)).abs() < 0.05,
        "it rested at {resting} and is at {fallen}"
    );
    assert!(
        (x - f64::from(PLACE[0])).abs() < 0.1 && (z - f64::from(PLACE[1])).abs() < 0.1,
        "{x} {z}"
    );
}

#[test]
fn a_ship_released_over_a_planet_that_is_turned_and_far_from_the_origin_rests_on_its_ground_wherever_that_is()
 {
    let mut scene = Scene::new();
    // A quarter turn about the x axis takes the planet's +y (its pole) to the world's +z, and its +z to the world's -y: a place at (x, y, z) in the planet is at
    // (x, -z, y) from its centre. The centre is put where the ground of the patch is at z = 700.
    let quarter = std::f64::consts::FRAC_1_SQRT_2;
    let turned = Orientation::from_components([quarter, 0.0, 0.0, quarter]).unwrap();
    let ground = ground_under_the_cube();
    make_a_planet(
        &mut scene,
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [0.0, 300.0, (700 - ground) as f32]),
            turned,
        ),
    );
    // The planet's z from 5.5 to 9.5 is the world's y from 294.5 down to 290.5, and its x is the world's.
    make_ship(
        &mut scene,
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [0.5, 290.5, 710.0]),
            Orientation::IDENTITY,
        ),
        false,
        [CUBE; 3],
        |_| true,
    );
    scene.ticks(150);
    let [x, y, z] = place_of_the_ship(&scene, 1);
    assert!((z - 700.0).abs() < 0.05, "the ship is at z = {z}");
    assert!(
        (x - 0.5).abs() < 0.1 && (y - 290.5).abs() < 0.1,
        "it stayed where it fell: {x} {y}"
    );
}

#[test]
fn a_planet_that_is_moved_has_its_ground_where_it_is_put_and_one_that_is_gone_has_none() {
    let mut scene = Scene::new();
    let centre = make_the_planet(&mut scene);
    let ground = ground_under_the_cube();
    // A ship held out in the sky keeps the frame of the bodies, with the planet in it.
    make_ship(
        &mut scene,
        pose_at(500.0, 3000.0, 500.0),
        true,
        [1; 3],
        |_| true,
    );
    scene.ticks(3);
    // The planet is moved: 7 along x and 100 up.
    let moved = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [7.0, (centre + 100.0) as f32, 0.0]),
        Orientation::IDENTITY,
    );
    scene.run(move |world| world.set_grid_pose(planet(), moved).unwrap());
    let ground = ground as f64 + centre + 100.0;
    // A cube over the same patch of the moved planet.
    make_ship(
        &mut scene,
        pose_at(PLACE[0] + 7.0, (ground + 10.0) as f32, PLACE[1]),
        false,
        [CUBE; 3],
        |_| true,
    );
    scene.ticks(120);
    let [x, height, _] = place_of_the_ship(&scene, 2);
    assert!(
        (height - ground).abs() < 0.05,
        "the ship is at {height}, the ground at {ground}"
    );
    assert!((x - (f64::from(PLACE[0]) + 7.0)).abs() < 0.1, "{x}");

    // The planet goes, and the pull of the level comes back: nothing is under the ship.
    scene.run(|world| {
        world.remove_grid(planet()).unwrap();
        world.set_level_gravity(LevelIndex::MAIN, [0.0, -10.0, 0.0]);
    });
    scene.ticks(40);
    let [_, fallen, _] = place_of_the_ship(&scene, 2);
    // (Two seconds at ten is twenty voxels; the overworld is far below.)
    assert!(fallen < ground - 15.0, "{fallen} against {ground}");
}

/// Bricks of terrain that got colliders over `ticks` ticks (the level's and the planet's together) when a cube of stone is put `height` voxels over the ground of the
/// patch (where the ground of the planet is, if there is a planet), held or free, and the most in one tick.
fn bricks_made_by_a_cube(height: f64, held: bool, with_a_planet: bool, ticks: usize) -> (u64, u64) {
    let (mut scene, report) = Scene::reporting();
    let ground = ground_under_the_cube() as f64;
    let centre = if with_a_planet {
        make_the_planet(&mut scene)
    } else {
        TOP_OF_THE_PLANET - (top_of_the_column(0, 0) + 1) as f64
    };
    make_ship(
        &mut scene,
        pose_at(PLACE[0], (ground + centre + height) as f32, PLACE[1]),
        held,
        [CUBE; 3],
        |_| true,
    );
    scene.ticks(ticks);
    let report = report();
    (
        report.terrain_bricks_made,
        report.terrain_bricks_made_most_in_a_tick,
    )
}

#[test]
fn a_planet_has_colliders_only_for_the_bricks_near_the_ships_that_can_hit_it() {
    // The same cube ten voxels over the ground, and 4,992 (156 bricks) higher: the level's own bricks near it are the same in number.
    let high = 10.0 + 32.0 * 156.0;
    let near_held = bricks_made_by_a_cube(10.0, true, true, 60);
    let far_held = bricks_made_by_a_cube(high, true, true, 60);
    let near_free = bricks_made_by_a_cube(10.0, false, true, 60);
    let far_free = bricks_made_by_a_cube(high, false, true, 60);
    // A ship that is held cannot hit a planet, which is fixed too: nothing of the planet is made for it, near it or not (it is the same as with no planet there).
    assert_eq!(near_held, far_held);
    assert_eq!(near_held, bricks_made_by_a_cube(10.0, true, false, 60));
    // One that is free has the bricks of the planet near it made (the bricks it falls through on the way to the ground are made as it comes to them): a few
    // for the one on the ground, and for the one in the sky the bricks of the air around it (which have none of the cost of a brick of ground, but are counted).
    assert!(near_free.0 > near_held.0, "{near_free:?} {near_held:?}");
    assert!(far_free.0 > far_held.0, "{far_free:?} {far_held:?}");
    // And never many, and never many in one tick, however large the planet is.
    for (total, most) in [near_free, far_free] {
        assert!(total <= 24 && most <= 12, "{total} {most}");
    }
}

#[test]
fn a_ship_that_goes_away_from_a_planet_and_comes_back_finds_its_ground_where_it_was() {
    let mut scene = Scene::new();
    let centre = make_the_planet(&mut scene);
    let ground = ground_under_the_cube() as f64 + centre;
    drop_a_cube_over_the_ground(&mut scene, ground, 4.0);
    scene.ticks(80);
    assert!((place_of_the_ship(&scene, 1)[1] - ground).abs() < 0.05);
    // It is taken a hundred voxels up and kept there for a while (the colliders of the ground it left are let go), and put back where it was, at rest.
    let away = pose_at(PLACE[0], (ground + 100.0) as f32, PLACE[1]);
    scene.run(move |world| {
        world.set_grid_pose(ship_id(1), away).unwrap();
        world
            .set_grid_velocity(ship_id(1), [0.0; 3], [0.0; 3])
            .unwrap();
    });
    scene.ticks(10);
    let back = pose_at(PLACE[0], (ground + 3.0) as f32, PLACE[1]);
    scene.run(move |world| {
        world.set_grid_pose(ship_id(1), back).unwrap();
        world
            .set_grid_velocity(ship_id(1), [0.0; 3], [0.0; 3])
            .unwrap();
    });
    scene.ticks(100);
    let [_, height, _] = place_of_the_ship(&scene, 1);
    assert!(
        (height - ground).abs() < 0.05,
        "the ship is at {height}, the ground at {ground}"
    );
}

#[test]
fn a_ship_rests_on_what_is_built_on_a_planet_past_the_bricks_its_generator_makes() {
    let mut scene = Scene::new();
    // A planet whose bricks reach to the one that holds the ground (the voxels to 63,711 are made, and the ground of the patch is at 63,680), put where the
    // ground of the patch is at 400. A slab of stone is built over the patch, 40 voxels higher: in the bricks past the ones that are made.
    let ground = ground_under_the_cube();
    let centre = 400.0 - ground as f64;
    make_a_planet_that_reaches(
        &mut scene,
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [0.0, centre as f32, 0.0]),
            Orientation::IDENTITY,
        ),
        1990,
    );
    let content = CaveContent::new();
    let slab = ground + 40;
    assert!(
        slab > 1990 * 32 + 31,
        "the slab is past the bricks that are made"
    );
    scene.run(move |world| {
        let mut planet_view = world.grid(planet()).unwrap();
        for x in -2..10 {
            for z in 3..15 {
                planet_view
                    .set(
                        content.block_channel(),
                        VoxelCoordinate::new(x, slab, z),
                        content.blocks.stone,
                    )
                    .unwrap();
            }
        }
    });
    // A cube twelve voxels over the slab, which is a voxel thick.
    let top_of_the_slab = (slab + 1) as f64 + centre;
    make_ship(
        &mut scene,
        pose_at(PLACE[0], (top_of_the_slab + 12.0) as f32, PLACE[1]),
        false,
        [CUBE; 3],
        |_| true,
    );
    scene.ticks(120);
    let [_, height, _] = place_of_the_ship(&scene, 1);
    assert!(
        (height - top_of_the_slab).abs() < 0.05,
        "the ship is at {height}, the top of the slab at {top_of_the_slab}"
    );
}
