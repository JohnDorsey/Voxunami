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
/// The most a kept speed may be from the true one, in parts of the closing speed: an eightieth of the true answer of a fifth. The study measured today's world (none of the
/// levers) 0.04 to 0.14 for a rigid chain, a difference of up to 80 percent of the answer; with the passes and the cut (step 4c-5-A) the chain kept 0.1963 to 0.1998 over 12 moments of
/// a step at 1 to 150 voxels a second, 1.9 percent short at the worst; with the settling passes as well it keeps 0.1985 to 0.2000, 0.75 percent short at the worst (the game's
/// coupling, a ball, at 1 voxel a second; at 5 to 150 it keeps 0.1988 to 0.1999). What is left is the solver's own softness: a contact that gives way a little.
const TOLERANCE: f64 = 0.0025;
/// What the stranger keeps of its speed when it hits the chain: momentum says one car's mass in five.
const KEPT_ON_THE_CHAIN: f64 = 1.0 / (CARS as f64 + 1.0);

/// A source whose one whole cube has `side` voxels along each edge, at the origin of its body (a cube of 1 is the voxel at the origin).
struct Block(i128);

impl CollisionSource for Block {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let edge = 0..self.0;
        if edge.contains(&voxel.x()) && edge.contains(&voxel.y()) && edge.contains(&voxel.z()) {
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

fn block_mass(side: i128) -> BodyMass {
    let voxels = (0..side).flat_map(|x| {
        (0..side)
            .flat_map(move |y| (0..side).map(move |z| MassMoments::of_voxel([x, y, z], DENSITY)))
    });
    MassMoments::sum(voxels)
        .properties()
        .expect("a cube has a mass")
}

/// A cube of `side` voxels along each edge (up to 32: one brick) with its origin, its lower corner, at `origin`, going at `velocity`.
fn add_block(world: &mut BodyWorld, id: u64, side: i128, origin: [f64; 3], velocity: [f64; 3]) {
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
        .set_brick(BodyId(id), BrickCoordinate::ORIGIN, &mut Block(side))
        .unwrap();
    world.set_mass(BodyId(id), block_mass(side)).unwrap();
}

/// A cube of one voxel with its origin at `origin`, going at `velocity`.
fn add_cube(world: &mut BodyWorld, id: u64, origin: [f64; 3], velocity: [f64; 3]) {
    add_block(world, id, 1, origin, velocity);
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

/// `CARS` cubes end to end along x, at rest, each coupled to the next as `coupling` says.
fn add_chain(world: &mut BodyWorld, coupling: &Coupling) {
    for car in 0..CARS {
        add_cube(world, car as u64 + 1, [car as f64, 10.0, 10.0], [0.0; 3]);
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
}

/// A chain of `CARS` cubes end to end along x, and the stranger coming at its far end from `gap` voxels away at `speed`, in a world made with `settings`.
fn strike_under(coupling: &Coupling, speed: f64, settings: BodySettings, gap: f64) -> Outcome {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    add_chain(&mut world, coupling);
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
fn a_hit_on_the_games_coupling_comes_out_within_a_hundredth_of_the_answer_at_every_moment_of_a_step()
 {
    // The answer is a fifth of the speed, and the game's coupling (a ball) keeps 0.1988 to 0.2000 of it over the 12 moments of a step at 5 to 150 voxels a second, where it kept
    // 0.1986 to 0.1990 with one settling pass: nearer the answer, and no more alike (a spread of 0.0014 against 0.0004), because the settling passes take away what the soft
    // contact lets through and what they leave is the less the longer the piece that meets the touch.
    let coupling = &couplings()[2];
    let mut wrong = Vec::new();
    for speed in [5.0, 20.0, 60.0, 150.0] {
        let kept = kept_over_a_step(coupling, speed, settings_with(3));
        let (low, high) = lowest_and_highest(&kept);
        if (low - KEPT_ON_THE_CHAIN)
            .abs()
            .max((high - KEPT_ON_THE_CHAIN).abs())
            > TOLERANCE
        {
            wrong.push(format!(
                "at {speed} voxels a second the stranger keeps {low:.4} to {high:.4} over 12 moments of a step"
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
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
    // two bodies with no joint square too. With one settling pass the cubes turned at up to 0.01 radians a second at 60 voxels a second, with none of the extra passes either
    // 0.2, and at 0.6 with no cut as well. The world makes the settling passes of a hit for half a second after a touch and one pass after that, and what that leaves is
    // 2e-6 to 4e-6 of the speed as a turn, the push out of the contact that the one pass does not take away: 2e-4 radians a second at 60 voxels a second, which is a tenth
    // of a degree in a second. Made at every step, as the world did before the cost was cut, the passes leave under 5e-15.)
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
            if (kept - 0.5).abs() > TOLERANCE || turn > 1e-5 * speed {
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

/// How far into each other two cubes of one voxel went at the most, at the end of a tick, when one that was `gap` voxels away came at the other (at rest) at `speed` along x.
fn overlap_of_two_cubes(speed: f64, gap: f64, settings: BodySettings) -> f64 {
    let along_x = |world: &BodyWorld, id: u64| {
        world
            .state(BodyId(id))
            .unwrap()
            .position
            .relative_to(WorldPosition::from_voxels([0.0; 3]))[0]
    };
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    add_cube(&mut world, 1, [0.0, 10.0, 10.0], [0.0; 3]);
    add_cube(&mut world, 2, [1.0 + gap, 10.0, 10.0], [-speed, 0.0, 0.0]);
    let mut deepest: f64 = 0.0;
    for _ in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
        world.step();
        deepest = deepest.max(1.0 - (along_x(&world, 2) - along_x(&world, 1)));
    }
    deepest
}

#[test]
fn two_cubes_that_meet_go_hardly_into_each_other_wherever_in_a_step_they_meet() {
    // With the cut: at most 0.0008, 0.0032, 0.011 and 0.028 voxels at 5, 20, 60 and 150 voxels a second, about twice what a cube goes into terrain at the same speeds (below),
    // because the contact between two bodies is half as stiff as one with terrain (Rapier's 30 and 60 hertz).
    let mut wrong = Vec::new();
    for speed in [5.0, 20.0, 60.0, 150.0] {
        let deepest = (0..12)
            .map(|moment| {
                overlap_of_two_cubes(
                    speed,
                    gap_at_phase(speed, 3, f64::from(moment) / 12.0),
                    settings_with(3),
                )
            })
            .fold(0.0, f64::max);
        if deepest > 0.04 {
            wrong.push(format!(
                "{speed} voxels a second: the cubes were {deepest:.4} voxels into each other at the most"
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn without_the_cut_two_cubes_that_meet_go_deep_into_each_other() {
    // The control of the cut for bodies: a step lets two bodies go the whole of its way into each other (0.6 voxels at 60 voxels a second, with the settling passes; the cut is
    // not what makes the speed a hit leaves come out right, now that the settling passes are).
    let settings = BodySettings {
        cut_at_first_touch: false,
        ..settings_with(3)
    };
    let deepest = (0..12)
        .map(|moment| {
            overlap_of_two_cubes(
                60.0,
                gap_at_phase(60.0, 3, f64::from(moment) / 12.0),
                settings,
            )
        })
        .fold(0.0, f64::max);
    assert!(
        deepest > 0.3,
        "with no cut two cubes meeting at 60 voxels a second were only {deepest} voxels into each other"
    );
}

#[test]
fn with_one_settling_pass_a_head_on_hit_turns_the_cars() {
    // The control of the settling passes: with the extra passes and one settling pass a stranger that meets a car squarely turns it at 0.2 to 2.5 ten-thousandths of the
    // closing speed in radians a second (0.01 at 60 voxels a second).
    let settings = BodySettings {
        settling_passes: 1,
        ..settings_with(3)
    };
    for speed in [5.0, 20.0, 60.0] {
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
            worst > 5e-5 * speed,
            "with one settling pass the cubes turned at {worst} radians a second at most at {speed} voxels a second"
        );
    }
}

#[test]
fn without_the_passes_a_hit_on_a_chain_comes_out_short_and_a_head_on_hit_turns_the_cars() {
    // The control of the passes: with the cut and none (nor any settling pass beyond one), the stranger on a chain keeps less than 0.195 at 20 voxels a second (the highest of
    // the moments of a step, 0.1787; with the world's, 0.1998 at the highest and 0.1982 at the lowest), and a stranger that meets a car squarely turns it at over two thousandths of
    // the closing speed in radians a second (0.2 at 60 voxels a second).
    let settings = BodySettings {
        solver: SolverDials::default(),
        settling_passes: 1,
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
        add_chain(&mut world, coupling);
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

/// What a landing came to: the deepest the cube was in the ground at the end of a tick, in voxels, the fastest it went up afterwards, and how fast it turns at the end, in radians
/// a second (a cube that lands squarely has no reason to go up or to turn).
struct Landing {
    deepest: f64,
    up: f64,
    turn: f64,
}

/// A world with flat terrain, every voxel below the height 0, three bricks wide each way.
fn floor_world(settings: BodySettings) -> BodyWorld {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    for x in -1..=1 {
        for z in -1..=1 {
            world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Floor);
        }
    }
    world
}

/// A cube of `side` voxels along each edge that falls onto terrain (no pull, so the speed is the impact's alone) from `gap` voxels above it.
fn land_block(side: i128, speed: f64, gap: f64, settings: BodySettings) -> Landing {
    let mut world = floor_world(settings);
    add_block(&mut world, 1, side, [10.0, gap, 10.0], [0.0, -speed, 0.0]);
    let (mut deepest, mut up): (f64, f64) = (0.0, 0.0);
    // (A voxel contact engages only when the shapes have gone about 0.02 voxels into each other, which takes 0.02 voxels over the speed of seconds more at a gentle landing.)
    for _ in 0..(((gap + 0.03) / speed + 1.0) / TICK).ceil() as usize {
        world.step();
        let state = world.state(BodyId(1)).unwrap();
        let height = state
            .position
            .relative_to(WorldPosition::from_voxels([0.0; 3]))[1];
        deepest = deepest.max(-height);
        up = up.max(state.linear_velocity[1]);
    }
    let spin = world.state(BodyId(1)).unwrap().angular_velocity;
    Landing {
        deepest,
        up,
        turn: (spin[0] * spin[0] + spin[1] * spin[1] + spin[2] * spin[2]).sqrt(),
    }
}

/// A cube of one voxel that lands `phase` of the way through a step: (deepest, up, turn) of [`land_block`].
fn land(speed: f64, phase: f64, settings: BodySettings) -> (f64, f64, f64) {
    let gap = gap_at_phase(speed, settings.substeps, phase);
    let landing = land_block(1, speed, gap, settings);
    (landing.deepest, landing.up, landing.turn)
}

/// The worst landing of a cube of `side` voxels at `speed` over the 24 moments of a tick (the steps a tick is cut into do not matter: the gap is in ticks): the highest of each
/// of the three.
fn worst_landing_over_a_tick(side: i128, speed: f64, settings: BodySettings) -> Landing {
    let mut worst = Landing {
        deepest: 0.0,
        up: 0.0,
        turn: 0.0,
    };
    for moment in 0..24 {
        let gap = gap_at_tick(speed, 3.0 + f64::from(moment) / 24.0);
        let landing = land_block(side, speed, gap, settings);
        worst.deepest = worst.deepest.max(landing.deepest);
        worst.up = worst.up.max(landing.up);
        worst.turn = worst.turn.max(landing.turn);
    }
    worst
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
fn a_cube_that_lands_squarely_neither_goes_up_nor_turns_wherever_in_a_tick_it_lands() {
    // A cube that lands squarely on flat ground has no reason to go up or to turn: both are what the solver leaves. With one settling pass a cube of one voxel left 0.055, 0.22,
    // 0.81 and 3.3 voxels a second up and 0.082, 0.33, 1.2 and 4.8 radians a second (1.1 to 2.2 and 1.6 to 3.2 hundredths of its speed) at 5, 20, 60 and 150 voxels a second, by
    // the moment: more where the piece that meets the touch was shorter. With the world's passes, under a thousandth of the speed for cubes of 1, 4, 16 and 32 voxels (the
    // highest is the cube of 32 at 150 voxels a second, going up at 0.056: the body's own mass, which more passes do not change).
    let mut wrong = Vec::new();
    for side in [1, 4, 16, 32] {
        for speed in [5.0, 20.0, 60.0, 150.0] {
            let worst = worst_landing_over_a_tick(side, speed, settings_with(3));
            if worst.up > 0.001 * speed || worst.turn > 0.001 * speed {
                wrong.push(format!(
                    "a cube of {side} at {speed} voxels a second: up at {:.4} voxels a second and turning at {:.4} radians a second at the most over 24 moments of a tick",
                    worst.up, worst.turn
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn with_one_settling_pass_a_cube_that_lands_squarely_goes_up_and_turns() {
    // The control of the one above: the solver's own single pass (Rapier's default, and the world's before step 4c-5-A) leaves a cube of one voxel going up at over 0.5 and
    // turning at over 1 hundredth of its speed at the worst of the moments, at each of the four speeds (0.011 to 0.022 and 0.016 to 0.032).
    let settings = BodySettings {
        settling_passes: 1,
        ..settings_with(3)
    };
    let mut wrong = Vec::new();
    for speed in [5.0, 20.0, 60.0, 150.0] {
        let worst = worst_landing_over_a_tick(1, speed, settings);
        if worst.up < 0.005 * speed || worst.turn < 0.01 * speed {
            wrong.push(format!(
                "{speed} voxels a second: with one settling pass the cube went up at {:.4} and turned at {:.4} at the most",
                worst.up, worst.turn
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
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

#[test]
fn a_world_in_which_nothing_touches_never_makes_the_passes_of_a_hit() {
    // Two cubes go past each other, half a voxel apart (closing at 40 voxels a second, so the question of a first touch is asked and has the answer none), and a third goes
    // on alone in free space; none is coupled to another. Nothing is about to touch, so no step is cut and no piece is solved as a hit.
    let mut world = free_space(3);
    add_cube(&mut world, 1, [0.0, 10.0, 10.0], [20.0, 0.0, 0.0]);
    add_cube(&mut world, 2, [30.0, 11.5, 10.0], [-20.0, 0.0, 0.0]);
    add_cube(&mut world, 3, [50.0, 30.0, 10.0], [3.0, 7.0, -2.0]);
    for _ in 0..40 {
        world.step();
    }
    assert_eq!(world.cuts_made(), 0, "a step was cut where nothing touched");
    assert_eq!(
        world.hit_pieces_made(),
        0,
        "a piece was solved as a hit where nothing touched"
    );
}

#[test]
fn the_passes_of_a_hit_are_made_for_half_a_second_after_a_touch_and_then_one_pass_again() {
    // A cube of one voxel falls onto terrain at 60 voxels a second, under the game's pull, and then lies on it. The world solves what meets a touch with the settling passes of a
    // hit, for half a second (the push out of the contact takes that long to be gone: `HIT_LASTS` in `first_touch.rs`), and goes back to the one pass that Rapier makes: a cube at
    // rest that went on being solved as a hit would pay for the passes of a hit in every tick. The touch is a twentieth of a second in, so the last tick that has a piece solved
    // as a hit is the tenth.
    let mut world = floor_world(BodySettings::default());
    add_cube(&mut world, 1, [10.0, 2.0, 10.0], [0.0, -60.0, 0.0]);
    let (mut before, mut last_tick_with_a_hit_piece) = (0, None);
    for tick in 0..240 {
        world.step();
        if world.hit_pieces_made() > before {
            before = world.hit_pieces_made();
            last_tick_with_a_hit_piece = Some(tick);
        }
    }
    let last = last_tick_with_a_hit_piece
        .expect("a cube landed at 60 voxels a second and no piece was solved as a hit");
    assert!(
        (8..=12).contains(&last),
        "the last piece that was solved as a hit was in tick {last} of a cube that landed in the first, where half a second later is the tenth"
    );
}

#[test]
fn each_touch_makes_the_next_half_second_a_hit_again() {
    // Two cubes fall onto terrain, the second 0.4 seconds after the first: the world is still solving as a hit when the second touches, and the half second begins again there,
    // so the last piece solved as a hit is in the tick 0.5 seconds after the second touch (the 18th), and not the one 0.5 seconds after the first (the 10th).
    let mut world = floor_world(BodySettings::default());
    add_cube(&mut world, 1, [10.0, 2.0, 10.0], [0.0, -60.0, 0.0]);
    add_cube(
        &mut world,
        2,
        [20.0, 2.0 + 60.0 * 0.4, 20.0],
        [0.0, -60.0, 0.0],
    );
    let (mut before, mut last_tick_with_a_hit_piece) = (0, None);
    for tick in 0..240 {
        world.step();
        if world.hit_pieces_made() > before {
            before = world.hit_pieces_made();
            last_tick_with_a_hit_piece = Some(tick);
        }
    }
    let last =
        last_tick_with_a_hit_piece.expect("two cubes landed and no piece was solved as a hit");
    assert!(
        (16..=20).contains(&last),
        "the last piece that was solved as a hit was in tick {last}, where half a second after the second touch is the 18th"
    );
}

#[test]
fn a_slow_touch_is_a_hit_whatever_number_of_steps_a_tick_is_cut_into() {
    // The world looks for a touch at each step, and a step is as short as the fastest body anywhere in the world makes it (up to 24 to a tick). A hit at 2 voxels a second must be
    // found, and made the passes of a hit for, whether a tick is cut into 3 steps or 24: asked "can they cross the solver's margin in this step?" a step of 2 milliseconds could not
    // see anything slower than 9.6 voxels a second, so a landing at 5 was solved with one pass in a world where something else went fast. (And the floor is not what a tick can take
    // across the margin, 0.4 voxels a second, either: a landing at 0.2 left a bounce and a turn of 0.05, which the test that follows the next ones asks about.)
    for substeps in [3, 6, 12, 24] {
        for speed in [0.005, 0.05, 0.2, 0.5, 2.0, 5.0] {
            let mut world = floor_world(settings_with(substeps));
            add_cube(&mut world, 1, [10.0, 0.5 * speed, 10.0], [0.0, -speed, 0.0]);
            for _ in 0..40 {
                world.step();
            }
            assert!(
                world.hit_pieces_made() > 0,
                "a cube that landed at {speed} voxels a second in a world cut into {substeps} steps a tick was not solved as a hit"
            );
        }
    }
}

#[test]
fn with_the_passes_of_a_hit_between_hits_as_well_a_head_on_hit_turns_neither_cube() {
    // The control of the cost cut: the world as it was before it, with the settling passes of a hit made at every step, leaves a head-on hit exactly square (under 1e-12 of
    // the speed), and the world that makes them for half a second after a touch leaves 2e-6 to 4e-6 of the speed: what the cut costs, a push out that the one pass does not take
    // away, which is why the world makes the passes of a hit for as long as that lasts.
    let settings = BodySettings {
        settling_passes_between_hits: BodySettings::default().settling_passes,
        ..settings_with(3)
    };
    for speed in [5.0, 20.0, 60.0] {
        let gap = gap_at_phase(speed, 3, 0.4);
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
        add_cube(&mut world, 1, [0.0, 10.0, 10.0], [0.0; 3]);
        add_cube(&mut world, 2, [1.0 + gap, 10.0, 10.0], [-speed, 0.0, 0.0]);
        for _ in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
            world.step();
        }
        let turn = spin(&world, 1).max(spin(&world, 2));
        assert!(
            turn < 1e-12 * speed,
            "with the passes of a hit at every step the cubes turned at {turn} radians a second at {speed} voxels a second"
        );
    }
}

#[test]
fn shapes_that_touch_and_go_against_each_other_are_solved_as_a_hit_for_as_long_as_they_do() {
    // A cube lies on terrain and is given a sideways speed of 5 voxels a second: it slides, friction slows it, and it stops. A contact that is there is no first touch, but a cube
    // that slides (or rocks onto another leg, or is swung into the car it is joined to) is a hit on what it touches as much, and the world solves each piece as a hit while the two go
    // against each other, and for half a second after. (Slower than a millimetre a second they are at rest.)
    let mut world = floor_world(BodySettings::default());
    add_cube(&mut world, 1, [10.0, 0.0, 10.0], [0.0; 3]);
    for _ in 0..40 {
        world.step();
    }
    let at_rest = world.hit_pieces_made();
    for _ in 0..40 {
        world.step();
    }
    assert_eq!(
        world.hit_pieces_made(),
        at_rest,
        "a cube at rest on the ground was solved as a hit"
    );
    let mut state = world.state(BodyId(1)).unwrap();
    state.linear_velocity = [5.0, 0.0, 0.0];
    world.set_state(BodyId(1), state).unwrap();
    let (mut before, mut sliding_ticks_that_were_hits) = (world.hit_pieces_made(), 0);
    let (mut stopped_at, mut last_hit_at) = (None, None);
    for tick in 0..120 {
        world.step();
        let speed = velocity(&world, 1)[0].abs();
        if stopped_at.is_none() && speed < 0.001 {
            stopped_at = Some(tick);
        }
        if world.hit_pieces_made() > before {
            before = world.hit_pieces_made();
            last_hit_at = Some(tick);
            if stopped_at.is_none() {
                sliding_ticks_that_were_hits += 1;
            }
        }
    }
    let (stopped_at, last_hit_at) = (
        stopped_at.expect("the cube went on sliding for six seconds"),
        last_hit_at.expect("a cube slid on the ground and no piece was solved as a hit"),
    );
    assert_eq!(
        sliding_ticks_that_were_hits, stopped_at,
        "the cube slid for {stopped_at} ticks and was solved as a hit in {sliding_ticks_that_were_hits} of them"
    );
    assert!(
        (stopped_at + 8..=stopped_at + 12).contains(&last_hit_at),
        "the cube went slower than 0.001 voxels a second in tick {stopped_at} and the last piece solved as a hit was in tick {last_hit_at}, where half a second later is {}",
        stopped_at + 10
    );
}

/// A platform of 8 by 1 by 8 voxels on four legs of 1 by 2 by 1 at its corners (16 points of contact in four clusters), its origin at its lower corner.
struct Gear;

impl CollisionSource for Gear {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let (x, y, z) = (voxel.x(), voxel.y(), voxel.z());
        let on_a_corner = (x == 0 || x == 7) && (z == 0 || z == 7);
        let platform = y == 2 && (0..8).contains(&x) && (0..8).contains(&z);
        if platform || ((0..2).contains(&y) && on_a_corner) {
            Collision::Full
        } else {
            Collision::Empty
        }
    }
}

fn gear_mass() -> BodyMass {
    let voxels = (0..8).flat_map(|x| {
        (0..3).flat_map(move |y| {
            (0..8)
                .filter(move |&z| {
                    Gear.collision_at(VoxelCoordinate::new(x, y, z)) == Collision::Full
                })
                .map(move |z| MassMoments::of_voxel([x, y, z], DENSITY))
        })
    });
    MassMoments::sum(voxels)
        .properties()
        .expect("a platform has a mass")
}

/// The state of every body at the end of each tick of a run.
type Run = Vec<Vec<BodyState>>;

/// A platform on four legs, turned a tenth of a radian about z so that its legs on one side meet the ground first and it then rocks onto the others, lands on terrain at `speed`
/// (no pull) with its lowest point `gap` voxels over it; its state at the end of each of the ticks until a second after it comes down.
fn tilted_gear_landing(speed: f64, gap: f64, settings: BodySettings) -> Run {
    // (sin and cos of 0.1 and 0.05, which do not need the standard functions.)
    let (sine, half_sine, half_cosine) = (
        0.099_833_416_646_828_15,
        0.049_979_169_270_678_33,
        0.998_750_260_394_966,
    );
    let mut world = floor_world(settings);
    let state = BodyState {
        // The leg at x = 8 is the lowest point of the platform turned by -0.1 radian: 8 sin 0.1 under its origin.
        position: WorldPosition::from_voxels([10.0, gap + 8.0 * sine, 10.0]),
        orientation: Orientation::from_components([0.0, 0.0, -half_sine, half_cosine]).unwrap(),
        linear_velocity: [0.0, -speed, 0.0],
        angular_velocity: [0.0; 3],
    };
    world.add_body(BodyId(1), state, BodyKind::Dynamic).unwrap();
    world
        .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut Gear)
        .unwrap();
    world.set_mass(BodyId(1), gear_mass()).unwrap();
    (0..((gap / speed + 1.0) / TICK).ceil() as usize)
        .map(|_| {
            world.step();
            vec![world.state(BodyId(1)).unwrap()]
        })
        .collect()
}

/// The most that two runs of the same bodies are apart at the end of any tick: in position (voxels), in speed (voxels a second) and in turn (radians a second).
fn most_apart_over(first: &Run, second: &Run) -> (f64, f64, f64) {
    let (mut position, mut speed, mut turn): (f64, f64, f64) = (0.0, 0.0, 0.0);
    let length = |vector: [f64; 3]| vector.iter().map(|part| part * part).sum::<f64>().sqrt();
    let minus = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    for (these, those) in first.iter().zip(second) {
        for (one, other) in these.iter().zip(those) {
            position = position.max(length(one.position.relative_to(other.position)));
            speed = speed.max(length(minus(one.linear_velocity, other.linear_velocity)));
            turn = turn.max(length(minus(one.angular_velocity, other.angular_velocity)));
        }
    }
    (position, speed, turn)
}

#[test]
fn a_platform_that_lands_tilted_and_rocks_onto_its_other_legs_does_what_it_did_before_the_cost_was_cut()
 {
    // The legs on one side meet the ground first (a touch of the platform and the ground, found and solved as a hit), and the platform then rocks onto the others: the same two
    // shapes, with a new point of contact, which is no first touch. The world as it was before the cost of the settling passes was cut made them at every step; the world that makes
    // them while a touch is found, or shapes that touch go against each other, and for half a second after, must come to the same: the two runs are less than a thousandth of a voxel
    // apart in position, and a thousandth of the speed in speed and in turn, at every tick. (With one pass in a hit as well they are not: the control, below.)
    let always = BodySettings {
        settling_passes_between_hits: BodySettings::default().settling_passes,
        ..settings_with(3)
    };
    let mut wrong = Vec::new();
    for speed in [5.0, 20.0, 60.0] {
        let (mut position, mut apart_in_speed, mut turn): (f64, f64, f64) = (0.0, 0.0, 0.0);
        for moment in 0..12 {
            let gap = gap_at_tick(speed, 3.0 + f64::from(moment) / 12.0);
            let now = tilted_gear_landing(speed, gap, settings_with(3));
            let before = tilted_gear_landing(speed, gap, always);
            let apart = most_apart_over(&now, &before);
            position = position.max(apart.0);
            apart_in_speed = apart_in_speed.max(apart.1);
            turn = turn.max(apart.2);
        }
        if position > 0.001 || apart_in_speed > 0.001 * speed || turn > 0.001 * speed {
            wrong.push(format!(
                "{speed} voxels a second: the runs were {position:.5} voxels, {apart_in_speed:.4} voxels a second and {turn:.4} radians a second apart at the most"
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn with_one_pass_in_a_hit_as_well_a_platform_that_lands_tilted_comes_to_something_else() {
    // The control of the one above: with one settling pass everywhere (the world before step 4c-5-A) the same landing is not the same run.
    let always = BodySettings {
        settling_passes_between_hits: BodySettings::default().settling_passes,
        ..settings_with(3)
    };
    let one_pass = BodySettings {
        settling_passes: 1,
        settling_passes_between_hits: 1,
        ..settings_with(3)
    };
    let speed = 60.0;
    let mut most_apart: f64 = 0.0;
    for moment in 0..12 {
        let gap = gap_at_tick(speed, 3.0 + f64::from(moment) / 12.0);
        let apart = most_apart_over(
            &tilted_gear_landing(speed, gap, one_pass),
            &tilted_gear_landing(speed, gap, always),
        );
        most_apart = most_apart.max(apart.1).max(apart.2);
    }
    assert!(
        most_apart > 0.01 * speed,
        "with one pass everywhere the tilted landing was only {most_apart} apart in speed or turn from the one with the passes of a hit at every step"
    );
}

/// The state of each of the cars of a chain of four, which is set swinging (the first and third about z at 1 radian a second, the second and fourth the other way), at the end
/// of each of `ticks` ticks, in free space, in a world made with `settings`.
fn swinging_chain(settings: BodySettings, ticks: usize) -> Run {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    add_chain(&mut world, &couplings()[2]);
    // (Two ticks first: the broad phase knows a body only after a step, and a touch that begins in the first step of a world is not seen.)
    world.step();
    world.step();
    for car in 0..CARS as u64 {
        let mut state = world.state(BodyId(car + 1)).unwrap();
        state.angular_velocity = [0.0, 0.0, if car % 2 == 0 { 1.0 } else { -1.0 }];
        world.set_state(BodyId(car + 1), state).unwrap();
    }
    (0..ticks)
        .map(|_| {
            world.step();
            (1..=CARS as u64)
                .map(|car| world.state(BodyId(car)).unwrap())
                .collect()
        })
        .collect()
}

#[test]
fn a_chain_that_swings_into_its_own_cars_does_what_it_did_before_the_cost_was_cut() {
    // A chain of cars that swings into its own cars is the mechanism that promise (a) of the joint plan is about (`games/voxunami_cave/tests/group_off_a_rail.rs`: a chain swung at
    // 70 and at 140 kilometres comes to the same to 1e-7), and its hits are chaotic: with the passes of a hit made only while the question of a first touch finds a hit, the two
    // chains differed by 0.018, a rounding that puts a speed on one side of the floor in one run and on the other in the other being made large by the impacts of the swing. A world
    // with a coupling in it is solved as a hit all the time (the test of that is above), so the swing is what it was with the passes of a hit made at every step, to the bit.
    let always = BodySettings {
        settling_passes_between_hits: BodySettings::default().settling_passes,
        ..settings_with(3)
    };
    let (now, before) = (
        swinging_chain(settings_with(3), 200),
        swinging_chain(always, 200),
    );
    let (position, speed, turn) = most_apart_over(&now, &before);
    assert_eq!(
        (position, speed, turn),
        (0.0, 0.0, 0.0),
        "the swung chain was not what it was with the passes of a hit at every step, to the bit (voxels, voxels a second, radians a second)"
    );
}

#[test]
fn a_cube_that_rests_on_a_platform_that_carries_it_is_not_solved_as_a_hit() {
    // A platform of one brick goes at 30 voxels a second (a kinematic body, which carries what rests on it, as a planet on a rail carries the ships on its ground) and a cube lies on
    // it going the same way: the two touch and do not go against each other, whatever either does in the world, so after the cube has come to rest on it no piece is solved as a
    // hit. (The speeds that count are the speeds against each other, not the speeds in the world, which is the cost of a ship at rest on a planet that goes round.)
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
    let platform = BodyState {
        position: WorldPosition::from_voxels([0.0, -32.0, 0.0]),
        orientation: Orientation::IDENTITY,
        linear_velocity: [30.0, 0.0, 0.0],
        angular_velocity: [0.0; 3],
    };
    world
        .add_body(BodyId(1), platform, BodyKind::Kinematic)
        .unwrap();
    world
        .set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut Block(32))
        .unwrap();
    world.set_mass(BodyId(1), block_mass(32)).unwrap();
    add_cube(&mut world, 2, [10.0, 0.0, 10.0], [30.0, 0.0, 0.0]);
    for _ in 0..60 {
        world.step();
    }
    let at_rest = world.hit_pieces_made();
    for _ in 0..60 {
        world.step();
    }
    assert_eq!(
        world.hit_pieces_made(),
        at_rest,
        "a cube that rests on a platform that carries it was solved as a hit"
    );
    // (It was carried: it is still on the platform, going at the platform's speed.)
    let cube = world.state(BodyId(2)).unwrap();
    assert!(
        (cube.linear_velocity[0] - 30.0).abs() < 1e-3,
        "the cube went at {} voxels a second on a platform that goes at 30",
        cube.linear_velocity[0]
    );
}

#[test]
fn a_cube_that_is_sent_into_a_cube_it_touches_is_a_hit_from_the_first_piece() {
    // Two cubes lie with their faces together, with no gap, and one is given 20 voxels a second into the other. There is no first touch to find, for they touch now (a contact is
    // there, and the cut has nothing to cut at: no step was cut): the world finds the contact going against the other from the first piece, and solves the hit as a hit. The cubes
    // turn at 3e-6 of the speed afterwards (2.4e-4 with one pass everywhere, 1e-25 with the passes of a hit at every step).
    let mut world = free_space(3);
    add_cube(&mut world, 1, [0.0, 10.0, 10.0], [0.0; 3]);
    add_cube(&mut world, 2, [1.0, 10.0, 10.0], [0.0; 3]);
    // (Two ticks first: the broad phase knows a body only after a step.)
    world.step();
    world.step();
    let mut state = world.state(BodyId(2)).unwrap();
    state.linear_velocity = [-20.0, 0.0, 0.0];
    world.set_state(BodyId(2), state).unwrap();
    for _ in 0..40 {
        world.step();
    }
    let turn = spin(&world, 1).max(spin(&world, 2));
    assert!(
        turn < 1e-5 * 20.0,
        "two cubes that touched, one sent into the other at 20 voxels a second, turned at {turn} radians a second"
    );
    assert!(world.hit_pieces_made() > 0, "no piece was solved as a hit");
}

#[test]
fn a_world_that_has_a_coupling_in_it_makes_the_passes_of_a_hit_all_the_time() {
    // The hits of a mechanism are chaotic: a chain swung at 70 and at 140 kilometres (promise (a) of the joint plan, `games/voxunami_cave/tests/group_off_a_rail.rs`) came to two swings
    // 0.018 apart, where without the cost cut they agree to 5e-10, when the passes of a hit were made only some of the time: a rounding that puts a speed on one side of the floor
    // of a hit in one run and on the other in the other run is made large by the impacts of the swing. So a world with a coupling in it solves every piece as a hit, and one with
    // none goes back to one pass when nothing touches.
    let mut world = free_space(3);
    add_chain(&mut world, &couplings()[2]);
    for _ in 0..40 {
        world.step();
    }
    // (A tick of nothing that is fast is 3 steps, and none was cut.)
    assert_eq!(world.cuts_made(), 0);
    assert_eq!(
        world.hit_pieces_made(),
        40 * 3,
        "a chain at rest was not solved as a hit in every piece"
    );
    for car in 0..CARS as u64 - 1 {
        world.decouple(CouplingId(car + 1)).unwrap();
    }
    let before = world.hit_pieces_made();
    for _ in 0..40 {
        world.step();
    }
    assert_eq!(
        world.hit_pieces_made(),
        before,
        "cars that are not coupled any more, at rest, were solved as a hit"
    );
}

#[test]
fn a_touch_that_is_too_slow_for_a_step_to_take_across_the_margin_is_a_hit_and_is_not_cut_at() {
    // A cube lands on terrain at 0.8 voxels a second in a world cut into 3 steps a tick: a step cannot take it across the solver's margin of 0.02 voxels (it takes 1.2 voxels a second
    // to), and it is far over the floor of a hit: the world solves the landing as a hit and does not cut a step at the touch, as it never did at one that slow. (Cut at them, a
    // chain swung at 70 and at 140 kilometres came to two swings that differ by 0.5, where they agree to 5e-10: a cut is a break in the solving that a swinging chain makes much of.)
    let mut world = floor_world(settings_with(3));
    add_cube(&mut world, 1, [10.0, 0.3, 10.0], [0.0, -0.8, 0.0]);
    for _ in 0..40 {
        world.step();
    }
    assert_eq!(
        world.cuts_made(),
        0,
        "a step was cut at a touch of 0.8 voxels a second"
    );
    assert!(
        world.hit_pieces_made() > 0,
        "a touch of 0.8 voxels a second was not solved as a hit"
    );
    // (At 5 voxels a second a step can, and the step is cut.)
    let mut world = floor_world(settings_with(3));
    add_cube(&mut world, 1, [10.0, 0.3, 10.0], [0.0, -5.0, 0.0]);
    for _ in 0..10 {
        world.step();
    }
    assert!(
        world.cuts_made() > 0,
        "a touch of 5 voxels a second was not cut at"
    );
}

#[test]
fn a_cube_that_lands_gently_neither_goes_up_nor_turns_whatever_number_of_steps_a_tick_is_cut_into()
{
    // A gentle touchdown is the commonest landing there is. A voxel contact engages only when the shapes have gone about 0.02 voxels into each other, which a cube that comes
    // down slowly takes 0.02 over its speed in seconds to do (a second at 0.02 voxels a second), unresisted; the contact then pushes it out, and with one settling pass that leaves a
    // cube that came down at 0.002 to 1 voxels a second going up at 0.02 to 0.09 voxels a second and turning at 0.02 to 0.09 radians a second whatever its speed, where the world
    // that makes the passes of a hit at every step leaves nothing. So a touch, and shapes that touch with no contact yet and go against each other, are a hit at any speed that is a
    // speed: the floor is a millimetre a second.
    let mut wrong = Vec::new();
    for substeps in [3, 24] {
        for speed in [0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0] {
            let (mut up, mut turn): (f64, f64) = (0.0, 0.0);
            for moment in 0..6 {
                let gap = gap_at_tick(speed, 3.0 + f64::from(moment) / 6.0);
                let landing = land_block(1, speed, gap, settings_with(substeps));
                up = up.max(landing.up);
                turn = turn.max(landing.turn);
            }
            if up > 0.002 || turn > 0.002 {
                wrong.push(format!(
                    "{speed} voxels a second, {substeps} steps a tick: up at {up:.5} voxels a second and turning at {turn:.5} radians a second at the most over 6 moments of a tick"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

/// The passes over the constraints that the solver is told to make for a body.
fn extra_passes_of(world: &BodyWorld, id: u64) -> usize {
    world
        .solver_dials_in_force(BodyId(id))
        .expect("a body")
        .extra_passes
}

fn couple_cubes(world: &mut BodyWorld, coupling: u64, first: u64, second: u64) {
    world
        .couple(
            CouplingId(coupling),
            BodyId(first),
            BodyId(second),
            &[pivot(0.5, 0.5)],
            CouplingOptions::default(),
        )
        .unwrap();
}

#[test]
fn the_extra_passes_are_made_in_a_hit_and_not_between_hits() {
    // The 15 extra passes (`BodySettings::solver`) are what an impact needs (an impact through a chain of joints, or the landing of a body of a thousand cubes: a cube of 32 voxels that
    // lands at 20 voxels a second goes up at 0.77 without them); a body at rest has no speed of approach for them to take away, and they are paid for in every piece by every body that is awake
    // and touching. So they are made as the settling passes are: in a piece that is solved as a hit, which is from a touch for half a second.
    let wanted = BodySettings::default().solver.extra_passes;
    assert!(wanted > 0, "the world gives no extra passes in a hit");
    assert_eq!(BodySettings::default().solver_between_hits.extra_passes, 0);
    let mut world = floor_world(settings_with(3));
    add_cube(&mut world, 1, [10.0, 4.0, 10.0], [0.0, -10.0, 0.0]);
    add_cube(&mut world, 2, [30.0, 40.0, 10.0], [0.0; 3]);
    assert_eq!(extra_passes_of(&world, 1), 0, "before the first step");
    let mut in_a_hit = Vec::new();
    for _ in 0..80 {
        world.step();
        in_a_hit.push((extra_passes_of(&world, 1), extra_passes_of(&world, 2)));
    }
    // (The cube that falls from 4 voxels at 10 voxels a second is down in under half a second and is a hit from the touch for half a second.)
    let first_hit = in_a_hit
        .iter()
        .position(|&(landing, _)| landing > 0)
        .expect("the passes of a hit were never made for the cube that landed");
    assert!(first_hit < 10, "the hit began at tick {first_hit}");
    assert!(
        in_a_hit[first_hit..first_hit + 8]
            .iter()
            .all(|&(landing, _)| landing == wanted),
        "the passes of a hit were not kept for the half second after the touch"
    );
    assert_eq!(
        in_a_hit.last().unwrap().0,
        0,
        "the cube at rest went on being given the extra passes"
    );
    // The same for a body that nothing is near: it is given them while the world is solved as a hit, which a world is in all of it.
    assert!(
        in_a_hit[first_hit..first_hit + 8]
            .iter()
            .all(|&(_, far)| far == wanted),
        "a body that is not in the hit is solved in a world that is"
    );
}

#[test]
fn a_world_that_has_a_coupling_in_it_gives_every_body_the_extra_passes_all_the_time() {
    // As the settling passes are made all the time in a world with a coupling (the test above of them), so are the extra: the chain's impacts are what they are for, and a coupling
    // that came to them only some of the time swings differently (promise (a) of the joint plan).
    let wanted = BodySettings::default().solver.extra_passes;
    let mut world = free_space(3);
    for id in 1..=3 {
        add_cube(&mut world, id, [id as f64 * 5.0, 10.0, 10.0], [0.0; 3]);
    }
    couple_cubes(&mut world, 1, 1, 2);
    for _ in 0..40 {
        world.step();
    }
    for id in 1..=3 {
        assert_eq!(
            extra_passes_of(&world, id),
            wanted,
            "body {id}, in a world with a coupling"
        );
    }
    world.decouple(CouplingId(1)).unwrap();
    world.step();
    for id in 1..=3 {
        assert_eq!(
            extra_passes_of(&world, id),
            0,
            "body {id}, when none is coupled and nothing is near"
        );
    }
}

#[test]
fn a_body_that_is_added_while_the_world_is_solved_as_a_hit_is_given_the_extra_passes() {
    // The world tells every body what to do when it goes into a hit or out of one, so a body that is made after that has to be told as well: the hit lasts half a second, and the
    // body is in the next piece.
    let wanted = BodySettings::default().solver.extra_passes;
    let mut world = floor_world(settings_with(3));
    add_cube(&mut world, 1, [10.0, 2.0, 10.0], [0.0, -10.0, 0.0]);
    let mut ticks = 0;
    while extra_passes_of(&world, 1) == 0 {
        world.step();
        ticks += 1;
        assert!(ticks < 30, "the cube never landed");
    }
    add_cube(&mut world, 2, [30.0, 10.0, 10.0], [0.0; 3]);
    assert_eq!(
        extra_passes_of(&world, 2),
        wanted,
        "a body added during a hit"
    );
    // And when the hit is over, it is told so, with the others.
    for _ in 0..40 {
        world.step();
    }
    assert_eq!(extra_passes_of(&world, 1), 0);
    assert_eq!(
        extra_passes_of(&world, 2),
        0,
        "the body that was added during a hit, after it"
    );
}

#[test]
fn the_extra_substeps_of_the_world_are_made_in_a_hit_and_not_between_hits_as_the_extra_passes_are()
{
    // (The default has none; a world that is given some has them where it has the passes.)
    let settings = BodySettings {
        solver: SolverDials {
            extra_substeps: 2,
            extra_passes: 0,
        },
        solver_between_hits: SolverDials {
            extra_substeps: 1,
            extra_passes: 0,
        },
        ..settings_with(3)
    };
    let substeps = |world: &BodyWorld| {
        world
            .solver_dials_in_force(BodyId(1))
            .unwrap()
            .extra_substeps
    };
    let mut alone = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    add_cube(&mut alone, 1, [5.0, 10.0, 10.0], [0.0; 3]);
    alone.step();
    assert_eq!(substeps(&alone), 1, "between hits");
    let mut coupled = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    add_cube(&mut coupled, 1, [5.0, 10.0, 10.0], [0.0; 3]);
    add_cube(&mut coupled, 2, [10.0, 10.0, 10.0], [0.0; 3]);
    couple_cubes(&mut coupled, 1, 1, 2);
    coupled.step();
    assert_eq!(substeps(&coupled), 2, "in a hit");
}

#[test]
fn what_a_body_is_asked_for_is_in_addition_where_it_is_more_than_the_world_gives() {
    let wanted = BodySettings::default().solver.extra_passes;
    let mut world = free_space(3);
    for id in 1..=3 {
        add_cube(&mut world, id, [id as f64 * 5.0, 10.0, 10.0], [0.0; 3]);
    }
    world
        .set_solver_dials(
            BodyId(1),
            SolverDials {
                extra_substeps: 2,
                extra_passes: wanted + 16,
            },
        )
        .unwrap();
    world
        .set_solver_dials(
            BodyId(2),
            SolverDials {
                extra_substeps: 0,
                extra_passes: 3,
            },
        )
        .unwrap();
    world.step();
    assert_eq!(
        extra_passes_of(&world, 1),
        wanted + 16,
        "more than the world gives, between hits"
    );
    assert_eq!(
        extra_passes_of(&world, 2),
        3,
        "less than the world gives in a hit, between hits"
    );
    assert_eq!(extra_passes_of(&world, 3), 0);
    assert_eq!(
        world
            .solver_dials_in_force(BodyId(1))
            .unwrap()
            .extra_substeps,
        2
    );
    couple_cubes(&mut world, 1, 2, 3);
    world.step();
    assert_eq!(
        extra_passes_of(&world, 1),
        wanted + 16,
        "more than the world gives, in a hit"
    );
    assert_eq!(
        extra_passes_of(&world, 2),
        wanted,
        "less than the world gives, in a hit"
    );
    assert_eq!(extra_passes_of(&world, 3), wanted);
}

/// `count` cubes of one voxel on each other on terrain, under the game's pull, kept awake by a force too small to see: the fastest any goes (voxels a second) over the last
/// 100 of 200 ticks, and how many pieces were solved as a hit over those.
fn stack_at_rest(count: u64, settings: BodySettings) -> (f64, u64) {
    let mut world = floor_world(settings);
    for level in 0..count {
        add_cube(&mut world, level + 1, [10.0, level as f64, 10.0], [0.0; 3]);
    }
    let (mut fastest, mut hits_before) = (0.0f64, 0);
    for tick in 0..200 {
        if tick == 100 {
            hits_before = world.hit_pieces_made();
        }
        for id in 1..=count {
            world
                .apply_force(BodyId(id), [1e-9, 0.0, 0.0], None)
                .unwrap();
        }
        world.step();
        if tick >= 100 {
            for id in 1..=count {
                let [x, y, z] = velocity(&world, id);
                fastest = fastest.max(x.abs()).max(y.abs()).max(z.abs());
            }
        }
    }
    (fastest, world.hit_pieces_made() - hits_before)
}

#[test]
fn a_stack_of_cubes_rests_as_still_with_the_extra_passes_made_in_a_hit_only_and_is_no_hit() {
    // What the 15 extra passes did for a stack of cubes at rest: with none at all (not made in a hit either) a stack of five went at 2.9e-4 voxels a second, of ten 8.7e-4
    // (`engine_probe restnoise`, with `NO_PASSES=1`), over the floor of a hit (`HIT_SPEED_FLOOR`), so that it was solved as a hit most of the time. Made in the hit that the stack's
    // own settling is (the cubes touch from the first step, and a hit lasts half a second), they leave it as still as made in every piece: measured for 1 to 9 cubes, over the last
    // 100 of 200 ticks, 7e-16, 2e-14, 7e-14, 1e-12, 5e-12, 3e-11, 2e-8, 2e-6 and 4e-5 voxels a second (with them in every piece: 8e-16 to 7e-5), and no piece solved as a hit in them.
    // (A stack of ten goes at 2.8e-4 either way, and is a hit in 42 of the 300 pieces.)
    let settings = BodySettings::default();
    for count in 1..=9 {
        let (fastest, hit_pieces) = stack_at_rest(count, settings);
        assert!(
            fastest < 1e-4,
            "a stack of {count} at rest went at {fastest:e} voxels a second"
        );
        assert_eq!(
            hit_pieces, 0,
            "a stack of {count} at rest was solved as a hit {hit_pieces} times in 100 ticks"
        );
    }
}
