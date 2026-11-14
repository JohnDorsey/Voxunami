//! A hit comes out the same whatever moment of a step it falls at and however many steps a tick is cut into (step 4c-5-A of `docs/decisions/joints-and-cars.md`; the study is
//! section 18 of `docs/history/loop-probe.md`, and `docs/history/spikes/loops` has the probes, `engine_probe levers|landing|pairspin|chainmoments`).
//!
//! The scene is the study's, in the game's own world: a stranger cube comes along -x at a chain of cubes that are coupled to each other, and nothing bounces (no restitution, so
//! a hit is perfectly inelastic), so every cube ends going with the others at the speed momentum alone gives: a stranger of the mass of one car on a chain of four keeps a fifth of
//! its speed. What a solver gives instead depends on the moment of a step at which the cubes first touch (the stranger's gap says when), on the number of steps (the world cuts
//! a tick into 3 to 24 by its fastest body, so a faster hit is cut into more), and on how many passes the solver makes over its constraints. The world is given the levers that
//! take that away (passes for every body, a cut of the step where two bodies first touch, a fixed joint for a rigid coupling), and these tests are what each does.

use physics::{
    BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld, BrickHint, Collision,
    CollisionSource, CouplingId, CouplingOptions, MassMoments, PivotPair, SolverDials,
};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const TICK: f64 = 1.0 / 20.0;
/// The mass of a voxel: a cube of one voxel weighs 1000.
const DENSITY: f64 = 1000.0;
/// The cars of the chain, and the one that hits them.
const CARS: usize = 4;
const STRANGER: u64 = CARS as u64 + 1;
/// The most a kept speed may be from the true one, in parts of the closing speed: a fortieth of the true answer of a fifth. The study measured today's world (none of the
/// levers) 0.04 to 0.14 for a rigid chain, a difference of up to 80 percent of the answer; with them the chain keeps 0.1963 to 0.1998 over 12 moments of a step at 1 to 150
/// voxels a second, 1.9 percent short at the worst (0.1963 is the rigid and the hinged chain at 150 voxels a second, where a step is 5 ms; the game's coupling, a ball, keeps
/// 0.1976 to 0.1990). What is left is the solver's own softness: a contact that gives way a little, and a small turn that the four points of a contact give the cars.
const TOLERANCE: f64 = 0.005;
/// What the stranger keeps of its speed when it hits the chain: momentum says one car's mass in five.
const KEPT_ON_THE_CHAIN: f64 = 1.0 / (CARS as f64 + 1.0);

/// A source whose one whole cube is the voxel at the origin of its body.
struct UnitCube;

impl CollisionSource for UnitCube {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if (voxel.x(), voxel.y(), voxel.z()) == (0, 0, 0) {
            Collision::Full
        } else {
            Collision::Empty
        }
    }
}

/// Terrain: every voxel below the height 0 is a whole cube.
struct Floor;

impl CollisionSource for Floor {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if voxel.y() < 0 {
            Collision::Full
        } else {
            Collision::Empty
        }
    }

    fn brick_hint(&mut self, brick: BrickCoordinate) -> BrickHint {
        if brick.y() < 0 {
            BrickHint::Solid
        } else {
            BrickHint::Empty
        }
    }
}

fn unit_cube_mass() -> BodyMass {
    MassMoments::sum([MassMoments::of_voxel([0, 0, 0], DENSITY)])
        .properties()
        .expect("a cube has a mass")
}

/// A cube of one voxel with its origin at `origin`, going at `velocity`.
fn add_cube(world: &mut BodyWorld, id: u64, origin: [f64; 3], velocity: [f64; 3]) {
    let state = BodyState {
        position: WorldPosition::from_voxels(origin),
        orientation: Orientation::IDENTITY,
        linear_velocity: velocity,
        angular_velocity: [0.0; 3],
    };
    world
        .add_body(BodyId(id), state, BodyKind::Dynamic)
        .unwrap();
    world
        .set_brick(BodyId(id), BrickCoordinate::ORIGIN, &mut UnitCube)
        .unwrap();
    world.set_mass(BodyId(id), unit_cube_mass()).unwrap();
}

fn velocity(world: &BodyWorld, id: u64) -> [f64; 3] {
    world.state(BodyId(id)).expect("a body").linear_velocity
}

