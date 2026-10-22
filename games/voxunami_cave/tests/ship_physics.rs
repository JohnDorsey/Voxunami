//! Ships as bodies in Cave: they fall under gravity, rest on the ground and on each other, weigh what their blocks do, can be held still,
//! carry on from where anything else puts them, and are the same on every machine.

mod common;

use common::{FLOOR, Scene, flat_floor, heavy_planet, make_ship, pose_at, ship_id as id};
use server::GridView;
use voxel_space::{
    BrickCoordinate, GravitySettings, GridId, GridPose, LevelIndex, Orientation, VoxelCoordinate,
    WorldPosition,
};
use voxunami_cave::content::CaveContent;

/// Makes a ship that is a cube of stone with this many voxels along an edge.
fn make_a_cube(scene: &mut Scene, size: i64, pose: GridPose) {
    make_a_cube_that_may_be_held(scene, size, pose, false);
}

/// The same, held still from the tick it is made in if it is to be.
fn make_a_cube_that_may_be_held(scene: &mut Scene, size: i64, pose: GridPose, held: bool) {
    make_ship(scene, pose, held, [size; 3], |_| true);
}

fn ship_pose(scene: &Scene, number: u64) -> GridPose {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(id(number))
        .expect("the ship exists")
        .pose
}

fn height_of(pose: GridPose) -> f32 {
    pose.position.brick().y() as f32 * 32.0 + pose.position.offset()[1]
}

fn velocity_of(scene: &Scene, number: u64) -> [f32; 3] {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(id(number))
        .unwrap()
        .linear_velocity
}

#[test]
fn ships_fall_at_the_pull_of_cave_five_metres_a_second_a_second() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    make_a_cube(&mut scene, 4, pose_at(14.0, FLOOR + 40.0, 14.0));
    // Two ticks to make and weigh the body, then a second (20 ticks) of free fall: 5 metres a second faster, and 2.5 metres lower.
    scene.ticks(2);
    let (speed_before, height_before) =
        (velocity_of(&scene, 0)[1], height_of(ship_pose(&scene, 0)));
    scene.ticks(20);
    let (speed, height) = (velocity_of(&scene, 0)[1], height_of(ship_pose(&scene, 0)));
    assert!(
        ((speed_before - speed) - 5.0).abs() < 0.05,
        "{speed_before} to {speed}"
    );
    // (From the speed it had, v, over a second: v + 2.5.)
    assert!(
        ((height_before - height) - (-speed_before + 2.5)).abs() < 0.05,
        "{height_before} to {height}"
    );
}

#[test]
fn a_ship_falls_onto_the_ground_rests_on_it_and_the_player_sees_it_fall() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    let mut player = scene.rig.join([16.5, FLOOR as f64 + 14.0, 40.5], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    make_a_cube(&mut scene, 4, pose_at(14.0, FLOOR + 10.0, 14.0));
    let edited_bricks = scene.rig.server.world().main().overrides.brick_count();
    let mut heights = Vec::new();
    for _ in 0..8 {
        scene.ticks(1);
        player.advance().unwrap();
        heights.push(height_of(ship_pose(&scene, 0)));
        // What the player was told is where the ship is.
        assert_eq!(
            player.client().world().grids().get(id(0)).unwrap().pose,
            ship_pose(&scene, 0)
        );
    }
    assert!(
        heights.windows(2).all(|pair| pair[1] < pair[0]),
        "{heights:?}"
    );
    scene.ticks(120);
    let rested = ship_pose(&scene, 0);
    assert!(
        (height_of(rested) - FLOOR).abs() < 0.05,
        "rests on the ground: {} against {FLOOR}",
        height_of(rested)
    );
    assert!(
        velocity_of(&scene, 0)
            .iter()
            .all(|speed| speed.abs() < 0.01)
    );
    // A ship at rest sleeps, and is not moved or told of again.
    scene.ticks(20);
    assert_eq!(ship_pose(&scene, 0), rested);
    // Nothing was written into the terrain.
    assert_eq!(
        scene.rig.server.world().main().overrides.brick_count(),
        edited_bricks
    );
}

