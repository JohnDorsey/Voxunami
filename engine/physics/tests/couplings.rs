//! Couplings: two bodies held together at pivots by impulse joints (`docs/decisions/joints-and-cars.md`, "Physics").
//!
//! The bodies here have a mass and no colliders unless a test says, so the only thing that acts between them is the coupling. The cars are cubes: a cube's tensor of inertia is
//! a number times the identity, so the energy and the angular momentum of a car do not need its turn to be worked out beyond where its centre is.

use physics::{
    BodyError, BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld, BrickHint,
    Collision, CollisionSource, CouplingError, CouplingId, CouplingOptions, MassMoments, PivotPair,
    SolverDials,
};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const FIRST: BodyId = BodyId(1);
const SECOND: BodyId = BodyId(2);
const THIRD: BodyId = BodyId(3);
const JOINT: CouplingId = CouplingId(7);
/// The mass of a voxel, in kilograms.
const DENSITY: f64 = 5.0;
const TICK: f64 = 1.0 / 20.0;

fn world_with_gravity(gravity: [f64; 3]) -> BodyWorld {
    BodyWorld::new(
        BrickCoordinate::ORIGIN,
        BodySettings {
            gravity,
            ..BodySettings::default()
        },
    )
}

fn free_space() -> BodyWorld {
    world_with_gravity([0.0; 3])
}

/// The mass of a cube of `side` voxels with a corner at the origin of the car: its centre is the middle of the cube.
fn cube_mass(side: i128) -> BodyMass {
    let voxels = (0..side).flat_map(|x| {
        (0..side)
            .flat_map(move |y| (0..side).map(move |z| MassMoments::of_voxel([x, y, z], DENSITY)))
    });
    MassMoments::sum(voxels)
        .properties()
        .expect("a cube has a mass")
}

/// A car: a cube of `side` voxels whose origin is at `origin`, going at `velocity` (of its centre of mass) and turning at `spin`.
fn add_car(
    world: &mut BodyWorld,
    id: BodyId,
    kind: BodyKind,
    side: i128,
    origin: [f64; 3],
    velocity: [f64; 3],
    spin: [f64; 3],
) {
    let state = BodyState {
        position: WorldPosition::from_voxels(origin),
        orientation: Orientation::IDENTITY,
        linear_velocity: velocity,
        angular_velocity: spin,
    };
    world.add_body(id, state, kind).unwrap();
    world.set_mass(id, cube_mass(side)).unwrap();
}

/// The pivot at the middle of the face where a cube of `first_side` at the origin meets a cube of `second_side` on its +x side (the second car's origin is at x = `first_side`).
fn face_pivot(first_side: i128, second_side: i128) -> PivotPair {
    PivotPair {
        first: [
            first_side as f64,
            first_side as f64 / 2.0,
            first_side as f64 / 2.0,
        ],
        second: [0.0, second_side as f64 / 2.0, second_side as f64 / 2.0],
    }
}

/// How a car goes whose pivot goes at `pivot_velocity` and which turns at `spin`: the velocity of its centre of mass is that of the pivot plus the spin's part, `spin x arm`,
/// where `arm` goes from the pivot to the centre. A joint takes away any other (the first step would be a collision, and lose energy that the joint did not make).
fn velocity_of_a_car_on_a_pivot(pivot_velocity: Vec3, spin: Vec3, arm: Vec3) -> Vec3 {
    add(pivot_velocity, cross(spin, arm))
}

/// A ball joint between two cubes of the same side that touch along x, with the pivot at the middle of their meeting face going at `pivot_velocity`, each car turning at its own
/// spin (the ball lets them turn alike or against each other), and neither moving in any other way than the joint allows.
fn two_cars_on_a_ball(
    world: &mut BodyWorld,
    side: i128,
    pivot_velocity: Vec3,
    first_spin: Vec3,
    second_spin: Vec3,
) {
    let half = side as f64 / 2.0;
    add_car(
        world,
        FIRST,
        BodyKind::Dynamic,
        side,
        [100.0; 3],
        velocity_of_a_car_on_a_pivot(pivot_velocity, first_spin, [-half, 0.0, 0.0]),
        first_spin,
    );
    add_car(
        world,
        SECOND,
        BodyKind::Dynamic,
        side,
        [100.0 + side as f64, 100.0, 100.0],
        velocity_of_a_car_on_a_pivot(pivot_velocity, second_spin, [half, 0.0, 0.0]),
        second_spin,
    );
    world
        .couple(
            JOINT,
            FIRST,
            SECOND,
            &[face_pivot(side, side)],
            CouplingOptions::default(),
        )
        .unwrap();
}