/// How fast a body turns, about its fastest axis.
fn spin(world: &BodyWorld, id: u64) -> f64 {
    world
        .state(BodyId(id))
        .expect("a body")
        .angular_velocity
        .iter()
        .fold(0.0, |most: f64, part| most.max(part.abs()))
}

/// How two cubes that touch along x are held together.
struct Coupling {
    name: &'static str,
    pivots: Vec<PivotPair>,
    /// Whether the cubes of a pair meet each other (the game's coupling: they do).
    contacts_between_cars: bool,
}

fn pivot(y: f64, z: f64) -> PivotPair {
    PivotPair {
        first: [1.0, y, z],
        second: [0.0, y, z],
    }
}

fn couplings() -> Vec<Coupling> {
    vec![
        Coupling {
            name: "rigid, three pivots on the shared face, the cars pass through each other",
            pivots: vec![pivot(0.0, 0.0), pivot(1.0, 0.0), pivot(0.0, 1.0)],
            contacts_between_cars: false,
        },
        Coupling {
            name: "rigid, three pivots on the shared face, the cars meet",
            pivots: vec![pivot(0.0, 0.0), pivot(1.0, 0.0), pivot(0.0, 1.0)],
            contacts_between_cars: true,
        },
        Coupling {
            name: "a ball at the middle of the shared face, the cars meet (the game's coupling)",
            pivots: vec![pivot(0.5, 0.5)],
            contacts_between_cars: true,
        },
        Coupling {
            name: "a hinge about z through the middle of the shared face",
            pivots: vec![pivot(0.5, 0.0), pivot(0.5, 1.0)],
            contacts_between_cars: false,
        },
    ]
}

/// The steps a tick is cut into when the fastest body goes at `speed` and the settings say `base` (the world's rule: 0.8 voxels a step at most, 24 steps at most).
fn steps_for(speed: f64, base: u32) -> u32 {
    ((speed * TICK / 0.8).ceil() as u32).clamp(base, 24.max(base))
}

/// The gap that makes the stranger arrive `phase` of the way through the seventh step of a tick cut as the world will cut it.
fn gap_at_phase(speed: f64, base: u32, phase: f64) -> f64 {
    speed * TICK / f64::from(steps_for(speed, base)) * (6.0 + phase)
}

/// The gap that makes the stranger arrive `ticks` ticks in, however the ticks are cut.
fn gap_at_tick(speed: f64, ticks: f64) -> f64 {
    speed * TICK * ticks
}

fn settings_with(substeps: u32) -> BodySettings {
    BodySettings {
        substeps,
        gravity: [0.0; 3],
        ..BodySettings::default()
    }
}

fn free_space(substeps: u32) -> BodyWorld {
    BodyWorld::new(BrickCoordinate::ORIGIN, settings_with(substeps))
}

/// What the world was in one second after the stranger touched the chain.
struct Outcome {
    /// What the stranger keeps of its closing speed (positive: it goes on the way it came).
    kept: f64,
    /// The same for the first car of the chain, the one farthest from the hit.
    first_car: f64,
    /// How much the momentum of everything changed, in parts of what the stranger had.
    momentum_change: f64,
}

/// A chain of `CARS` cubes end to end along x, and the stranger coming at its far end from `gap` voxels away at `speed`, in a world made with `settings`.
fn strike_under(coupling: &Coupling, speed: f64, settings: BodySettings, gap: f64) -> Outcome {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    for car in 0..CARS {
        add_cube(
            &mut world,
            car as u64 + 1,
            [car as f64, 10.0, 10.0],
            [0.0; 3],
        );
    }
    for car in 0..CARS as u64 - 1 {
        world
            .couple(
                CouplingId(car + 1),
                BodyId(car + 1),
                BodyId(car + 2),
                &coupling.pivots,
                CouplingOptions {
                    contacts_between_bodies: coupling.contacts_between_cars,
                },
            )
            .unwrap();
    }
    add_cube(
        &mut world,
        STRANGER,
        [CARS as f64 + gap, 10.0, 10.0],
        [-speed, 0.0, 0.0],
    );
    let ticks = ((gap / speed + 1.0) / TICK).ceil() as usize;
    for _ in 0..ticks {
        world.step();
    }
    let momentum: f64 = (1..=STRANGER).map(|id| velocity(&world, id)[0]).sum();
    Outcome {
        kept: -velocity(&world, STRANGER)[0] / speed,
        first_car: -velocity(&world, 1)[0] / speed,
        momentum_change: (momentum + speed).abs() / speed,
    }
}

