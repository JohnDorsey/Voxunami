//! A single ship that rides a rail **stays on it while a stranger passes near, has a frame of its own only while one is near, and is given the exact kick of a hit when one touches
//! it** (step 4c-5-4c of `docs/decisions/joints-and-cars.md`; the owner's answers of 2026-11-09: "two railed ships that pass close both stay on their rails; their collision is worked
//! out in a temporary shared frame in which the rails are the authority, and only a real hit changes either", and "a collision with the outside of a ship gives its rail a kick").
//!
//! This is `group_on_a_rail_is_hit.rs` for a ship that is alone. A ship that does not turn has no body of its own on a rail (it is a fixed one, which a stranger meets as a wall that
//! cannot move), so a hit on it would have given it nothing; until now anything near it took it off its rail. Now the bodies make it a frame **when a stranger is near** (the old
//! rule's room: what would have taken it off the rail) and drop it when the stranger has gone; the frame holds a free body at the rail's point, which is a side of the contact ghost.

mod common;

use common::chains::{
    a_wall, car, centre_of, mass_of, most_apart, orientation_of, origins_of, spin_of,
};
use common::docking::{make, make_all_going, make_going};
use common::rails::put_on_its_rail_in;
use common::{Scene, circular_speed, make_ship, pose_at, ship_id};
use voxel_space::{GridId, GridPose, Orientation, WorldPosition};
use world_storage::rail::Rail;

const DISTANCE: f64 = 70_016.0;

/// How far from the east end of the ship along x a stranger begins, in voxels.
const FIRST_OFFSET: f64 = 150.0;

/// The ship's edge, in voxels (a cube of stone).
const EDGE: i128 = 4;

fn rail_of(scene: &Scene, ship: GridId) -> Option<Rail> {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .rail
        .clone()
}

/// A ship (number 1) of stone in orbit at `DISTANCE`, going along z at the speed of a circle, on its rail, and what it needs to be watched.
fn a_ship_on_its_rail() -> (
    Scene,
    impl Fn() -> grid_bodies::BodiesReport,
    GridId,
    f64,
    Rail,
) {
    let (scene, report, ship, speed) = a_ship_in_orbit(true);
    let rail = rail_of(&scene, ship).expect("it rides a rail");
    (scene, report, ship, speed, rail)
}

/// The ship of [`a_ship_on_its_rail`], put on its rail or not (the same orbit either way: a ship that is not on a rail is moved by the bodies).
fn a_ship_in_orbit(
    on_its_rail: bool,
) -> (Scene, impl Fn() -> grid_bodies::BodiesReport, GridId, f64) {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    // (It goes along z, across the way a wall closes on it along x: along the way it goes the orbit's curve would turn its speed into the speed that is measured.)
    make_ship(
        &mut scene,
        GridPose::new(
            WorldPosition::from_voxels([
                -(EDGE as f64) / 2.0,
                DISTANCE - EDGE as f64 / 2.0,
                -(EDGE as f64) / 2.0,
            ]),
            Orientation::IDENTITY,
        ),
        false,
        [EDGE; 3],
        |_| true,
    );
    let ship = ship_id(1);
    scene.run(move |world| {
        world
            .set_grid_velocity(ship, [0.0, 0.0, speed], [0.0; 3])
            .unwrap();
        if on_its_rail {
            put_on_its_rail_in(world, ship);
        }
    });
    scene.ticks(3);
    (scene, report, ship, speed)
}

/// A wall (ship number 2) that begins `FIRST_OFFSET` voxels past the east end of the ship, `lateral` voxels off its axis along y, going at the ship's speed and `closing` metres a
/// second toward it along x.
fn send_a_wall(scene: &mut Scene, ship: GridId, speed: f64, closing: f64, lateral: f64) {
    let origin = origins_of(scene, &[ship])[0];
    make_all_going(
        scene,
        vec![(
            pose_at(
                origin[0] + EDGE as f64 + FIRST_OFFSET,
                origin[1] + EDGE as f64 / 2.0 + lateral,
                origin[2] + EDGE as f64 / 2.0,
            ),
            a_wall(),
        )],
        [-closing, 0.0, speed],
    );
}

