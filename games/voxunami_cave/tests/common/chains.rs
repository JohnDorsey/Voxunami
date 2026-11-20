//! Chains of cars with dev ball joints in orbit round the planet of a new world, for the tests of a group of ships riding a rail (step 4c-3 of
//! `docs/decisions/joints-and-cars.md`): the cars, the chain they make end to end, where its centre of mass and its rail are, and how to look at it.

#![allow(dead_code)] // Each test file uses some of it.

use super::docking::{PLAIN, Placed, make_all_going, stone, stones};
use super::joints::{ball_end_toward, joint_block};
use super::{Scene, length, pose_at, ship_id};
use voxel_space::{BrickCoordinate, FaceDirection, GridId, GridPose, Orientation, WorldPosition};
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

/// A chain of three cars (`a_chain`) end to end along x, its first car's origin `offset` voxels (in the world's axes) from the middle of ship `planet`, going `against` the
/// ground as said and at the speed of the ground there (the planet's and the turn of the planet about its middle); made in one tick and given two ticks, in
/// which the bodies couple the cars, and then **put where `offset` says from the planet as it is by then, and given the speed it has by then and `against`** (the planet goes on in
/// the two ticks and turns its speed, and a chain made where it was is a little off the place and the speed that were asked: 0.07 voxels and 0.4 a second at 1,257 voxels a second).
/// The cars' ids, the first is the one that is at the place `offset` says.
pub fn chain_over(
    scene: &mut Scene,
    planet: u64,
    offset: [f64; 3],
    against: [f64; 3],
) -> Vec<GridId> {
    let first = scene.rig.server.world().grids().iter().count() as u64;
    let places: Vec<([f64; 3], Vec<Placed>)> = {
        let mut along = 0.0;
        a_chain()
            .into_iter()
            .map(|(blocks, length)| {
                let place = [offset[0] + along, offset[1], offset[2]];
                along += length;
                (place, blocks)
            })
            .collect()
    };
    let relative: Vec<[f64; 3]> = places.iter().map(|(place, _)| *place).collect();
    let middle = super::ship_voxels(scene, planet);
    let speed = planet_speed(scene, planet);
    let made = places
        .into_iter()
        .map(|(place, blocks)| {
            (
                pose_at(
                    middle[0] + place[0],
                    middle[1] + place[1],
                    middle[2] + place[2],
                ),
                blocks,
            )
        })
        .collect::<Vec<_>>();
    let count = made.len() as u64;
    make_all_going(
        scene,
        made,
        [0, 1, 2].map(|axis| speed[axis] + against[axis]),
    );
    scene.ticks(2);
    let cars: Vec<GridId> = (first..first + count).map(ship_id).collect();
    let given = cars.clone();
    // (The planet is read in the tick that puts the cars, where it is in that tick: the world has moved it before anything of the tick runs.)
    scene.run(move |world| {
        let planet = world
            .data()
            .grids()
            .get(ship_id(planet))
            .expect("the planet is there");
        let (middle, speed, turn) = (
            planet.pose.position,
            planet.linear_velocity,
            planet.angular_velocity.map(f64::from),
        );
        for (car, place) in given.into_iter().zip(relative) {
            // The ground goes at the planet's velocity and the turn of the planet about its middle: omega cross the place of the car from the middle.
            let round = [
                turn[1] * place[2] - turn[2] * place[1],
                turn[2] * place[0] - turn[0] * place[2],
                turn[0] * place[1] - turn[1] * place[0],
            ];
            world
                .set_grid_pose(
                    car,
                    super::GridPose::new(middle.translated(place), Orientation::IDENTITY),
                )
                .unwrap();
            world
                .set_grid_velocity(
                    car,
                    [0, 1, 2].map(|axis| speed[axis] + round[axis] + against[axis]),
                    [0.0; 3],
                )
                .unwrap();
        }
    });
    cars
}

