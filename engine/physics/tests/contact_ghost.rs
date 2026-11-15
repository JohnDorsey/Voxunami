//! The contact ghost (step 4c-5-2 of `docs/decisions/joints-and-cars.md`; the study is sections 16 to 18 of `docs/history/loop-probe.md`): **what a mechanism does never changes by a
//! bit because something is near it, only because something touches it.**
//!
//! The scene is `settled_hit.rs`'s: a chain of four cubes, coupled to each other, and a stranger cube that comes along -x at its far end. Here the chain is a world of its own, as the
//! game's frame of a group that is alone is (the truth of the mechanism), and the stranger is in a world of its own too; neither knows of the other. A [`ContactGhost`] holds a copy of
//! both in two worlds of its own, one in which the two sides touch each other and one in which they pass through each other, steps both the same way, and gives back, when the two
//! sides touched, what the contacts did (the difference of the two worlds) to each body of each side: that is added to the body's own step. Where nothing touches, nothing is given
//! back, so what each world did on its own is what it did, to the bit.

mod block_bodies;

use block_bodies::{
    CARS, Coupling, STRANGER, TICK, add_chain, add_cube, couplings, free_space, settings_with,
    velocity,
};
use physics::{BodyId, BodySettings, BodyWorld, ContactGhost, CouplingId, Side};
use voxel_space::BrickCoordinate;

const CHAIN: Side = Side(0);
const STRANGERS: Side = Side(1);

/// What the stranger keeps of its speed on a chain of four when momentum alone says what it keeps (`settled_hit.rs`: a fifth), and how far from it the ghost may be: the same as
/// the one world is allowed there. (Measured, 2026-11-15: the ghost keeps 0.1997 to 0.2003 over 12 moments of a tick at 5 to 300 voxels a second, on the four couplings.)
const KEPT_ON_THE_CHAIN: f64 = 1.0 / (CARS as f64 + 1.0);
const TOLERANCE: f64 = 0.0025;

/// A chain in a world of its own, a stranger in one of its own, and the ghost between them.
struct Scene {
    chain: BodyWorld,
    stranger: BodyWorld,
    ghost: ContactGhost,
    /// How many ticks the sides touched in.
    touching_ticks: usize,
    /// A force on the stranger in every tick (an engine), in voxel masses times voxels a second a second.
    push: [f64; 3],
}

fn chain_world(coupling: &Coupling, settings: BodySettings) -> BodyWorld {
    let mut chain = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    add_chain(&mut chain, coupling);
    chain
}

fn stranger_world(origin: [f64; 3], velocity: [f64; 3], settings: BodySettings) -> BodyWorld {
    let mut stranger = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    add_cube(&mut stranger, STRANGER, origin, velocity);
    stranger
}

impl Scene {
    /// A chain along x from the origin and a stranger at `origin` going at `velocity`.
    fn new(
        coupling: &Coupling,
        origin: [f64; 3],
        velocity: [f64; 3],
        settings: BodySettings,
    ) -> Scene {
        let chain = chain_world(coupling, settings);
        let stranger = stranger_world(origin, velocity, settings);
        let mut ghost = ContactGhost::new(BrickCoordinate::ORIGIN, settings);
        for car in 1..=CARS as u64 {
            let state = chain.state(BodyId(car)).unwrap();
            ghost
                .add_body_from(CHAIN, &chain, BodyId(car), state)
                .unwrap();
        }
        for car in 1..CARS as u64 {
            ghost.add_coupling_from(&chain, CouplingId(car)).unwrap();
        }
        let state = stranger.state(BodyId(STRANGER)).unwrap();
        ghost
            .add_body_from(STRANGERS, &stranger, BodyId(STRANGER), state)
            .unwrap();
        Scene {
            chain,
            stranger,
            ghost,
            touching_ticks: 0,
            push: [0.0; 3],
        }
    }