fn turn_of(scene: &Scene, ship: GridId) -> (Orientation, [f64; 3]) {
    (orientation_of(scene, ship), spin_of(scene, ship))
}

#[test]
fn a_stranger_that_passes_a_ship_on_a_rail_without_touching_leaves_it_on_the_rail_and_changes_nothing()
 {
    // The wall goes by 24 voxels off the ship's axis (its edge is 18 off), inside the room of the ghost and of the old rule, closing at 50 metres a second.
    let ticks = 200;
    let (mut alone, _, ship, _, _) = a_ship_on_its_rail();
    // (Making the wall takes a tick, so the ship alone has one too: the two are compared tick for tick.)
    alone.ticks(1);
    let mut baseline = Vec::new();
    let mut baseline_places = Vec::new();
    for _ in 0..ticks {
        alone.ticks(1);
        baseline.push(turn_of(&alone, ship));
        baseline_places.push(origins_of(&alone, &[ship])[0]);
    }
    let (mut scene, report, ship, speed, rail) = a_ship_on_its_rail();
    let frames_alone = report().frames;
    send_a_wall(&mut scene, ship, speed, 50.0, 24.0);
    let mut watched = Vec::new();
    let mut places = Vec::new();
    let mut nearest = f64::INFINITY;
    let mut most_frames = 0;
    for tick in 0..ticks {
        scene.ticks(1);
        assert_eq!(
            rail_of(&scene, ship),
            Some(rail.clone()),
            "tick {tick}: the ship is on its rail, the wall going by"
        );
        watched.push(turn_of(&scene, ship));
        places.push(origins_of(&scene, &[ship])[0]);
        most_frames = most_frames.max(report().frames);
        let wall = origins_of(&scene, &[ship_id(2)])[0];
        let end = origins_of(&scene, &[ship])[0];
        nearest = nearest.min(common::length([0, 1, 2].map(|axis| wall[axis] - end[axis])));
    }
    // (The wall did go by near it, inside what the old rule called near, and the ghost had it.)
    assert!(
        nearest < 40.0,
        "the wall came to within {nearest} voxels of the ship's origin"
    );
    assert!(
        report().ghosts_made > 0,
        "the wall was near enough to be in a ghost"
    );
    // The ship had a frame of its own while the wall was near, and has none again when it has gone: a frame is made on demand.
    assert!(
        most_frames > frames_alone,
        "the ship had {most_frames} frames at the most and {frames_alone} alone: no frame was made for it"
    );
    assert_eq!(
        report().frames,
        frames_alone,
        "the wall is far off again, and the ship's frame is no more"
    );
    let apart = most_apart(&watched, &baseline);
    assert!(
        apart < 1e-7,
        "with the wall going by the ship's turn differs from the ship alone by {apart}"
    );
    let moved = places
        .iter()
        .zip(&baseline_places)
        .map(|(found, expected)| common::length(common::minus(*found, *expected)))
        .fold(0.0f64, f64::max);
    assert!(
        moved < 1e-7,
        "with the wall going by the ship's place differs from the ship alone by {moved} voxels"
    );
}

#[test]
fn a_ship_on_a_rail_with_nothing_near_has_no_frame_of_its_own() {
    let (mut scene, report, ship, speed, rail) = a_ship_on_its_rail();
    let frames_alone = report().frames;
    // A wall 5,000 voxels along its way, at its own speed: far outside any room.
    let origin = origins_of(&scene, &[ship])[0];
    make_all_going(
        &mut scene,
        vec![(pose_at(origin[0] + 5_000.0, origin[1], origin[2]), a_wall())],
        [0.0, 0.0, speed],
    );
    for _ in 0..60 {
        scene.ticks(1);
        assert_eq!(report().frames, frames_alone, "no frame for a ship alone");
    }
    assert_eq!(rail_of(&scene, ship), Some(rail), "and it is on its rail");
    assert_eq!(
        report().ghosts_made,
        0,
        "nothing was near enough for a ghost"
    );
}

