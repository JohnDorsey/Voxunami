//! Players aboard ships: a walker who stands on a ship is simulated in the ship's own coordinates and carried by it exactly; gravity
//! there is the world's, in the ship's axes, so a ship turned on its side or over has them stand on what is now its floor; they leave when
//! the ship has nothing under them, keeping the speed it had; and none of it depends on the machine.

mod common;

use common::{
    FLOOR, Player, PlayerExt, Scene, each_tick, eyes_in_the_world, eyes_in_their_frame, flat_floor,
    make_ship, player_pose, pose_at, scene_with_a_deck, ship_id, ship_pose, tick_and_follow,
    walker_on_the_deck,
};
use game_shell::KeyCode;
use player_presence::{PlayerClient, player_kind};
use protocol::ClientId;
use server::GridView;
use voxel_space::{BrickCoordinate, GridPose, Orientation, WorldPosition};
use voxunami_cave::aboard::in_voxels;

#[test]
fn a_walker_who_lands_on_a_ship_is_aboard_it_standing_in_its_coordinates() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, ship_id(0), "aboard the deck");
    // They fell straight down onto it from x = 20, z = 24: (15, 4) in the deck's coordinates, eyes 1.62 above the top, which is 2.
    let [x, y, z] = in_voxels(pose.position);
    assert!((x - 15.0).abs() < 0.01 && (z - 4.0).abs() < 0.01, "{x} {z}");
    assert!((y - (2.0 + 1.62)).abs() < 0.03, "{y}");
    // Standing quietly, to the bit.
    let standing = pose;
    tick_and_follow(&mut scene, &mut player, 40);
    assert_eq!(player_pose(&scene, 1), standing);
    // The world has them where the deck puts them: 5 above the floor and the eyes 1.62 above that.
    let eyes = eyes_in_the_world(&scene, 1);
    assert!(
        (eyes[1] - (FLOOR as f64 + 5.0 + 1.62)).abs() < 0.03,
        "{eyes:?}"
    );
    assert!((eyes[0] - 20.0).abs() < 0.01 && (eyes[2] - 24.0).abs() < 0.01);
}

#[test]
fn a_ship_that_moves_carries_the_walker_exactly_and_the_walker_does_not_move_in_it() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    let in_the_ship = player_pose(&scene, 1);
    let before = eyes_in_the_world(&scene, 1);
    // The ship goes five east in one tick, then a tenth a tick for thirty ticks. Nothing pushes the walker: no fictitious forces.
    scene.run(|world| {
        world
            .set_grid_pose(ship_id(0), pose_at(10.0, FLOOR + 3.0, 20.0))
            .unwrap();
    });
    player.advance().unwrap();
    assert_eq!(player_pose(&scene, 1), in_the_ship, "not moved in the ship");
    let after = eyes_in_the_world(&scene, 1);
    assert!(
        (after[0] - before[0] - 5.0).abs() < 1e-4,
        "{before:?} {after:?}"
    );
    assert!((after[1] - before[1]).abs() < 1e-4 && (after[2] - before[2]).abs() < 1e-4);
    each_tick(&mut scene, &mut player, 30, |world, tick| {
        let x = 10.0 + 0.1 * (tick as f32 + 1.0);
        world
            .set_grid_pose(ship_id(0), pose_at(x, FLOOR + 3.0, 20.0))
            .unwrap();
    });
    assert_eq!(player_pose(&scene, 1), in_the_ship);
    let carried = eyes_in_the_world(&scene, 1);
    assert!((carried[0] - before[0] - 8.0).abs() < 1e-3, "{carried:?}");
    // What the player is shown is where the ship puts them too: the camera follows.
    let camera = common::voxels_of(&player.camera);
    assert!((camera[0] - carried[0]).abs() < 1e-3 && (camera[1] - carried[1]).abs() < 1e-3);
}

#[test]
fn walking_aboard_is_in_the_coordinates_of_the_ship_whichever_way_it_faces() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    let before = eyes_in_their_frame(&scene, 1);
    // The ship is turned a quarter about the vertical axis, with its origin put so that the player is where they were in the world.
    let turned = GridPose::new(
        WorldPosition::new(
            BrickCoordinate::ORIGIN,
            [20.0 + 4.0, FLOOR + 3.0, 24.0 + 15.0],
        ),
        Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
    );
    scene.run(move |world| world.set_grid_pose(ship_id(0), turned).unwrap());
    player.advance().unwrap();
    assert_eq!(eyes_in_their_frame(&scene, 1), before);
    assert_eq!(ship_pose(&scene, 0), turned);
    let in_the_world = eyes_in_the_world(&scene, 1);
    // W walks the way the player faces in the ship's frame (yaw 0 is the ship's -z): along the ship, whichever way the ship faces.
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut scene, &mut player, 10);
    let walked = eyes_in_their_frame(&scene, 1);
    assert!(
        (before[2] - walked[2] - 2.03).abs() < 0.15,
        "{before:?} {walked:?}"
    );
    assert!((walked[0] - before[0]).abs() < 0.01 && (walked[1] - before[1]).abs() < 0.03);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    // In the world that is along the ship's -z, which a quarter turn takes to the world's -x.
    let then = eyes_in_the_world(&scene, 1);
    assert!(
        (in_the_world[0] - then[0] - 2.03).abs() < 0.15,
        "{in_the_world:?} {then:?}"
    );
    assert!((then[2] - in_the_world[2]).abs() < 0.01);
}

#[test]
fn landing_on_a_ship_that_moves_the_walker_comes_to_rest_in_it_and_goes_with_it() {
    let mut scene = scene_with_a_deck();
    let mut player = scene
        .rig
        .join_walking([20.0, FLOOR as f64 + 13.0, 24.0], 0.0, 0.0);
    // The deck goes east at four a second from now, while the walker falls.
    let moving = |world: &mut server::ServerWorld, tick: usize| {
        let x = 5.0 + 0.2 * (tick as f32 + 1.0);
        world
            .set_grid_pose(ship_id(0), pose_at(x, FLOOR + 3.0, 20.0))
            .unwrap();
        world
            .set_grid_velocity(ship_id(0), [4.0, 0.0, 0.0], [0.0; 3])
            .unwrap();
    };
    each_tick(&mut scene, &mut player, 60, moving);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0), "aboard");
    // At rest in the ship: it kept no speed of its own against it, so over the next while they do not move in it.
    let at_rest = eyes_in_their_frame(&scene, 1);
    let world_before = eyes_in_the_world(&scene, 1);
    each_tick(&mut scene, &mut player, 20, move |world, tick| {
        moving(world, tick + 60)
    });
    let later = eyes_in_their_frame(&scene, 1);
    for axis in 0..3 {
        assert!(
            (later[axis] - at_rest[axis]).abs() < 1e-3,
            "{at_rest:?} {later:?}"
        );
    }
    // And the world has them go with it: four a second for a second.
    let world_after = eyes_in_the_world(&scene, 1);
    assert!(
        (world_after[0] - world_before[0] - 4.0).abs() < 0.05,
        "{world_before:?} {world_after:?}"
    );
}