/// How fast ship `planet` goes in the world, as the world has it.
fn planet_speed(scene: &Scene, planet: u64) -> [f64; 3] {
    scene
        .rig
        .server
        .world()
        .grids()
        .get(ship_id(planet))
        .expect("the planet is there")
        .linear_velocity
}

/// Which way the cars of a chain lie: as they are made, with their length along x (the way a car's blocks are set), or each turned a quarter about y (the way up from the planet) so
/// that the length lies along z, the way the chain goes. The inertia of a car in the world's axes is not the one it has in its own when it is turned, and a test that turns the cars is
/// the only one that can tell the two apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lying {
    AlongX,
    AlongZ,
}

impl Lying {
    /// The turn of a car: none, or a quarter about y that has the car's x along the world's z (and its z along the world's minus x).
    pub fn orientation(self) -> Orientation {
        match self {
            Lying::AlongX => Orientation::IDENTITY,
            Lying::AlongZ => {
                let half = std::f64::consts::FRAC_1_SQRT_2;
                Orientation::from_components([0.0, -half, 0.0, half])
                    .expect("a quarter turn is a rotation")
            }
        }
    }

    /// The pose of a car whose origin is at `origin` in the world (its length is along `self`, so the car goes on from there).
    pub fn pose_at(self, origin: Vec3) -> GridPose {
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, origin),
            self.orientation(),
        )
    }

    /// The world's axis the length of a car lies along.
    pub fn axis(self) -> usize {
        match self {
            Lying::AlongX => 0,
            Lying::AlongZ => 2,
        }
    }
}

/// Makes the cars of a chain end to end along x, from the origin, `distance` from the middle of the planet along y, going at `speed` along z, in a scene that has the planet (the
/// ship before them), all in one tick. Their numbers are 1, 2 and so on. Two ticks after, the bodies have coupled them.
pub fn a_chain_in_orbit(
    scene: &mut Scene,
    cars: Vec<(Vec<Placed>, f64)>,
    distance: f64,
    speed: f64,
) -> Vec<GridId> {
    a_chain_in_orbit_lying(scene, cars, distance, speed, Lying::AlongX)
}