/// What a wall that hits a ship on its rail did.
struct Hit {
    /// The tick (from the first after the wall was sent) in which the rail came off, if it did.
    rail_off_at: Option<usize>,
    /// How far the wall's origin was from the east end of the ship, along x, in that tick.
    gap_then: f64,
    /// The momentum, along x, the ship and the wall gained, together, and the ship alone.
    momentum_gained: f64,
    momentum_of_the_ship: f64,
    /// How the ship's speed went along x before and after.
    ship_change: f64,
    still_on_its_rail_before: bool,
    kicks: u64,
}

fn a_wall_that_hits_a_ship_on_its_rail(closing: f64, ticks: usize) -> Hit {
    let (mut scene, report, ship, speed, rail) = a_ship_on_its_rail();
    send_a_wall(&mut scene, ship, speed, closing, 0.0);
    scene.ticks(2);
    let wall = ship_id(2);
    let before = (centre_of(&scene, &[ship]).1, centre_of(&scene, &[wall]).1);
    let (ship_mass, wall_mass) = (mass_of(&scene, &[ship]), mass_of(&scene, &[wall]));
    let (mut rail_off_at, mut gap_then) = (None, f64::NAN);
    let mut still_on_its_rail_before = true;
    for tick in 0..ticks {
        scene.ticks(1);
        let rail_now = rail_of(&scene, ship);
        if rail_off_at.is_none() {
            if rail_now.is_none() {
                rail_off_at = Some(tick);
                let end = origins_of(&scene, &[ship])[0][0] + EDGE as f64;
                gap_then = origins_of(&scene, &[wall])[0][0] - end;
            } else {
                still_on_its_rail_before &= rail_now == Some(rail.clone());
            }
        }
    }
    let after = (centre_of(&scene, &[ship]).1, centre_of(&scene, &[wall]).1);
    let (ship_change, wall_change) = (after.0[0] - before.0[0], after.1[0] - before.1[0]);
    Hit {
        rail_off_at,
        gap_then,
        momentum_gained: ship_mass * ship_change + wall_mass * wall_change,
        momentum_of_the_ship: ship_mass * ship_change,
        ship_change,
        still_on_its_rail_before,
        kicks: report().rail_kicks,
    }
}

#[test]
fn a_wall_that_hits_a_ship_on_its_rail_takes_it_off_the_rail_at_the_touch_with_the_momentum_of_the_hit()
 {
    for (closing, ticks) in [(50.0, 100), (150.0, 50)] {
        let hit = a_wall_that_hits_a_ship_on_its_rail(closing, ticks);
        let off = hit
            .rail_off_at
            .unwrap_or_else(|| panic!("closing at {closing}: the rail never came off"));
        assert!(
            hit.still_on_its_rail_before,
            "closing at {closing}: before the touch the ship was on the rail it had"
        );
        // The rail came off at the touch and not before it (the old rule took it off 64 voxels and a second of the closing speed before): the wall was at the ship's end.
        assert!(
            hit.gap_then.abs() < 3.0,
            "closing at {closing}: the rail came off in tick {off} with the wall {} voxels from the ship's end",
            hit.gap_then
        );
        // The ship went the way the wall went, and the two kept the momentum: what the ship gained is what the wall lost, to a hundredth.
        assert!(
            hit.ship_change < -0.1 * closing,
            "closing at {closing}: the ship's speed along x changed by {}",
            hit.ship_change
        );
        assert!(
            hit.momentum_gained.abs() < 0.01 * hit.momentum_of_the_ship.abs(),
            "closing at {closing}: together they gained {} of momentum and the ship {}",
            hit.momentum_gained,
            hit.momentum_of_the_ship
        );
        assert_eq!(hit.kicks, 1, "closing at {closing}: one kick");
    }
}