#[test]
fn walking_off_the_edge_of_a_ship_that_moves_leaves_it_with_the_speed_it_had() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    // The deck sets off east at six a second (the walker is not pushed), and the walker walks toward its -z edge, four away.
    let moving = |world: &mut server::ServerWorld, tick: usize| {
        let x = 5.0 + 0.3 * (tick as f32 + 1.0);
        world
            .set_grid_pose(ship_id(0), pose_at(x, FLOOR + 3.0, 20.0))
            .unwrap();
        world
            .set_grid_velocity(ship_id(0), [6.0, 0.0, 0.0], [0.0; 3])
            .unwrap();
    };
    player.hold(&[KeyCode::KeyW]);
    let mut left_at = None;
    for tick in 0..60 {
        each_tick(&mut scene, &mut player, 1, move |world, _| {
            moving(world, tick)
        });
        if player_pose(&scene, 1).grid.root_level().is_some() {
            left_at = Some(tick);
            break;
        }
        assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    }
    let left_at = left_at.expect("they walked off the edge and left the ship");
    // Four voxels at four and a bit a second: a little under a second, less the quarter of a voxel of the edge that carries them.
    assert!((15..28).contains(&left_at), "left at tick {left_at}");
    // In the world now, with the ship's speed: east, and in the first tick as far as six a second goes.
    let here = eyes_in_the_world(&scene, 1);
    each_tick(&mut scene, &mut player, 1, move |world, _| {
        moving(world, left_at + 1)
    });
    let next = eyes_in_the_world(&scene, 1);
    assert!(
        (0.25..0.34).contains(&(next[0] - here[0])),
        "went {} east in a tick",
        next[0] - here[0]
    );
    // And on they go: falling to the floor, in the level.
    each_tick(&mut scene, &mut player, 40, move |world, tick| {
        moving(world, left_at + 2 + tick)
    });
    let pose = player_pose(&scene, 1);
    assert!(pose.grid.root_level().is_some());
    let landed = in_voxels(pose.position);
    assert!(
        (landed[1] - (FLOOR as f64 + 1.62)).abs() < 0.05,
        "{landed:?}"
    );
}

#[test]
fn a_ship_that_turns_carries_the_walker_round_without_moving_them_in_it() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    let in_the_ship = player_pose(&scene, 1);
    let origin = ship_pose(&scene, 0).position;
    each_tick(&mut scene, &mut player, 60, move |world, tick| {
        let turned = GridPose::new(
            origin,
            Orientation::from_yaw_pitch(0.02 * (tick as f32 + 1.0), 0.0),
        );
        world.set_grid_pose(ship_id(0), turned).unwrap();
    });
    // Turned 1.2 radians, nothing in the ship has changed for the walker: not their place, not their view in the ship.
    assert_eq!(player_pose(&scene, 1), in_the_ship);
    // The world has them where the turned deck puts them: round the ship's origin, at a distance of sqrt(15^2 + 4^2) from it.
    let eyes = eyes_in_the_world(&scene, 1);
    let origin = in_voxels(origin);
    let radius = (15.0f64 * 15.0 + 4.0 * 4.0).sqrt();
    let (dx, dz) = (eyes[0] - origin[0], eyes[2] - origin[2]);
    let distance = (dx * dx + dz * dz).sqrt();
    assert!((distance - radius).abs() < 1e-3, "{distance} {radius}");
    let expected = ship_pose(&scene, 0)
        .local_to_world(in_the_ship.position)
        .unwrap();
    let expected = in_voxels(expected);
    for axis in 0..3 {
        assert!((expected[axis] - eyes[axis]).abs() < 1e-3);
    }
    assert!(
        (eyes[0] - 20.0).abs() > 3.0,
        "they are not where they were: {eyes:?}"
    );
    // The view is carried with the ship: they look where the ship's -z points, in the world, turned 1.2 radians from where it did.
    let ship_ahead = ship_pose(&scene, 0).direction_to_world([0.0, 0.0, -1.0]);
    let looking = player.camera.forward().to_array();
    for axis in 0..3 {
        assert!(
            (looking[axis] - ship_ahead[axis]).abs() < 1e-4,
            "{looking:?} against {ship_ahead:?}"
        );
    }
    assert!(looking[0] < -0.9, "(a turn of 1.2 faces mostly -x)");
}

/// The pose that puts the middle of a ship (its origin plus `half`, in its own coordinates) at `centre` in the world, turned as
/// `orientation` says: what a ship that turns about its middle is at each moment.
fn about_the_middle(centre: [f32; 3], half: [f32; 3], orientation: Orientation) -> GridPose {
    let origin = WorldPosition::new(BrickCoordinate::ORIGIN, [0.0; 3]);
    let turned = GridPose::new(origin, orientation).direction_to_world(half);
    GridPose::new(
        WorldPosition::new(
            BrickCoordinate::ORIGIN,
            [0, 1, 2].map(|axis| centre[axis] - turned[axis]),
        ),
        orientation,
    )
}

/// A turn of `angle` radians about the z axis.
fn roll(angle: f32) -> Orientation {
    Orientation::from_components([0.0, 0.0, libm::sinf(angle / 2.0), libm::cosf(angle / 2.0)])
        .unwrap()
}

/// The middle of the cabin of these tests, in the air high above the floor.
const CABIN_MIDDLE: [f32; 3] = [24.0, FLOOR + 20.0, 24.0];

/// A hollow cabin of stone, 8 along each edge with walls a voxel thick, held, with its middle at `CABIN_MIDDLE`; and a walker who falls
/// onto its floor from a little way up inside it, and is aboard it standing still.
fn walker_in_a_cabin(scene: &mut Scene) -> Player {
    flat_floor(scene);
    make_ship(
        scene,
        about_the_middle(CABIN_MIDDLE, [4.0; 3], Orientation::IDENTITY),
        true,
        [8, 8, 8],
        |[x, y, z]| [x, y, z].iter().any(|&edge| edge == 0 || edge == 7),
    );
    // The floor inside is at the middle's height less 3; the walker's eyes start half a voxel above where they will be standing.
    let mut player = scene.rig.join_walking(
        [24.0, f64::from(CABIN_MIDDLE[1]) - 3.0 + 1.62 + 0.5, 24.0],
        0.0,
        0.0,
    );
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(scene, &mut player, 60);
    player
}

