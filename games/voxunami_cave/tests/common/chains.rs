//! Chains of cars with dev ball joints in orbit round the planet of a new world, for the tests of a group of ships riding a rail (step 4c-3 of
//! `docs/decisions/joints-and-cars.md`): the cars, the chain they make end to end, where its centre of mass and its rail are, and how to look at it.

#![allow(dead_code)] // Each test file uses some of it.

use super::docking::{Placed, make_all_going, stones};
use super::joints::{ball_end_toward, joint_block};
use super::{Scene, length, pose_at, ship_id};
use voxel_space::{FaceDirection, GridId, Orientation, WorldPosition};
use world_storage::rail::{Centre, Orbit, Rail, RailTime};

pub type Vec3 = [f64; 3];

pub const TICKS_PER_SECOND: u32 = 20;

/// A car for a chain: `stones_count` blocks of stone in a row along x, with a joint block at the west end if asked (its ball end looks west, its pivot is at x = 0) and one at the
/// east end if asked (its ball end looks east, its pivot is at the length). The blocks, and the length.
pub fn car(west_joint: bool, stones_count: i128, east_joint: bool) -> (Vec<Placed>, f64) {
    let start = i128::from(west_joint);
    let mut blocks: Vec<Placed> = Vec::new();
    if west_joint {
        blocks.push((
            [0, 0, 0],
            joint_block(),
            ball_end_toward(FaceDirection::NegativeX),
        ));
    }
    blocks.extend(stones(start, start + stones_count));
    let end = start + stones_count;
    if east_joint {
        blocks.push((
            [end, 0, 0],
            joint_block(),
            ball_end_toward(FaceDirection::PositiveX),
        ));
    }
    (blocks, (end + i128::from(east_joint)) as f64)
}

/// A chain of three cars, end to end along x, whose pivots are together.
pub fn a_chain() -> Vec<(Vec<Placed>, f64)> {
    vec![car(false, 6, true), car(true, 4, true), car(true, 5, false)]
}

/// Makes the cars of a chain end to end along x, from the origin, `distance` from the middle of the planet along y, going at `speed` along z, in a scene that has the planet (the
/// ship before them), all in one tick. Their numbers are 1, 2 and so on. Two ticks after, the bodies have coupled them.
pub fn a_chain_in_orbit(
    scene: &mut Scene,
    cars: Vec<(Vec<Placed>, f64)>,
    distance: f64,
    speed: f64,
) -> Vec<GridId> {
    let count = cars.len() as u64;
    let mut at = 0.0;
    let mut made = Vec::new();
    for (blocks, length) in cars {
        made.push((pose_at(at, distance, 0.0), blocks));
        at += length;
    }
    make_all_going(scene, made, [0.0, 0.0, speed]);
    scene.ticks(2);
    (1..=count).map(ship_id).collect()
}

/// Where the centre of mass of these ships is and how fast it goes, as the world has it.
pub fn centre_of(scene: &Scene, ships: &[GridId]) -> (Vec3, Vec3) {
    let centre = scene
        .rig
        .server
        .world()
        .grids()
        .group_centre(ships)
        .expect("the ships are in memory");
    (centre.position.in_voxels(), centre.velocity)
}

/// The rail of the centre of mass of these ships, as it is now (see the head of the file).
pub fn rail_of_the_group(world: &server::ServerWorld, planet: GridId, ships: &[GridId]) -> Rail {
    let centre = world.data().grids().group_centre(ships).unwrap();
    let planet_grid = world.data().grids().get(planet).unwrap();
    let place = planet_grid
        .pose
        .position
        .displacement_to(centre.position)
        .in_voxels();
    let parameter = world.gravity_settings().pull_strength * planet_grid.terrain.unwrap().mass;
    Rail {
        spin: None,
        orbit: Some(
            Orbit::from_place(
                Centre::Grid(planet),
                place,
                centre.velocity,
                parameter,
                RailTime::of_tick(world.data().clock(), TICKS_PER_SECOND),
            )
            .expect("the group is in a free orbit"),
        ),
    }
}

/// Sets the ships going in the tick it runs: each at the speed of the orbit and what is added, and turning as said. (Moving or speeding a ship is a disturbance: the group of
/// it is not quiet from this tick, and has no rail.)
pub fn set_going(scene: &mut Scene, ships: &[GridId], speed: f64, motion: Vec<(Vec3, Vec3)>) {
    let ships = ships.to_vec();
    scene.run(move |world| set_going_in(world, &ships, speed, motion));
}

fn set_going_in(
    world: &mut server::ServerWorld,
    ships: &[GridId],
    speed: f64,
    motion: Vec<(Vec3, Vec3)>,
) {
    for (ship, (added, spin)) in ships.iter().zip(motion) {
        world
            .set_grid_velocity(*ship, [added[0], added[1], speed + added[2]], spin)
            .unwrap();
    }
}