#[test]
fn a_ship_that_was_hit_is_put_on_a_new_rail_when_the_wall_has_gone_and_the_rail_is_the_ships() {
    let (mut scene, report, ship, speed, rail) = a_ship_on_its_rail();
    send_a_wall(&mut scene, ship, speed, 100.0, 0.0);
    let mut off = None;
    for tick in 0..80 {
        scene.ticks(1);
        if rail_of(&scene, ship).is_none() {
            off = Some(tick);
            break;
        }
    }
    assert!(
        off.is_some(),
        "the wall hit the ship and took it off its rail"
    );
    assert_eq!(report().rail_kicks, 1, "one kick");
    // The wall is taken far off (a program that puts a ship somewhere); the ship, left alone for a second, is put on a rail again.
    let wall = ship_id(2);
    scene.run(move |world| {
        let at = world.data().grids().get(wall).unwrap().pose.position;
        world
            .set_grid_pose(
                wall,
                voxel_space::GridPose::new(
                    at.translated([0.0, 0.0, 5_000.0]),
                    Orientation::IDENTITY,
                ),
            )
            .unwrap();
    });
    let mut on_again = None;
    for tick in 0..120 {
        scene.ticks(1);
        if let Some(new) = rail_of(&scene, ship) {
            on_again = Some((tick, new));
            break;
        }
    }
    let (tick, new) = on_again.expect("the ship was put on a rail again");
    assert!(
        tick >= 19,
        "after a second of quiet and not before: tick {tick}"
    );
    assert_ne!(
        new, rail,
        "it is another rail than the one the hit took it from"
    );
    let (place, going) = centre_of(&scene, &[ship]);
    let (on_the_rail, rail_going) = common::chains::rail_point(&scene, &new);
    common::chains::close(place, on_the_rail, 1e-6, "the ship is on its new rail");
    common::chains::close(going, rail_going, 1e-6, "and goes as it does");
}

#[test]
fn a_hit_on_another_ship_far_off_does_not_kick_a_ship_on_its_rail() {
    // Ship B (a wall of its own to be hit by) is 600 voxels along from ship A, which is on its rail: A is near nothing, and what the ghost of B does to B is none of A's.
    let ticks = 120;
    let (mut alone, _, ship, _, _) = a_ship_on_its_rail();
    alone.ticks(2);
    let mut baseline = Vec::new();
    for _ in 0..ticks {
        alone.ticks(1);
        baseline.push(turn_of(&alone, ship));
    }
    let (mut scene, report, ship, speed, rail) = a_ship_on_its_rail();
    let origin = origins_of(&scene, &[ship])[0];
    let blocks = (0..EDGE)
        .flat_map(|x| {
            (0..EDGE).flat_map(move |y| {
                (0..EDGE)
                    .map(move |z| ([x, y, z], common::docking::stone(), common::docking::PLAIN))
            })
        })
        .collect::<Vec<_>>();
    make_all_going(
        &mut scene,
        vec![
            (pose_at(origin[0] + 600.0, origin[1], origin[2]), blocks),
            (
                pose_at(
                    origin[0] + 600.0 + EDGE as f64 + FIRST_OFFSET,
                    origin[1] + 2.0,
                    origin[2] + 2.0,
                ),
                a_wall(),
            ),
        ],
        [0.0, 0.0, speed],
    );
    // (The wall is the second of the two made: number 3; it goes 50 metres a second along x toward ship B.)
    scene.run(|world| {
        let wall = ship_id(3);
        let going = world.data().grids().get(wall).unwrap().linear_velocity;
        world
            .set_grid_velocity(wall, [going[0] - 50.0, going[1], going[2]], [0.0; 3])
            .unwrap();
    });
    let mut watched = Vec::new();
    for tick in 0..ticks {
        scene.ticks(1);
        assert_eq!(
            rail_of(&scene, ship),
            Some(rail.clone()),
            "tick {tick}: ship A is on its rail while ship B is hit"
        );
        watched.push(turn_of(&scene, ship));
    }
    assert_eq!(report().rail_kicks, 0, "and no ship on a rail was kicked");
    let apart = most_apart(&watched, &baseline);
    assert!(apart < 1e-7, "ship A turns as it did alone to {apart}");
}