#[test]
fn a_ship_that_has_gone_to_sleep_has_no_speed_left_on_record_not_the_last_it_had_before() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    // A cube dropped from a little way up, turned a little so that it tumbles as it lands and has some turning to lose.
    let turned = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [14.0, FLOOR + 3.0, 14.0]),
        Orientation::from_yaw_pitch(0.4, 0.2),
    );
    make_a_cube(&mut scene, 4, turned);
    let mut asleep_from = None;
    let mut last = None;
    for tick in 0..600 {
        scene.ticks(1);
        let pose = ship_pose(&scene, 0);
        if last == Some(pose) && asleep_from.is_none() {
            asleep_from = Some(tick);
        }
        last = Some(pose);
    }
    assert!(asleep_from.is_some(), "the cube came to rest and slept");
    let ship = scene.rig.server.world().grids().get(id(0)).unwrap();
    assert_eq!(ship.linear_velocity, [0.0; 3]);
    assert_eq!(ship.angular_velocity, [0.0; 3]);
}

#[test]
fn a_ship_weighs_what_its_blocks_do_so_the_same_push_moves_a_light_one_faster() {
    let mut scene = Scene::new();
    // High above the ground, so that nothing but the push matters sideways.
    make_a_cube(&mut scene, 2, pose_at(0.0, 400.0, 0.0));
    make_a_cube(&mut scene, 3, pose_at(100.0, 400.0, 0.0));
    for _ in 0..10 {
        scene.run(|world| {
            world
                .apply_grid_force(id(0), [270_000.0, 0.0, 0.0], None)
                .unwrap();
            world
                .apply_grid_force(id(1), [270_000.0, 0.0, 0.0], None)
                .unwrap();
        });
    }
    let (light, heavy) = (velocity_of(&scene, 0)[0], velocity_of(&scene, 1)[0]);
    // The stone of Cave weighs the engine's default of 1000 a voxel: 8,000 and 27,000. Ten pushes of a twentieth of a second.
    assert!((light - 270_000.0 / 8_000.0 * 0.5).abs() < 0.1, "{light}");
    assert!((heavy - 270_000.0 / 27_000.0 * 0.5).abs() < 0.1, "{heavy}");
    assert!(light > heavy * 3.0);
    // A push is for one tick: the speed stays when it stops.
    scene.ticks(5);
    assert!((velocity_of(&scene, 0)[0] - light).abs() < 1e-3);
    // Half the blocks of the heavy one go, and it weighs half as much: the same push gives it twice the speed.
    let before = velocity_of(&scene, 1)[0];
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut view = world.grid(id(1)).unwrap();
        for x in 0..3 {
            for z in 0..3 {
                let at = VoxelCoordinate::new(x, 2, z);
                view.set(content.block_channel(), at, 0).unwrap();
            }
        }
        // (A layer of three is 9 of 27 blocks: a third. The mass is 18,000 now.)
    });
    scene.run(|world| {
        world
            .apply_grid_force(id(1), [270_000.0, 0.0, 0.0], None)
            .unwrap();
    });
    let gained = velocity_of(&scene, 1)[0] - before;
    assert!(
        (gained - 270_000.0 / 18_000.0 * 0.05).abs() < 0.01,
        "{gained}"
    );
}

#[test]
fn taking_blocks_from_under_a_resting_ship_or_the_ground_under_it_lets_it_sink() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    make_a_cube(&mut scene, 4, pose_at(14.0, FLOOR, 14.0));
    scene.ticks(60);
    assert!((height_of(ship_pose(&scene, 0)) - FLOOR).abs() < 0.05);
    // The bottom layer of the ship goes.
    let content = CaveContent::new();
    scene.run(move |world| {
        let mut view = world.grid(id(0)).unwrap();
        for x in 0..4 {
            for z in 0..4 {
                view.set(content.block_channel(), VoxelCoordinate::new(x, 0, z), 0)
                    .unwrap();
            }
        }
    });
    scene.ticks(80);
    let height = height_of(ship_pose(&scene, 0));
    assert!(
        (height - (FLOOR - 1.0)).abs() < 0.05,
        "{height} against {}",
        FLOOR - 1.0
    );
    // The top layer of the floor under it goes too, in a pit that is wider than the ship.
    let content = CaveContent::new();
    scene.run(move |world| {
        for x in 8..24 {
            for z in 8..24 {
                world
                    .grid(GridId::root(LevelIndex::MAIN))
                    .unwrap()
                    .set(content.block_channel(), VoxelCoordinate::new(x, 200, z), 0)
                    .unwrap();
            }
        }
    });
    scene.ticks(100);
    let height = height_of(ship_pose(&scene, 0));
    assert!(
        (height - (FLOOR - 2.0)).abs() < 0.05,
        "{height} against {}",
        FLOOR - 2.0
    );
}