/// Sets the ships going (see [`set_going`]), and puts the group on the rail of its centre of mass, all in one tick, by hand: the rail is the last thing done.
pub fn put_on_its_rail(
    scene: &mut Scene,
    ships: &[GridId],
    speed: f64,
    motion: Vec<(Vec3, Vec3)>,
) -> Rail {
    let ships = ships.to_vec();
    let rail: std::sync::Arc<std::sync::Mutex<Option<Rail>>> = Default::default();
    let made = std::sync::Arc::clone(&rail);
    scene.run(move |world| {
        set_going_in(world, &ships, speed, motion);
        let planet = ship_id(0);
        let rail = rail_of_the_group(world, planet, &ships);
        world.set_grid_rail(ships[0], Some(rail.clone())).unwrap();
        *made.lock().unwrap() = Some(rail);
    });
    rail.lock().unwrap().take().expect("the rail was made")
}

pub fn world_of(scene: &Scene, ship: GridId, local: Vec3) -> Vec3 {
    let ship = scene.rig.server.world().grids().get(ship).unwrap();
    ship.pose
        .local_to_world(WorldPosition::from_voxels(local))
        .unwrap()
        .in_voxels()
}

/// How far apart two pivots are, in voxels: the east pivot of a car and the west pivot of the next.
pub fn pivot_gap(scene: &Scene, east: (GridId, f64), west: GridId) -> f64 {
    let first = world_of(scene, east.0, [east.1, 0.5, 0.5]);
    let second = world_of(scene, west, [0.0, 0.5, 0.5]);
    length([0, 1, 2].map(|axis| first[axis] - second[axis]))
}

pub fn orientation_of(scene: &Scene, ship: GridId) -> Orientation {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .pose
        .orientation
}

pub fn spin_of(scene: &Scene, ship: GridId) -> Vec3 {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship)
        .unwrap()
        .angular_velocity
}

/// How alike the turns of two ships are: the cosine of half of the angle between them, 1 for the same turn.
pub fn alike_in_turn(scene: &Scene, first: GridId, second: GridId) -> f64 {
    let (first, second) = (
        orientation_of(scene, first).components(),
        orientation_of(scene, second).components(),
    );
    (0..4)
        .map(|axis| first[axis] * second[axis])
        .sum::<f64>()
        .abs()
}

/// The centre of mass of the group on the rail the group is on (a rail of one orbit: the planet is still).
pub fn rail_point(scene: &Scene, rail: &Rail) -> (Vec3, Vec3) {
    rail_point_in(scene, rail, 0)
}

/// The same, this many ticks from the world's clock.
pub fn rail_point_in(scene: &Scene, rail: &Rail, ticks: u64) -> (Vec3, Vec3) {
    let planet = scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(0))
        .unwrap()
        .pose
        .position
        .in_voxels();
    let state = rail.state(
        RailTime::of_tick(scene.rig.server.world().clock() + ticks, TICKS_PER_SECOND),
        planet,
        [0.0; 3],
    );
    (state.position.unwrap(), state.velocity)
}

pub fn close(found: Vec3, expected: Vec3, tolerance: f64, what: &str) {
    for axis in 0..3 {
        assert!(
            (found[axis] - expected[axis]).abs() <= tolerance,
            "{what}: {found:?} is not {expected:?}"
        );
    }
}

/// The motion the chain is given: the middle car swings about z, the others the other way, gently, with nothing added to their speeds.
pub fn a_swing() -> Vec<(Vec3, Vec3)> {
    vec![
        ([0.0; 3], [0.0, 0.0, 0.2]),
        ([0.0; 3], [0.0, 0.0, -0.2]),
        ([0.0; 3], [0.0, 0.0, 0.2]),
    ]
}

/// The total mass of these ships, as the world has it.
pub fn mass_of(scene: &Scene, ships: &[GridId]) -> f64 {
    ships
        .iter()
        .map(|ship| scene.rig.server.world().grids().get(*ship).unwrap().mass)
        .sum()
}

/// Where the origin of each ship is, in voxels.
pub fn origins_of(scene: &Scene, ships: &[GridId]) -> Vec<Vec3> {
    ships
        .iter()
        .map(|ship| {
            scene
                .rig
                .server
                .world()
                .grids()
                .get(*ship)
                .unwrap()
                .pose
                .position
                .in_voxels()
        })
        .collect()
}

pub fn minus(first: Vec3, second: Vec3) -> Vec3 {
    [0, 1, 2].map(|axis| first[axis] - second[axis])
}

/// Where a chain is when a test asks what its mechanism does: on the rail of its centre of mass (step 4c-3), free and alone (step 4c-4: a frame of its own, in free fall, centred on
/// it), or free with a stranger a hundred voxels from it, which is in the way of it by the room that a group needs to be alone, so that it is in the frame everything is in (the control:
/// there the mechanism depends on where it is and how fast it goes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    OnARail,
    Alone,
    WithAStranger,
}