#[test]
fn a_ship_on_a_rail_whose_joint_end_pulls_on_the_joint_end_of_another_ship_leaves_the_rail_and_is_coupled()
 {
    // Ports do not take a ship alone on a rail off it any more (`ports_on_rails.rs`), but joint ends do: a coupling is made between ships of one frame and a joint is a body's, so the
    // ship goes off its rail for the ends that pull, as it always did (and a group does, `group_on_a_rail_is_hit.rs`).
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let (first, length) = car(false, 3, true);
    let (second, _) = car(true, 3, false);
    make(&mut scene, pose_at(0.0, DISTANCE, 0.0), false, first);
    scene.ticks(2);
    let ship = ship_id(1);
    scene.run(move |world| {
        world
            .set_grid_velocity(ship, [0.0, 0.0, speed], [0.0; 3])
            .unwrap();
        put_on_its_rail_in(world, ship);
    });
    scene.ticks(3);
    assert!(
        rail_of(&scene, ship).is_some(),
        "the first car rides a rail"
    );
    // The second car is made going as the first goes, 3 voxels from the end of it (the range of a pull is 6); the first has gone round its orbit in the ticks that put it on its rail.
    let origin = origins_of(&scene, &[ship])[0];
    make_going(
        &mut scene,
        pose_at(origin[0] + length + 3.0, origin[1], origin[2]),
        second,
        [0.0, 0.0, speed],
    );
    let mut coupled_at = None;
    for tick in 0..200 {
        scene.ticks(1);
        if report().couplings > 0 {
            coupled_at = Some(tick);
            break;
        }
    }
    assert!(coupled_at.is_some(), "the two cars were coupled");
    assert!(
        report().rails_left_for_ends >= 1,
        "the first car was taken off its rail for the joint end that pulled on it"
    );
    assert_eq!(rail_of(&scene, ship), None, "and it is off it");
}

/// What a push (a force or a torque, asked of the world as an engine or a seat asks it) does to a ship in its orbit, on its rail or not: its velocity and its spin after the tick that
/// has the push, and whether it still rides a rail. `ServerWorld::apply_grid_force` and `apply_grid_torque` split the push of a ship that rides a rail alone (4c-5-4b): the force at
/// the centre of mass is the rail's, the torque (and what a force's lever makes) is the bodies', so the ship stays on its rail and does what the one that was never on a rail does.
/// **The speeds are compared after the push, not as a change from before it**: the speed a ship on a rail has stored is a tick behind the one that a ship with a body has
/// (-1.24 and -1.66 metres a second along y, the orbit's curve, in the same tick), so a change from before the push is a tick of the orbit's turn too much for the one that was on a rail
/// (probed: the speeds after the push are the same to 1e-6 along y).
fn push_the_ship_and_see(
    on_its_rail: bool,
    push: impl FnOnce(&mut server::ServerWorld, GridId, f64) + Send + 'static,
) -> ([f64; 3], [f64; 3], bool) {
    let (mut scene, _, ship, _) = a_ship_in_orbit(on_its_rail);
    let mass = mass_of(&scene, &[ship]);
    scene.run(move |world| push(world, ship, mass));
    let after = scene.rig.server.world().grids().get(ship).unwrap();
    (
        after.linear_velocity,
        after.angular_velocity,
        after.rail.is_some(),
    )
}

/// A push of 400 metres a second a second along z at the centre of mass (what an engine does), for the one tick it is asked in: 20 metres a second.
fn force_along_z(world: &mut server::ServerWorld, ship: GridId, mass: f64) {
    world
        .apply_grid_force(ship, [0.0, 0.0, 400.0 * mass], None)
        .unwrap();
}