/// `a_chain_in_orbit` with the chain lying as asked: from the origin along x, or along z.
pub fn a_chain_in_orbit_lying(
    scene: &mut Scene,
    cars: Vec<(Vec<Placed>, f64)>,
    distance: f64,
    speed: f64,
    lying: Lying,
) -> Vec<GridId> {
    let count = cars.len() as u64;
    let mut at = 0.0;
    let mut made = Vec::new();
    for (blocks, length) in cars {
        let mut origin = [0.0, distance, 0.0];
        origin[lying.axis()] = at;
        made.push((lying.pose_at(origin), blocks));
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
/// it), or free with a stranger fifteen voxels from it, which has the chain a ghost (step 4c-5-2: before it, the stranger took the chain into the frame everything is in, where the
/// mechanism depended on where it was and how fast it went).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    OnARail,
    Alone,
    WithAStranger,
}

/// A chain of three cars, set swinging as `a_swing` says at this distance from the planet going round it, and, if asked, with a stranger (a ship of one stone) fifteen voxels
/// beyond its east end going the same way: near enough to have the chain's ghost, and too far for the swing to touch it. The chain's ships (numbers 1 to 3).
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
        made.push((pose_at(at + 15.0, distance, 0.0), stones(0, 1)));
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
/// before is made large, so that two chains that were in different frames for a while agree to eight digits until then and not after. **It is an impact with two outcomes that
/// rounding alone decides** (a car's spin after it is 0.034 or 0.080): an alone chain at 70 and at 140 kilometres agreed to 5e-10 at the 200th tick with the old step of the frame
/// and differ by 0.19 with velocity Verlet, and a change of 1e-13 in the speed of the frame flipped it with the old step too: `docs/decisions/joints-and-cars.md`, 4c-5-2.)
pub fn how_the_chain_turns_after(
    distance: f64,
    setting: Setting,
    ticks: u32,
) -> Vec<(Orientation, Vec3)> {
    let planet_speed = {
        let scene = Scene::in_a_new_world();
        super::circular_speed(&scene, 0, distance)
    };
    how_the_chain_turns_going(distance, planet_speed, setting, ticks)
}

/// The same, going at `speed` along z in the world and not at the speed of a circle (a chain at rest over the ground, say).
pub fn how_the_chain_turns_going(
    distance: f64,
    planet_speed: f64,
    setting: Setting,
    ticks: u32,
) -> Vec<(Orientation, Vec3)> {
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

/// What a chain that is pushed is, `ticks` after the push: the facing and the turn of each car, and where its origin is from the centre of mass of the cars (which a push moves, and
/// the mechanism does not follow).
pub struct PushedChain {
    pub turns: Vec<(Orientation, Vec3)>,
    pub standing: Vec<Vec3>,
}

/// A chain of three cars, set swinging as `a_swing` says at this distance from the planet going round it, in this setting (not `WithAStranger`), left to swing for `before` ticks,
/// then pushed by a force of `force` on the car `car` (0 to 2), at its centre of mass, for one tick, and followed for `after` ticks more.
pub fn how_the_chain_goes_when_pushed(
    distance: f64,
    setting: Setting,
    before: u32,
    car: usize,
    force: Vec3,
    after: u32,
) -> PushedChain {
    let planet_speed = {
        let scene = Scene::in_a_new_world();
        super::circular_speed(&scene, 0, distance)
    };
    let (mut scene, ships) = a_swinging_chain_at(distance, planet_speed, false);
    if setting == Setting::OnARail {
        put_on_its_rail(&mut scene, &ships, planet_speed, a_swing());
    } else {
        set_going(&mut scene, &ships, planet_speed, a_swing());
    }
    scene.ticks(before as usize);
    let pushed = ships[car];
    scene.run(move |world| world.apply_grid_force(pushed, force, None).unwrap());
    scene.ticks(after as usize);
    let (centre, _) = centre_of(&scene, &ships);
    PushedChain {
        turns: ships
            .iter()
            .map(|ship| (orientation_of(&scene, *ship), spin_of(&scene, *ship)))
            .collect(),
        standing: origins_of(&scene, &ships)
            .into_iter()
            .map(|origin| minus(origin, centre))
            .collect(),
    }
}

/// Whether a chain, as `how_the_chain_turns` leaves it, has swung: some car faces more than a fifth of a radian from where it began (the cars begin facing one way; the w of a
/// facing is the cosine of half the turn from that). The control of the tests that two chains agree: two chains that did not move would agree for nothing. (It was that some car
/// was still turning at over 0.05 radians a second at the end, until step 4c-5-A: the solver's passes make the impact at the 130th tick, where the swing turns back, lose more of the
/// swing, which then ends at 0.03 radians a second (0.04 before the settling passes, `docs/history/loop-probe.md`, section 20), and the car that turned most faces 0.78 radians
/// from where it began.)
pub fn has_swung(chain: &[(Orientation, Vec3)]) -> bool {
    chain
        .iter()
        .any(|(facing, _)| facing.components()[3] < 0.995)
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

/// A wall of stone, 13 by 13 voxels and one thick, its middle voxel at the origin of its ship: a stranger that a few metres of aim cannot make miss. (A ship at orbital speed in the
/// frame everything is in climbs by the gain of energy that its steps are known to have, `future-goals.md`, "Forces over solver steps": twenty metres in the seven seconds that a
/// stranger closing at 20 metres a second needs to cover a hundred and fifty voxels, which a single stone went over.)
pub fn a_wall() -> Vec<Placed> {
    (-6..=6)
        .flat_map(|y| (-6..=6).map(move |z| ([0, y, z], stone(), PLAIN)))
        .collect()
}