#[test]
fn a_cabin_turned_on_its_side_has_the_walker_fall_to_the_wall_that_is_now_its_floor() {
    let mut scene = Scene::new();
    let mut player = walker_in_a_cabin(&mut scene);
    let standing = player_pose(&scene, 1);
    assert_eq!(standing.grid, ship_id(0));
    let [x, y, z] = in_voxels(standing.position);
    assert!((x - 4.0).abs() < 0.01 && (z - 4.0).abs() < 0.01, "{x} {z}");
    assert!(
        (y - (1.0 + 1.62)).abs() < 0.03,
        "on the floor, which is the voxels up to 1: {y}"
    );

    // The cabin rolls a quarter turn about the line through its middle along z, a degree a tick.
    let quarter = std::f32::consts::FRAC_PI_2;
    each_tick(&mut scene, &mut player, 90, move |world, tick| {
        let angle = quarter * (tick as f32 + 1.0) / 90.0;
        world
            .set_grid_pose(
                ship_id(0),
                about_the_middle(CABIN_MIDDLE, [4.0; 3], roll(angle)),
            )
            .unwrap();
        assert_eq!(world.entities().len(), 1);
    });
    tick_and_follow(&mut scene, &mut player, 80);

    // Aboard still, and standing on the wall that is the floor now: the wall at x = 0 to 1, up being +x in the ship (the world's down is
    // the ship's -x), against the wall of the old floor (y = 1 to 0) that they slid to: its face at y = 1 and half the player's width.
    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, ship_id(0), "aboard");
    let [x, y, z] = in_voxels(pose.position);
    assert!(
        (x - (1.0 + 1.62)).abs() < 0.05,
        "eyes 1.62 above the wall's face at x = 1: {x}"
    );
    // (It slid down the old floor, now a wall, to where that wall and the new floor meet, or nearly: the corner of its box is within
    // a fifth of a voxel of the face it slid down.)
    assert!((1.3..1.5).contains(&y), "{y}");
    assert!((z - 4.0).abs() < 0.05, "{z}");
    // In the world: on the floor of the rotated cabin, which is where the floor of the unrotated one was, 3 below the middle.
    let eyes = eyes_in_the_world(&scene, 1);
    assert!(
        (eyes[1] - (f64::from(CABIN_MIDDLE[1]) - 3.0 + 1.62)).abs() < 0.08,
        "{eyes:?}"
    );
    // Standing quietly there, to the bit.
    tick_and_follow(&mut scene, &mut player, 30);
    assert_eq!(player_pose(&scene, 1), pose);
}

#[test]
fn a_cabin_turned_over_in_a_moment_has_the_walker_fall_to_what_is_now_its_floor_the_ceiling() {
    let mut scene = Scene::new();
    let mut player = walker_in_a_cabin(&mut scene);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    // A half turn about z, all at once: the world's down is now the ship's up.
    let over = about_the_middle(CABIN_MIDDLE, [4.0; 3], roll(std::f32::consts::PI));
    scene.run(move |world| world.set_grid_pose(ship_id(0), over).unwrap());
    player.advance().unwrap();
    tick_and_follow(&mut scene, &mut player, 60);
    let pose = player_pose(&scene, 1);
    assert_eq!(
        pose.grid,
        ship_id(0),
        "they did not leave it by falling the length of it"
    );
    // Their feet are on the face of the ceiling at y = 7 and the eyes 1.62 below it (the ship's up is the world's down), where they were.
    let [x, y, z] = in_voxels(pose.position);
    assert!((y - (7.0 - 1.62)).abs() < 0.05, "{y}");
    assert!((x - 4.0).abs() < 0.05 && (z - 4.0).abs() < 0.05, "{x} {z}");
    // In the world they are standing on the floor of the turned cabin, 3 below its middle.
    let eyes = eyes_in_the_world(&scene, 1);
    assert!(
        (eyes[1] - (f64::from(CABIN_MIDDLE[1]) - 3.0 + 1.62)).abs() < 0.05,
        "{eyes:?}"
    );
}

#[test]
fn a_ship_taken_from_under_the_walker_leaves_them_in_the_world_where_they_were_to_fall() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    let before = eyes_in_the_world(&scene, 1);
    scene.run(|world| {
        world.remove_grid(ship_id(0)).unwrap();
    });
    player.advance().unwrap();
    // At once they are in the level, where they were.
    let pose = player_pose(&scene, 1);
    assert!(pose.grid.root_level().is_some());
    let now = in_voxels(pose.position);
    for axis in 0..3 {
        assert!((now[axis] - before[axis]).abs() < 0.2, "{before:?} {now:?}");
    }
    // And they fall the five voxels the deck was above the floor, and stand on it.
    tick_and_follow(&mut scene, &mut player, 40);
    let landed = in_voxels(player_pose(&scene, 1).position);
    assert!(
        (landed[1] - (FLOOR as f64 + 1.62)).abs() < 0.05,
        "{landed:?}"
    );
    assert!((landed[0] - 20.0).abs() < 0.05 && (landed[2] - 24.0).abs() < 0.05);
    assert!(player_pose(&scene, 1).grid.root_level().is_some());
}

#[test]
fn a_ship_taken_from_under_a_walker_does_not_turn_their_view_and_they_walk_the_way_they_face() {
    let mut scene = scene_with_a_deck();
    // A ship turned a radian, with the middle of its deck under the place the walker falls to (20, 24) (see the test of the view).
    let turned = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [11.23, FLOOR + 3.0, 30.25]),
        Orientation::from_yaw_pitch(1.0, 0.0),
    );
    scene.run(move |world| world.set_grid_pose(ship_id(0), turned).unwrap());
    let mut player = scene
        .rig
        .join_walking([50.0, FLOOR as f64 + 3.0, 50.0], 30.0, -10.0);
    scene.rig.settle(&mut [&mut player]);
    player.send_command(&format!("/tp 20 {} 24", FLOOR as i64 + 13));
    tick_and_follow(&mut scene, &mut player, 80);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0), "aboard");
    // The ship turns on, about the same point, to 1.8 radians, while they are aboard: their view is carried with it, so that the way they
    // face in the world is no longer the way they faced when they boarded. (With a ship that had not turned since they boarded, the
    // turn of their room in the world would be none and nothing would be tested.)
    let at_boarding = player.camera.forward().to_array();
    let turned_on = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [11.23, FLOOR + 3.0, 30.25]),
        Orientation::from_yaw_pitch(1.8, 0.0),
    );
    scene.run(move |world| world.set_grid_pose(ship_id(0), turned_on).unwrap());
    tick_and_follow(&mut scene, &mut player, 3);
    let facing = player.camera.forward().to_array();
    assert!(
        (0..3).any(|axis| (facing[axis] - at_boarding[axis]).abs() > 0.1),
        "the ship turned and carried the view: {at_boarding:?} then {facing:?}"
    );
    scene.run(|world| {
        world.remove_grid(ship_id(0)).unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 3);
    assert!(player_pose(&scene, 1).grid.root_level().is_some());
    // The ship is gone and the way they face in the world has not changed: the turn of the room they were in is theirs, in the level.
    let now = player.camera.forward().to_array();
    for axis in 0..3 {
        assert!(
            (now[axis] - facing[axis]).abs() < 1e-4,
            "{now:?} against {facing:?}"
        );
    }
    // They fall to the floor, and when they walk forward they go the way they face.
    tick_and_follow(&mut scene, &mut player, 40);
    let start = eyes_in_the_world(&scene, 1);
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut scene, &mut player, 10);
    let end = eyes_in_the_world(&scene, 1);
    let (along_x, along_z) = (end[0] - start[0], end[2] - start[2]);
    let walked = (along_x * along_x + along_z * along_z).sqrt();
    let flat = f64::from((facing[0] * facing[0] + facing[2] * facing[2]).sqrt());
    let cosine =
        (along_x * f64::from(facing[0]) + along_z * f64::from(facing[2])) / (walked * flat);
    assert!(
        walked > 1.0 && cosine > 0.999,
        "walked {walked} voxels at a cosine of {cosine} to the way they face {facing:?}"
    );
}