type Vec3 = [f64; 3];

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: Vec3, by: f64) -> Vec3 {
    a.map(|part| part * by)
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

/// A vector turned by a unit quaternion `[x, y, z, w]`.
fn turned(quaternion: [f64; 4], v: Vec3) -> Vec3 {
    let u = [quaternion[0], quaternion[1], quaternion[2]];
    let w = quaternion[3];
    let t = scale(cross(u, v), 2.0);
    add(add(v, scale(t, w)), cross(u, t))
}

/// Where a car's centre of mass is, measured from the origin of the world (small numbers here, so no precision is lost by the `f64`).
fn centre_of(world: &BodyWorld, id: BodyId, mass: &BodyMass) -> Vec3 {
    let state = world.state(id).unwrap();
    let origin = state
        .position
        .relative_to(WorldPosition::from_voxels([0.0; 3]));
    add(
        origin,
        turned(state.orientation.components(), mass.centre_of_mass),
    )
}

/// The momentum of some cars (all cubes), their angular momentum about the world's origin, and their kinetic energy.
fn motion_of(world: &BodyWorld, cars: &[(BodyId, i128)]) -> (Vec3, Vec3, f64) {
    let (mut momentum, mut spin, mut energy) = ([0.0; 3], [0.0; 3], 0.0);
    for &(id, side) in cars {
        let mass = cube_mass(side);
        let state = world.state(id).unwrap();
        let centre = centre_of(world, id, &mass);
        let inertia = mass.inertia[0][0];
        momentum = add(momentum, scale(state.linear_velocity, mass.mass));
        spin = add(
            spin,
            add(
                cross(centre, scale(state.linear_velocity, mass.mass)),
                scale(state.angular_velocity, inertia),
            ),
        );
        energy += 0.5 * mass.mass * dot(state.linear_velocity, state.linear_velocity)
            + 0.5 * inertia * dot(state.angular_velocity, state.angular_velocity);
    }
    (momentum, spin, energy)
}

fn difference(a: Vec3, b: Vec3) -> f64 {
    length([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

/// Steps `ticks` ticks and returns the largest gap of the coupling seen at the end of any of them.
fn worst_gap(world: &mut BodyWorld, ticks: usize) -> f64 {
    let mut worst: f64 = 0.0;
    for _ in 0..ticks {
        world.step();
        worst = worst.max(world.pivot_gap(JOINT).expect("the coupling is there"));
    }
    worst
}

#[test]
fn the_pivots_of_two_cars_swinging_about_them_stay_together() {
    let mut world = free_space();
    // Two cubes of 4 voxels; one goes up and spins, the other goes down and spins the other way, so they swing about the ball between them.
    two_cars_on_a_ball(
        &mut world,
        4,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.5, 0.0, -1.0],
    );
    assert_eq!(world.pivot_gap(JOINT), Some(0.0));
    let worst = worst_gap(&mut world, 200);
    assert!(
        worst < GAP_AT_GAME_SETTINGS,
        "the pivots were {worst} voxels apart"
    );
}

/// What the gap of an ordinary coupling may reach at the game's settings (a tick of a twentieth of a second in 3 steps, Rapier's 4 substeps): a bound that the gentle scenes
/// below stay inside (their actual gaps were not recorded), not a measurement. `loop-probe.md` (section 14) found 0.85 mm for a ring and a chain at these settings; the hard case
/// (a thousand to one) has a measured bound of its own.
const GAP_AT_GAME_SETTINGS: f64 = 0.002;

#[test]
fn a_coupling_keeps_the_momentum_and_the_angular_momentum_of_its_cars_to_rounding() {
    let mut world = free_space();
    two_cars_on_a_ball(
        &mut world,
        4,
        [1.0, -0.5, 0.2],
        [0.0, 0.3, 1.0],
        [0.5, 0.0, -1.0],
    );
    let cars = [(FIRST, 4), (SECOND, 4)];
    let (momentum, spin, _) = motion_of(&world, &cars);
    for _ in 0..200 {
        world.step();
    }
    let (momentum_after, spin_after, _) = motion_of(&world, &cars);
    let total_mass = 2.0 * cube_mass(4).mass;
    // A joint pushes the two cars with equal and opposite impulses: the speed of the middle of the two does not change but by rounding.
    assert!(
        difference(momentum, momentum_after) / total_mass < MOMENTUM_ERROR,
        "the middle of the two changed its velocity by {:e}",
        difference(momentum, momentum_after) / total_mass
    );
    assert!(
        difference(spin, spin_after) / length(spin) < SPIN_ERROR,
        "angular momentum changed by a fraction {:e}",
        difference(spin, spin_after) / length(spin)
    );
}

/// Linear momentum is conserved to rounding (the two impulses of a joint are equal and opposite). Angular momentum about a point is not: the two impulses act at points a
/// hair apart (the gap), so it moves by the gap times the impulse; measured 2026-11-09 as 1.4e-5 of the whole over 10 seconds of this scene.
const MOMENTUM_ERROR: f64 = 1e-9;
const SPIN_ERROR: f64 = 1e-4;

#[test]
fn a_coupling_loses_a_little_energy_and_makes_none() {
    let mut world = free_space();
    two_cars_on_a_ball(
        &mut world,
        4,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.5, 0.0, -1.0],
    );
    let cars = [(FIRST, 4), (SECOND, 4)];
    let (_, _, energy) = motion_of(&world, &cars);
    let mut highest: f64 = energy;
    for _ in 0..200 {
        world.step();
        highest = highest.max(motion_of(&world, &cars).2);
    }
    let (_, _, energy_after) = motion_of(&world, &cars);
    assert!(
        highest <= energy * (1.0 + ENERGY_GAIN),
        "the energy rose by a fraction {:e}",
        highest / energy - 1.0
    );
    assert!(
        energy_after >= energy * (1.0 - ENERGY_LOSS),
        "the energy fell by a fraction {:e}",
        1.0 - energy_after / energy
    );
}

const ENERGY_GAIN: f64 = 1e-3;
const ENERGY_LOSS: f64 = 0.01;

#[test]
fn a_pendulum_on_a_fixed_car_swings_with_the_energy_it_was_dropped_with() {
    // A cube of 2 voxels hangs by a ball at its top corner from a fixed cube, in the gravity of the game.
    let mut world = world_with_gravity([0.0, -5.0, 0.0]);
    add_car(
        &mut world,
        FIRST,
        BodyKind::Fixed,
        2,
        [100.0, 110.0, 100.0],
        [0.0; 3],
        [0.0; 3],
    );
    // The second car's top corner is at the fixed one's bottom corner, and its middle is a voxel below it and a voxel out each way: it swings down from there.
    add_car(
        &mut world,
        SECOND,
        BodyKind::Dynamic,
        2,
        [100.0, 108.0, 100.0],
        [0.0; 3],
        [0.0; 3],
    );
    world
        .couple(
            JOINT,
            FIRST,
            SECOND,
            &[PivotPair {
                first: [0.0, 0.0, 0.0],
                second: [0.0, 2.0, 0.0],
            }],
            CouplingOptions::default(),
        )
        .unwrap();
    let mass = cube_mass(2);
    let drop_energy = |world: &BodyWorld| {
        let height = centre_of(world, SECOND, &mass)[1];
        mass.mass * 5.0 * height + motion_of(world, &[(SECOND, 2)]).2
    };
    let start = drop_energy(&world);
    let worst = worst_gap(&mut world, 100);
    let end = drop_energy(&world);
    assert!(
        worst < GAP_AT_GAME_SETTINGS,
        "the pivot was {worst} voxels out"
    );
    assert!(
        (end - start).abs() / (mass.mass * 5.0) < PENDULUM_ENERGY_ERROR,
        "the energy moved by {:e} of a voxel's worth of fall",
        (end - start) / (mass.mass * 5.0)
    );
}

const PENDULUM_ENERGY_ERROR: f64 = 0.02;

/// A car of 20 voxels (8,000 voxels of mass) and, held to the middle of its +x face, a car of 2 (8): 1,000 to 1. The small car's pivot goes up at 5 voxels a second as the car
/// turns about it at 3 radians a second. The most the gap reaches in 10 seconds, with the dials on both bodies.
fn gap_of_a_light_car_on_a_heavy_one(dials: SolverDials) -> f64 {
    let mut world = free_space();
    add_car(
        &mut world,
        FIRST,
        BodyKind::Dynamic,
        20,
        [100.0; 3],
        [0.0; 3],
        [0.0; 3],
    );
    let spin = [0.0, 0.0, 3.0];
    add_car(
        &mut world,
        SECOND,
        BodyKind::Dynamic,
        2,
        [120.0, 100.0, 100.0],
        velocity_of_a_car_on_a_pivot([0.0, 5.0, 0.0], spin, [1.0, 0.0, 0.0]),
        spin,
    );
    assert_eq!(cube_mass(20).mass / cube_mass(2).mass, 1000.0);
    world
        .couple(
            JOINT,
            FIRST,
            SECOND,
            &[PivotPair {
                first: [20.0, 10.0, 10.0],
                second: [0.0, 1.0, 1.0],
            }],
            CouplingOptions::default(),
        )
        .unwrap();
    for id in [FIRST, SECOND] {
        world.set_solver_dials(id, dials).unwrap();
    }
    worst_gap(&mut world, 200)
}

#[test]
fn a_light_car_and_a_car_a_thousand_times_heavier_stay_together() {
    let plain = gap_of_a_light_car_on_a_heavy_one(SolverDials::default());
    let strong = gap_of_a_light_car_on_a_heavy_one(SolverDials {
        extra_substeps: 12,
        extra_passes: 0,
    });
    // Measured 2026-11-09: 6.0e-3 and 1.6e-4 (a thirty-seventh), with the margins of the bounds about 1.7 and 3. A light car on a heavy one is the hard case for the solver at
    // the game's four substeps (6 millimetres here); the dials are what bring it down.
    assert!(plain < 0.01, "the pivots were {plain} voxels apart");
    assert!(
        strong < 0.0005,
        "with 16 substeps the pivots were {strong} voxels apart"
    );
    assert!(strong < plain / 10.0);
}

#[test]
fn two_pivots_on_a_line_make_a_hinge_that_turns_about_the_line_alone() {
    let mut world = free_space();
    add_car(
        &mut world,
        FIRST,
        BodyKind::Fixed,
        4,
        [100.0; 3],
        [0.0; 3],
        [0.0; 3],
    );
    // The hinge line runs along z through the face at x = 4, at y = 2: the second car is spun about y and z and may turn only about the line.
    add_car(
        &mut world,
        SECOND,
        BodyKind::Dynamic,
        4,
        [104.0, 100.0, 100.0],
        [0.0; 3],
        [0.0, 1.0, 1.0],
    );
    world
        .couple(
            JOINT,
            FIRST,
            SECOND,
            &[
                PivotPair {
                    first: [4.0, 2.0, 0.0],
                    second: [0.0, 2.0, 0.0],
                },
                PivotPair {
                    first: [4.0, 2.0, 4.0],
                    second: [0.0, 2.0, 4.0],
                },
            ],
            CouplingOptions::default(),
        )
        .unwrap();
    let worst = worst_gap(&mut world, 40);
    assert!(
        worst < GAP_AT_GAME_SETTINGS,
        "the pivots were {worst} voxels apart"
    );
    let spin = world.state(SECOND).unwrap().angular_velocity;
    assert!(
        spin[0].abs() < HINGE_OFF_AXIS && spin[1].abs() < HINGE_OFF_AXIS,
        "it turns at {spin:?}, not about the line (z) alone"
    );
    assert!(spin[2] > 0.0, "the spin about the line is kept: {spin:?}");
}

const HINGE_OFF_AXIS: f64 = 1e-3;

#[test]
fn a_coupling_is_gone_with_either_of_its_bodies_and_the_other_body_goes_on_free() {
    let mut world = free_space();
    two_cars_on_a_ball(&mut world, 4, [0.0, 1.0, 0.0], [0.0; 3], [0.0; 3]);
    assert_eq!(world.couplings_of(FIRST), vec![JOINT]);
    assert_eq!(world.couplings_of(SECOND), vec![JOINT]);
    world.remove_body(FIRST).unwrap();
    assert_eq!(world.couplings_of(SECOND), Vec::<CouplingId>::new());
    assert_eq!(world.pivot_gap(JOINT), None);
    // The coupling's id is free to be used again, and the car moves as a free body would.
    let before = world.state(SECOND).unwrap().linear_velocity;
    for _ in 0..20 {
        world.step();
    }
    assert_eq!(world.state(SECOND).unwrap().linear_velocity, before);
    assert_eq!(
        world.decouple(JOINT),
        Err(CouplingError::UnknownCoupling(JOINT))
    );
}

#[test]
fn decoupled_cars_go_on_at_the_speeds_they_had_and_come_apart() {
    let mut world = free_space();
    two_cars_on_a_ball(&mut world, 4, [0.0; 3], [0.0, 0.0, 0.5], [0.0, 0.0, 0.5]);
    // Held together, the two cars turn as one about their meeting point, which stands still.
    for _ in 0..10 {
        world.step();
    }
    world.decouple(JOINT).unwrap();
    assert_eq!(world.couplings_of(FIRST), Vec::<CouplingId>::new());
    let cars = [(FIRST, cube_mass(4)), (SECOND, cube_mass(4))];
    let (first_before, second_before) = (
        world.state(FIRST).unwrap().linear_velocity,
        world.state(SECOND).unwrap().linear_velocity,
    );
    let apart_before = [0, 1, 2].map(|axis| {
        centre_of(&world, SECOND, &cars[1].1)[axis] - centre_of(&world, FIRST, &cars[0].1)[axis]
    });
    for _ in 0..20 {
        world.step();
    }
    assert_eq!(world.state(FIRST).unwrap().linear_velocity, first_before);
    assert_eq!(world.state(SECOND).unwrap().linear_velocity, second_before);
    // Free bodies: the vector between the middles grows by the difference of the velocities, for the second that the 20 ticks were.
    let apart_after = [0, 1, 2].map(|axis| {
        centre_of(&world, SECOND, &cars[1].1)[axis] - centre_of(&world, FIRST, &cars[0].1)[axis]
    });
    let expected = [0, 1, 2]
        .map(|axis| apart_before[axis] + (second_before[axis] - first_before[axis]) * 20.0 * TICK);
    assert!(
        difference(apart_after, expected) < 1e-9,
        "{apart_after:?} against {expected:?}"
    );
    // (They were going round the pivot together at a fixed distance; now they go straight, one up and one down, and the distance grows.)
    assert!(length(apart_after) > length(apart_before) + 0.3);
}

#[test]
fn a_coupling_that_cannot_be_made_says_why_and_changes_nothing() {
    let mut world = free_space();
    add_car(
        &mut world,
        FIRST,
        BodyKind::Dynamic,
        2,
        [100.0; 3],
        [0.0; 3],
        [0.0; 3],
    );
    add_car(
        &mut world,
        SECOND,
        BodyKind::Dynamic,
        2,
        [102.0, 100.0, 100.0],
        [0.0; 3],
        [0.0; 3],
    );
    let pivot = [face_pivot(2, 2)];
    let options = CouplingOptions::default();
    assert_eq!(
        world.couple(JOINT, FIRST, THIRD, &pivot, options),
        Err(CouplingError::Body(BodyError::UnknownBody(THIRD)))
    );
    assert_eq!(
        world.couple(JOINT, FIRST, FIRST, &pivot, options),
        Err(CouplingError::SameBody(FIRST))
    );
    assert_eq!(
        world.couple(JOINT, FIRST, SECOND, &[], options),
        Err(CouplingError::NoPivots)
    );
    assert_eq!(
        world.couple(
            JOINT,
            FIRST,
            SECOND,
            &[PivotPair {
                first: [f64::NAN, 0.0, 0.0],
                second: [0.0; 3]
            }],
            options
        ),
        Err(CouplingError::PivotNotANumber)
    );
    assert_eq!(world.couplings_of(FIRST), Vec::<CouplingId>::new());
    world.couple(JOINT, FIRST, SECOND, &pivot, options).unwrap();
    assert_eq!(
        world.couple(JOINT, FIRST, SECOND, &pivot, options),
        Err(CouplingError::DuplicateCoupling(JOINT))
    );
    // A second coupling of the same two cars under another number is allowed (the rule that its pivots must be consistent is the game's, not the solver's).
    world
        .couple(CouplingId(8), SECOND, FIRST, &pivot, options)
        .unwrap();
    assert_eq!(world.couplings_of(FIRST), vec![JOINT, CouplingId(8)]);
}

/// A cube of `side` voxels with a corner at the origin of the body, as a source of terrain for a body's colliders.
struct Cube(i128);

impl CollisionSource for Cube {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let inside = |part: i128| (0..self.0).contains(&part);
        if inside(voxel.x()) && inside(voxel.y()) && inside(voxel.z()) {
            Collision::Full
        } else {
            Collision::Empty
        }
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        if brick == BrickCoordinate::ORIGIN {
            BrickHint::Unknown
        } else {
            BrickHint::Empty
        }
    }
}

fn two_overlapping_cars(contacts_between_bodies: bool, spin: Vec3) -> BodyWorld {
    let mut world = free_space();
    // Two cubes of 4 voxels that overlap by half a voxel along x, held at a pivot in the overlap, both turning about it at `spin` (so that they move as the joint allows).
    // (A deeper overlap is not pushed apart at all: the contacts of two voxel colliders found 2 voxels deep give nothing, measured 2026-11-09, so the scene is a shallow one.)
    let pivot = [103.75, 102.0, 102.0];
    for (id, side_origin) in [(FIRST, 100.0), (SECOND, 103.5)] {
        let centre = [side_origin + 2.0, 102.0, 102.0];
        let arm = [centre[0] - pivot[0], 0.0, 0.0];
        add_car(
            &mut world,
            id,
            BodyKind::Dynamic,
            4,
            [side_origin, 100.0, 100.0],
            velocity_of_a_car_on_a_pivot([0.0; 3], spin, arm),
            spin,
        );
        world
            .set_brick(id, BrickCoordinate::ORIGIN, &mut Cube(4))
            .unwrap();
    }
    world
        .couple(
            JOINT,
            FIRST,
            SECOND,
            &[PivotPair {
                first: [3.75, 2.0, 2.0],
                second: [0.25, 2.0, 2.0],
            }],
            CouplingOptions {
                contacts_between_bodies,
            },
        )
        .unwrap();
    world
}

fn energy_after(world: &mut BodyWorld, ticks: usize) -> (f64, f64) {
    let cars = [(FIRST, 4), (SECOND, 4)];
    let before = motion_of(world, &cars).2;
    for _ in 0..ticks {
        world.step();
    }
    (before, motion_of(world, &cars).2)
}

#[test]
fn cars_that_overlap_and_are_coupled_make_no_energy_when_their_contacts_are_off() {
    let mut world = two_overlapping_cars(false, [0.0, 0.0, 1.0]);
    let (before, after) = energy_after(&mut world, 100);
    // Measured 2026-11-09: 1,833.3 to 1,813.2 in five seconds, a loss of 1.1 percent (the loss of the solver, as in a pair that does not overlap).
    assert!(
        after <= before * (1.0 + ENERGY_GAIN),
        "the energy rose from {before} to {after}"
    );
    assert!(
        after >= before * (1.0 - 0.02),
        "the energy fell from {before} to {after}"
    );
}

#[test]
fn cars_that_overlap_at_their_pivot_cannot_swing_when_their_contacts_are_on() {
    // (Why the option exists, and what it costs: the contact pushes the overlapping cubes apart where the joint holds their pivot, so they lock up.)
    let mut world = two_overlapping_cars(true, [0.0, 0.0, 1.0]);
    let (before, after) = energy_after(&mut world, 100);
    // Measured 2026-11-09: 1,833.3 to 6.3 in five seconds.
    assert!(after < before * 0.05, "the energy was {after} of {before}");
}

#[test]
fn moving_the_frame_of_the_world_does_not_change_a_coupling() {
    let mut world = free_space();
    two_cars_on_a_ball(
        &mut world,
        4,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.5, 0.0, -1.0],
    );
    for _ in 0..10 {
        world.step();
    }
    world.recentre(BrickCoordinate::new(5, -2, 7));
    let worst = worst_gap(&mut world, 100);
    assert!(
        worst < GAP_AT_GAME_SETTINGS,
        "the pivots were {worst} voxels apart"
    );
}

