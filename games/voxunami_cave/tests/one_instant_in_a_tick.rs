//! **Every ship is at one instant while the systems of a tick that come before the bodies' step run** (the redesign "remove the skew" of `docs/decisions/joints-and-cars.md`, 4c-5-4d).
//!
//! Today the world puts a ship on a rail at its place of the end of the tick at the start of it (`Server::step`, `apply_rails`), and a free ship is at its place of the start until the
//! bodies step it, so **a ship on a rail is a tick of its speed ahead of a free one for exactly the stretch in which pilots and other controllers act** (measured 2026-11-22: 38.07
//! voxels at 761 voxels a second). Two things follow, and they are what these tests say must not be so:
//!
//! 1. What a controller reads of a ship on a rail is a tick ahead of what it reads of a free one (`the_places_a_controller_reads_are_the_places_the_last_tick_ended_at`).
//! 2. A controller that disturbs a ship on a rail (`set_grid_velocity`, `set_grid_pose`, which take the rail off where the ship is) gets the ship stepped a second time from the place
//!    of the end of the tick: **a 76.14-voxel step instead of 38.07, and the ship a tick ahead of its undisturbed twin for good**
//!    (`disturbing_a_ship_on_a_rail_before_the_step_does_not_make_it_go_a_tick_too_far`). The pilot's `seat.rs` does it when it stops a turn.
//!
//! The two tests that say so were `#[ignore]`d until the rails of ships alone with no frame were placed after the step (4c-5-4d, phase 1, built 2026-11-22); each was run with
//! `--ignored` before and failed for its reason (76.14 voxels against 38.07; 38.07 voxels from where the last tick left the ship). They are for a ship alone on a rail with no frame
//! of its own (not turning, nothing near it, no torque waiting): ships with a frame, groups and planets are the later phases, and each gets its cases here when it is built. The
//! control, which held before and holds now, is a force or a torque asked of a ship alone on a rail.

mod common;

use common::docking::{make_going, stones};
use common::rails::put_on_a_rail_near_another_ship;
use common::{Scene, circular_speed, length, minus, pose_at, ship_id};
use server::ServerWorld;
use voxel_space::GridId;

const DISTANCE: f64 = 70_016.0;
const TICKS_PER_SECOND: f64 = 20.0;

/// Something a controller asks of the world about a ship.
type Asked = fn(&mut ServerWorld, GridId);

/// A lone ship in a circular orbit that the world has put on a rail (a second after it was made, with nothing near), and the speed it goes at.
fn a_ship_on_a_rail() -> (Scene, GridId, f64) {
    let (mut scene, _) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    make_going(
        &mut scene,
        pose_at(0.0, DISTANCE, 0.0),
        stones(0, 4),
        [0.0, 0.0, speed],
    );
    let ship = ship_id(1);
    for _ in 0..80 {
        scene.ticks(1);
        if rides(&scene, ship) {
            return (scene, ship, speed);
        }
    }
    panic!("the world did not put the ship on a rail in 80 ticks");
}

fn place(scene: &Scene, ship: GridId) -> [f64; 3] {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .pose
        .position
        .in_voxels()
}

fn apart(first: [f64; 3], second: [f64; 3]) -> f64 {
    length(minus(first, second))
}

fn rides(scene: &Scene, ship: GridId) -> bool {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .rail
        .is_some()
}

/// Where the ship was at the end of the tick before, where a system that runs before the bodies' step reads it (`action` runs there, after the read), and where it is at the end of
/// the tick.
struct Places {
    before: [f64; 3],
    read_in_the_tick: [f64; 3],
    after: [f64; 3],
}