#[test]
fn a_jump_on_a_deck_stays_aboard_and_comes_down_where_it_started() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    let standing = eyes_in_their_frame(&scene, 1);
    player.hold(&[KeyCode::Space]);
    player.hold(&[]);
    let mut highest = standing[1];
    for _ in 0..30 {
        tick_and_follow(&mut scene, &mut player, 1);
        assert_eq!(player_pose(&scene, 1).grid, ship_id(0), "in the jump");
        highest = highest.max(eyes_in_their_frame(&scene, 1)[1]);
    }
    assert!(
        (1.1..1.5).contains(&(highest - standing[1])),
        "{}",
        highest - standing[1]
    );
    let landed = eyes_in_their_frame(&scene, 1);
    for axis in 0..3 {
        assert!(
            (landed[axis] - standing[axis]).abs() < 0.05,
            "{standing:?} {landed:?}"
        );
    }
}

#[test]
fn a_walker_who_climbs_onto_a_ship_from_the_ground_is_aboard_it_once_they_stand_on_it() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    // A deck one thick on the floor: a step of one voxel, which is more than a walker steps up and less than a jump.
    make_ship(
        &mut scene,
        pose_at(20.0, FLOOR, 20.0),
        true,
        [8, 1, 8],
        |_| true,
    );
    // Facing east (yaw a quarter to the right), on the floor, 3 from the deck's face.
    let mut player = scene.rig.join_walking(
        [16.5, f64::from(FLOOR) + 1.62 + 0.3, 24.0],
        -90.0, // (degrees: a quarter turn to the right, facing +x)
        0.0,
    );
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 40);
    assert!(player_pose(&scene, 1).grid.root_level().is_some());
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut scene, &mut player, 30);
    let at_the_face = in_voxels(player_pose(&scene, 1).position);
    assert!(
        (at_the_face[0] - 19.7).abs() < 0.1,
        "walked up to it and stopped: {at_the_face:?}"
    );
    assert!(
        player_pose(&scene, 1).grid.root_level().is_some(),
        "not aboard until on it"
    );
    // Over it: jump, still walking.
    player.hold(&[KeyCode::KeyW, KeyCode::Space]);
    tick_and_follow(&mut scene, &mut player, 6);
    player.hold(&[]);
    tick_and_follow(&mut scene, &mut player, 40);
    let pose = player_pose(&scene, 1);
    assert_eq!(pose.grid, ship_id(0), "on the deck, so aboard it");
    let [x, y, z] = in_voxels(pose.position);
    assert!(
        x > 0.2 && x < 4.0,
        "on the deck, a little way from its edge: {x}"
    );
    assert!((y - (1.0 + 1.62)).abs() < 0.05, "{y}");
    assert!((z - 4.0).abs() < 0.05);
}

#[test]
fn a_flyer_over_a_ship_is_not_aboard_it_and_is_not_carried() {
    let mut scene = scene_with_a_deck();
    // (A flying player stays where they are put.)
    let mut player = scene
        .rig
        .join([20.0, f64::from(FLOOR) + 5.0 + 1.62 + 0.5, 24.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 40);
    let hovering = player_pose(&scene, 1);
    assert!(hovering.grid.root_level().is_some());
    scene.run(|world| {
        world
            .set_grid_pose(ship_id(0), pose_at(30.0, FLOOR + 3.0, 20.0))
            .unwrap();
    });
    tick_and_follow(&mut scene, &mut player, 10);
    assert_eq!(player_pose(&scene, 1), hovering, "not carried");
}

#[test]
fn a_passenger_who_logs_out_aboard_is_aboard_when_they_come_back_even_if_the_ship_moved() {
    let mut scene = scene_with_a_deck();
    let player = walker_on_the_deck(&mut scene);
    let in_the_ship = player_pose(&scene, 1);
    assert_eq!(in_the_ship.grid, ship_id(0));
    drop(player);
    scene.ticks(3);
    // Away, they are still in the world in the frame of the ship: hidden, not removed, and not lost to the ship if it is saved.
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    // The ship moves, and turns, while they are away.
    let moved = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [12.0, FLOOR + 3.0, 30.0]),
        Orientation::from_yaw_pitch(0.7, 0.0),
    );
    scene.run(move |world| world.set_grid_pose(ship_id(0), moved).unwrap());
    scene.ticks(5);
    // They come back, as the same install, from the other side of the world as far as their own program knows.
    let mut back =
        scene
            .rig
            .join_walking_as(1, [-300.0, f64::from(FLOOR) + 40.0, -300.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut back]);
    tick_and_follow(&mut scene, &mut back, 30);
    // (The connection is the second; the entity is the one they had.)
    let now = {
        let client = PlayerClient(ClientId::new(2));
        scene
            .rig
            .server
            .world()
            .entities()
            .iter()
            .find(|entity| {
                entity.kind == player_kind()
                    && entity.components.get::<PlayerClient>() == Ok(Some(client))
            })
            .expect("the player has an entity again")
            .pose
    };
    assert_eq!(now.grid, ship_id(0), "still aboard");
    for axis in 0..3 {
        assert!(
            (in_voxels(now.position)[axis] - in_voxels(in_the_ship.position)[axis]).abs() < 0.01,
            "{:?} {:?}",
            now.position,
            in_the_ship.position
        );
    }
    // And the world has them where the moved ship puts them, which is where the view is.
    let expected = in_voxels(moved.local_to_world(now.position).unwrap());
    let camera = common::voxels_of(&back.camera);
    for axis in 0..3 {
        assert!(
            (camera[axis] - expected[axis]).abs() < 0.01,
            "{camera:?} {expected:?}"
        );
    }
}

#[test]
fn teleporting_a_passenger_puts_them_in_the_level_at_the_place_asked_and_not_aboard() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    // Above the floor, well away from the deck (which is over x 5 to 35, z 20 to 28).
    player.send_command("/tp 50 230 50");
    tick_and_follow(&mut scene, &mut player, 60);
    let pose = player_pose(&scene, 1);
    assert!(pose.grid.root_level().is_some(), "in the level, not aboard");
    let [x, y, z] = in_voxels(pose.position);
    assert!(
        (x - 50.0).abs() < 0.01 && (z - 50.0).abs() < 0.01,
        "{x} {z}"
    );
    assert!(
        (y - (FLOOR as f64 + 1.62)).abs() < 0.05,
        "fell to the floor: {y}"
    );
    // And their view is the world's again: upright, not the turn of any room.
    assert_eq!(player.camera.forward().to_array(), [0.0, 0.0, -1.0]);
    assert_eq!(player.camera.up().to_array(), [0.0, 1.0, 0.0]);
}