#[test]
fn a_held_ship_stays_in_the_air_until_it_is_let_go_and_a_ship_put_somewhere_goes_on_from_there() {
    let mut scene = Scene::new();
    heavy_planet(&mut scene);
    flat_floor(&mut scene);
    make_a_cube(&mut scene, 3, pose_at(14.0, FLOOR + 20.0, 14.0));
    scene.run(|world| world.set_grid_held(id(0), true).unwrap());
    let held = ship_pose(&scene, 0);
    scene.ticks(30);
    assert_eq!(ship_pose(&scene, 0), held);
    scene.run(|world| world.set_grid_held(id(0), false).unwrap());
    scene.ticks(10);
    assert!(height_of(ship_pose(&scene, 0)) < height_of(held) - 1.0);
    scene.ticks(100);
    assert!((height_of(ship_pose(&scene, 0)) - FLOOR).abs() < 0.05);
    // Something else puts it somewhere in the air: it falls from there, not from where it was, and not back.
    scene.run(|world| {
        world
            .set_grid_pose(id(0), pose_at(40.0, FLOOR + 15.0, 40.0))
            .unwrap();
    });
    scene.ticks(3);
    let pose = ship_pose(&scene, 0);
    // (x = 40 is in the next brick along, with an offset of 8.)
    assert_eq!(pose.position.brick().x(), 1);
    assert!((pose.position.offset()[0] - 8.0).abs() < 0.01);
    assert!(height_of(pose) < FLOOR + 15.0 && height_of(pose) > FLOOR + 10.0);
    scene.ticks(120);
    assert!(
        (height_of(ship_pose(&scene, 0)) - FLOOR).abs() < 0.05,
        "it rests on the ground where it was put"
    );
}

#[test]
fn a_ship_rests_on_a_held_one_and_removing_ships_leaves_the_others_to_carry_on() {
    let mut scene = Scene::new();
    heavy_planet(&mut scene);
    flat_floor(&mut scene);
    make_a_cube_that_may_be_held(&mut scene, 4, pose_at(14.0, FLOOR, 14.0), true);
    make_a_cube(&mut scene, 2, pose_at(15.0, FLOOR + 12.0, 15.0));
    scene.ticks(100);
    assert!((height_of(ship_pose(&scene, 1)) - (FLOOR + 4.0)).abs() < 0.05);
    assert_eq!(height_of(ship_pose(&scene, 0)), FLOOR);
    // The one below goes: the one on it falls to the ground.
    scene.run(|world| {
        assert!(world.remove_grid(id(0)).is_some());
    });
    scene.ticks(60);
    assert!((height_of(ship_pose(&scene, 1)) - FLOOR).abs() < 0.05);
    // And when they are all gone and a new one comes, it works as the first did.
    scene.run(|world| {
        assert!(world.remove_grid(id(1)).is_some());
    });
    scene.ticks(3);
    make_a_cube(&mut scene, 2, pose_at(20.0, FLOOR + 6.0, 20.0));
    scene.ticks(80);
    assert!((height_of(ship_pose(&scene, 2)) - FLOOR).abs() < 0.05);
    // With none left, the next one sets the frame again, wherever it is: a ship far from where the others were still falls.
    scene.run(|world| {
        assert!(world.remove_grid(id(2)).is_some());
    });
    scene.ticks(3);
    let faraway = GridPose::new(
        WorldPosition::new(BrickCoordinate::new(3000, 9, 0), [0.0; 3]),
        Orientation::IDENTITY,
    );
    make_a_cube(&mut scene, 2, faraway);
    scene.ticks(10);
    assert!(height_of(ship_pose(&scene, 3)) < 9.0 * 32.0 - 1.0);
}