    /// The stranger comes along -x at `speed` from `gap` voxels beyond the far end of the chain.
    fn striking(coupling: &Coupling, speed: f64, gap: f64, settings: BodySettings) -> Scene {
        Scene::new(
            coupling,
            [CARS as f64 + gap, 10.0, 10.0],
            [-speed, 0.0, 0.0],
            settings,
        )
    }

    /// One tick of the server: the ghost is told where everything is, steps as the chain's world does (its own steps: the sides are stepped alike, the ghost like the group), then
    /// each world takes its own step, and what the contacts did, if the sides touched, is added to it.
    fn tick(&mut self) {
        if self.push != [0.0; 3] {
            self.stranger
                .apply_force(BodyId(STRANGER), self.push, None)
                .unwrap();
        }
        for car in 1..=CARS as u64 {
            let state = self.chain.state(BodyId(car)).unwrap();
            self.ghost.refresh(BodyId(car), &self.chain, state).unwrap();
        }
        let state = self.stranger.state(BodyId(STRANGER)).unwrap();
        self.ghost
            .refresh(BodyId(STRANGER), &self.stranger, state)
            .unwrap();
        self.ghost.step(self.chain.steps_this_tick());
        self.chain.step();
        self.stranger.step();
        if self.ghost.touched() {
            self.touching_ticks += 1;
        }
        for car in 1..=CARS as u64 {
            if let Some(correction) = self.ghost.correction(BodyId(car)) {
                self.chain
                    .apply_correction(BodyId(car), &correction)
                    .unwrap();
            }
        }
        if let Some(correction) = self.ghost.correction(BodyId(STRANGER)) {
            self.stranger
                .apply_correction(BodyId(STRANGER), &correction)
                .unwrap();
        }
    }

    fn ticks(&mut self, count: usize) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// The momentum along x of everything, in parts of the stranger's at the start (`speed`, a cube of one voxel).
    fn momentum_along_x(&self) -> f64 {
        (1..=CARS as u64)
            .map(|car| velocity(&self.chain, car)[0])
            .sum::<f64>()
            + velocity(&self.stranger, STRANGER)[0]
    }
}

#[test]
fn a_stranger_that_touches_nothing_changes_no_bit_of_the_chain_or_of_itself() {
    // Near and far, at rest and going past, whatever the coupling (in two the cars of the chain meet each other all the time, which is no touch of the two sides).
    let near_misses: [(&str, [f64; 3], [f64; 3]); 4] = [
        ("far off, at rest", [100.0, 10.0, 10.0], [0.0; 3]),
        (
            "a voxel off the end, at rest",
            [CARS as f64 + 1.0, 10.0, 10.0],
            [0.0; 3],
        ),
        (
            "going past along the chain, a third of a voxel above it",
            [0.0, 11.3, 10.0],
            [20.0, 0.0, 0.0],
        ),
        (
            "coming at the end of the chain but passing beside it",
            [CARS as f64 + 30.0, 12.5, 10.0],
            [-20.0, 0.0, 0.0],
        ),
    ];
    for coupling in couplings() {
        for (name, origin, velocity) in near_misses {
            let settings = settings_with(3);
            let mut scene = Scene::new(&coupling, origin, velocity, settings);
            let mut chain_alone = chain_world(&coupling, settings);
            let mut stranger_alone = stranger_world(origin, velocity, settings);
            for tick in 0..60 {
                scene.tick();
                chain_alone.step();
                stranger_alone.step();
                assert_eq!(
                    scene.chain.state_hash(),
                    chain_alone.state_hash(),
                    "{} / {name}: the chain differs from the chain alone in tick {tick}",
                    coupling.name
                );
                assert_eq!(
                    scene.stranger.state_hash(),
                    stranger_alone.state_hash(),
                    "{} / {name}: the stranger differs from the stranger alone in tick {tick}",
                    coupling.name
                );
            }
            assert_eq!(scene.touching_ticks, 0, "{} / {name}", coupling.name);
        }
    }
}

