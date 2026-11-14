//! What the contraption kit must keep doing (the study of `docs/history/loop-probe.md`, sections 16 to 18): the properties the findings rest on, and the bugs that were found in it.
//! Run: `cd docs/history/spikes/loops && cargo test --release`.

use loop_probe::contraption::Shape;
use loop_probe::contraption::*;
use loop_probe::scenes::*;
use loop_probe::*;

fn v(x: f64, y: f64, z: f64) -> Vector {
    Vector::new(x, y, z)
}

fn passes(passes: usize) -> Options {
    Options { world_effort: (0, passes), ..Options::default() }
}

/// The bug that was found 2026-11-13: a pair that already touches (a time of impact of 0) hid every touch still to come, so a step was never cut while anything rested on the ground.
#[test]
fn a_resting_pair_does_not_hide_a_touch_to_come() {
    let mut scene = Scene::new("a box rests, a ball comes down", 0.5);
    scene.gravity = v(0.0, -9.81, 0.0);
    scene.ground.push(Ground { shape: Shape::Cuboid(v(200.0, 0.5, 200.0)), pose: Pose::from_translation(v(0.0, -0.5, 0.0)), friction: 0.5 });
    // The box rests under its weight, pressing on the floor: its pair with the floor is a touch that is there now.
    scene.add_car(CarSpec::new("resting box", 0, Shape::Cuboid(Vector::splat(0.5)), 1000.0, v(0.0, 0.5, 0.0)));
    // The ball is held up by a force of its weight (a ship under thrust) and goes down at 2 m/s: it touches the floor 0.2062 s in, 0.37 of the way through a step.
    let ball = scene.add_car(CarSpec::new("ball", 1, Shape::Ball(0.5), 100.0, v(10.0, 0.5 + 0.4 + 0.37 * 2.0 / 60.0, 0.0)).moving(v(0.0, -2.0, 0.0)));
    scene.pushes.push(Push { car: ball, force: v(0.0, 100.0 * 9.81, 0.0), from: 0.0, until: 0.5 });
    let trace = run_options(&scene, Scheme::Game, Settings::default(), Options { toi: true, ..Options::default() });
    assert!(trace.cuts >= 1, "the touch of the ball with the floor was not found while a box rested on it (cuts: {})", trace.cuts);
}