/// A hash of the pose of the player's record and of the ship's, bit for bit, after every tick of a scripted run: a deck that moves
/// and turns, a walker who falls onto it, walks, jumps and strafes about it, and walks off.
fn scripted_run() -> u64 {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |value: u64| {
        for byte in value.to_le_bytes() {
            state ^= u64::from(byte);
            state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    let mut scene = scene_with_a_deck();
    let mut player = scene
        .rig
        .join_walking([20.0, FLOOR as f64 + 13.0, 24.0], 0.0, 0.0);
    let origin = ship_pose(&scene, 0).position;
    for tick in 0..260usize {
        let keys: &[KeyCode] = match tick {
            60..=79 => &[KeyCode::KeyW],
            80..=81 => &[KeyCode::KeyW, KeyCode::Space],
            82..=119 => &[KeyCode::KeyD],
            120..=129 => &[KeyCode::KeyW, KeyCode::ControlLeft],
            130..=199 => &[KeyCode::KeyA],
            _ => &[],
        };
        if tick == 60 || tick == 80 || tick == 82 || tick == 120 || tick == 130 || tick == 200 {
            player.camera.yaw = 0.01 * tick as f32;
            player.hold(keys);
        }
        scene.run(move |world| {
            let turned = GridPose::new(
                origin.translated([0.05 * tick as f32, 0.0, 0.0]),
                Orientation::from_yaw_pitch(0.004 * tick as f32, 0.0),
            );
            world.set_grid_pose(ship_id(0), turned).unwrap();
            world
                .set_grid_velocity(ship_id(0), [1.0, 0.0, 0.0], [0.0, 0.2, 0.0])
                .unwrap();
        });
        player.advance().unwrap();
        let pose = player_pose(&scene, 1);
        feed(pose.grid.get());
        let (brick, offset) = (pose.position.brick(), pose.position.offset());
        for value in [brick.x(), brick.y(), brick.z()] {
            feed(value as u64);
        }
        for value in offset.into_iter().chain(pose.orientation.components()) {
            feed(u64::from(value.to_bits()));
        }
    }
    let at_the_end = player_pose(&scene, 1).grid;
    // (So that the number locks something: they boarded, and they got off again.)
    assert!(at_the_end.root_level().is_some(), "they left the deck");
    state
}

#[test]
fn a_scripted_run_aboard_ends_in_the_same_bits_every_time_and_on_every_machine() {
    let first = scripted_run();
    assert_eq!(first, scripted_run());
    // Taken on Windows; the test passes on Linux too, so the two agree to the bit.
    assert_eq!(first, 16_974_554_394_980_614_153);
}

#[test]
fn a_world_saved_with_a_passenger_aboard_holds_them_in_the_level_where_the_ship_has_them() {
    let directory =
        std::env::temp_dir().join(format!("voxunami_cave_aboard_save_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("world.voxunami");

    let mut scene = scene_with_a_deck();
    let player = walker_on_the_deck(&mut scene);
    // The ship moves and turns, so that where the passenger is in the world is not where their coordinates in it say.
    let moved = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [14.0, FLOOR + 3.0, 33.0]),
        Orientation::from_yaw_pitch(0.9, 0.0),
    );
    scene.run(move |world| world.set_grid_pose(ship_id(0), moved).unwrap());
    scene.ticks(3);
    let in_the_world = eyes_in_the_world(&scene, 1);
    // They log out aboard; the world is saved while they are away, ships not being saved yet.
    drop(player);
    scene.ticks(3);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    let Scene { rig, .. } = scene;
    let common::Rig {
        content, server, ..
    } = rig;
    let mut world = server.into_world();
    game_shell::files::save_world(&content, &mut world, &path).unwrap();
    // Saving did not take them off the ship in the running world.
    let entity = |world: &world_storage::WorldData| {
        world
            .entities()
            .iter()
            .find(|entity| entity.kind == player_kind())
            .expect("the player has an entity")
            .pose
    };
    assert_eq!(entity(&world).grid, ship_id(0));

    // The file has them where the ship had them, in the level: the next program run has no ship to be aboard.
    let mut content = voxunami_cave::content::CaveContent::new();
    let loaded = game_shell::files::open_world(&mut content, Some(&path), 1).unwrap();
    let saved = entity(&loaded);
    assert!(saved.grid.root_level().is_some());
    let [x, y, z] = in_voxels(saved.position);
    for (found, wanted) in [x, y, z].into_iter().zip(in_the_world) {
        assert!(
            (found - wanted).abs() < 1e-3,
            "{saved:?} against {in_the_world:?}"
        );
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn another_player_sees_a_passenger_where_the_ship_has_them_and_not_where_their_coordinates_say() {
    let mut scene = scene_with_a_deck();
    // A flyer who watches from beside the deck.
    let mut watcher = scene
        .rig
        .join([20.0, f64::from(FLOOR) + 8.0, 40.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut watcher]);
    let mut passenger = walker_on_the_deck(&mut scene);
    // Turn and move the ship, so that the coordinates of the passenger in it (15, 4) are nowhere near where they are in the world.
    let moved = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [30.0, FLOOR + 3.0, 40.0]),
        Orientation::from_yaw_pitch(1.1, 0.0),
    );
    scene.run(move |world| world.set_grid_pose(ship_id(0), moved).unwrap());
    tick_and_follow(&mut scene, &mut passenger, 3);
    watcher.advance().unwrap();
    // (The passenger joined second.)
    let expected = eyes_in_the_world(&scene, 2);
    let others: Vec<_> = watcher.other_players().collect();
    assert_eq!(others.len(), 1, "the passenger is in the picture");
    let seen = in_voxels(others[0].1.position);
    for axis in 0..3 {
        assert!(
            (seen[axis] - expected[axis]).abs() < 1e-3,
            "{seen:?} against {expected:?}"
        );
    }
}

#[test]
fn a_walker_over_a_hole_in_the_deck_falls_through_it_and_leaves_the_ship() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    // The blocks under them, and around, go: a hole three wide, through the deck (two thick).
    let content = voxunami_cave::content::CaveContent::new();
    scene.run(move |world| {
        let mut deck = world.grid(ship_id(0)).unwrap();
        for x in 13..=17 {
            for y in 0..2 {
                for z in 2..=6 {
                    deck.set(
                        content.block_channel(),
                        voxel_space::VoxelCoordinate::new(x, y, z),
                        content.blocks.air,
                    )
                    .unwrap();
                }
            }
        }
    });
    tick_and_follow(&mut scene, &mut player, 60);
    // They fell: through the deck, out of the ship, onto the floor under it, five voxels down.
    let pose = player_pose(&scene, 1);
    assert!(pose.grid.root_level().is_some(), "left the ship");
    let [x, y, z] = in_voxels(pose.position);
    assert!((y - (FLOOR as f64 + 1.62)).abs() < 0.05, "{y}");
    assert!(
        (x - 20.0).abs() < 0.05 && (z - 24.0).abs() < 0.05,
        "{x} {z}"
    );
}

/// What the server has in a voxel of the ship, as a block.
fn block_of_the_ship(scene: &mut Scene, at: voxel_space::VoxelCoordinate) -> u32 {
    let content = voxunami_cave::content::CaveContent::new();
    let world = scene.rig.server.world();
    world
        .grids()
        .get(ship_id(0))
        .expect("the ship exists")
        .voxels
        .get(content.block_channel(), at)
}

#[test]
fn a_walker_aboard_breaks_and_places_blocks_of_the_ship_they_stand_on_aiming_in_its_axes() {
    let content = voxunami_cave::content::CaveContent::new();
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    // The ship turns a good way about the vertical axis; the walker's view is carried with it, so what they aim at is in the ship's
    // axes whatever the world's are.
    let origin = ship_pose(&scene, 0).position;
    let turned = GridPose::new(origin, Orientation::from_yaw_pitch(0.8, 0.0));
    scene.run(move |world| world.set_grid_pose(ship_id(0), turned).unwrap());
    tick_and_follow(&mut scene, &mut player, 3);
    // They look down and a little ahead, at the deck.
    player.camera.look_by(0.0, -1.0);
    player.send_pose();
    tick_and_follow(&mut scene, &mut player, 3);
    let hit = player.target().expect("the deck is under the crosshair");
    assert_eq!(hit.grid, ship_id(0), "the ship, not the terrain below it");
    assert_eq!(hit.voxel.y(), 1, "the top of the deck, which is two thick");
    assert_eq!(
        block_of_the_ship(&mut scene, hit.voxel),
        content.blocks.stone
    );
    // Breaking it: the server removes that voxel of the ship, and no other.
    voxunami_cave::game::break_block(&mut player);
    tick_and_follow(&mut scene, &mut player, 4);
    assert_eq!(block_of_the_ship(&mut scene, hit.voxel), content.blocks.air);
    let beside = voxel_space::VoxelCoordinate::new(hit.voxel.x() + 1, hit.voxel.y(), hit.voxel.z());
    assert_eq!(block_of_the_ship(&mut scene, beside), content.blocks.stone);
    // Placing against the face they now see puts a block in the hole, in the ship's own coordinates.
    let hit = player.target().expect("the bottom of the hole");
    assert_eq!(hit.grid, ship_id(0));
    voxunami_cave::game::place_block(&mut player, "base_blocks:stone");
    tick_and_follow(&mut scene, &mut player, 4);
    assert_eq!(
        block_of_the_ship(&mut scene, hit.previous),
        content.blocks.stone
    );
}

#[test]
fn landing_on_a_ship_that_moves_fast_the_walker_slides_back_on_it_by_the_speed_they_did_not_have() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    // A long deck, so that wherever they land they land on it, held in the air.
    common::make_ship(
        &mut scene,
        pose_at(0.0, FLOOR + 3.0, 20.0),
        true,
        [120, 2, 8],
        |_| true,
    );
    let mut player = scene
        .rig
        .join_walking([30.0, FLOOR as f64 + 9.0, 24.0], 0.0, 0.0);
    // The deck goes east at 20 a second, a metre a tick, from the start. The walker falls from above with no speed along x, so on
    // landing they are going 20 west of it: the ground's friction (80 a second a second) stops that in 20^2 / 160 = 2.5 voxels.
    let moving = |world: &mut server::ServerWorld, tick: usize| {
        world
            .set_grid_pose(ship_id(0), pose_at(1.0 * tick as f32, FLOOR + 3.0, 20.0))
            .unwrap();
        world
            .set_grid_velocity(ship_id(0), [20.0, 0.0, 0.0], [0.0; 3])
            .unwrap();
    };
    let mut landed = None;
    for tick in 0..60 {
        each_tick(&mut scene, &mut player, 1, move |world, _| {
            moving(world, tick)
        });
        let pose = player_pose(&scene, 1);
        if landed.is_none() && pose.grid == ship_id(0) {
            landed = Some(in_voxels(pose.position));
        }
    }
    let landed = landed.expect("they landed on the deck");
    let rested = in_voxels(player_pose(&scene, 1).position);
    let slid = landed[0] - rested[0];
    assert!(
        (slid - 2.5).abs() < 0.3,
        "slid {slid} against the deck; landed {landed:?}, rested {rested:?}"
    );
    // And then they stay: no speed relative to the ship is left.
    let before = eyes_in_their_frame(&scene, 1);
    each_tick(&mut scene, &mut player, 10, move |world, tick| {
        moving(world, 60 + tick)
    });
    let after = eyes_in_their_frame(&scene, 1);
    for (now, then) in after.iter().zip(before) {
        assert!((now - then).abs() < 1e-3);
    }
}

