//! Ships as bodies in Cave: they fall under gravity, rest on the ground and on each other, weigh what their blocks do, can be held still,
//! carry on from where anything else puts them, and are the same on every machine.

mod common;

use common::{FLOOR, Scene, flat_floor, make_ship, pose_at, ship_id as id};
use server::GridView;
use voxel_space::{
    BrickCoordinate, GravitySettings, GridPose, LevelIndex, Orientation, VoxelCoordinate,
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
                    .set_voxel(
                        LevelIndex::MAIN,
                        content.block_channel(),
                        VoxelCoordinate::new(x, 200, z),
                        0,
                    )
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
    // A floor of fire, which bodies pass through: the ship goes through it and on down to the hills.
    let content = CaveContent::new();
    scene.run(move |world| {
        for x in 0..32 {
            for z in 0..32 {
                world
                    .set_voxel(
                        LevelIndex::MAIN,
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
    assert_eq!(hash_of_poses(&busy_scene()), 1_355_701_843_541_054_234);
}

#[test]
fn the_same_ships_move_the_same_bits_every_time_and_on_every_machine() {
    let (first, second) = (busy_scene(), busy_scene());
    assert_eq!(hash_of_ships(&first), hash_of_ships(&second));
    // Written down once; a change in it is a change in the simulation (Windows and Linux must agree on it).
    // (It changed in Phase 32g, when a ship that sleeps was given no speed on record: 187_330_083_679_574_898 before; the poses did not.)
    assert_eq!(hash_of_ships(&first), 16_582_222_097_314_237_682);
}