#[test]
fn a_car_that_is_given_a_new_state_takes_its_coupling_with_it_and_not_its_partner() {
    let mut world = free_space();
    two_cars_on_a_ball(&mut world, 4, [0.0; 3], [0.0; 3], [0.0; 3]);
    // Moved by something outside the solver (a command), the car leaves the pivot behind: the gap is how far, and the solver pulls it in over the following steps.
    let mut moved = world.state(SECOND).unwrap();
    moved.position = moved.position.translated([0.0, 0.5, 0.0]);
    world.set_state(SECOND, moved).unwrap();
    assert!((world.pivot_gap(JOINT).unwrap() - 0.5).abs() < 1e-9);
    for _ in 0..40 {
        world.step();
    }
    assert!(world.pivot_gap(JOINT).unwrap() < GAP_AT_GAME_SETTINGS);
}

#[test]
fn extra_solver_dials_on_the_bodies_of_a_coupling_make_its_gap_smaller() {
    let gap_with = |dials: SolverDials| {
        let mut world = free_space();
        add_car(
            &mut world,
            FIRST,
            BodyKind::Dynamic,
            20,
            [100.0; 3],
            [0.0; 3],
            [0.0; 3],
        );
        add_car(
            &mut world,
            SECOND,
            BodyKind::Dynamic,
            2,
            [120.0, 100.0, 100.0],
            [0.0, 30.0, 0.0],
            [0.0, 0.0, 6.0],
        );
        world
            .couple(
                JOINT,
                FIRST,
                SECOND,
                &[PivotPair {
                    first: [20.0, 10.0, 10.0],
                    second: [0.0, 1.0, 1.0],
                }],
                CouplingOptions::default(),
            )
            .unwrap();
        for id in [FIRST, SECOND] {
            world.set_solver_dials(id, dials).unwrap();
        }
        worst_gap(&mut world, 100)
    };
    let plain = gap_with(SolverDials::default());
    let strong = gap_with(SolverDials {
        extra_substeps: 12,
        extra_passes: 0,
    });
    assert!(
        plain > 1e-6,
        "the scene is too gentle to show anything: {plain}"
    );
    assert!(
        strong < plain / 3.0,
        "16 substeps gave {strong}, 4 gave {plain}"
    );
}