#[test]
fn landing_on_a_ship_that_is_turned_and_moving_the_walker_slides_back_along_the_ship_not_across_it()
{
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    // A long deck turned a quarter about the vertical axis, so that its length (its x) is the world's -z and its width (its z) the world's
    // x: from world x 26 to 34 and, at the start, z 4 to 124. Held in the air; it goes along its length, along the world's -z.
    let pose_at_tick = |tick: usize| {
        GridPose::new(
            WorldPosition::new(
                BrickCoordinate::ORIGIN,
                [26.0, FLOOR + 3.0, 124.0 - tick as f32],
            ),
            Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
        )
    };
    make_ship(&mut scene, pose_at_tick(0), true, [120, 2, 8], |_| true);
    let mut player = scene
        .rig
        .join_walking([30.0, FLOOR as f64 + 9.0, 24.0], 0.0, 0.0);
    // A voxel a tick, 20 a second. The walker falls from above with no speed along the world's z, so on landing they are going 20 the
    // other way relative to the deck, along its x: the ground's friction (80 a second a second) stops that in 20^2 / 160 = 2.5 voxels.
    // Taken as a speed in the deck's axes without being turned, it would be 20 along the deck's z, across it.
    let moving = move |world: &mut server::ServerWorld, tick: usize| {
        world.set_grid_pose(ship_id(0), pose_at_tick(tick)).unwrap();
        world
            .set_grid_velocity(ship_id(0), [0.0, 0.0, -20.0], [0.0; 3])
            .unwrap();
    };
    let mut landed = None;
    for tick in 0..60 {
        each_tick(&mut scene, &mut player, 1, move |world, _| {
            moving(world, tick)
        });
        let pose = player_pose(&scene, 1);
        if landed.is_none() && pose.grid == ship_id(0) {
            landed = Some(in_voxels(pose.position));
        }
    }
    let landed = landed.expect("they landed on the deck");
    let rested = in_voxels(player_pose(&scene, 1).position);
    let (along, across) = (landed[0] - rested[0], landed[2] - rested[2]);
    assert!(
        (along - 2.5).abs() < 0.3 && across.abs() < 0.1,
        "slid {along} along the deck and {across} across it; landed {landed:?}, rested {rested:?}"
    );
}