/// The same push, along z at a point 2 voxels off the centre of mass along x: it turns the ship about y as well as pushing it.
fn force_along_z_off_the_centre(world: &mut server::ServerWorld, ship: GridId, mass: f64) {
    world
        .apply_grid_force(ship, [0.0, 0.0, 400.0 * mass], Some([4.0, 2.0, 2.0]))
        .unwrap();
}

/// A push along no axis of the world, at a point off the centre along no axis of the ship, so that the lever turns the ship about all three axes (the push above turns it about y
/// only, which cannot say whether the other two parts of the lever's cross product are right).
fn force_in_all_directions_off_the_centre(
    world: &mut server::ServerWorld,
    ship: GridId,
    mass: f64,
) {
    world
        .apply_grid_force(
            ship,
            [100.0 * mass, -200.0 * mass, 300.0 * mass],
            Some([4.0, 3.0, 1.0]),
        )
        .unwrap();
}

/// A torque about y that turns the ship and does not push its centre.
fn torque_about_y(world: &mut server::ServerWorld, ship: GridId, mass: f64) {
    world
        .apply_grid_torque(ship, [0.0, 30.0 * mass, 0.0])
        .unwrap();
}

#[test]
fn a_force_on_a_ship_on_its_rail_changes_its_speed_as_it_changes_that_of_a_ship_that_was_never_on_one_and_leaves_it_on_the_rail()
 {
    let nothing = |_: &mut server::ServerWorld, _: GridId, _: f64| {};
    let (rail_alone, _, _) = push_the_ship_and_see(true, nothing);
    let (free_alone, _, _) = push_the_ship_and_see(false, nothing);
    let (rail_pushed, spin_on_a_rail, still_on_a_rail) = push_the_ship_and_see(true, force_along_z);
    let (free_pushed, spin_free, _) = push_the_ship_and_see(false, force_along_z);
    assert!(still_on_a_rail, "the push left the ship on its rail");
    // (**The change, not the speed**: a ship on a rail is a tick of the orbit's turn from one with a body in the speed it has stored, 0.41 along y here, and the push is 400 metres a
    // second a second for the tick of a twentieth of a second, 20 metres a second along z. The free ship's own turn during the tick is a little changed by the push: 1e-2.)
    for axis in 0..3 {
        let on_a_rail = rail_pushed[axis] - rail_alone[axis];
        let free = free_pushed[axis] - free_alone[axis];
        assert!(
            (on_a_rail - free).abs() <= 1e-2,
            "the push changed the speed of the ship on a rail by {on_a_rail} and that of the one that was not by {free} along axis {axis}"
        );
        assert!(
            spin_on_a_rail[axis].abs() < 1e-9 && spin_free[axis].abs() < 1e-9,
            "a push at the centre of mass does not turn the ship: {spin_on_a_rail:?}, {spin_free:?}"
        );
    }
    assert!(
        (rail_pushed[2] - rail_alone[2] - 20.0).abs() < 1e-6,
        "the rail's speed along z is 20 metres a second more: {rail_pushed:?} against {rail_alone:?}"
    );
}

// ---- Step 4c-5-4b (built for a ship alone on its rail): a push does not take a ship off its rail. The force changes the rail; the torque is the bodies'. A port that pulls is a push too: see `ports_on_rails.rs`. ----
//
// These say what the split of `docs/decisions/joints-and-cars.md` (4c-5-4b) does, written before it was built (each was run with `--ignored` and failed for the reason in its message).

/// A tick of the world, in seconds (20 a second).
const TICK: f64 = 0.05;

/// The rail of the ship of [`a_ship_in_orbit`] before it is pushed, after the tick that has the push, and the ship's spin after it.
fn push_the_ship_on_its_rail_and_see_its_rail(
    push: impl FnOnce(&mut server::ServerWorld, GridId, f64) + Send + 'static,
) -> (Rail, Option<Rail>, [f64; 3]) {
    let (mut scene, _, ship, _) = a_ship_in_orbit(true);
    let before = rail_of(&scene, ship).expect("it rides a rail");
    let mass = mass_of(&scene, &[ship]);
    scene.run(move |world| push(world, ship, mass));
    (before, rail_of(&scene, ship), spin_of(&scene, ship))
}