#[test]
fn flames_are_not_in_the_way_of_a_ship() {
    let mut scene = Scene::new();
    heavy_planet(&mut scene);
    // A floor of fire, which bodies pass through: the ship goes through it and on down to the hills.
    let content = CaveContent::new();
    scene.run(move |world| {
        for x in 0..32 {
            for z in 0..32 {
                world
                    .grid(GridId::root(LevelIndex::MAIN))
                    .unwrap()
                    .set(
                        content.block_channel(),
                        VoxelCoordinate::new(x, 200, z),
                        content.blocks.fire,
                    )
                    .unwrap();
            }
        }
    });
    make_a_cube(&mut scene, 2, pose_at(10.0, FLOOR + 6.0, 10.0));
    scene.ticks(40);
    assert!(height_of(ship_pose(&scene, 0)) < FLOOR - 5.0);
}

#[test]
fn a_ship_too_far_from_the_first_one_of_its_level_stays_where_it_is() {
    let mut scene = Scene::new();
    heavy_planet(&mut scene);
    make_a_cube(&mut scene, 2, pose_at(0.0, 300.0, 0.0));
    // A ship 2000 bricks away from the first would need more than the frame can hold.
    let far = GridPose::new(
        WorldPosition::new(BrickCoordinate::new(2000, 9, 0), [0.0; 3]),
        Orientation::IDENTITY,
    );
    make_a_cube(&mut scene, 2, far);
    scene.ticks(20);
    assert!(
        height_of(ship_pose(&scene, 0)) < 300.0 - 10.0,
        "the first one falls"
    );
    assert_eq!(ship_pose(&scene, 1), far, "the far one does not");
}

/// Space: no gravity of the level, and a body of at least a million kilograms pulls at a strength that shows in a few ticks.
fn pulling_space(threshold: Option<f32>) -> GravitySettings {
    GravitySettings {
        base: [0.0; 3],
        pull_strength: 1e-3,
        pull_mass_threshold: threshold,
        softening: 1.0,
    }
}

#[test]
fn a_ship_that_weighs_enough_pulls_the_ships_near_it_toward_its_centre_and_one_that_does_not_pulls_nothing()
 {
    let pulled = |threshold: Option<f32>| {
        let mut scene = Scene::new();
        scene.run(move |world| world.set_gravity_settings(pulling_space(threshold)));
        // A cube of 16 voxels along an edge: 4,096 blocks of 1,000 kilograms, held; and a small one 30 voxels from it.
        make_a_cube_that_may_be_held(&mut scene, 16, pose_at(200.0, 400.0, 0.0), true);
        make_a_cube(&mut scene, 6, pose_at(170.0, 400.0, 0.0));
        scene.ticks(10);
        velocity_of(&scene, 1)
    };
    let velocity = pulled(Some(1e6));
    // Toward the big one's centre, (208, 408, 8), from the small one's, (173, 403, 3): a distance of 35.7, an acceleration of
    // G m / (d^2 + 1) = 1e-3 * 4.096e6 / 1275 = 3.2 for eleven ticks of a twentieth of a second (the tick that made the ship is one). The pull
    // is at the centre of mass, not at the corner a ship is placed by: the two are 5 voxels apart.
    let toward = [35.0f32, 5.0, 5.0];
    let distance = (toward[0] * toward[0] + toward[1] * toward[1] + toward[2] * toward[2]).sqrt();
    let acceleration = 1e-3 * 4.096e6 / (distance * distance + 1.0);
    let seconds = 0.55;
    for axis in 0..3 {
        let expected = acceleration * toward[axis] / distance * seconds;
        assert!(
            (velocity[axis] - expected).abs() < 0.03 * expected.abs() + 0.01,
            "axis {axis}: {velocity:?}, expected {expected}"
        );
    }
    // With the threshold above its mass, nothing pulls anything.
    assert_eq!(pulled(Some(5e6)), [0.0; 3]);
    assert_eq!(pulled(None), [0.0; 3]);
}