#[test]
fn leaving_a_ship_that_is_turned_and_moving_gives_the_speed_in_the_worlds_axes() {
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    // The ship is turned a quarter about the vertical axis, put so that the walker is where they were in the world, and then goes
    // north (world -z) at 6 a second while the walker walks along the ship's -z, which is the world's -x, off its edge.
    let origin = WorldPosition::new(
        BrickCoordinate::ORIGIN,
        [20.0 + 4.0, FLOOR + 3.0, 24.0 + 15.0],
    );
    let moving = move |world: &mut server::ServerWorld, tick: usize| {
        let pose = GridPose::new(
            origin.translated([0.0, 0.0, -0.3 * tick as f32]),
            Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
        );
        world.set_grid_pose(ship_id(0), pose).unwrap();
        world
            .set_grid_velocity(ship_id(0), [0.0, 0.0, -6.0], [0.0; 3])
            .unwrap();
    };
    player.hold(&[KeyCode::KeyW]);
    let mut left_at = None;
    for tick in 0..80 {
        each_tick(&mut scene, &mut player, 1, move |world, _| {
            moving(world, tick)
        });
        if player_pose(&scene, 1).grid.root_level().is_some() {
            left_at = Some(tick);
            break;
        }
    }
    let left_at = left_at.expect("they walked off the end of the deck and left the ship");
    let here = eyes_in_the_world(&scene, 1);
    each_tick(&mut scene, &mut player, 1, move |world, _| {
        moving(world, left_at + 1)
    });
    let next = eyes_in_the_world(&scene, 1);
    // In the first tick in the world: along -z with the ship (6 a second, 0.3 a tick) and along -x with their own walking, which the
    // ship's turn put on the world's -x (at four and a bit, 0.2 a tick): not along +z or +x, which is where a speed taken in the ship's
    // axes without turning it would put them.
    let went = [next[0] - here[0], next[2] - here[2]];
    assert!((-0.26..-0.12).contains(&went[0]), "went {went:?}");
    assert!((-0.34..-0.22).contains(&went[1]), "went {went:?}");
}

#[test]
fn boarding_and_leaving_do_not_turn_the_view() {
    let mut scene = scene_with_a_deck();
    // The ship is turned a good way (a radian) before anyone is on it, so that boarding is not a matter of the ship being unturned. Its origin
    // is where the point of the deck 10 along x and 4 along z from it comes to (20, 24), under the place the player falls to: that is
    // (20 - (10 cos 1 + 4 sin 1), 24 + (10 sin 1 - 4 cos 1)).
    let turned = GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [11.23, FLOOR + 3.0, 30.25]),
        Orientation::from_yaw_pitch(1.0, 0.0),
    );
    scene.run(move |world| world.set_grid_pose(ship_id(0), turned).unwrap());
    let mut player = scene
        .rig
        .join_walking([50.0, FLOOR as f64 + 3.0, 50.0], 30.0, -10.0);
    scene.rig.settle(&mut [&mut player]);
    assert!(
        player_pose(&scene, 1).grid.root_level().is_some(),
        "they start in the level, away from the ship"
    );
    // Sent up over the deck (settling takes longer than a fall from there), they fall onto it.
    player.send_command(&format!("/tp 20 {} 24", FLOOR as i64 + 13));
    let mut views = Vec::new();
    let mut frames = Vec::new();
    for _ in 0..80 {
        tick_and_follow(&mut scene, &mut player, 1);
        views.push(player.camera.forward().to_array());
        frames.push(player_pose(&scene, 1).grid);
    }
    assert_eq!(*frames.last().unwrap(), ship_id(0), "aboard at the end");
    assert!(
        frames.iter().any(|grid| grid.root_level().is_some()),
        "and they started in the level"
    );
    // They walk off the edge: leaving, too.
    player.hold(&[KeyCode::KeyA]);
    for _ in 0..80 {
        tick_and_follow(&mut scene, &mut player, 1);
        views.push(player.camera.forward().to_array());
        frames.push(player_pose(&scene, 1).grid);
    }
    assert!(
        frames.last().unwrap().root_level().is_some(),
        "and out of it at the end"
    );
    // The way they face in the world never changes: the ship is still, they only moved.
    for view in &views {
        for axis in 0..3 {
            assert!(
                (view[axis] - views[0][axis]).abs() < 1e-4,
                "{view:?} against {:?}",
                views[0]
            );
        }
    }
}

/// How high a jump takes the walker's eyes: the most they rise above where they stood, tapping the jump key once.
fn height_of_a_jump(scene: &mut Scene, player: &mut Player) -> f64 {
    let standing = eyes_in_their_frame(scene, 1)[1];
    player.hold(&[KeyCode::Space]);
    player.hold(&[]);
    let mut highest = standing;
    for _ in 0..60 {
        tick_and_follow(scene, player, 1);
        highest = highest.max(eyes_in_their_frame(scene, 1)[1]);
    }
    highest - standing
}

#[test]
fn the_walkers_feel_the_gravity_of_the_world_in_a_level_and_aboard_a_ship() {
    // A moon: a quarter of the gravity, so a jump of 9 a second goes four times as high as the 1.27 voxels of the usual (9^2 / 16 = 5.06).
    let moon = voxel_space::GravitySettings {
        base: [0.0, -8.0, 0.0],
        ..voxel_space::GravitySettings::default()
    };
    let mut scene = scene_with_a_deck();
    scene.run(move |world| world.set_gravity_settings(moon));
    let mut player = walker_on_the_deck(&mut scene);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    let aboard = height_of_a_jump(&mut scene, &mut player);
    assert!((aboard - 5.06).abs() < 0.2, "aboard: {aboard}");
    assert_eq!(
        player_pose(&scene, 1).grid,
        ship_id(0),
        "and still aboard at the top"
    );
    // In the level, on the floor.
    player.send_command("/tp 50 230 50");
    tick_and_follow(&mut scene, &mut player, 150);
    assert!(player_pose(&scene, 1).grid.root_level().is_some());
    let in_the_level = height_of_a_jump(&mut scene, &mut player);
    assert!(
        (in_the_level - 5.06).abs() < 0.2,
        "in the level: {in_the_level}"
    );
}

#[test]
fn a_ship_in_the_next_brick_is_in_the_walkers_world_so_the_walker_is_stopped_by_its_face() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    // A deck one thick with its face on the boundary between two bricks, at x = 32, standing on the floor and a step high; the walker is
    // on the floor in the brick before it, facing east, and walks at it: the deck is too high to step onto.
    make_ship(
        &mut scene,
        pose_at(32.0, FLOOR, 20.0),
        true,
        [8, 3, 8],
        |_| true,
    );
    let mut player =
        scene
            .rig
            .join_walking([24.5, f64::from(FLOOR) + 1.62 + 0.3, 24.0], -90.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 40);
    player.hold(&[KeyCode::KeyW]);
    tick_and_follow(&mut scene, &mut player, 60);
    let at = in_voxels(player_pose(&scene, 1).position);
    // Stopped against the face with their body, 0.3 and the controller's gap short of x = 32.
    assert!((at[0] - 31.69).abs() < 0.03, "{at:?}");
    assert!(
        player_pose(&scene, 1).grid.root_level().is_some(),
        "not aboard: on the floor"
    );
}