/// A step is cut where the first touch is, so how a hit comes out must not depend on where in a step it falls. A stiff chain hit by a stranger from a gap that is scanned over a step.
#[test]
fn a_cut_step_gives_the_same_hit_whenever_in_the_step_it_falls() {
    let speed = 20.0;
    let mut exits = Vec::new();
    for k in 0..6 {
        let mut scene = chain_scene(JointKind::Fixed, Hit::Axial, 1000.0, 4);
        let last = scene.cars.len() - 1;
        let gap = 5.0 + speed * (1.0 / 60.0) * (k as f64 / 6.0);
        scene.cars[last].pose = Pose::from_translation(v(3.0 + 0.5 + gap + 0.5, 0.0, 0.0));
        scene.cars[last].linvel = v(-speed, 0.0, 0.0);
        scene.seconds = ((gap / speed + 1.0) / 0.05f64).ceil() * 0.05;
        let options = Options { world_effort: (0, 15), toi: true, ..Options::default() };
        let trace = run_options(&scene, Scheme::Game, Settings::default(), options);
        exits.push(trace.last()[last].linvel.x / -speed);
    }
    let (low, high) = (exits.iter().cloned().fold(f64::INFINITY, f64::min), exits.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    assert!((high - low) < 0.01, "the exit speed changes with the moment of the hit: {exits:?}");
    // The inelastic answer: the stranger keeps 0.2 of its speed, with the chain.
    assert!((exits[0] - 0.2).abs() < 0.01, "exit {} of the closing speed, not 0.2", exits[0]);
}

/// Extra passes make the outcome of a hit the same whatever the frame's number of steps (today's world does not: -2.28 at 3 steps and -3.19 at 24 for a stranger at 20 m/s that should keep -4).
#[test]
fn passes_make_a_hit_independent_of_the_steps_a_tick_is_cut_into() {
    let scene = stiff_approach_scene(20.0);
    let exit = |steps: usize, effort: usize| run_options(&scene, Scheme::Game, Settings { steps_per_tick: steps, ..Settings::default() }, passes(effort)).last()[4].linvel.x;
    let today = [exit(3, 0), exit(24, 0)];
    assert!((today[0] - today[1]).abs() > 0.5, "today's world was found to depend on the step count: {today:?}");
    for steps in [3, 6, 12, 24] {
        let converged = exit(steps, 15);
        assert!((converged + 4.0).abs() < 0.05, "with passes 15 and {steps} steps a tick the exit is {converged}, not -4");
    }
}

/// A ghost that touches nothing changes nothing, bit for bit (promise (a)), for the forms of the third round too.
#[test]
fn a_ghost_of_the_third_round_that_touches_nothing_changes_nothing() {
    let (alone, _) = gear_scene(2.0, 2.0);
    let mut with = alone.clone();
    with.add_car(CarSpec::new("bystander", 1, Shape::Cuboid(Vector::splat(0.5)), 1000.0, v(3.0, 0.5, 0.0)).friction(0.5));
    let cars = alone.cars.len();
    let ghost = Options { ghost_effort: (0, 15), world_effort: (0, 15), toi: true, ..Options::default() };
    for (scheme, options) in [
        (Scheme::GhostDifference, ghost),
        (Scheme::Ghost, ghost),
        (Scheme::GhostWeldedDifference, ghost),
        (Scheme::GhostDifference, Options { own_ground: true, ..ghost }),
        (Scheme::Ghost, Options { own_ground: true, ..ghost }),
    ] {
        let a = run_options(&alone, scheme, Settings::default(), options);
        let b = run_options(&with, scheme, Settings::default(), options);
        for (fa, fb) in a.frames.iter().zip(&b.frames) {
            for i in 0..cars {
                // With the ground in the ghost (not the own-ground layout) the bystander's own contact with the floor is the ghost's too, and the gear's does not change by it: the same bits.
                assert_eq!(fa[i].pose.translation, fb[i].pose.translation, "{} moved car {i} by a bystander that touches nothing", scheme.name());
                assert_eq!(fa[i].linvel, fb[i].linvel, "{} changed car {i}'s speed by a bystander that touches nothing", scheme.name());
            }
        }
    }
}

/// What a hit keeps, with the cut and the passes: the momentum (no gravity) to rounding, and the energy of a perfectly inelastic hit (a fifth of it, a stranger of one chain-car's... 1 t on 4 t).
#[test]
fn the_cut_and_the_passes_keep_momentum_and_the_energy_of_an_inelastic_hit() {
    let mut scene = stiff_approach_scene(20.0);
    scene.seconds = 1.25;
    for scheme in [Scheme::Game, Scheme::GhostDifference, Scheme::GhostWeldedDifference, Scheme::Ghost] {
        let options = Options { ghost_effort: (0, 15), world_effort: (0, 15), toi: true, ..Options::default() };
        let trace = run_options(&scene, scheme, Settings::default(), options);
        let change = (total_momentum(&scene, trace.last()) - total_momentum(&scene, &trace.frames[0])).length();
        assert!(change < 1e-6, "{} changed the momentum by {change}", scheme.name());
        let kept = kinetic_energy(&scene, trace.last()) / kinetic_energy(&scene, &trace.frames[0]);
        assert!((kept - 0.2).abs() < 0.005, "{} kept {kept} of the energy, not a fifth", scheme.name());
    }
}

/// The ghost's own steps must be those of the groups' (at rest, a difference of two worlds stepped differently is a bias that does not go away): the ghost at four times the steps
/// with the groups at one jitters at rest, and the matched one does not.
#[test]
fn a_ghost_stepped_finer_than_the_groups_is_a_bias_at_rest() {
    let (scene, ids) = gear_scene(2.0, 8.0);
    let noise = |options: Options| {
        let trace = run_options(&scene, Scheme::GhostDifference, Settings::default(), options);
        trace.since(5.0).iter().map(|f| f.iter().map(|s| s.linvel.length()).fold(0.0, f64::max)).fold(0.0, f64::max)
    };
    let matched = noise(Options { ghost_effort: (0, 15), world_effort: (0, 15), toi: true, ..Options::default() });
    let finer = noise(Options { ghost_calls: 4, ghost_effort: (0, 15), world_effort: (0, 15), ..Options::default() });
    assert!(matched < 1e-3, "the matched ghost jitters at rest: {matched} m/s");
    assert!(finer > 0.02, "the finer ghost was found to jitter at rest, and does not now: {finer} m/s");
    let _ = ids;
}

/// The bits of some runs of the third round, as the Windows and the Linux of 2026-11-13 both gave them (`results/fingerprints-third-round-*.txt`): a refactor of the kit must keep them.
#[test]
fn pinned_runs_keep_their_bits() {
    let scene = stiff_approach_scene(20.0);
    let pinned: [(&str, Scheme, usize, Options, &str); 3] = [
        ("one world, 3 steps, (0, 15), cut", Scheme::Game, 3, Options { world_effort: (0, 15), toi: true, ..Options::default() }, "stiff chain, stranger at 20 m/s"),
        ("welded ghost diff", Scheme::GhostWeldedDifference, 3, Options { ghost_effort: (0, 15), world_effort: (0, 15), toi: true, ..Options::default() }, "stiff chain, stranger at 20 m/s"),
        ("today", Scheme::Game, 3, Options::default(), "stiff chain, stranger at 20 m/s"),
    ];
    let file = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/results/fingerprints-third-round-windows.txt")).expect("the stored fingerprints");
    for (label, scheme, steps, options, scene_name) in pinned {
        let trace = run_options(&scene, scheme, Settings { steps_per_tick: steps, ..Settings::default() }, options);
        let hash = format!("{:016x}", trace.fingerprint());
        // The stored lines name the design as the study's tables do; the three here are found by the hash being in the line of that scene.
        let found = file.lines().any(|line| line.contains(scene_name) && line.trim_end().ends_with(&hash));
        assert!(found, "the run '{label}' has the bits {hash}, which the stored third-round fingerprints do not have for '{scene_name}'");
    }
}