#[test]
fn a_ship_does_not_pull_itself_and_a_held_one_that_is_pulled_does_not_move() {
    let mut scene = Scene::new();
    scene.run(|world| world.set_gravity_settings(pulling_space(Some(1e6))));
    // A big ship alone in space that is free to move: nothing pulls it.
    make_a_cube(&mut scene, 16, pose_at(200.0, 400.0, 0.0));
    scene.ticks(20);
    assert_eq!(velocity_of(&scene, 0), [0.0; 3]);
    // A held one of the same weight beside it pulls it, and is not moved by being pulled or by pulling.
    make_a_cube_that_may_be_held(&mut scene, 16, pose_at(260.0, 400.0, 0.0), true);
    let held = ship_pose(&scene, 1);
    scene.ticks(20);
    assert_eq!(ship_pose(&scene, 1), held);
    assert!(
        velocity_of(&scene, 0)[0] > 0.5,
        "{:?}",
        velocity_of(&scene, 0)
    );
}

/// A scene with ships of different shapes, one turned, one pushed, one edited, on the floor and in the air.
fn busy_scene() -> Scene {
    let mut scene = Scene::new();
    heavy_planet(&mut scene);
    flat_floor(&mut scene);
    make_a_cube(&mut scene, 4, pose_at(14.0, FLOOR + 6.0, 14.0));
    let turned = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [30.0, FLOOR + 12.0, 12.0]),
        Orientation::from_yaw_pitch(0.7, -0.4),
    );
    make_a_cube(&mut scene, 3, turned);
    make_a_cube(&mut scene, 2, pose_at(15.0, FLOOR + 30.0, 15.0));
    for tick in 0..150 {
        let content = CaveContent::new();
        scene.run(move |world| {
            if tick % 7 == 0 {
                world
                    .apply_grid_force(id(1), [90_000.0, 0.0, -30_000.0], Some([1.0, 1.0, 1.0]))
                    .unwrap();
            }
            if tick == 60 {
                world
                    .grid(id(0))
                    .unwrap()
                    .set(content.block_channel(), VoxelCoordinate::new(0, 0, 0), 0)
                    .unwrap();
            }
        });
    }
    scene
}