fn one_tick(
    scene: &mut Scene,
    ship: GridId,
    action: impl FnOnce(&mut ServerWorld, GridId) + Send + 'static,
) -> Places {
    let before = place(scene, ship);
    let read: std::sync::Arc<std::sync::Mutex<[f64; 3]>> = Default::default();
    let sink = std::sync::Arc::clone(&read);
    scene.run(move |world| {
        *sink.lock().unwrap() = world
            .data()
            .grids()
            .get(ship)
            .unwrap()
            .pose
            .position
            .in_voxels();
        action(world, ship);
    });
    let read_in_the_tick = *read.lock().unwrap();
    Places {
        before,
        read_in_the_tick,
        after: place(scene, ship),
    }
}

/// What a controller does that the world answers by taking a ship off its rail, each given the value the controller has just read: the ship should go on as it was.
fn disturbances() -> Vec<(&'static str, Asked)> {
    vec![
        ("set_grid_velocity to what it has", |world, ship| {
            let (velocity, spin) = {
                let grid = world.data().grids().get(ship).unwrap();
                (grid.linear_velocity, grid.angular_velocity)
            };
            world.set_grid_velocity(ship, velocity, spin).unwrap();
        }),
        ("set_grid_pose to what it has", |world, ship| {
            let pose = world.data().grids().get(ship).unwrap().pose;
            world.set_grid_pose(ship, pose).unwrap();
        }),
    ]
}

#[test]
fn the_places_a_controller_reads_are_the_places_the_last_tick_ended_at() {
    let (mut scene, ship, speed) = a_ship_on_a_rail();
    for tick in 0..4 {
        let places = one_tick(&mut scene, ship, |_, _| {});
        assert!(
            apart(places.before, places.read_in_the_tick) < 1.0e-6,
            "tick {tick}: a system before the step read the ship {} voxels from where the last tick left it (a tick of its speed is {})",
            apart(places.before, places.read_in_the_tick),
            speed / TICKS_PER_SECOND
        );
        assert!(
            (apart(places.before, places.after) - speed / TICKS_PER_SECOND).abs() < 0.05,
            "tick {tick}: the ship went {} voxels in the tick",
            apart(places.before, places.after)
        );
    }
}