/// A chain of three cars, set swinging as `a_swing` says at this distance from the planet going round it, and, if asked, with a stranger (a ship of one stone) a hundred voxels
/// beyond its east end going the same way. The chain's ships (numbers 1 to 3).
fn a_swinging_chain_at(distance: f64, speed: f64, stranger: bool) -> (Scene, Vec<GridId>) {
    let mut scene = Scene::in_a_new_world();
    let cars = a_chain();
    let mut at = 0.0;
    let mut made = Vec::new();
    for (blocks, length) in cars {
        made.push((pose_at(at, distance, 0.0), blocks));
        at += length;
    }
    if stranger {
        made.push((pose_at(at + 100.0, distance, 0.0), stones(0, 1)));
    }
    super::docking::make_all_going(&mut scene, made, [0.0, 0.0, speed]);
    scene.ticks(2);
    (scene, (1..=3).map(ship_id).collect())
}

/// The facing and the turn of each car of the chain, 200 ticks (ten seconds) after it is set swinging at this distance from the planet, in this setting.
pub fn how_the_chain_turns(distance: f64, setting: Setting) -> Vec<(Orientation, Vec3)> {
    how_the_chain_turns_after(distance, setting, 200)
}

/// The same, `ticks` ticks after it is set swinging. (The swing of this chain turns back at about the 130th tick, a moment at which a difference of the rounding of a few ticks
/// before is made large, so that two chains that were in different frames for a while agree to eight digits until then and not after.)
pub fn how_the_chain_turns_after(
    distance: f64,
    setting: Setting,
    ticks: u32,
) -> Vec<(Orientation, Vec3)> {
    let planet_speed = {
        let scene = Scene::in_a_new_world();
        super::circular_speed(&scene, 0, distance)
    };
    let (mut scene, ships) =
        a_swinging_chain_at(distance, planet_speed, setting == Setting::WithAStranger);
    if setting == Setting::OnARail {
        put_on_its_rail(&mut scene, &ships, planet_speed, a_swing());
    } else {
        set_going(&mut scene, &ships, planet_speed, a_swing());
    }
    scene.ticks(ticks as usize);
    ships
        .iter()
        .map(|ship| (orientation_of(&scene, *ship), spin_of(&scene, *ship)))
        .collect()
}

/// Whether a chain, as `how_the_chain_turns` leaves it, has swung: some car faces more than a fifth of a radian from where it began (the cars begin facing one way; the w of a
/// facing is the cosine of half the turn from that). The control of the tests that two chains agree: two chains that did not move would agree for nothing. (It was that some car
/// was still turning at over 0.05 radians a second at the end, until step 4c-5-A: the solver's passes make the impact at the 130th tick, where the swing turns back, lose more of the
/// swing, which then ends at 0.04 radians a second, and the car that turned most faces 0.76 radians from where it began.)
pub fn has_swung(chain: &[(Orientation, Vec3)]) -> bool {
    chain
        .iter()
        .any(|(facing, _)| facing.components()[3] < 0.995)
}

/// A chain as its mirror image in the plane it swings in (z to -z): the facing's x and y and the turn's x and y change sign.
fn mirrored(chain: &[(Orientation, Vec3)]) -> Vec<(Orientation, Vec3)> {
    chain
        .iter()
        .map(|(facing, spin)| {
            let [x, y, z, w] = facing.components();
            (
                Orientation::from_components([-x, -y, z, w])
                    .expect("the mirror image of a facing is one"),
                [-spin[0], -spin[1], spin[2]],
            )
        })
        .collect()
}

/// The most the facings and the turns of two chains differ by, taking the second as it is or as its mirror image in the plane the chain swings in, whichever is nearer. A chain that
/// swings in a plane has a mirror symmetry, and a symmetric impact in it (two faces that meet squarely, as at the 130th tick of this swing) leaves it tilted a hair to one side or
/// to the other, **which of the two is decided by the rounding of the numbers and not by where the chain is**: since the solver makes 15 extra passes (step 4c-5-A, 2026-11-14) the
/// chains at 70 and at 140 kilometres are exact mirror images of each other after that impact, 0.0034 apart as they are (the tilt, 0.0019 in the facing's x, is the same to nine
/// digits and of the other sign) and 4e-9 apart mirrored, which is what they were apart as they were before (2e-9). The size of the tilt, and everything in the plane, is still
/// compared.
pub fn most_apart_up_to_the_mirror(
    first: &[(Orientation, Vec3)],
    second: &[(Orientation, Vec3)],
) -> f64 {
    most_apart(first, second).min(most_apart(first, &mirrored(second)))
}

/// The most the facings and the turns of two chains differ by.
pub fn most_apart(first: &[(Orientation, Vec3)], second: &[(Orientation, Vec3)]) -> f64 {
    let mut worst: f64 = 0.0;
    for ((facing, spin), (other_facing, other_spin)) in first.iter().zip(second) {
        for axis in 0..4 {
            worst = worst.max((facing.components()[axis] - other_facing.components()[axis]).abs());
        }
        for axis in 0..3 {
            worst = worst.max((spin[axis] - other_spin[axis]).abs());
        }
    }
    worst
}