/// What a rail says of its body (its place and its speed from the centre it goes round, which is taken to be at rest at the origin) at `time`.
fn place_and_speed(rail: &Rail, time: world_storage::rail::RailTime) -> ([f64; 3], [f64; 3]) {
    let state = rail.state(time, [0.0; 3], [0.0; 3]);
    (state.position.expect("an orbit"), state.velocity)
}

/// The rail after a force is **the orbit through the same place, at the instant it was made (`since`), with the speed changed by the force over the tick and over the mass**: a kick,
/// then the orbit goes on from there (the stopgap of 4c-5-4b, measured 2026-11-19: with the place and the speed exact it loses nothing).
fn assert_the_rail_was_kicked_by(
    before: &Rail,
    after: &Option<Rail>,
    change_of_speed: [f64; 3],
    what: &str,
) {
    let after = after.as_ref().unwrap_or_else(|| {
        panic!("{what}: the ship is still on a rail after the push, and it was taken off it")
    });
    let since = after
        .orbit
        .as_ref()
        .and_then(|orbit| orbit.since)
        .expect("a rail made from a state says when");
    assert_ne!(
        Some(since),
        before.orbit.as_ref().and_then(|orbit| orbit.since),
        "{what}: the rail is a new one, made at the push"
    );
    let ((place_before, speed_before), (place_after, speed_after)) = (
        place_and_speed(before, since),
        place_and_speed(after, since),
    );
    for axis in 0..3 {
        assert!(
            (place_after[axis] - place_before[axis]).abs() < 1e-9,
            "{what}: the ship is where it was at {since:?}: {place_before:?}, {place_after:?}"
        );
        assert!(
            (speed_after[axis] - speed_before[axis] - change_of_speed[axis]).abs() < 1e-9,
            "{what}: its speed changed by {change_of_speed:?} there: {speed_before:?}, {speed_after:?}"
        );
    }
}

#[test]
fn a_force_on_a_ship_on_its_rail_leaves_it_on_the_rail_and_the_rail_has_the_speed_the_force_gave() {
    let (before, after, spin) = push_the_ship_on_its_rail_and_see_its_rail(force_along_z);
    assert_the_rail_was_kicked_by(&before, &after, [0.0, 0.0, 400.0 * TICK], "a force");
    assert!(
        spin.iter().all(|part| part.abs() < 1e-9),
        "a force at the centre of mass does not turn the ship: {spin:?}"
    );
}

#[test]
fn a_force_off_the_centre_of_a_ship_on_its_rail_kicks_the_rail_the_same_and_turns_the_ship_as_it_turns_one_that_was_never_on_one()
 {
    let (before, after, spin) =
        push_the_ship_on_its_rail_and_see_its_rail(force_along_z_off_the_centre);
    assert_the_rail_was_kicked_by(
        &before,
        &after,
        [0.0, 0.0, 400.0 * TICK],
        "a force off the centre",
    );
    let (_, spin_free, _) = push_the_ship_and_see(false, force_along_z_off_the_centre);
    assert!(
        spin_free[1].abs() > 1e-6,
        "the lever turned the ship that was never on a rail about y: {spin_free:?}"
    );
    for axis in 0..3 {
        assert!(
            (spin[axis] - spin_free[axis]).abs() <= 1e-9 * spin_free[1].abs(),
            "the spin of the ship that stayed on its rail, {spin:?}, is that of the one that was never on one, {spin_free:?}"
        );
    }
}