fn strike(coupling: &Coupling, speed: f64, substeps: u32, gap: f64) -> Outcome {
    strike_under(coupling, speed, settings_with(substeps), gap)
}

fn lowest_and_highest(values: &[f64]) -> (f64, f64) {
    (
        values.iter().copied().fold(f64::INFINITY, f64::min),
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    )
}

/// The kept speeds of a stranger on a chain over 12 moments of a step, in a world made with `settings`.
fn kept_over_a_step(coupling: &Coupling, speed: f64, settings: BodySettings) -> Vec<f64> {
    let base = settings.substeps;
    (0..12)
        .map(|moment| {
            let gap = gap_at_phase(speed, base, f64::from(moment) / 12.0);
            strike_under(coupling, speed, settings, gap).kept
        })
        .collect()
}

#[test]
fn a_stranger_keeps_a_fifth_of_its_speed_on_a_chain_of_four_wherever_in_a_step_it_hits() {
    let mut wrong = Vec::new();
    for coupling in couplings() {
        for speed in [1.0, 5.0, 20.0, 60.0, 150.0] {
            let kept = kept_over_a_step(&coupling, speed, settings_with(3));
            let (low, high) = lowest_and_highest(&kept);
            if (low - KEPT_ON_THE_CHAIN)
                .abs()
                .max((high - KEPT_ON_THE_CHAIN).abs())
                > TOLERANCE
            {
                wrong.push(format!(
                    "{} at {speed} voxels a second: the stranger keeps {low:.4} to {high:.4} of its speed, over 12 moments of a step, where momentum says {KEPT_ON_THE_CHAIN}",
                    coupling.name
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn a_touch_at_the_very_start_of_a_step_comes_out_as_one_in_the_middle_of_it() {
    // The piece before such a touch is only the overshoot, a hundred-millionth of a second, and a contact that acts in a step so short (one within the solver's margin does, when
    // the piece begins) is solved violently: the stranger kept 0.177 where in the middle of a step it keeps 0.1987. The world solves that piece with no margin.
    let mut wrong = Vec::new();
    for coupling in couplings() {
        for speed in [5.0, 20.0, 60.0, 150.0] {
            let kept = kept_over_a_step(&coupling, speed, settings_with(3));
            if (kept[0] - kept[6]).abs() > 0.003 {
                wrong.push(format!(
                    "{} at {speed} voxels a second: a touch at the start of a step keeps {:.4}, one in the middle of it {:.4}",
                    coupling.name, kept[0], kept[6]
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "
{}",
        wrong.join(
            "
"
        )
    );
}

#[test]
fn a_hit_on_the_games_coupling_keeps_the_same_speed_at_every_moment_of_a_step() {
    // The hit is solved in a piece of one length (a twelfth of a tick, or what is left of the step if that is less), and comes out alike at all the moments of a step: within
    // 0.0008 of the speed (0.1986 to 0.1990), where the piece that was left of the step, anything up to a whole step, gave 0.1975 to 0.1989.
    let coupling = &couplings()[2];
    let mut wrong = Vec::new();
    for speed in [5.0, 20.0, 60.0, 150.0] {
        let kept = kept_over_a_step(coupling, speed, settings_with(3));
        let (low, high) = lowest_and_highest(&kept);
        if high - low > 0.0008 {
            wrong.push(format!(
                "at {speed} voxels a second the stranger keeps {low:.4} to {high:.4} over 12 moments of a step"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "
{}",
        wrong.join(
            "
"
        )
    );
}

#[test]
fn a_hit_comes_out_the_same_whatever_number_of_steps_a_tick_is_cut_into() {
    let mut wrong = Vec::new();
    for coupling in couplings() {
        for speed in [5.0, 20.0, 60.0] {
            // The same hit (the stranger arrives at the same moment of the tick), stepped as 3, 6, 12 and 24 steps a tick cut it.
            for moment in 0..6 {
                let arrives = 2.0 + f64::from(moment) / 6.0;
                let kept: Vec<f64> = [3, 6, 12, 24]
                    .into_iter()
                    .map(|substeps| {
                        strike(&coupling, speed, substeps, gap_at_tick(speed, arrives)).kept
                    })
                    .collect();
                let (low, high) = lowest_and_highest(&kept);
                if high - low > TOLERANCE / 2.0 {
                    wrong.push(format!(
                        "{} at {speed} voxels a second, arriving {arrives:.2} ticks in: kept {kept:.4?} with 3, 6, 12 and 24 steps a tick",
                        coupling.name
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn a_hit_on_a_car_alone_keeps_half_the_speed_and_hardly_turns_it_wherever_in_a_step_it_falls() {
    // Two cubes of one mass, one at rest: they go on together at half the speed, and a hit that is square turns neither. (Nothing is coupled here: the passes make a hit between
    // two bodies with no joint square too, and today's world turned the cubes at 0.2 radians a second at 60 voxels a second, and at 0.6 with no cut either.)
    let mut wrong = Vec::new();
    for speed in [2.0, 5.0, 20.0, 60.0] {
        for moment in 0..12 {
            let gap = gap_at_phase(speed, 3, f64::from(moment) / 12.0);
            let mut world = free_space(3);
            add_cube(&mut world, 1, [0.0, 10.0, 10.0], [0.0; 3]);
            add_cube(&mut world, 2, [1.0 + gap, 10.0, 10.0], [-speed, 0.0, 0.0]);
            for _ in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
                world.step();
            }
            let kept = -velocity(&world, 2)[0] / speed;
            let turn = spin(&world, 1).max(spin(&world, 2));
            if (kept - 0.5).abs() > TOLERANCE || turn > 0.002 * speed {
                wrong.push(format!(
                    "{speed} voxels a second, moment {moment} of 12: the stranger keeps {kept:.4}, not a half, and a cube turns at {turn:.4} radians a second"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn a_hit_keeps_the_momentum_of_everything_in_it() {
    for coupling in couplings() {
        for speed in [5.0, 20.0, 60.0] {
            let outcome = strike(&coupling, speed, 3, gap_at_phase(speed, 3, 0.4));
            assert!(
                outcome.momentum_change < 1e-9,
                "{} at {speed}: the momentum changed by {} of the stranger's",
                coupling.name,
                outcome.momentum_change
            );
            // And the chain is whole at the end: its far car goes with the stranger.
            assert!(
                (outcome.first_car - outcome.kept).abs() < TOLERANCE,
                "{} at {speed}: the stranger keeps {} and the far car {}",
                coupling.name,
                outcome.kept,
                outcome.first_car
            );
        }
    }
}

#[test]
fn without_the_cut_a_hit_depends_on_where_in_a_step_it_falls() {
    // The control of the cut: with the passes and no cut, the stranger keeps anything from 0.15 to 0.20 over the moments of a step (the study: today's world 0.04 to 0.14).
    let settings = BodySettings {
        cut_at_first_touch: false,
        ..settings_with(3)
    };
    for coupling in couplings() {
        let (low, high) = lowest_and_highest(&kept_over_a_step(&coupling, 20.0, settings));
        assert!(
            high - low > 0.03,
            "{}: with no cut the stranger kept {low:.4} to {high:.4}, a spread that the cut was found to be what takes away",
            coupling.name
        );
    }
}

#[test]
fn without_the_passes_a_hit_on_a_chain_comes_out_short_and_a_head_on_hit_turns_the_cars() {
    // The control of the passes: with the cut and none, the stranger on a chain keeps less than 0.195 at 20 voxels a second (the highest of the moments of a step, 0.1906; with
    // them 0.1961 at the lowest), and a stranger that meets a car squarely turns it at over two thousandths of the closing speed in radians a second (0.2 at 60 voxels a second;
    // with them under two ten-thousandths).
    let settings = BodySettings {
        solver: SolverDials::default(),
        ..settings_with(3)
    };
    for coupling in couplings() {
        let (_, high) = lowest_and_highest(&kept_over_a_step(&coupling, 20.0, settings));
        assert!(
            high < 0.195,
            "{}: with no passes the stranger kept up to {high:.4}, more than the passes were found to be what is needed for",
            coupling.name
        );
    }
    let speed = 60.0;
    let mut worst: f64 = 0.0;
    for moment in 1..12 {
        let gap = gap_at_phase(speed, 3, f64::from(moment) / 12.0);
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
        add_cube(&mut world, 1, [0.0, 10.0, 10.0], [0.0; 3]);
        add_cube(&mut world, 2, [1.0 + gap, 10.0, 10.0], [-speed, 0.0, 0.0]);
        for _ in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
            world.step();
        }
        worst = worst.max(spin(&world, 1)).max(spin(&world, 2));
    }
    assert!(
        worst > 0.002 * speed,
        "with no passes the cubes turned at {worst} radians a second at most, where they were found to turn at 0.2"
    );
}

#[test]
fn a_step_is_cut_where_two_bodies_first_touch_and_never_where_nothing_touches() {
    let coupling = &couplings()[2];
    let speed = 20.0;
    let mut cuts_when_hit = 0;
    // (Moments 1 to 11: a touch that falls on a boundary of a step, as moment 0 does, may be one that the rounding of the gap leaves a hair into the other cube, which is no
    // first touch to cut at, and the hit is as true.)
    for moment in 1..12 {
        let gap = gap_at_phase(speed, 3, f64::from(moment) / 12.0);
        let mut world = free_space(3);
        add_cube(&mut world, 1, [0.0, 10.0, 10.0], [0.0; 3]);
        add_cube(&mut world, 2, [1.0 + gap, 10.0, 10.0], [-speed, 0.0, 0.0]);
        for _ in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
            world.step();
        }
        assert!(
            world.cuts_made() >= 1,
            "moment {moment} of 12: a stranger touched a cube and no step was cut"
        );
        cuts_when_hit += world.cuts_made();
    }
    assert!(
        cuts_when_hit < 11 * 4,
        "{cuts_when_hit} cuts for 11 hits: a cut is made over and over"
    );
    // The same stranger, a voxel and a half too high to touch: nothing is cut, and the world is what it is with the cut off, bit for bit.
    let run = |cut: bool| {
        let settings = BodySettings {
            cut_at_first_touch: cut,
            ..settings_with(3)
        };
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
        for car in 0..CARS {
            add_cube(
                &mut world,
                car as u64 + 1,
                [car as f64, 10.0, 10.0],
                [0.0; 3],
            );
        }
        for car in 0..CARS as u64 - 1 {
            world
                .couple(
                    CouplingId(car + 1),
                    BodyId(car + 1),
                    BodyId(car + 2),
                    &coupling.pivots,
                    CouplingOptions::default(),
                )
                .unwrap();
        }
        add_cube(
            &mut world,
            STRANGER,
            [CARS as f64 + 3.0, 11.5, 10.0],
            [-speed, 0.0, 0.0],
        );
        for _ in 0..40 {
            world.step();
        }
        (world.cuts_made(), world.state_hash())
    };
    let (cuts, with_the_cut) = run(true);
    let (_, without) = run(false);
    assert_eq!(
        cuts, 0,
        "a stranger that passed over the chain without touching it cut a step"
    );
    assert_eq!(
        with_the_cut, without,
        "the cut changed a world in which nothing touched"
    );
}

/// A unit cube that falls onto terrain (no pull, so the speed is the impact's alone) from a gap that makes it land `phase` of the way through a step: the deepest it was in the ground
/// at the end of a tick, in voxels, the fastest it went up afterwards, and how fast it turns at the end, in radians a second (a cube that lands squarely has no reason to turn).
fn land(speed: f64, phase: f64, settings: BodySettings) -> (f64, f64, f64) {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    for x in -1..=1 {
        for z in -1..=1 {
            world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Floor);
        }
    }
    let gap = gap_at_phase(speed, settings.substeps, phase);
    add_cube(&mut world, 1, [10.0, gap, 10.0], [0.0, -speed, 0.0]);
    let (mut deepest, mut up): (f64, f64) = (0.0, 0.0);
    for _ in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
        world.step();
        let state = world.state(BodyId(1)).unwrap();
        let height = state
            .position
            .relative_to(WorldPosition::from_voxels([0.0; 3]))[1];
        deepest = deepest.max(-height);
        up = up.max(state.linear_velocity[1]);
    }
    let spin = world.state(BodyId(1)).unwrap().angular_velocity;
    (
        deepest,
        up,
        (spin[0] * spin[0] + spin[1] * spin[1] + spin[2] * spin[2]).sqrt(),
    )
}

#[test]
fn a_cube_that_lands_on_terrain_goes_hardly_into_it_and_hardly_bounces_wherever_in_a_step_it_lands()
{
    let mut wrong = Vec::new();
    for speed in [5.0, 20.0, 60.0, 150.0] {
        let landings: Vec<(f64, f64, f64)> = (0..12)
            .map(|moment| land(speed, f64::from(moment) / 12.0, settings_with(3)))
            .collect();
        let deepest = landings.iter().map(|landing| landing.0).fold(0.0, f64::max);
        let up = landings.iter().map(|landing| landing.1).fold(0.0, f64::max);
        // (Today's world: 0.05, 0.24, 0.72 and 0.73 voxels deep at these speeds, because the contact of two voxel shapes is found only once they overlap and a step lets them
        // go a whole step's way in; the cut ends the step where they touch: 0.0003, 0.0012, 0.0054 and 0.0129.)
        if deepest > 0.02 || up > 0.03 * speed {
            wrong.push(format!(
                "{speed} voxels a second: the cube was {deepest:.4} voxels into the ground at the most and went up at {up:.3} voxels a second"
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn a_cube_that_lands_squarely_at_5_and_20_voxels_a_second_turns_by_under_two_hundredths_of_its_speed_wherever_in_a_step_it_lands()
 {
    // The turn a landing leaves, in radians a second, at the highest of the 12 moments of a step: 0.082 and 0.326 with the cut (0.016 of the speed at both); 0.25 and 0.70 in
    // today's world (0.049 and 0.035 of the speed), where a hit that falls late in a step turns the cube much more than one that falls early.
    // This is not claimed at 60 and 150 voxels a second, and it does not hold there: where the piece that meets the touch is cut short by the end of a step the highest turn is
    // 1.18 and 4.76 (`docs/history/loop-probe.md`, section 19, "What the landing table hid").
    let mut wrong = Vec::new();
    for speed in [5.0, 20.0] {
        let turn = (0..12)
            .map(|moment| land(speed, f64::from(moment) / 12.0, settings_with(3)).2)
            .fold(0.0, f64::max);
        if turn > 0.02 * speed {
            wrong.push(format!(
                "{speed} voxels a second: the cube turned at {turn:.3} radians a second at the most"
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn without_the_cut_a_cube_that_lands_squarely_turns_by_more_than_two_hundredths_of_its_speed() {
    // The control of the one above: with no cut (and the passes) a landing at 5 or 20 voxels a second turns the cube at 0.046 and 0.031 of its speed at the worst moment.
    let settings = BodySettings {
        cut_at_first_touch: false,
        ..settings_with(3)
    };
    let worst = [5.0, 20.0].map(|speed| {
        (0..12)
            .map(|moment| land(speed, f64::from(moment) / 12.0, settings).2 / speed)
            .fold(0.0, f64::max)
    });
    assert!(
        worst.iter().all(|turn| *turn > 0.02),
        "with no cut the highest turn of a landing was {worst:?} of the speed at 5 and 20 voxels a second"
    );
}

#[test]
fn without_the_cut_a_cube_that_lands_on_terrain_goes_deep_into_it() {
    // The control of the cut for terrain: a step lets a body go the whole of its way into voxels, and in today's world it went 0.7 voxels deep at 60 voxels a second.
    let settings = BodySettings {
        cut_at_first_touch: false,
        ..settings_with(3)
    };
    let deepest = (0..12)
        .map(|moment| land(60.0, f64::from(moment) / 12.0, settings).0)
        .fold(0.0, f64::max);
    assert!(
        deepest > 0.3,
        "with no cut a cube landing at 60 voxels a second was only {deepest} voxels into the ground"
    );
}

/// A bar of 12 voxels along x, with its middle at the middle of the bar.
struct Bar;

impl CollisionSource for Bar {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if (0..12).contains(&voxel.x()) && voxel.y() == 0 && voxel.z() == 0 {
            Collision::Full
        } else {
            Collision::Empty
        }
    }
}

#[test]
fn a_bar_that_turns_into_a_cube_is_cut_at_the_touch_and_makes_no_momentum_or_energy() {
    // A bar of 12 voxels (12,000) turning at 1 radian a second about its middle, level and with its upper face up at the touch, meets a cube (1,000) at rest on that face over its
    // tip. The cut follows a body that turns by the speed at the middle of the overlap of the two boxes: the touch is found, and what the hit does keeps momentum, and energy
    // is not made.
    let spin_speed = 1.0;
    let mut world = free_space(3);
    // The bar is turned a tenth of a radian from level: its middle (6, 0.5, 0.5 in its own coordinates) is at (20, 20, 20.5), its origin where that puts it. (sin and cos of
    // 0.1 and 0.05 by their series, which do not need the standard functions.)
    let (sine, cosine) = (0.099_833_416_646_828_15, 0.995_004_165_278_025_8);
    let (half_sine, half_cosine) = (0.049_979_169_270_678_33, 0.998_750_260_394_966);
    let origin = [
        20.0 - (6.0 * cosine + 0.5 * sine),
        20.0 - (-6.0 * sine + 0.5 * cosine),
        20.0,
    ];
    let state = BodyState {
        position: WorldPosition::from_voxels(origin),
        // Turned by -0.1 radian about z: it is level after 0.1 s at 1 radian a second.
        orientation: Orientation::from_components([0.0, 0.0, -half_sine, half_cosine]).unwrap(),
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0, 0.0, spin_speed],
    };
    world.add_body(BodyId(1), state, BodyKind::Dynamic).unwrap();
    world
        .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut Bar)
        .unwrap();
    world
        .set_mass(
            BodyId(1),
            MassMoments::sum((0..12).map(|x| MassMoments::of_voxel([x, 0, 0], DENSITY)))
                .properties()
                .unwrap(),
        )
        .unwrap();
    add_cube(&mut world, 2, [25.5, 20.5, 20.0], [0.0; 3]);
    // The kinetic energy of the bar before, from its turning alone: the bar of 12 voxels by 1 by 1 turns about z with I = m (12^2 + 1^2) / 12, and the cube's is m / 6.
    let bar_inertia = 12_000.0 * (144.0 + 1.0) / 12.0;
    let cube_inertia = 1_000.0 / 6.0;
    let before = 0.5 * bar_inertia * spin_speed * spin_speed;
    for _ in 0..40 {
        world.step();
    }
    let (bar, cube) = (
        world.state(BodyId(1)).unwrap(),
        world.state(BodyId(2)).unwrap(),
    );
    assert!(
        world.cuts_made() >= 1,
        "the bar met the cube and no step was cut"
    );
    let momentum_along =
        |axis: usize| 12_000.0 * bar.linear_velocity[axis] + 1_000.0 * cube.linear_velocity[axis];
    let momentum: f64 = (0..3)
        .map(|axis| momentum_along(axis) * momentum_along(axis))
        .sum::<f64>()
        .sqrt();
    assert!(
        momentum < 1e-6 * 12_000.0 * 6.0,
        "the momentum of the bar and the cube is {momentum} after a hit that began with none"
    );
    let speed_squared = |state: &BodyState| {
        state
            .linear_velocity
            .iter()
            .map(|part| part * part)
            .sum::<f64>()
    };
    let after = 0.5 * 12_000.0 * speed_squared(&bar)
        + 0.5 * 1_000.0 * speed_squared(&cube)
        + 0.5 * bar_inertia * bar.angular_velocity[2] * bar.angular_velocity[2]
        + 0.5 * cube_inertia * cube.angular_velocity[2] * cube.angular_velocity[2];
    assert!(
        after < before * (1.0 + 1e-6),
        "the hit made energy: {before} before and {after} after"
    );
    // The cube was driven up by the tip: the bar's tip goes at 6 voxels a second and the pair on the face share it (4.5 upward for a hit through the middle of the cube).
    assert!(
        cube.linear_velocity[1] > 3.0 && cube.linear_velocity[1] < 6.0,
        "the cube went up at {}",
        cube.linear_velocity[1]
    );
}