#[test]
fn a_stranger_that_hits_the_chain_keeps_a_fifth_of_its_speed_wherever_in_a_step_it_hits() {
    let mut wrong = Vec::new();
    for coupling in couplings() {
        for speed in [5.0, 20.0, 60.0, 150.0] {
            let mut kept = Vec::new();
            for moment in 0..12 {
                // The gap that makes the stranger arrive `moment` twelfths into a tick of the world, as `settled_hit.rs` does it.
                let gap = speed * TICK * (3.0 + f64::from(moment) / 12.0);
                let mut scene = Scene::striking(&coupling, speed, gap, settings_with(3));
                scene.ticks(((gap / speed + 1.0) / TICK).ceil() as usize);
                kept.push(-velocity(&scene.stranger, STRANGER)[0] / speed);
                assert!(
                    scene.touching_ticks > 0,
                    "{} at {speed}: the sides never touched",
                    coupling.name
                );
            }
            let (low, high) = (
                kept.iter().copied().fold(f64::INFINITY, f64::min),
                kept.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            );
            if (low - KEPT_ON_THE_CHAIN)
                .abs()
                .max((high - KEPT_ON_THE_CHAIN).abs())
                > TOLERANCE
            {
                wrong.push(format!(
                    "{} at {speed} voxels a second: the stranger keeps {low:.4} to {high:.4} of its speed over 12 moments of a tick, where momentum says {KEPT_ON_THE_CHAIN}",
                    coupling.name
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn a_hit_keeps_momentum_to_the_rounding_of_the_solves() {
    for coupling in couplings() {
        for speed in [5.0, 60.0, 150.0] {
            let gap = speed * TICK * 3.4;
            let mut scene = Scene::striking(&coupling, speed, gap, settings_with(3));
            scene.ticks(((gap / speed + 1.0) / TICK).ceil() as usize);
            // (Four cars and the stranger, a cube of one voxel each, all of the one mass: the stranger had `-speed`.)
            let change = (scene.momentum_along_x() + speed).abs() / speed;
            assert!(
                change < 1e-9,
                "{} at {speed}: momentum changed by {change} of the stranger's",
                coupling.name
            );
        }
    }
}

#[test]
fn a_stranger_at_three_hundred_voxels_a_second_does_not_go_through_a_chain_that_is_cut_into_three_steps()
 {
    // (The ghost steps like the world of the chain, three steps; the cut of a step where the shapes first touch is what holds a hit at this speed: `settled_hit.rs`.)
    let coupling = &couplings()[2];
    let speed = 300.0;
    let gap = speed * TICK * 3.5;
    let mut scene = Scene::striking(coupling, speed, gap, settings_with(3));
    scene.ticks(((gap / speed + 1.0) / TICK).ceil() as usize);
    let kept = -velocity(&scene.stranger, STRANGER)[0] / speed;
    assert!(
        (kept - KEPT_ON_THE_CHAIN).abs() < TOLERANCE,
        "the stranger keeps {kept} of its speed"
    );
}

#[test]
fn a_chain_that_has_been_at_rest_and_asleep_for_a_long_time_is_hit_all_the_same() {
    // The bodies of a world that touch something and rest go to sleep (`sleeping.rs`); the copies in the ghost do not, and what the ghost gives a body wakes it.
    let coupling = &couplings()[1];
    let speed = 2.0;
    let gap = 30.0;
    let mut scene = Scene::striking(coupling, speed, gap, settings_with(3));
    scene.ticks(100);
    assert!(
        (1..=CARS as u64).any(|car| scene.chain.is_asleep(BodyId(car))),
        "(the chain was asleep before the stranger came)"
    );
    scene.ticks(((gap / speed + 1.0) / TICK).ceil() as usize - 100);
    let kept = -velocity(&scene.stranger, STRANGER)[0] / speed;
    assert!(
        (kept - KEPT_ON_THE_CHAIN).abs() < TOLERANCE,
        "the stranger keeps {kept} of its speed"
    );
}

#[test]
fn an_edit_of_a_body_reaches_its_copy_in_the_ghost() {
    // The cars of the chain are emptied of their colliders (their bricks are taken away) before the stranger comes: it meets nothing, and the copies in the ghost, which the stranger
    // would have hit, are made again from what the cars are now.
    let coupling = &couplings()[2];
    let speed = 20.0;
    let gap = 20.0;
    let mut scene = Scene::striking(coupling, speed, gap, settings_with(3));
    scene.ticks(2);
    for car in 1..=CARS as u64 {
        scene
            .chain
            .remove_brick(BodyId(car), BrickCoordinate::ORIGIN)
            .unwrap();
    }
    scene.ticks(((gap / speed + 1.0) / TICK).ceil() as usize);
    assert_eq!(scene.touching_ticks, 0);
    assert_eq!(velocity(&scene.stranger, STRANGER)[0], -speed);
}

#[test]
fn the_pieces_of_a_step_that_a_world_is_cut_into_can_be_taken_again_by_another() {
    // The ghost's world without the contacts goes the way the world with them did: the same pieces (where a step was cut at a touch), the same passes. Two worlds that are the
    // same, the one stepped and recorded and the other stepped by that record, are the same to the bit.
    let coupling = &couplings()[2];
    let settings = settings_with(3);
    let mut first = chain_world(coupling, settings);
    add_cube(
        &mut first,
        STRANGER,
        [CARS as f64 + 1.0, 10.0, 10.0],
        [-30.0, 0.0, 0.0],
    );
    let mut second = chain_world(coupling, settings);
    add_cube(
        &mut second,
        STRANGER,
        [CARS as f64 + 1.0, 10.0, 10.0],
        [-30.0, 0.0, 0.0],
    );
    let mut cut = false;
    for _ in 0..20 {
        let steps = first.steps_this_tick();
        let plan = first.step_recording(steps);
        cut |= plan.pieces() > steps as usize;
        second.step_replaying(&plan);
        assert_eq!(first.state_hash(), second.state_hash());
    }
    assert!(
        cut,
        "(a step was cut at the touch, so this was more than the plain steps)"
    );
}

#[test]
fn what_is_applied_to_a_body_for_a_tick_is_applied_to_its_copy_in_the_ghost() {
    // An engine pushes the stranger, at rest, onto the chain, which is at rest: the stranger arrives at about a quarter of the way through the second tick and keeps on pushing.
    // The one world, with the same force on the same cube, says what comes of it; the ghost gets there by the force it is told the stranger has.
    let coupling = &couplings()[2];
    let push = [-10_000.0, 0.0, 0.0];
    let mut scene = Scene::striking(coupling, 0.0, 6.0, settings_with(3));
    scene.push = push;
    let mut one = free_chain_with_stranger(coupling, 6.0);
    for _ in 0..60 {
        scene.tick();
        one.apply_force(BodyId(STRANGER), push, None).unwrap();
        one.step();
    }
    assert!(scene.touching_ticks > 0, "(the stranger reached the chain)");
    let apart = |id: u64| {
        let (ghost, whole) = (
            if id == STRANGER {
                velocity(&scene.stranger, id)
            } else {
                velocity(&scene.chain, id)
            },
            velocity(&one, id),
        );
        (ghost[0] - whole[0]).abs()
    };
    for id in 1..=STRANGER {
        // (They go at about 15 voxels a second by now: a hundredth of that is what the two may differ by, and an engine that the ghost did not know of is 15 off.)
        assert!(
            apart(id) < 0.15,
            "body {id}: the ghost and the one world differ by {} voxels a second",
            apart(id)
        );
    }
}

fn free_chain_with_stranger(coupling: &Coupling, gap: f64) -> BodyWorld {
    let mut world = free_space(3);
    add_chain(&mut world, coupling);
    add_cube(
        &mut world,
        STRANGER,
        [CARS as f64 + gap, 10.0, 10.0],
        [0.0; 3],
    );
    world
}

#[test]
fn a_hit_off_the_middle_turns_what_it_hits_as_the_one_world_does() {
    // The stranger is half a voxel higher than the chain, so it hits the end of it off its middle and everything turns: the corrections turn the bodies and not only move them.
    for coupling in couplings() {
        let speed = 20.0;
        let gap = 10.0;
        let origin = [CARS as f64 + gap, 10.5, 10.0];
        let mut scene = Scene::new(&coupling, origin, [-speed, 0.0, 0.0], settings_with(3));
        let mut one = free_space(3);
        add_chain(&mut one, &coupling);
        add_cube(&mut one, STRANGER, origin, [-speed, 0.0, 0.0]);
        for _ in 0..40 {
            scene.tick();
            one.step();
        }
        assert!(scene.touching_ticks > 0);
        let mut most_turn: f64 = 0.0;
        let mut worst: f64 = 0.0;
        for id in 1..=STRANGER {
            let world = if id == STRANGER {
                &scene.stranger
            } else {
                &scene.chain
            };
            let (ghost, whole) = (
                world.state(BodyId(id)).unwrap().angular_velocity,
                one.state(BodyId(id)).unwrap().angular_velocity,
            );
            for axis in 0..3 {
                most_turn = most_turn.max(whole[axis].abs());
                worst = worst.max((ghost[axis] - whole[axis]).abs());
            }
        }
        // (It does turn: a hit that left them all still would test nothing. How much: the solver's own turn for a glancing hit is the least settled number there is: the one world's
        // differs by 0.28 to 0.66 radians a second between 3 and 12 steps a tick, of a turn of 1.1 to 1.5, so the ghost is held to a tenth of the turn from the one world's at the
        // same settings, where it was measured (2026-11-15) 0.0000 to 0.069 radians a second off. That the turn is *given* right is `a_correction_is_added_to_a_body...` below.)
        assert!(
            most_turn > 0.05,
            "{}: the one world turns by {most_turn} radians a second at most",
            coupling.name
        );
        assert!(
            worst < 0.1 * most_turn,
            "{}: the turns differ by {worst} radians a second, of a turn of {most_turn}",
            coupling.name
        );
    }
}

#[test]
fn a_body_that_touched_nothing_is_given_nothing_though_others_in_the_ghost_touched() {
    // A second stranger is in the ghost, far from everything and asleep in its own world; the first hits the chain. The ghost has corrections for those that were hit and none for
    // the one that was not: nothing is added to it and it is not woken.
    let coupling = &couplings()[2];
    let settings = settings_with(3);
    let speed = 20.0;
    let mut chain = chain_world(coupling, settings);
    let mut hitter = stranger_world(
        [CARS as f64 + 3.0, 10.0, 10.0],
        [-speed, 0.0, 0.0],
        settings,
    );
    let bystander_id = STRANGER + 1;
    let mut bystander = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    add_cube(&mut bystander, bystander_id, [300.0, 10.0, 10.0], [0.0; 3]);
    let mut ghost = ContactGhost::new(BrickCoordinate::ORIGIN, settings);
    for car in 1..=CARS as u64 {
        let state = chain.state(BodyId(car)).unwrap();
        ghost
            .add_body_from(CHAIN, &chain, BodyId(car), state)
            .unwrap();
    }
    for car in 1..CARS as u64 {
        ghost.add_coupling_from(&chain, CouplingId(car)).unwrap();
    }
    for (side, world, id) in [
        (STRANGERS, &hitter, STRANGER),
        (Side(2), &bystander, bystander_id),
    ] {
        let state = world.state(BodyId(id)).unwrap();
        ghost.add_body_from(side, world, BodyId(id), state).unwrap();
    }
    let mut hit = false;
    for _ in 0..10 {
        for (world, ids) in [
            (&chain, (1..=CARS as u64).collect::<Vec<_>>()),
            (&hitter, vec![STRANGER]),
            (&bystander, vec![bystander_id]),
        ] {
            for id in ids {
                let state = world.state(BodyId(id)).unwrap();
                ghost.refresh(BodyId(id), world, state).unwrap();
            }
        }
        ghost.step(chain.steps_this_tick());
        chain.step();
        hitter.step();
        bystander.step();
        if ghost.touched() {
            hit = true;
            assert!(ghost.correction(BodyId(STRANGER)).is_some());
            assert!(ghost.correction(BodyId(1)).is_some());
            assert_eq!(
                ghost.correction(BodyId(bystander_id)),
                None,
                "the bystander touched nothing"
            );
        }
        for id in (1..=CARS as u64).chain([STRANGER]) {
            if let Some(correction) = ghost.correction(BodyId(id)) {
                let world = if id == STRANGER {
                    &mut hitter
                } else {
                    &mut chain
                };
                world.apply_correction(BodyId(id), &correction).unwrap();
            }
        }
    }
    assert!(hit, "(the stranger hit the chain)");
}

/// The product of two rotations as `[x, y, z, w]`: `first` turns after `second` (the order of matrices).
fn product(first: [f64; 4], second: [f64; 4]) -> [f64; 4] {
    let (a, b) = (first, second);
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

#[test]
fn a_correction_is_added_to_a_body_the_turn_before_the_turn_it_has_and_the_rest_to_what_it_has() {
    // A body a quarter turn about x, at (1, 2, 3), going (4, 5, 6) and turning (0, 0, 1); the correction is a quarter turn about z, 10 along x, and speeds of (1, 1, 1) and (0, 0, 2).
    let quarter = std::f64::consts::FRAC_1_SQRT_2;
    let about_x = [quarter, 0.0, 0.0, quarter];
    let about_z = [0.0, 0.0, quarter, quarter];
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings_with(3));
    add_cube(&mut world, 1, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]);
    let mut state = world.state(BodyId(1)).unwrap();
    state.orientation = voxel_space::Orientation::from_components(about_x).unwrap();
    state.angular_velocity = [0.0, 0.0, 1.0];
    world.set_state(BodyId(1), state).unwrap();
    world
        .apply_correction(
            BodyId(1),
            &physics::Correction {
                position: [10.0, 0.0, 0.0],
                rotation: about_z,
                linear_velocity: [1.0, 1.0, 1.0],
                angular_velocity: [0.0, 0.0, 2.0],
            },
        )
        .unwrap();
    let after = world.state(BodyId(1)).unwrap();
    let expected = product(about_z, about_x);
    let got = after.orientation.components();
    for axis in 0..4 {
        assert!(
            (got[axis] - expected[axis]).abs() < 1e-12,
            "orientation {got:?}, where the correction's turn before the body's is {expected:?}"
        );
    }
    assert_eq!(after.position.offset(), [11.0, 2.0, 3.0]);
    assert_eq!(after.linear_velocity, [5.0, 6.0, 7.0]);
    assert_eq!(after.angular_velocity, [0.0, 0.0, 3.0]);
}

#[test]
fn a_ghost_with_nothing_that_could_touch_in_the_tick_is_not_stepped() {
    // The copies of a ghost never sleep, so stepping them costs the mechanism awake, twice, all the time (the cost row of `examples/ghost_cost.rs`: 378 microseconds a tick for a chain
    // that costs 15 asleep); a tick in which no two bodies of different sides could touch is not stepped.
    let coupling = &couplings()[2];
    for (name, origin, velocity) in [
        ("far off, at rest", [300.0, 10.0, 10.0], [0.0; 3]),
        // (The box of a voxel collider is rounded out to the chunks it keeps its voxels in, so that of a cube of one voxel reaches about eight voxels from it: the gate of such a
        // cube opens at a gap of nine and is shut at twelve, measured. A ship of any size has a box that is its own, to within a chunk.)
        (
            "twenty voxels off the end, at rest",
            [CARS as f64 + 20.0, 10.0, 10.0],
            [0.0; 3],
        ),
        (
            "going past, forty voxels above the chain, at 20 voxels a second",
            [0.0, 50.0, 10.0],
            [20.0, 0.0, 0.0],
        ),
    ] {
        let mut scene = Scene::new(coupling, origin, velocity, settings_with(3));
        scene.ticks(60);
        assert_eq!(scene.ghost.steps_made(), 0, "{name}");
    }
    // And it is stepped when they could: a stranger that is coming (a tick of its way, and its slack, from the chain), and one that touches.
    let mut scene = Scene::striking(coupling, 20.0, 4.0, settings_with(3));
    scene.ticks(3);
    assert!(scene.ghost.steps_made() > 0);
    let mut resting_on_it = Scene::new(
        coupling,
        [CARS as f64, 10.0, 10.0],
        [0.0; 3],
        settings_with(3),
    );
    resting_on_it.ticks(10);
    assert_eq!(resting_on_it.ghost.steps_made(), 10);
}

mod gate {
    //! The gate that leaves a tick unstepped must never be shut when the sides would have touched: random scenes of two cubes, one on each side, near each other, going every way and
    //! turning, stepped with it and without it. (Cubes of 16 voxels: the box of a voxel collider is rounded out to its chunks, which for a cube of one voxel is eight voxels of
    //! room, and a gate with room to spare cannot be shown to be tight enough.)

    use super::*;
    use crate::block_bodies::add_block;

    /// A small generator of numbers that is the same on every machine.
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
        }

        fn between(&mut self, low: f64, high: f64) -> f64 {
            low + (high - low) * self.next()
        }
    }

    const SIDE: i128 = 16;

    /// A ghost of two cubes, the one on each side, with the second at `offset` from the first, going at the given speeds and turning.
    fn ghost_of(
        offset: [f64; 3],
        velocities: [[f64; 3]; 2],
        spins: [[f64; 3]; 2],
        gated: bool,
    ) -> ContactGhost {
        let settings = settings_with(3);
        let mut ghost = ContactGhost::new(BrickCoordinate::ORIGIN, settings);
        ghost.set_gated(gated);
        for (index, at) in [[0.0; 3], offset].into_iter().enumerate() {
            let id = index as u64 + 1;
            let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
            add_block(&mut world, id, SIDE, at, velocities[index]);
            let mut state = world.state(BodyId(id)).unwrap();
            state.angular_velocity = spins[index];
            world.set_state(BodyId(id), state).unwrap();
            ghost
                .add_body_from(Side(index as u32), &world, BodyId(id), state)
                .unwrap();
            ghost.refresh(BodyId(id), &world, state).unwrap();
        }
        ghost
    }

    #[test]
    fn a_tick_in_which_the_sides_touch_is_stepped() {
        let mut random = Random(0x9e37_79b9_7f4a_7c15);
        let (mut touched, mut shut) = (0, 0);
        for scene in 0..1500 {
            // The second cube is off the first along x (a gap of up to forty voxels, or overlapping a little), and across it by up to a size; they go every way up to
            // 150 voxels a second, so that in a tick of a twentieth of a second they cross up to fifteen voxels, and turn up to 6 radians a second.
            let gap = random.between(-1.0, 40.0);
            let offset = [
                SIDE as f64 + gap,
                random.between(-16.0, 16.0),
                random.between(-16.0, 16.0),
            ];
            // (How fast is chosen for each scene, from slow to the fastest: at the fastest a gate leaves nothing shut, and at the slowest nothing can touch.)
            let scale = random.next();
            let velocity = |random: &mut Random| {
                [
                    scale * random.between(-150.0, 150.0),
                    scale * random.between(-40.0, 40.0),
                    scale * random.between(-40.0, 40.0),
                ]
            };
            let velocities = [velocity(&mut random), velocity(&mut random)];
            let spin = |random: &mut Random| {
                [
                    scale * random.between(-6.0, 6.0),
                    scale * random.between(-6.0, 6.0),
                    scale * random.between(-6.0, 6.0),
                ]
            };
            let spins = [spin(&mut random), spin(&mut random)];
            let mut open = ghost_of(offset, velocities, spins, false);
            let mut gated = ghost_of(offset, velocities, spins, true);
            let steps = settings_with(3).substeps;
            open.step(steps);
            let stepped = gated.step(steps);
            if open.touched() {
                touched += 1;
                assert!(
                    stepped,
                    "scene {scene}: the sides touched in the tick and the gate was shut (offset {offset:?}, velocities {velocities:?}, spins {spins:?})"
                );
                assert_eq!(open.touched(), gated.touched());
                for id in 1..=2 {
                    assert_eq!(open.correction(BodyId(id)), gated.correction(BodyId(id)));
                }
            }
            if !stepped {
                shut += 1;
            }
        }
        // (The scenes are not all touches, nor all clear: a property of a gate that is open or shut in all of them is nothing.)
        assert!(touched > 60, "only {touched} of the scenes had a touch");
        eprintln!("{touched} of the scenes touched, the gate was shut in {shut}");
        assert!(shut > 100, "the gate was shut in only {shut} of the scenes");
    }

    #[test]
    fn a_tick_in_which_a_corner_that_turns_reaches_the_other_side_is_stepped() {
        // Neither cube goes anywhere: one turns fast about its middle, and the corner of it sweeps (14 voxels from the middle, at 10 radians a second: 7 voxels in the tick) into the
        // other, which is a few voxels off. A gate that counted only how far a body goes would shut on them.
        let mut random = Random(0x5851_f42d_4c95_7f2d);
        let mut touched = 0;
        for scene in 0..400 {
            let offset = [
                SIDE as f64 + random.between(0.2, 5.0),
                random.between(-6.0, 6.0),
                random.between(-6.0, 6.0),
            ];
            let spins = [
                [
                    random.between(-10.0, 10.0),
                    random.between(-10.0, 10.0),
                    random.between(-10.0, 10.0),
                ],
                [0.0; 3],
            ];
            let mut open = ghost_of(offset, [[0.0; 3]; 2], spins, false);
            let mut gated = ghost_of(offset, [[0.0; 3]; 2], spins, true);
            open.step(3);
            let stepped = gated.step(3);
            if open.touched() {
                touched += 1;
                assert!(
                    stepped,
                    "scene {scene}: a corner reached the other cube in the tick and the gate was shut (offset {offset:?}, spins {spins:?})"
                );
            }
        }
        assert!(touched > 20, "only {touched} of the scenes had a touch");
    }

    #[test]
    fn a_tick_in_which_a_force_brings_the_sides_together_from_rest_is_stepped() {
        // A cube at rest, a little over half a voxel from another, is pushed so hard that it goes a voxel in a tick (800 voxels a second a second is a hundred times the pull of a
        // voxel's gravity): what it is going at, which is nothing, says it goes nowhere, and a gate that believed that would shut.
        let settings = settings_with(3);
        let mass = 1000.0 * (SIDE * SIDE * SIDE) as f64;
        let mut ghost = ContactGhost::new(BrickCoordinate::ORIGIN, settings);
        let mut worlds = Vec::new();
        for (index, at) in [[0.0; 3], [SIDE as f64 + 0.6, 0.0, 0.0]]
            .into_iter()
            .enumerate()
        {
            let id = index as u64 + 1;
            let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
            add_block(&mut world, id, SIDE, at, [0.0; 3]);
            let state = world.state(BodyId(id)).unwrap();
            ghost
                .add_body_from(Side(index as u32), &world, BodyId(id), state)
                .unwrap();
            worlds.push((id, world));
        }
        let (_, pushed) = &mut worlds[0];
        pushed
            .apply_force(BodyId(1), [mass * 2000.0, 0.0, 0.0], None)
            .unwrap();
        for (id, world) in &worlds {
            let state = world.state(BodyId(*id)).unwrap();
            ghost.refresh(BodyId(*id), world, state).unwrap();
        }
        assert!(
            ghost.step(3),
            "the gate was shut for a cube that is pushed into another in the tick"
        );
        assert!(ghost.touched());
    }
}