#[test]
fn cars_in_free_space_stay_awake_at_rest_whether_they_may_sleep_or_not() {
    // A mechanism that stops because its cars fell asleep, mid-swing or at rest, works differently from one that does not, and a promise of the owner's is that no
    // mechanism does (`docs/decisions/joints-and-cars.md`, "Promises", answer 10). Rapier's own rule would have put two cars at rest to sleep after half a second; the rule of
    // this world is that a body sleeps only while it touches something (`sleep_only_in_contact.rs`), and these touch nothing, so they are awake either way. (That a car
    // kept awake stays awake on the ground, and one that may sleep there does, is in that file.)
    for can_sleep in [true, false] {
        let mut world = free_space();
        two_cars_on_a_ball(&mut world, 4, [0.0; 3], [0.0; 3], [0.0; 3]);
        for id in [FIRST, SECOND] {
            world.set_can_sleep(id, can_sleep).unwrap();
        }
        for _ in 0..100 {
            world.step();
        }
        assert!(!world.is_asleep(FIRST) && !world.is_asleep(SECOND));
    }
    assert_eq!(
        world_with_gravity([0.0; 3]).set_can_sleep(BodyId(99), false),
        Err(BodyError::UnknownBody(BodyId(99)))
    );
}

#[test]
fn the_state_of_a_run_with_a_coupling_is_the_same_to_the_bit_on_every_machine() {
    let mut world = world_with_gravity([0.0, -5.0, 0.0]);
    two_cars_on_a_ball(
        &mut world,
        4,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.5, 0.0, -1.0],
    );
    add_car(
        &mut world,
        THIRD,
        BodyKind::Dynamic,
        2,
        [108.0, 100.0, 100.0],
        [0.0; 3],
        [0.0; 3],
    );
    world
        .couple(
            CouplingId(9),
            SECOND,
            THIRD,
            &[PivotPair {
                first: [4.0, 2.0, 2.0],
                second: [0.0, 1.0, 1.0],
            }],
            CouplingOptions::default(),
        )
        .unwrap();
    for _ in 0..100 {
        world.step();
    }
    assert_eq!(world.state_hash(), PINNED_HASH);
}

/// Taken on Windows on 2026-11-09 and checked under Linux (WSL) the same day.
const PINNED_HASH: u64 = 15_871_630_393_772_392_020;

#[test]
fn the_tick_is_what_the_pins_assume() {
    // (The scenes above are written for a tick of a twentieth of a second.)
    assert_eq!(BodySettings::default().time_step, TICK);
}