/// A hash of the pose and speed of every ship, bit for bit.
fn hash_of_ships(scene: &Scene) -> u64 {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |value: u64| {
        for byte in value.to_le_bytes() {
            state ^= u64::from(byte);
            state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for (grid, ship) in scene.rig.server.world().grids().iter() {
        feed(grid.get());
        let (brick, offset) = (ship.pose.position.brick(), ship.pose.position.offset());
        for value in [brick.x(), brick.y(), brick.z()] {
            feed(value as u64);
        }
        for value in offset
            .into_iter()
            .chain(ship.pose.orientation.components())
            .chain(ship.linear_velocity)
            .chain(ship.angular_velocity)
        {
            feed(u64::from(value.to_bits()));
        }
    }
    state
}

/// A hash of the pose alone of every ship, bit for bit: where the ships are, whatever is on record of how fast they were going.
fn hash_of_poses(scene: &Scene) -> u64 {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |value: u64| {
        for byte in value.to_le_bytes() {
            state ^= u64::from(byte);
            state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for (grid, ship) in scene.rig.server.world().grids().iter() {
        feed(grid.get());
        let (brick, offset) = (ship.pose.position.brick(), ship.pose.position.offset());
        for value in [brick.x(), brick.y(), brick.z()] {
            feed(value as u64);
        }
        for value in offset.into_iter().chain(ship.pose.orientation.components()) {
            feed(u64::from(value.to_bits()));
        }
    }
    state
}

#[test]
fn the_poses_of_the_ships_are_the_same_whatever_is_kept_of_their_speeds() {
    // Taken before a ship that went to sleep was given no speed on record (see the test of that), and not changed by it.
    // (It changed in Phase 32h, when a ship came to have colliders for the terrain within a few voxels of it and not for the bricks around its
    // bricks: 1_355_701_843_541_054_234 before. Two of the three ships are where they were to the bit, and the third, which slides and spins on the
    // ground, is 0.003 and 0.004 voxel from where it was and 0.0001 in the turn: the terrain that does not touch it is not the same in the solver.)
    // (It changed in Phase 32j, when a tick is cut into more solver steps while any point of a ship moves faster than about 48 voxels a second, which
    // the spinning ships of this scene do for a few ticks: 4_233_839_071_203_554_194 before.)
    assert_eq!(hash_of_poses(&busy_scene()), 15_101_816_868_498_766_766);
}

#[test]
fn the_same_ships_move_the_same_bits_every_time_and_on_every_machine() {
    let (first, second) = (busy_scene(), busy_scene());
    assert_eq!(hash_of_ships(&first), hash_of_ships(&second));
    // Written down once; a change in it is a change in the simulation (Windows and Linux must agree on it).
    // (It changed in Phase 32g, when a ship that sleeps was given no speed on record: 187_330_083_679_574_898 before; the poses did not.)
    // (It changed again in Phase 32h, with the terrain a ship has colliders for: 16_582_222_097_314_237_682 before; see the test of the poses.)
    // (And in Phase 32j, with the steps of a fast tick: 8_675_827_597_625_099_639 before; see the test of the poses.)
    assert_eq!(hash_of_ships(&first), 8_817_648_817_650_209_632);
}

/// A cube of 4 let go over the natural ground (not the flat floor of the other tests: it is what a player has), `height` voxels above where
/// the ground is at its middle. Returns how many bricks the terrain generator made in each of `ticks` ticks from the one before the cube was
/// made.
fn bricks_generated_each_tick_by_a_ship_falling_to_natural_ground(
    height: i64,
    ticks: usize,
) -> Vec<u64> {
    bricks_generated_each_tick_by_a_block_of_ship_falling_to_natural_ground([4; 3], height, ticks)
}

/// The same for a ship that is a box of stone of this size.
fn bricks_generated_each_tick_by_a_block_of_ship_falling_to_natural_ground(
    size: [i64; 3],
    height: i64,
    ticks: usize,
) -> Vec<u64> {
    let mut scene = Scene::new();
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let generated_so_far = |scene: &mut Scene| {
        let writer = std::sync::Arc::clone(&seen);
        scene.run(move |world| {
            writer.store(
                world.bricks_generated(),
                std::sync::atomic::Ordering::SeqCst,
            )
        });
        seen.load(std::sync::atomic::Ordering::SeqCst)
    };
    generated_so_far(&mut scene);
    let mut before = generated_so_far(&mut scene);
    // (The ground at x = 16, z = 16 is 102 high, in the world of the seed of these tests.)
    make_ship(
        &mut scene,
        pose_at(14.0, 102.0 + height as f32, 14.0),
        false,
        size,
        |_| true,
    );
    let mut counts = Vec::new();
    for _ in 0..ticks {
        let now = generated_so_far(&mut scene);
        counts.push(now - before);
        before = now;
    }
    counts
}

#[test]
fn a_ship_falling_to_natural_ground_makes_the_world_generate_only_the_terrain_near_it() {
    // (Found by a player: ships stopped the game for a quarter of a second as they came to the ground. A brick of natural terrain costs ten
    // milliseconds or more to generate and make colliders of, and a ship had colliders made for all the bricks around its bricks: 375
    // generated in these 120 ticks, 90 of them in one. It has them for the terrain within some voxels of its bounds: 50, 24 in the most.)
    let counts = bricks_generated_each_tick_by_a_ship_falling_to_natural_ground(14, 120);
    let (total, most) = (counts.iter().sum::<u64>(), *counts.iter().max().unwrap());
    assert!(
        total <= 100 && most <= 40,
        "{total} bricks generated in all, {most} in one tick: {counts:?}"
    );
}

#[test]
fn a_ship_that_falls_a_long_way_has_the_ground_ready_for_it_and_stops_on_it() {
    // From 300 above the ground a ship is going 138 voxels a second where it lands, nearly 7 a tick: the colliders of the ground are wanted
    // from far enough ahead of it, and if they were not it would go through. (The floor of these tests, which is flat: natural ground is
    // hills, and a cube that lands on a slope rolls to wherever it rolls.)
    let mut scene = Scene::new();
    heavy_planet(&mut scene);
    flat_floor(&mut scene);
    make_a_cube(&mut scene, 4, pose_at(14.0, FLOOR + 300.0, 14.0));
    let mut fastest = 0.0f32;
    for _ in 0..250 {
        scene.ticks(1);
        fastest = fastest.max(-velocity_of(&scene, 0)[1]);
    }
    assert!(fastest > 100.0, "it fell fast: {fastest}");
    let height = height_of(ship_pose(&scene, 0));
    assert!(
        (height - FLOOR).abs() < 0.05,
        "it rests on the floor, not in it or under it: {height}"
    );
    assert!(velocity_of(&scene, 0)[1].abs() < 0.1);
}

#[test]
fn a_ship_that_falls_faster_than_the_floor_is_thick_stops_on_it() {
    // From 1000 above, a ship is going about 250 voxels a second where it lands, which is more than the floor (three thick) is thick in a
    // solver step of the three a tick has: it went through, and landed on the hills below. A tick is cut into more steps while it is fast.
    let mut scene = Scene::new();
    heavy_planet(&mut scene);
    flat_floor(&mut scene);
    make_a_cube(&mut scene, 4, pose_at(14.0, FLOOR + 1000.0, 14.0));
    let mut fastest = 0.0f32;
    for _ in 0..400 {
        scene.ticks(1);
        fastest = fastest.max(-velocity_of(&scene, 0)[1]);
    }
    assert!(fastest > 200.0, "it fell fast: {fastest}");
    let height = height_of(ship_pose(&scene, 0));
    assert!(
        (height - FLOOR).abs() < 0.05,
        "it rests on the floor, not in it or under it: {height}"
    );
}

#[test]
fn the_terrain_a_ship_only_wants_gets_colliders_one_brick_a_tick_and_all_of_it_in_the_end() {
    // A raft of 30 by 30 held in the sky (every brick there is empty and cheap, so only the counts matter). The bricks its bounds reach within two
    // voxels are needed and are all made in the first tick (4: it is over two bricks in each direction); the rest of the 8 voxels around it (9
    // bricks in all) is only wanted, and comes one a tick, nearest first.
    let (mut scene, report) = Scene::reporting();
    make_ship(
        &mut scene,
        pose_at(2.0, 200.0, 2.0),
        true,
        [30, 1, 30],
        |_| true,
    );
    let mut per_tick = vec![report().terrain_bricks_made_last_tick];
    for _ in 0..8 {
        scene.ticks(1);
        per_tick.push(report().terrain_bricks_made_last_tick);
    }
    assert_eq!(per_tick, [5, 1, 1, 1, 1, 0, 0, 0, 0], "{per_tick:?}");
    assert_eq!(report().terrain_bricks_made, 9);
    assert_eq!(report().terrain_bricks_made_most_in_a_tick, 5);
}

#[test]
fn a_big_raft_falling_to_natural_ground_makes_the_world_generate_a_few_bricks_a_tick_and_not_a_wall_of_them()
 {
    // (The owner's play: rafts of 900 and 1600 blocks stopped the game for 70 to 80 ms as they came to new ground. Before the budget these two
    // had 24 and 31 bricks generated in their worst tick, and 12 now, which is the bricks the raft is in when it is made.)
    for size in [[30, 1, 30], [40, 1, 40]] {
        let counts =
            bricks_generated_each_tick_by_a_block_of_ship_falling_to_natural_ground(size, 14, 150);
        let most = *counts.iter().max().unwrap();
        assert!(
            most <= 14,
            "{size:?}: {most} bricks in one tick: {counts:?}"
        );
    }
}

/// How many blocks a ship has in a box of its coordinates.
fn blocks_of(scene: &Scene, number: u64, size: [i64; 3]) -> usize {
    let content = CaveContent::new();
    let ship = scene
        .rig
        .server
        .world()
        .grids()
        .get(common::ship_id(number))
        .expect("the ship exists");
    let mut count = 0;
    for x in 0..size[0] {
        for y in 0..size[1] {
            for z in 0..size[2] {
                let block = ship
                    .voxels
                    .get(content.block_channel(), VoxelCoordinate::new(x, y, z));
                count += usize::from(block != content.blocks.air);
            }
        }
    }
    count
}

/// Takes the blocks of a ship at these voxels away (in the next tick).
fn cut(scene: &mut Scene, number: u64, voxels: Vec<[i64; 3]>) {
    let content = CaveContent::new();
    scene.run(move |world| {
        for [x, y, z] in voxels {
            world
                .grid(common::ship_id(number))
                .unwrap()
                .set(
                    content.block_channel(),
                    VoxelCoordinate::new(x, y, z),
                    content.blocks.air,
                )
                .unwrap();
        }
    });
}

fn ship_numbers(scene: &Scene) -> Vec<u64> {
    scene
        .rig
        .server
        .world()
        .grids()
        .iter()
        .map(|(id, _)| id.get() - GridId::FIRST_ALLOCATED)
        .collect()
}

#[test]
fn a_ship_cut_in_two_becomes_two_ships_that_fall_and_rest_on_their_own() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    make_ship(
        &mut scene,
        pose_at(10.0, FLOOR + 20.0, 10.0),
        false,
        [20, 1, 4],
        |_| true,
    );
    scene.ticks(3);
    assert_eq!(ship_numbers(&scene), [0]);
    // A column of four blocks across the middle.
    cut(&mut scene, 0, (0..4).map(|z| [10, 0, z]).collect());
    scene.ticks(3);
    // The ship is gone, and two new ones are there, none with its id, with the 40 blocks to one side and the 36 to the other.
    let numbers = ship_numbers(&scene);
    assert_eq!(numbers.len(), 2, "{numbers:?}");
    assert!(numbers.iter().all(|number| *number > 0));
    let mut counts: Vec<usize> = numbers
        .iter()
        .map(|number| blocks_of(&scene, *number, [20, 1, 4]))
        .collect();
    counts.sort();
    assert_eq!(counts, [36, 40]);
    // The old id leads to both, and they go on falling, and rest on the floor.
    let resolved = scene.rig.server.world().grids().resolve(common::ship_id(0));
    assert_eq!(resolved.len(), 2);
    scene.ticks(200);
    for number in numbers {
        let height = height_of(ship_pose(&scene, number));
        assert!((height - FLOOR).abs() < 0.1, "{number} rests at {height}");
    }
}

#[test]
fn a_dent_or_a_hole_that_cuts_nothing_off_leaves_the_ship_whole() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    make_ship(
        &mut scene,
        pose_at(10.0, FLOOR + 20.0, 10.0),
        false,
        [10, 1, 10],
        |_| true,
    );
    // A hole in the middle, an end block, and a corner.
    cut(
        &mut scene,
        0,
        vec![[5, 0, 5], [0, 0, 0], [9, 0, 9], [9, 0, 8]],
    );
    scene.ticks(20);
    assert_eq!(ship_numbers(&scene), [0]);
    assert_eq!(blocks_of(&scene, 0, [10, 1, 10]), 96);
}

#[test]
fn a_big_ship_cut_in_two_is_whole_until_the_work_of_finding_out_is_done_and_the_work_is_counted_not_timed()
 {
    // Two halves of 7500 blocks: finding that the cut separates them takes both being flooded to the end of one, some 15000 voxels, which the
    // budget of 4096 a tick gives in four ticks.
    let mut scene = Scene::new();
    make_ship(
        &mut scene,
        pose_at(0.0, 5000.0, 0.0),
        true,
        [151, 1, 100],
        |_| true,
    );
    scene.ticks(2);
    cut(&mut scene, 0, (0..100).map(|z| [75, 0, z]).collect());
    let mut ticks_until_split = 0;
    while ship_numbers(&scene) == [0] && ticks_until_split < 20 {
        scene.ticks(1);
        ticks_until_split += 1;
    }
    assert!(
        (3..=6).contains(&ticks_until_split),
        "split after {ticks_until_split} ticks"
    );
    assert_eq!(ship_numbers(&scene).len(), 2);
    // Held, and still held: the parts are where the ship was.
    for number in ship_numbers(&scene) {
        assert!(
            scene
                .rig
                .server
                .world()
                .grids()
                .get(common::ship_id(number))
                .unwrap()
                .held
        );
    }
}