#[test]
fn a_force_in_all_directions_off_the_centre_of_a_ship_on_its_rail_turns_it_about_every_axis_as_it_turns_one_that_was_never_on_one()
 {
    let (before, after, spin) =
        push_the_ship_on_its_rail_and_see_its_rail(force_in_all_directions_off_the_centre);
    assert_the_rail_was_kicked_by(
        &before,
        &after,
        [100.0 * TICK, -200.0 * TICK, 300.0 * TICK],
        "a force in all directions",
    );
    let (_, spin_free, _) = push_the_ship_and_see(false, force_in_all_directions_off_the_centre);
    assert!(
        spin_free.iter().all(|part| part.abs() > 1e-6),
        "the lever turned the ship that was never on a rail about every axis: {spin_free:?}"
    );
    let largest = spin_free
        .iter()
        .fold(0.0f64, |most, part| most.max(part.abs()));
    for axis in 0..3 {
        assert!(
            (spin[axis] - spin_free[axis]).abs() <= 1e-9 * largest,
            "the spin of the ship that stayed on its rail, {spin:?}, is that of the one that was never on one, {spin_free:?}"
        );
    }
}

#[test]
fn a_torque_on_a_ship_on_its_rail_turns_it_as_it_turns_one_that_was_never_on_one_and_leaves_its_rail_as_it_was()
 {
    let (before, after, spin) = push_the_ship_on_its_rail_and_see_its_rail(torque_about_y);
    let (_, spin_free, _) = push_the_ship_and_see(false, torque_about_y);
    assert_eq!(
        after.as_ref(),
        Some(&before),
        "a torque does not move the centre of mass: the rail is the very rail it was"
    );
    assert!(spin_free[1] > 0.0, "the free ship turned: {spin_free:?}");
    for axis in 0..3 {
        assert!(
            (spin[axis] - spin_free[axis]).abs() <= 1e-9 * spin_free[1],
            "the spin of the ship on its rail, {spin:?}, is that of the one that was never on one, {spin_free:?}"
        );
    }
}

/// Whether a stranger that goes with the ship (the same velocity, 20 voxels off its east face), `offset` voxels along the way the ship goes from the ship's middle, is in a ghost with
/// it within six ticks.
fn is_a_stranger_beside_a_ship_on_its_rail_in_a_ghost(offset: f64) -> bool {
    let (mut scene, report, ship, speed, _) = a_ship_on_its_rail();
    let origin = origins_of(&scene, &[ship])[0];
    make_all_going(
        &mut scene,
        vec![(
            pose_at(
                origin[0] + EDGE as f64 + 20.0,
                origin[1] + EDGE as f64 / 2.0,
                origin[2] + EDGE as f64 / 2.0 + offset,
            ),
            a_wall(),
        )],
        [0.0, 0.0, speed],
    );
    scene.ticks(6);
    report().ghosts_made > 0
}

/// A ship on a rail is put at its place of the end of the tick and a stranger with a body at its place of the start, a tick of the ship's speed (38 voxels at 761 a second) behind; the
/// ghost's search for what is near puts the railed ship back by that tick (`ships_near_mechanism_lagging`). Without it the ghost's room is that far off the ship, ahead of it: this is
/// the mutant (the lag of 0 in `ghost_parts`) that the other tests of this file did not kill, because none of them has a stranger that goes with the ship and is near by its side.
/// Probed: the ghost is made for a stranger from 40 voxels behind the ship's middle to 36 ahead, and with the lag of 0 from 0 to 76 (strangers go by every 4 voxels).
#[test]
fn the_room_of_the_ghost_of_a_ship_on_a_rail_is_as_wide_ahead_of_it_as_behind_it() {
    for offset in [-30.0, 30.0] {
        assert!(
            is_a_stranger_beside_a_ship_on_its_rail_in_a_ghost(offset),
            "a stranger {offset} voxels along the ship's way, going with it and 20 voxels off its side, is in the ghost's room"
        );
    }
    for offset in [-56.0, 56.0] {
        assert!(
            !is_a_stranger_beside_a_ship_on_its_rail_in_a_ghost(offset),
            "and one {offset} voxels along is not"
        );
    }
}