// A ship that moves into a walker. The walker used to be pressed into the floor by a ship that fell on them, and to fall down through the
// terrain and the world (found by measuring: the eyes went from 202.6 to 94.6); a ship that jumped over them left them half in the floor.

#[test]
fn a_ship_that_falls_on_a_walker_lifts_them_onto_itself_and_does_not_press_them_into_the_ground() {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    let mut player = scene
        .rig
        .join_walking([20.0, FLOOR as f64 + 3.0, 32.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 40);
    let standing = eyes_in_the_world(&scene, 1)[1];
    assert!(
        (standing - (f64::from(FLOOR) + 1.62)).abs() < 0.03,
        "{standing}"
    );
    // A free box of 8 by 2 by 8 is made twelve above the floor, over the walker, and falls.
    make_ship(
        &mut scene,
        pose_at(16.0, FLOOR + 12.0, 28.0),
        false,
        [8, 2, 8],
        |_| true,
    );
    let (mut lowest, mut highest) = (f64::INFINITY, f64::NEG_INFINITY);
    for _ in 0..100 {
        tick_and_follow(&mut scene, &mut player, 1);
        let height = eyes_in_the_world(&scene, 1)[1];
        (lowest, highest) = (lowest.min(height), highest.max(height));
    }
    assert!(
        lowest > standing - 0.05,
        "pressed into the ground: the eyes went down to {lowest}"
    );
    // Lifted onto the box where it first touched them (its top then is about 204.7, the eyes 1.62 above it) and carried down with it: not
    // thrown up by the speed the box had when they came to stand on it. (They were thrown ten voxels high.)
    assert!(
        highest < f64::from(FLOOR) + 7.0,
        "thrown up by the box: the eyes went up to {highest}"
    );
    // Up and onto it, which is the shortest way out, and aboard it: they stand on a block of it.
    let eyes = eyes_in_the_world(&scene, 1);
    assert!(
        (eyes[1] - (f64::from(FLOOR) + 2.0 + 1.62)).abs() < 0.1,
        "{eyes:?}"
    );
    assert!((eyes[0] - 20.0).abs() < 0.1 && (eyes[2] - 32.0).abs() < 0.1);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
}

/// A box `thickness` along x (8 along y and z) held on the floor with its west face 12 east of a walker who stands, which goes west by
/// `step` voxels a tick until it is 12 past them. After each tick the walker is out of the box and above the floor. Returns whether they were
/// west of the box, ahead of it, after every tick.
fn sail_into_a_standing_walker(thickness: i64, step: f32) -> bool {
    let mut scene = Scene::new();
    flat_floor(&mut scene);
    make_ship(
        &mut scene,
        pose_at(32.0, FLOOR, 28.0),
        true,
        [thickness, 8, 8],
        |_| true,
    );
    let mut player = scene
        .rig
        .join_walking([20.0, FLOOR as f64 + 3.0, 32.0], 0.0, 0.0);
    scene.rig.settle(&mut [&mut player]);
    tick_and_follow(&mut scene, &mut player, 40);
    let ticks = ((12.0 + thickness as f32 + 12.0) / step).ceil() as usize;
    let mut ahead = true;
    for tick in 0..ticks {
        each_tick(&mut scene, &mut player, 1, move |world, _| {
            let x = 32.0 - step * (tick as f32 + 1.0);
            world
                .set_grid_pose(ship_id(0), pose_at(x, FLOOR, 28.0))
                .unwrap();
            world
                .set_grid_velocity(ship_id(0), [-step * 20.0, 0.0, 0.0], [0.0; 3])
                .unwrap();
        });
        let face = f64::from(32.0 - step * (tick as f32 + 1.0));
        let eyes = eyes_in_the_world(&scene, 1);
        let wall = thickness as f64;
        assert!(
            eyes[0] <= face - 0.27 || eyes[0] >= face + wall + 0.27,
            "thickness {thickness}, {step} a tick, tick {tick}: the walker is at {}, the box from {face} to {}",
            eyes[0],
            face + wall
        );
        assert!(
            eyes[1] > f64::from(FLOOR) + 1.62 - 0.05,
            "thickness {thickness}, {step} a tick, tick {tick}: pressed into the ground: the eyes are at {}",
            eyes[1]
        );
        ahead &= eyes[0] <= face - 0.27;
    }
    ahead
}

#[test]
fn a_ship_that_sails_into_a_walker_pushes_them_ahead_of_it_and_a_ship_that_jumps_over_them_leaves_them_in_the_open()
 {
    // Four a second, which is two tenths a tick; and 24 a second, 1.2 a tick; and 60 a second: three a tick, a third of the box and less
    // than the half of it.
    for step in [0.2f32, 1.2, 3.0] {
        assert!(
            sail_into_a_standing_walker(8, step),
            "{step} a tick: not ahead of the box all the way"
        );
    }
    // 6 a tick (120 a second) carries the box past the middle of the walker in a tick, and the nearest way out is the far side. And a wall one
    // thick that goes over the walker in a tick or less.
    sail_into_a_standing_walker(8, 6.0);
    for step in [0.2f32, 1.2, 3.0, 6.0] {
        sail_into_a_standing_walker(1, step);
    }
}

#[test]
fn a_block_built_where_a_walker_aboard_stands_moves_them_out_the_shortest_way_and_they_stay_aboard()
{
    let mut scene = scene_with_a_deck();
    let mut player = walker_on_the_deck(&mut scene);
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
    let before = in_voxels(player_pose(&scene, 1).position);
    // They fell onto the deck at (15, 4) in its coordinates, the corner that four voxels share. A block of stone two by two is built a
    // voxel above the deck, where their head is. The controller alone slides them out of it along the surface it finds, which is down,
    // into the deck, and the deck pushes them back, and they shake by half a voxel at every step (found with the physics alone).
    let content = voxunami_cave::content::CaveContent::new();
    scene.run(move |world| {
        let mut deck = world.grid(ship_id(0)).unwrap();
        for (x, z) in [(14, 3), (15, 3), (14, 4), (15, 4)] {
            deck.set(
                content.block_channel(),
                voxel_space::VoxelCoordinate::new(x, 3, z),
                content.blocks.stone,
            )
            .unwrap();
        }
    });
    let mut lowest = f64::INFINITY;
    for _ in 0..30 {
        tick_and_follow(&mut scene, &mut player, 1);
        lowest = lowest.min(in_voxels(player_pose(&scene, 1).position)[1]);
    }
    assert!(
        lowest > before[1] - 0.05,
        "pressed into the deck: the eyes went down to {lowest}, from {}",
        before[1]
    );
    // Out from under it the nearest way, which is sideways, along x, by a voxel and a quarter (up is two, with the block over the head),
    // standing where they stood, still aboard.
    let after = in_voxels(player_pose(&scene, 1).position);
    assert!(
        (after[0] - 16.375).abs() < 0.01 && (after[2] - before[2]).abs() < 1e-3,
        "{before:?} then {after:?}"
    );
    assert!(
        (after[1] - before[1]).abs() < 0.03,
        "{before:?} then {after:?}"
    );
    assert_eq!(player_pose(&scene, 1).grid, ship_id(0));
}