#[test]
fn disturbing_a_ship_on_a_rail_before_the_step_does_not_make_it_go_a_tick_too_far() {
    // (Every case is run before any is judged, so that one that fails does not hide another.)
    let mut problems = Vec::new();
    for (name, disturb) in disturbances() {
        let (mut undisturbed, ship, speed) = a_ship_on_a_rail();
        let (mut disturbed, same_ship, _) = a_ship_on_a_rail();
        assert_eq!(place(&undisturbed, ship), place(&disturbed, same_ship));
        let twin = one_tick(&mut undisturbed, ship, |_, _| {});
        let hit = one_tick(&mut disturbed, same_ship, disturb);
        let (step_of_the_twin, step) =
            (apart(twin.before, twin.after), apart(hit.before, hit.after));
        assert!(
            (step_of_the_twin - speed / TICKS_PER_SECOND).abs() < 0.05,
            "{name}: the twin went {step_of_the_twin} voxels in the tick"
        );
        // (Measured 2026-11-22: 76.14 against 38.07. The tolerance is the rail's against the solver's, 0.02 voxels a tick, with room.)
        if (step - step_of_the_twin).abs() >= 0.1 {
            problems.push(format!(
                "{name}: the disturbed ship went {step} voxels in the tick, its twin {step_of_the_twin}"
            ));
        }
        if rides(&disturbed, same_ship) {
            problems.push(format!("{name}: the ship was not taken off its rail"));
        }
        // (And it does not stay a tick ahead of the twin for good.)
        undisturbed.ticks(5);
        disturbed.ticks(5);
        let ahead = apart(place(&undisturbed, ship), place(&disturbed, same_ship));
        if ahead >= 0.5 {
            problems.push(format!(
                "{name}: five ticks later the disturbed ship is {ahead} voxels from its twin"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "{}",
        problems.join(
            "
"
        )
    );
}

/// What holds today and must go on holding: a force or a torque asked of a ship alone on a rail leaves it on the rail (4c-5-4b), and it goes as its twin does.
#[test]
fn a_force_or_a_torque_asked_of_a_ship_alone_on_a_rail_leaves_it_on_its_rail_and_going_as_its_twin()
{
    let asked: [(&str, Asked); 2] = [
        ("apply_grid_force of nothing", |world, ship| {
            world.apply_grid_force(ship, [0.0; 3], None).unwrap();
        }),
        ("apply_grid_torque of nothing", |world, ship| {
            world.apply_grid_torque(ship, [0.0; 3]).unwrap();
        }),
    ];
    for (name, ask) in asked {
        let (mut undisturbed, ship, _) = a_ship_on_a_rail();
        let (mut asked_of, same_ship, _) = a_ship_on_a_rail();
        let twin = one_tick(&mut undisturbed, ship, |_, _| {});
        let hit = one_tick(&mut asked_of, same_ship, ask);
        assert!(
            rides(&asked_of, same_ship),
            "{name}: the ship was taken off its rail"
        );
        assert!(
            apart(twin.after, hit.after) < 1.0e-6,
            "{name}: the ship is {} voxels from its twin after the tick",
            apart(twin.after, hit.after)
        );
    }
}

/// What a system that runs after the bodies (the watchers of a scene run in the settling phase) reads of a ship, each tick: it must be where the tick ended.
fn watch_the_place_of(
    scene: &mut Scene,
    ship: GridId,
) -> std::sync::Arc<std::sync::Mutex<Vec<[f64; 3]>>> {
    let seen: std::sync::Arc<std::sync::Mutex<Vec<[f64; 3]>>> = Default::default();
    let sink = std::sync::Arc::clone(&seen);
    scene.watch(move |world| {
        sink.lock().unwrap().push(
            world
                .data()
                .grids()
                .get(ship)
                .unwrap()
                .pose
                .position
                .in_voxels(),
        );
    });
    seen
}

#[test]
fn after_the_bodies_have_stepped_a_ship_on_a_rail_is_where_the_tick_ends() {
    let (mut scene, ship, _) = a_ship_on_a_rail();
    let seen = watch_the_place_of(&mut scene, ship);
    for tick in 0..4 {
        scene.ticks(1);
        let at_the_end = place(&scene, ship);
        let after_the_bodies = *seen.lock().unwrap().last().unwrap();
        assert_eq!(
            after_the_bodies,
            at_the_end,
            "tick {tick}: a system after the bodies read the ship {} voxels from where the tick ended",
            apart(after_the_bodies, at_the_end)
        );
    }
}

#[test]
fn a_ship_that_is_given_a_rail_in_a_tick_is_put_by_it_after_the_step_and_goes_as_its_twin() {
    let mut ends = Vec::new();
    for rail in [true, false] {
        let (mut scene, _) = Scene::reporting_in_a_new_world();
        let speed = circular_speed(&scene, 0, DISTANCE);
        make_going(
            &mut scene,
            pose_at(0.0, DISTANCE, 0.0),
            stones(0, 4),
            [0.0, 0.0, speed],
        );
        let ship = ship_id(1);
        scene.ticks(3);
        let seen = watch_the_place_of(&mut scene, ship);
        if rail {
            scene.run(put_on_a_rail_near_another_ship_of(ship));
        } else {
            scene.run(|_| {});
        }
        let at_the_end = place(&scene, ship);
        assert_eq!(rides(&scene, ship), rail);
        assert_eq!(
            *seen.lock().unwrap().last().unwrap(),
            at_the_end,
            "rail {rail}: a system after the bodies read the ship away from where the tick ended"
        );
        ends.push(at_the_end);
    }
    assert!(
        apart(ends[0], ends[1]) < 0.1,
        "the ship given a rail is {} voxels from the one left free",
        apart(ends[0], ends[1])
    );
}

fn put_on_a_rail_near_another_ship_of(
    ship: GridId,
) -> impl FnOnce(&mut ServerWorld) + Send + 'static {
    move |world| put_on_a_rail_near_another_ship(world, ship)
}
