//! The third round of the ghost study, in the game's own `physics::BodyWorld` (its real step rule, its real couplings, its voxel colliders): does what a hit does depend on how fast
//! the closing is (the world cuts its tick into 3 to 24 steps by its fastest body), and do the solver dials (`SolverDials`) take that dependence away?
//!
//! Run: `cargo run --release --bin engine_probe -- COMMAND`. The commands of the third round (section 18): `outcome`, `dials`, `phase`, `scan`, `cost`, `shapecast` (**since 2026-11-14 they run on the engine with the 4c-5-A defaults**: their rows for "today" or "no dials" are the new defaults, 15 passes and the cut; the output of before is `results/engine-probe-2026-11-13.txt`). Of step 4c-5-A
//! (section 19; the engine now has the passes, the cut and the fixed joint, and these measure it): `levers` (what each does alone and together, by coupling and speed),
//! `chainmoments` and `startphases` (the kept speed by moment of a step, and by how near the start of a step the touch is), `pairspin` (two lone cubes; `CUT_OFF=1`, `NO_PASSES=1`
//! switch the levers off), `landing`, `landingphases` and `landingspin` (a cube onto terrain: how deep, how fast it goes up, how fast it turns; `SIDE=n`, `OFFSET=dx,dz`, `PASSES=n` change the cube and the world), `castcost`, `margin` (when a voxel contact is found), `nonlinear` and `distances` (what parry can
//! cast and measure for voxel shapes), `spinhit` (a bar that turns into a cube), and the single runs `chainone`, `chaintrace`, `startone`, `landingone`, `landingtrace`, `spinone`.

use physics::{BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld, Collision, CollisionSource, CouplingId, CouplingOptions, MassMoments, PivotPair, SolverDials};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const TICK: f64 = 1.0 / 20.0;
/// The mass of a voxel (the toy's cars are 1000 kg, so a one-voxel cube is 1000).
const DENSITY: f64 = 1000.0;

struct Terrain<F: Fn(i128, i128, i128) -> bool>(F);

impl<F: Fn(i128, i128, i128) -> bool> CollisionSource for Terrain<F> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if (self.0)(voxel.x(), voxel.y(), voxel.z()) { Collision::Full } else { Collision::Empty }
    }
}

struct Solid(i128);

impl CollisionSource for Solid {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        let n = self.0;
        if (0..n).contains(&voxel.x()) && (0..n).contains(&voxel.y()) && (0..n).contains(&voxel.z()) { Collision::Full } else { Collision::Empty }
    }
}

fn cube_mass(side: i128) -> BodyMass {
    let voxels = (0..side).flat_map(|x| (0..side).flat_map(move |y| (0..side).map(move |z| MassMoments::of_voxel([x, y, z], DENSITY))));
    MassMoments::sum(voxels).properties().expect("a cube has a mass")
}

fn free_space() -> BodySettings {
    BodySettings { gravity: [0.0; 3], ..BodySettings::default() }
}

fn add_cube(world: &mut BodyWorld, id: u64, origin: [f64; 3], velocity: [f64; 3]) {
    add_block(world, id, 1, origin, velocity);
}

/// A cube of `side` voxels (up to 32, one brick), its origin at its lower corner.
fn add_block(world: &mut BodyWorld, id: u64, side: i128, origin: [f64; 3], velocity: [f64; 3]) {
    let state = BodyState { position: WorldPosition::from_voxels(origin), orientation: Orientation::IDENTITY, linear_velocity: velocity, angular_velocity: [0.0; 3] };
    world.add_body(BodyId(id), state, BodyKind::Dynamic).unwrap();
    world.set_brick(BodyId(id), BrickCoordinate::ORIGIN, &mut Solid(side)).unwrap();
    world.set_mass(BodyId(id), cube_mass(side)).unwrap();
}

/// Three pivots on the face two unit cubes share (not on a line: the coupling is rigid).
fn rigid_face() -> Vec<PivotPair> {
    vec![
        PivotPair { first: [1.0, 0.0, 0.0], second: [0.0, 0.0, 0.0] },
        PivotPair { first: [1.0, 1.0, 0.0], second: [0.0, 1.0, 0.0] },
        PivotPair { first: [1.0, 0.0, 1.0], second: [0.0, 0.0, 1.0] },
    ]
}

/// Two pivots on a line (a hinge about the line's axis, here along z) on the face the cubes share.
fn hinge_face() -> Vec<PivotPair> {
    vec![PivotPair { first: [1.0, 0.5, 0.0], second: [0.0, 0.5, 0.0] }, PivotPair { first: [1.0, 0.5, 1.0], second: [0.0, 0.5, 1.0] }]
}

/// The steps a tick is cut into for a fastest speed (the world's rule, `steps_for_this_tick`).
fn steps_for(speed: f64) -> u32 {
    ((speed * TICK / 0.8).ceil() as u32).clamp(3, 24)
}

fn velocity(world: &BodyWorld, id: u64) -> [f64; 3] {
    world.state(BodyId(id)).expect("body").linear_velocity
}

/// A chain of `cars` unit cubes joined by `pivots` (end to end along x) and a stranger cube that comes at them along -x at `speed` from half a voxel away. Returns the stranger's
/// velocity and the chain end's one second after the hit, and the mean speed of the chain.
fn strike(cars: usize, pivots: &[PivotPair], speed: f64, dials: Option<SolverDials>, substeps: u32) -> (f64, f64) {
    strike_from(cars, pivots, speed, dials, substeps, 0.5)
}

/// `gap`: the voxels between the stranger and the chain's end to begin with (the time the stranger arrives at, and so the moment of the tick it hits in).
fn strike_from(cars: usize, pivots: &[PivotPair], speed: f64, dials: Option<SolverDials>, substeps: u32, gap: f64) -> (f64, f64) {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings { substeps, ..free_space() });
    for k in 0..cars {
        add_cube(&mut world, k as u64 + 1, [k as f64, 10.0, 10.0], [0.0; 3]);
    }
    for k in 0..cars - 1 {
        world.couple(CouplingId(k as u64 + 1), BodyId(k as u64 + 1), BodyId(k as u64 + 2), pivots, CouplingOptions { contacts_between_bodies: false }).unwrap();
    }
    let stranger = cars as u64 + 1;
    add_cube(&mut world, stranger, [cars as f64 + gap, 10.0, 10.0], [-speed, 0.0, 0.0]);
    if let Some(dials) = dials {
        for id in 1..=stranger {
            world.set_solver_dials(BodyId(id), dials).unwrap();
        }
    }
    let ticks = ((gap / speed + 1.0) / TICK).ceil() as usize;
    for _ in 0..ticks {
        world.step();
    }
    (velocity(&world, stranger)[0], velocity(&world, 1)[0])
}

fn outcome() {
    println!("\nthe real BodyWorld: a stranger cube (1000) comes at a chain of 4 cubes (1000 each), 0.5 voxel away, at speed v along the chain; the cars are coupled rigidly (three pivots on each shared face)");
    println!("true (perfectly inelastic, nothing bounces): the stranger ends at 0.2 v, with the chain. exit/v is what the stranger keeps, and 'chain' what the far end of the chain has (should be the same).");
    println!("{:>8} {:>6} | {:>10} {:>10} {:>10}", "v", "steps", "exit/v", "chain/v", "gap");
    for speed in [2.0f64, 5.0, 10.0, 20.0, 40.0, 60.0, 100.0, 150.0, 300.0] {
        let (exit, chain) = strike(4, &rigid_face(), speed, None, 3);
        println!("{speed:>8.0} {:>6} | {:>10.4} {:>10.4} {:>10.4}", steps_for(speed), -exit / speed, -chain / speed, (exit - chain).abs() / speed);
    }
}

fn dials() {
    println!("\nthe same with the solver's dials (extra substeps, extra passes) on every body, in the real BodyWorld; exit/v of the stranger (true: 0.2), by closing speed");
    print!("{:>26}", "dials");
    let speeds = [5.0f64, 20.0, 60.0, 100.0, 150.0, 300.0];
    for v in speeds {
        print!(" {v:>8.0}");
    }
    println!();
    for (label, dial) in [("none (today)", None), ("passes 3", Some(SolverDials { extra_substeps: 0, extra_passes: 3 })), ("passes 7", Some(SolverDials { extra_substeps: 0, extra_passes: 7 })), ("passes 15", Some(SolverDials { extra_substeps: 0, extra_passes: 15 })), ("substeps 4, passes 15", Some(SolverDials { extra_substeps: 4, extra_passes: 15 })), ("substeps 12, passes 31", Some(SolverDials { extra_substeps: 12, extra_passes: 31 }))] {
        print!("{label:>26}");
        for v in speeds {
            let (exit, _) = strike(4, &rigid_face(), v, dial, 3);
            print!(" {:>8.4}", -exit / v);
        }
        println!();
    }
    println!("\nhinged chain of 4 (two pivots on a line) hit along its length: exit/v of the stranger, by closing speed");
    for (label, dial) in [("none (today)", None), ("passes 15", Some(SolverDials { extra_substeps: 0, extra_passes: 15 })), ("substeps 12, passes 31", Some(SolverDials { extra_substeps: 12, extra_passes: 31 }))] {
        print!("{label:>26}");
        for v in speeds {
            let (exit, _) = strike(4, &hinge_face(), v, dial, 3);
            print!(" {:>8.4}", -exit / v);
        }
        println!();
    }
}

fn phase() {
    println!("
exit/v of the stranger by closing speed, 1 voxel/s apart: is it the speed, the step count, or when in a tick the stranger arrives? (arrival in ticks = 0.5 / v / 0.05; the world cuts a tick into 3 steps)");
    println!("{:>6} {:>8} {:>8} | {:>9} {:>9} {:>9}", "v", "arrival", "steps", "today", "passes 15", "12 and 31");
    for v in (2..=40).step_by(2) {
        let v = v as f64;
        let today = strike(4, &rigid_face(), v, None, 3).0;
        let fifteen = strike(4, &rigid_face(), v, Some(SolverDials { extra_substeps: 0, extra_passes: 15 }), 3).0;
        let most = strike(4, &rigid_face(), v, Some(SolverDials { extra_substeps: 12, extra_passes: 31 }), 3).0;
        println!("{v:>6.0} {:>8.2} {:>8} | {:>9.4} {:>9.4} {:>9.4}", 0.5 / v / TICK, steps_for(v), -today / v, -fifteen / v, -most / v);
    }
}

/// The moment of the hit within a step, scanned: the stranger starts at gaps that make it arrive at 12 different points of one step (of a tick cut into `substeps`).
fn phases(speed: f64, dials: Option<SolverDials>, substeps: u32, pivots: &[PivotPair]) -> (f64, f64, f64) {
    let step = TICK / substeps.max(steps_for(speed)) as f64;
    let mut values = Vec::new();
    for k in 0..12 {
        let gap = speed * step * (6.0 + k as f64 / 12.0);
        values.push(-strike_from(4, pivots, speed, dials, substeps, gap).0 / speed);
    }
    let low = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let high = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    (values.iter().sum::<f64>() / values.len() as f64, low, high)
}

fn scan() {
    println!("
the moment of the hit within a step, scanned (12 moments over one step): exit/v of the stranger after a rigidly coupled chain of 4 (true 0.2): mean [lowest, highest]");
    println!("{:>9} {:>6} | {:>26} | {:>26} | {:>26}", "dials", "base", "v = 5", "v = 20", "v = 60");
    for (label, dial) in [("none", None), ("passes 15", Some(SolverDials { extra_substeps: 0, extra_passes: 15 })), ("4, 15", Some(SolverDials { extra_substeps: 4, extra_passes: 15 })), ("12, 31", Some(SolverDials { extra_substeps: 12, extra_passes: 31 }))] {
        for base in [3u32, 6, 12] {
            print!("{label:>9} {base:>6} |");
            for v in [5.0f64, 20.0, 60.0] {
                let (mean, low, high) = phases(v, dial, base, &rigid_face());
                print!(" {mean:>7.4} [{low:>7.4}, {high:>7.4}] |");
            }
            println!();
        }
    }
}

/// A cube of `side` voxels (one voxel collider of side^3 voxels) added to a world.
fn add_big_cube(world: &mut BodyWorld, id: u64, side: i128, origin: [f64; 3], velocity: [f64; 3]) {
    let state = BodyState { position: WorldPosition::from_voxels(origin), orientation: Orientation::IDENTITY, linear_velocity: velocity, angular_velocity: [0.0; 3] };
    world.add_body(BodyId(id), state, BodyKind::Dynamic).unwrap();
    world.set_brick(BodyId(id), BrickCoordinate::ORIGIN, &mut Solid(side)).unwrap();
    world.set_mass(BodyId(id), cube_mass(side)).unwrap();
}

/// Microseconds a tick costs while two cubes of `side` voxels meet at `speed` (for 1 s after they touch), or while one rests on the ground, under `settings`.
fn tick_cost(side: i128, speed: f64, settings: BodySettings, resting: bool) -> f64 {
    let gravity = if resting { [0.0, -5.0, 0.0] } else { [0.0; 3] };
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings { gravity, ..settings });
    let ticks;
    if resting {
        for x in -1..=1 {
            for z in -1..=1 {
                world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Terrain(|_, y, _| y < 0));
            }
        }
        add_big_cube(&mut world, 1, side, [10.0, 0.01, 10.0], [0.0; 3]);
        ticks = 100;
    } else {
        add_big_cube(&mut world, 1, side, [0.0, 10.0, 10.0], [0.0; 3]);
        add_big_cube(&mut world, 2, side, [side as f64 + 2.0, 10.0, 10.0], [-speed, 0.0, 0.0]);
        ticks = ((2.0 / speed + 1.0) / TICK).ceil() as usize;
    }
    for _ in 0..20 {
        world.step();
    }
    let started = std::time::Instant::now();
    for _ in 0..ticks {
        world.step();
    }
    started.elapsed().as_secs_f64() * 1e6 / ticks as f64
}

fn cost() {
    println!("
the real BodyWorld: what a tick costs, microseconds, for two cubes of 24 voxels (13,824 voxels each, one voxel collider each) that meet at 10 voxels/s (averaged over the 1 s after they touch, and the whole run), and for one that rests on the ground; this machine, one thread");
    println!("{:<44} {:>10} {:>10}", "settings", "meeting", "resting");
    let none = SolverDials::default();
    let passes = SolverDials { extra_substeps: 0, extra_passes: 15 };
    let heavy = SolverDials { extra_substeps: 4, extra_passes: 15 };
    for (label, solver, cut, base) in [
        ("before 4c-5-A: 3 steps, nothing extra", none, false, 3u32),
        ("passes 15 only", passes, false, 3),
        ("the cut only", none, true, 3),
        ("now: passes 15 and the cut", passes, true, 3),
        ("passes 15, the cut, 6 steps", passes, true, 6),
        ("passes 15, the cut, 12 steps", passes, true, 12),
        ("substeps 4, passes 15, the cut, 3 steps", heavy, true, 3),
    ] {
        let settings = BodySettings { substeps: base, solver, cut_at_first_touch: cut, ..BodySettings::default() };
        let meeting = tick_cost(24, 10.0, settings, false);
        let resting = tick_cost(24, 10.0, settings, true);
        println!("{label:<44} {meeting:>10.0} {resting:>10.0}");
    }
}

/// Can a shape be cast against the voxel colliders ships are made of (the step would be cut where the first touch is)? And what does it cost?
fn shapecast() {
    use rapier3d::parry::query::{ShapeCastOptions, cast_shapes};
    use rapier3d::prelude::{IVector, Pose, SharedShape, Vector};
    println!("
shape casts against the game's voxel colliders (`ColliderBuilder::voxels`): does a cast of a ball and of another voxel body come back, and when, and how long does it take?");
    for side in [8i64, 24, 64] {
        let mut keys = Vec::new();
        for x in 0..side {
            for y in 0..side {
                for z in 0..side {
                    keys.push(IVector::new(x, y, z));
                }
            }
        }
        let big = SharedShape::voxels(Vector::ONE, &keys);
        let ball = SharedShape::ball(0.5);
        let options = ShapeCastOptions { max_time_of_impact: 1.0, target_distance: 0.0, stop_at_penetration: true, compute_impact_geometry_on_penetration: false };
        // The voxel body sits at the origin; the ball comes along -x from 10 voxels beyond its far face, at 20 voxels a second: it touches after (10 - 0.5)/20 = 0.475 s.
        let start = Pose::from_translation(Vector::new(side as f64 + 10.0, side as f64 / 2.0, side as f64 / 2.0));
        let result = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*big, &start, Vector::new(-20.0, 0.0, 0.0), &*ball, options);
        let started = std::time::Instant::now();
        let runs = 200;
        for _ in 0..runs {
            let _ = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*big, &start, Vector::new(-20.0, 0.0, 0.0), &*ball, options);
        }
        let micros = started.elapsed().as_secs_f64() * 1e6 / runs as f64;
        // Voxel body against voxel body: a second cube of 8 voxels on its way.
        let mut small_keys = Vec::new();
        for x in 0..8i64 {
            for y in 0..8 {
                for z in 0..8 {
                    small_keys.push(IVector::new(x, y, z));
                }
            }
        }
        let small = SharedShape::voxels(Vector::ONE, &small_keys);
        let second = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*big, &Pose::from_translation(Vector::new(side as f64 + 10.0, 0.0, 0.0)), Vector::new(-20.0, 0.0, 0.0), &*small, options);
        let started = std::time::Instant::now();
        for _ in 0..runs {
            let _ = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*big, &Pose::from_translation(Vector::new(side as f64 + 10.0, 0.0, 0.0)), Vector::new(-20.0, 0.0, 0.0), &*small, options);
        }
        let micros_voxels = started.elapsed().as_secs_f64() * 1e6 / runs as f64;
        println!(
            "voxel body of {side}^3: ball cast {} ({micros:.1} us); voxel body of 8^3 cast {} ({micros_voxels:.1} us)",
            match &result {
                Ok(Some(hit)) => format!("hits at {:.4} s (true {:.4})", hit.time_of_impact, (10.0 - 0.5) / 20.0),
                Ok(None) => "finds nothing".to_string(),
                Err(_) => "is not supported".to_string(),
            },
            match &second {
                Ok(Some(hit)) => format!("hits at {:.4} s (true {:.4})", hit.time_of_impact, 10.0 / 20.0),
                Ok(None) => "finds nothing".to_string(),
                Err(_) => "is not supported".to_string(),
            }
        );
    }
}

/// One pivot at the middle of the face two unit cubes share (a ball: the game's coupling).
fn ball_face() -> Vec<PivotPair> {
    vec![PivotPair { first: [1.0, 0.5, 0.5], second: [0.0, 0.5, 0.5] }]
}

/// The kept speed (of the stranger, in parts of the closing speed; true 0.2) after a hit on a chain of 4 under `settings`, for a stranger that arrives `gap` voxels from the chain.
fn kept_under(pivots: &[PivotPair], contacts: bool, speed: f64, settings: BodySettings, gap: f64) -> f64 {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    for k in 0..4 {
        add_cube(&mut world, k as u64 + 1, [k as f64, 10.0, 10.0], [0.0; 3]);
    }
    for k in 0..3 {
        world.couple(CouplingId(k as u64 + 1), BodyId(k as u64 + 1), BodyId(k as u64 + 2), pivots, CouplingOptions { contacts_between_bodies: contacts }).unwrap();
    }
    add_cube(&mut world, 5, [4.0 + gap, 10.0, 10.0], [-speed, 0.0, 0.0]);
    for _ in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
        world.step();
    }
    -velocity(&world, 5)[0] / speed
}

/// What each lever (the passes of the solver, the cut of a step at a first touch) does alone and together to how a hit on a chain of four comes out over 12 moments of a step, for
/// each kind of coupling, in the game's real world (step 4c-5-A).
fn levers() {
    println!("\nthe real BodyWorld, a stranger on a chain of four (true: it keeps 0.2): the lowest and highest kept speed over 12 moments of a step at 3 steps a tick, by what the world is given");
    let none = SolverDials::default();
    let passes = SolverDials { extra_substeps: 0, extra_passes: 15 };
    let configurations = [("nothing (today's solver)", none, false), ("passes 15", passes, false), ("the cut", none, true), ("passes 15 and the cut", passes, true)];
    let couplings: [(&str, Vec<PivotPair>, bool); 4] = [
        ("rigid, cars pass through each other", rigid_face(), false),
        ("rigid, cars meet", rigid_face(), true),
        ("a ball, cars meet (the game's)", ball_face(), true),
        ("a hinge", hinge_face(), false),
    ];
    print!("{:<38} {:>6} |", "coupling", "v");
    for (name, _, _) in &configurations {
        print!(" {name:>26} |");
    }
    println!();
    for (label, pivots, contacts) in &couplings {
        for speed in [1.0f64, 5.0, 20.0, 60.0, 150.0] {
            print!("{label:<38} {speed:>6.0} |");
            for (_, dials, cut) in &configurations {
                let settings = BodySettings { substeps: 3, gravity: [0.0; 3], solver: *dials, cut_at_first_touch: *cut, ..BodySettings::default() };
                let step = TICK / steps_for(speed) as f64;
                let values: Vec<f64> = (0..12).map(|k| kept_under(pivots, *contacts, speed, settings, speed * step * (6.0 + k as f64 / 12.0))).collect();
                let low = values.iter().cloned().fold(f64::INFINITY, f64::min);
                let high = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                print!("   [{low:>7.4}, {high:>7.4}]       |");
            }
            println!();
        }
    }
}

/// What a landing came to, after a second of flight with no pull.
struct Landing {
    /// The deepest the cube's lower face was in the ground at the end of a tick (the ground's top is at 0).
    deepest: f64,
    /// The fastest it went up at the end of a tick.
    rebound: f64,
    /// Where its origin (its lower corner: it moves as the cube turns) was at the end.
    origin_height: f64,
    /// How many times a step was cut.
    cuts: u64,
    /// How fast it turns at the end, radians a second (a cube that lands squarely has no reason to turn).
    spin: f64,
}

/// A unit cube that falls onto flat terrain (no pull: the speed is the impact's alone) from a gap that makes it land `phase` of the way through a step, as the world with the
/// default passes and `cut` decide: see [`land_under`].
fn land(speed: f64, phase: f64, cut: bool) -> (f64, f64, f64, u64) {
    let landing = land_under(speed, phase, BodySettings { substeps: 3, gravity: [0.0; 3], cut_at_first_touch: cut, ..BodySettings::default() });
    (landing.deepest, landing.rebound, landing.origin_height, landing.cuts)
}

/// The landing of `land` under any settings.
fn land_under(speed: f64, phase: f64, settings: BodySettings) -> Landing {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    for x in -1..=1 {
        for z in -1..=1 {
            world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Terrain(|_, y, _| y < 0));
        }
    }
    let gap = speed * (TICK / steps_for(speed) as f64) * (6.0 + phase);
    // `OFFSET=dx,dz` lands the cube that far from where it is aligned with the ground's voxels (for finding what a spin is made of).
    let [dx, dz] = std::env::var("OFFSET")
        .ok()
        .and_then(|text| {
            let mut parts = text.split(',');
            Some([parts.next()?.parse().ok()?, parts.next()?.parse().ok()?])
        })
        .unwrap_or([0.0, 0.0]);
    // `SIDE=n` lands a cube of n voxels along each edge instead of one voxel (to see whether the turn of a landing grows or goes away with the size of the body).
    let side: i128 = std::env::var("SIDE").ok().and_then(|text| text.parse().ok()).unwrap_or(1);
    add_block(&mut world, 1, side, [10.0 + dx, gap, 10.0 + dz], [0.0, -speed, 0.0]);
    let (mut deepest, mut rebound) = (0.0f64, 0.0f64);
    for _ in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
        world.step();
        let state = world.state(BodyId(1)).unwrap();
        let height = state.position.relative_to(WorldPosition::from_voxels([0.0; 3]))[1];
        deepest = deepest.max(-height);
        rebound = rebound.max(state.linear_velocity[1]);
    }
    let end = world.state(BodyId(1)).unwrap();
    let spin = end.angular_velocity;
    Landing {
        deepest,
        rebound,
        origin_height: end.position.relative_to(WorldPosition::from_voxels([0.0; 3]))[1],
        cuts: world.cuts_made(),
        spin: (spin[0] * spin[0] + spin[1] * spin[1] + spin[2] * spin[2]).sqrt(),
    }
}

/// The landing of `land` over its 12 moments of a step, for the world as it was before step 4c-5-A, with the passes alone, with the cut alone, and as it is: the highest rebound
/// speed and the highest spin at the end (radians a second), and the lowest and the highest of them over the moments (a hit that does not depend on the moment has them near).
fn landing_spin() {
    println!("\na unit cube lands squarely on flat terrain at v voxels a second (no pull), 3 steps a tick, over 12 moments of a step: how fast it goes up after (voxels/s) and how fast it turns (radians/s), lowest to highest over the moments");
    let mut now = BodySettings { substeps: 3, gravity: [0.0; 3], ..BodySettings::default() };
    // `PASSES=n` changes the passes of the two columns that have them.
    if let Some(passes) = std::env::var("PASSES").ok().and_then(|text| text.parse().ok()) {
        now.solver.extra_passes = passes;
    }
    let none = SolverDials::default();
    let configurations = [
        ("before (no passes, no cut)", BodySettings { solver: none, cut_at_first_touch: false, ..now }),
        ("the passes alone", BodySettings { cut_at_first_touch: false, ..now }),
        ("the cut alone", BodySettings { solver: none, ..now }),
        ("now (passes and cut)", now),
    ];
    println!("{:>6} | {}", "v", configurations.map(|(label, _)| format!("{label:<40}")).join("| "));
    for speed in [5.0f64, 20.0, 60.0, 150.0] {
        print!("{speed:>6.0} |");
        for (_, settings) in &configurations {
            let runs: Vec<Landing> = (0..12).map(|k| land_under(speed, k as f64 / 12.0, *settings)).collect();
            let range = |f: &dyn Fn(&Landing) -> f64| {
                let low = runs.iter().map(f).fold(f64::INFINITY, f64::min);
                let high = runs.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
                format!("{low:.3}-{high:.3}")
            };
            print!(" up {:<14} turn {:<14}|", range(&|r| r.rebound), range(&|r| r.spin));
        }
        println!();
    }
}

/// The same landing as `land`, tick by tick: where the cube's origin (its lower corner, which moves as the cube turns) is, how fast it goes up, and how fast it turns (to see what the
/// numbers of `land` are made of). `CUT_OFF=1` and `NO_PASSES=1` as for `pairspin`.
fn land_trace(speed: f64, phase: f64) {
    let cut = std::env::var("CUT_OFF").is_err();
    let solver = if std::env::var("NO_PASSES").is_ok() { SolverDials::default() } else { BodySettings::default().solver };
    let settings = BodySettings { substeps: 3, gravity: [0.0; 3], cut_at_first_touch: cut, solver, ..BodySettings::default() };
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    for x in -1..=1 {
        for z in -1..=1 {
            world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Terrain(|_, y, _| y < 0));
        }
    }
    let gap = speed * (TICK / steps_for(speed) as f64) * (6.0 + phase);
    add_cube(&mut world, 1, [10.0, gap, 10.0], [0.0, -speed, 0.0]);
    println!("landing trace, v {speed}, moment {:.1} of 12, cut {cut}, passes {}: gap {gap:.4}", phase * 12.0, solver.extra_passes);
    for tick in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
        world.step();
        let state = world.state(BodyId(1)).unwrap();
        let height = state.position.relative_to(WorldPosition::from_voxels([0.0; 3]))[1];
        let spin = state.angular_velocity;
        println!("  tick {tick:>2}: height {height:>9.4}  vy {:>8.3}  spin {:>7.3} {:>7.3} {:>7.3}  cuts {}", state.linear_velocity[1], spin[0], spin[1], spin[2], world.cuts_made());
    }
}

/// Does a contact that is `gap` voxels away at the start of a step stop a cube that comes down at 20 voxels a second within that step? (The solver's margin is 0.002.) One step, 3 solver
/// steps of it, no cut: what the world's own contact does with a gap.
fn margin() {
    println!("\na unit cube 'gap' above terrain going down at 20 voxels/s, one tick of 1 step (no cut): where it is after, and its speed (does the contact catch it?)");
    for gap in [-1e-4, -1e-6, -1e-7, -1e-8, -1e-9, -1e-10, -1e-11, -1e-12, -1e-13, -1e-14, -1e-15, 0.0, 1e-15, 1e-6, 1e-3, 1.9e-3, 2.1e-3, 1e-2] {
        let settings = BodySettings { substeps: 1, gravity: [0.0; 3], cut_at_first_touch: false, time_step: 1.0 / 60.0, ..BodySettings::default() };
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
        for x in -1..=1 {
            for z in -1..=1 {
                world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Terrain(|_, y, _| y < 0));
            }
        }
        add_cube(&mut world, 1, [10.0, gap, 10.0], [0.0, -20.0, 0.0]);
        world.step();
        let state = world.state(BodyId(1)).unwrap();
        let height = state.position.relative_to(WorldPosition::from_voxels([0.0; 3]))[1];
        println!("  gap {gap:>9.1e}: after one step y {height:>12.3e}  vy {:>8.3}", state.linear_velocity[1]);
    }
}

fn landing_by_phase() {
    println!("\nby moment of the step (12), at v = 20 and 60: deepest / up / origin after 1 s / cuts, without and with the cut");
    for speed in [20.0f64, 60.0] {
        for k in 0..12 {
            let (a, b) = (land(speed, k as f64 / 12.0, false), land(speed, k as f64 / 12.0, true));
            println!("v {speed:>3} moment {k:>2}: no cut {:>7.4} / {:>6.3} / origin {:>7.4}   cut {:>7.4} / {:>6.3} / origin {:>7.4} / cuts {}", a.0, a.1, a.2, b.0, b.1, b.2, b.3);
        }
    }
}

fn landing() {
    println!("\na unit cube lands on flat terrain at v voxels a second (no pull), 3 steps a tick, over 12 moments of a step: the deepest it was in the ground at the end of a tick (voxels), the fastest it went up after (voxels/s), and where its origin (its lower corner: it moves as the cube turns) is a second after it landed (ground top at 0)");
    println!("{:>6} | {:>34} | {:>34}", "v", "no cut: deepest / up / origin after 1 s", "cut: deepest / up / origin after 1 s / cuts");
    for speed in [5.0f64, 20.0, 60.0, 150.0] {
        let summary = |cut: bool| {
            let runs: Vec<(f64, f64, f64, u64)> = (0..12).map(|k| land(speed, k as f64 / 12.0, cut)).collect();
            let worst = |f: &dyn Fn(&(f64, f64, f64, u64)) -> f64| runs.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
            let rest_low = runs.iter().map(|r| r.2).fold(f64::INFINITY, f64::min);
            format!("{:>7.4} / {:>7.3} / [{:>7.4}, {:>7.4}] / {}", worst(&|r| r.0), worst(&|r| r.1), rest_low, worst(&|r| r.2), runs.iter().map(|r| r.3).sum::<u64>())
        };
        println!("{speed:>6.0} | {:>34} | {:>34}", summary(false), summary(true));
    }
}

/// A voxel collider of the voxels `keep` says are filled in a box of `size` voxels.
fn voxel_shape(size: [i64; 3], keep: impl Fn(i64, i64, i64) -> bool) -> rapier3d::prelude::SharedShape {
    use rapier3d::prelude::{IVector, SharedShape, Vector};
    let mut keys = Vec::new();
    for x in 0..size[0] {
        for y in 0..size[1] {
            for z in 0..size[2] {
                if keep(x, y, z) {
                    keys.push(IVector::new(x, y, z));
                }
            }
        }
    }
    SharedShape::voxels(Vector::ONE, &keys)
}

/// What a first-touch question costs for realistic shapes (the cut of a step, step 4c-5-A): the exact cast of one voxel shape against another within one step of 1/60 s, by how far
/// apart they are, and for a hull inside a tunnel (a ship in a cave: the box of the tunnel's voxels holds the hull, the walls are away).
fn castcost() {
    use rapier3d::parry::query::{ShapeCastOptions, cast_shapes};
    use rapier3d::prelude::{Pose, Vector};
    let step = 1.0 / 60.0;
    let options = ShapeCastOptions { max_time_of_impact: step, target_distance: 0.0, stop_at_penetration: true, compute_impact_geometry_on_penetration: false };
    let time = |label: &str, first: &rapier3d::prelude::SharedShape, first_pose: Pose, second: &rapier3d::prelude::SharedShape, second_pose: Pose, speed: f64| {
        // The second goes at `speed` along -x relative to the first.
        let velocity = Vector::new(-speed, 0.0, 0.0);
        let answer = cast_shapes(&first_pose, Vector::ZERO, &**first, &second_pose, velocity, &**second, options);
        let runs = 20;
        let started = std::time::Instant::now();
        for _ in 0..runs {
            let _ = cast_shapes(&first_pose, Vector::ZERO, &**first, &second_pose, velocity, &**second, options);
        }
        let micros = started.elapsed().as_secs_f64() * 1e6 / runs as f64;
        let said = match &answer {
            Ok(Some(hit)) => format!("hits at {:.5} s", hit.time_of_impact),
            Ok(None) => "no touch".to_string(),
            Err(_) => "unsupported".to_string(),
        };
        println!("  {label:<64} {micros:>10.1} us  {said}");
    };
    println!("\nthe exact cast of one voxel shape against another within one step (1/60 s) at 20 voxels a second: microseconds, by shape and by the gap between the boxes");
    let solid24 = voxel_shape([24, 24, 24], |_, _, _| true);
    let hollow24 = voxel_shape([24, 24, 24], |x, y, z| x == 0 || y == 0 || z == 0 || x == 23 || y == 23 || z == 23);
    for (name, hull) in [("solid hull 24^3 (13,824 voxels)", &solid24), ("hollow hull 24^3 (2,402 voxels)", &hollow24)] {
        for gap in [0.1, 0.3, 5.0, 50.0] {
            let other = Pose::from_translation(Vector::new(24.0 + gap, 0.0, 0.0));
            time(&format!("{name} against the same, gap {gap}"), hull, Pose::IDENTITY, hull, other, 20.0);
        }
    }
    // A brick of terrain: flat ground, 32 x 16 x 32 filled, the hull above it.
    let ground = voxel_shape([32, 16, 32], |_, _, _| true);
    for gap in [0.1, 0.3, 5.0, 50.0] {
        let hull_pose = Pose::from_translation(Vector::new(4.0, 16.0 + gap, 4.0));
        let answer = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*ground, &hull_pose, Vector::new(0.0, -20.0, 0.0), &*solid24, options);
        let runs = 20;
        let started = std::time::Instant::now();
        for _ in 0..runs {
            let _ = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*ground, &hull_pose, Vector::new(0.0, -20.0, 0.0), &*solid24, options);
        }
        let micros = started.elapsed().as_secs_f64() * 1e6 / runs as f64;
        println!("  {:<64} {micros:>10.1} us  {}", format!("solid hull 24^3 down onto a terrain brick, gap {gap}"), match answer { Ok(Some(hit)) => format!("hits at {:.5} s", hit.time_of_impact), Ok(None) => "no touch".to_string(), Err(_) => "unsupported".to_string() });
    }
    // A tunnel: a brick of 32^3 whose walls are filled outside a 28^3 hollow; the hull (24^3) floats inside it, 2 voxels from the walls.
    let tunnel = voxel_shape([32, 32, 32], |x, y, z| !((2..30).contains(&x) && (2..30).contains(&y) && (2..30).contains(&z)));
    let inside = Pose::from_translation(Vector::new(4.0, 4.0, 4.0));
    let answer = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*tunnel, &inside, Vector::new(-20.0, 0.0, 0.0), &*solid24, options);
    let runs = 20;
    let started = std::time::Instant::now();
    for _ in 0..runs {
        let _ = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*tunnel, &inside, Vector::new(-20.0, 0.0, 0.0), &*solid24, options);
    }
    let micros = started.elapsed().as_secs_f64() * 1e6 / runs as f64;
    println!("  {:<64} {micros:>10.1} us  {}", "solid hull 24^3 inside a hollow brick (2 voxels from the walls)", match answer { Ok(Some(hit)) => format!("hits at {:.5} s", hit.time_of_impact), Ok(None) => "no touch".to_string(), Err(_) => "unsupported".to_string() });
    // A small ship (a 4^3 cube) in the same tunnel, and a cuboid bound of the hull against the tunnel.
    let small = voxel_shape([4, 4, 4], |_, _, _| true);
    let answer = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*tunnel, &Pose::from_translation(Vector::new(10.0, 10.0, 10.0)), Vector::new(-20.0, 0.0, 0.0), &*small, options);
    let started = std::time::Instant::now();
    for _ in 0..runs {
        let _ = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*tunnel, &Pose::from_translation(Vector::new(10.0, 10.0, 10.0)), Vector::new(-20.0, 0.0, 0.0), &*small, options);
    }
    let micros = started.elapsed().as_secs_f64() * 1e6 / runs as f64;
    println!("  {:<64} {micros:>10.1} us  {}", "small hull 4^3 inside the hollow brick", match answer { Ok(Some(hit)) => format!("hits at {:.5} s", hit.time_of_impact), Ok(None) => "no touch".to_string(), Err(_) => "unsupported".to_string() });
    let bound = rapier3d::prelude::SharedShape::cuboid(12.0, 12.0, 12.0);
    let answer = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*tunnel, &Pose::from_translation(Vector::new(16.0, 16.0, 16.0)), Vector::new(-20.0, 0.0, 0.0), &*bound, options);
    let started = std::time::Instant::now();
    for _ in 0..runs {
        let _ = cast_shapes(&Pose::IDENTITY, Vector::ZERO, &*tunnel, &Pose::from_translation(Vector::new(16.0, 16.0, 16.0)), Vector::new(-20.0, 0.0, 0.0), &*bound, options);
    }
    let micros = started.elapsed().as_secs_f64() * 1e6 / runs as f64;
    println!("  {:<64} {micros:>10.1} us  {}", "a cuboid (the hull's bound) inside the hollow brick", match answer { Ok(Some(hit)) => format!("hits at {:.5} s", hit.time_of_impact), Ok(None) => "no touch".to_string(), Err(_) => "unsupported".to_string() });
}

fn chain_by_moment() {
    println!("
the game's coupling (a ball, cars meet), kept speed by moment of the step (12), 3 steps a tick, with the cut and the default passes");
    for speed in [0.5f64, 1.0, 2.0, 5.0, 20.0, 60.0, 150.0] {
        let settings = BodySettings { substeps: 3, gravity: [0.0; 3], ..BodySettings::default() };
        let step = TICK / steps_for(speed) as f64;
        let values: Vec<String> = (0..12).map(|k| format!("{:.4}", kept_under(&ball_face(), true, speed, settings, speed * step * (6.0 + k as f64 / 12.0)))).collect();
        println!("v {speed:>3}: {}", values.join(" "));
    }
}

/// sin and cos of a small angle by their series (the probe keeps to the rules of the simulation: no std transcendental functions).
fn small_sin_cos(angle: f64) -> (f64, f64) {
    let (mut sine, mut cosine, mut term_sine, mut term_cosine) = (0.0, 0.0, angle, 1.0);
    for n in 0..12 {
        sine += term_sine;
        cosine += term_cosine;
        term_sine *= -angle * angle / ((2 * n + 2) as f64 * (2 * n + 3) as f64);
        term_cosine *= -angle * angle / ((2 * n + 1) as f64 * (2 * n + 2) as f64);
    }
    (sine, cosine)
}

/// A bar of 12 voxels (mass 12,000) turning about z at `spin` radians a second whose upper face comes down onto a cube (mass 1,000) at rest at the same angle (0) whatever moment of
/// a step it falls at: the cube's speed upwards and its spin after, and how many times a step was cut.
fn spin_hit(spin: f64, moment: f64, cut: bool) -> (f64, f64, u64) {
    let settings = BodySettings { substeps: 3, gravity: [0.0; 3], cut_at_first_touch: cut, ..BodySettings::default() };
    let step = TICK / 3.0;
    let arrives = step * (6.0 + moment / 12.0);
    // The bar is at angle -spin * arrives at the start and at angle 0 when its tip's upper corner touches the cube's lower face (y = 0.5 above the bar's middle line).
    let angle = -spin * arrives;
    let (sine, cosine) = small_sin_cos(angle / 2.0);
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    // The bar's middle (its centre of mass, 6, 0.5, 0.5 in its own coordinates) is at (20, 20, 20); its origin is where that puts it at this angle.
    let (full_sine, full_cosine) = small_sin_cos(angle);
    let centre = [20.0, 20.0, 20.5];
    let origin = [centre[0] - (6.0 * full_cosine - 0.5 * full_sine), centre[1] - (6.0 * full_sine + 0.5 * full_cosine), centre[2] - 0.5];
    let state = BodyState { position: WorldPosition::from_voxels(origin), orientation: Orientation::from_components([0.0, 0.0, sine, cosine]).unwrap(), linear_velocity: [0.0; 3], angular_velocity: [0.0, 0.0, spin] };
    world.add_body(BodyId(1), state, BodyKind::Dynamic).unwrap();
    struct Bar;
    impl CollisionSource for Bar {
        fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
            if (0..12).contains(&voxel.x()) && voxel.y() == 0 && voxel.z() == 0 { Collision::Full } else { Collision::Empty }
        }
    }
    world.set_brick(BodyId(1), BrickCoordinate::ORIGIN, &mut Bar).unwrap();
    let bar_mass = MassMoments::sum((0..12).map(|x| MassMoments::of_voxel([x, 0, 0], DENSITY))).properties().unwrap();
    world.set_mass(BodyId(1), bar_mass).unwrap();
    // The cube: its lower face is at y = 20.5 + 0.5 - 0.5... the bar's upper face at angle 0 is y = centre y + 0.5 = 20.5; the cube sits on it, over the tip (x from 25.5 to 26.5).
    add_cube(&mut world, 2, [25.5, 20.5, 20.0], [0.0; 3]);
    let mut per_tick = Vec::new();
    for _ in 0..((arrives + 1.0) / TICK).ceil() as usize {
        world.step();
        per_tick.push(world.cuts_made());
    }
    if std::env::var_os("SPIN_TRACE").is_some() {
        println!("cuts after each tick: {per_tick:?}");
    }
    let cube = world.state(BodyId(2)).unwrap();
    (cube.linear_velocity[1], cube.angular_velocity[2], world.cuts_made())
}

/// Does the cast that follows a turn work on voxel shapes, and what does it cost?
fn nonlinear() {
    use rapier3d::parry::query::{NonlinearRigidMotion, cast_shapes_nonlinear};
    use rapier3d::prelude::{Pose, Vector};
    let bar = voxel_shape([12, 1, 1], |_, _, _| true);
    let cube = voxel_shape([1, 1, 1], |_, _, _| true);
    println!("\nthe cast that follows a turn (parry cast_shapes_nonlinear) on voxel shapes: a bar of 12 turning about z at 1 rad/s above a cube it touches after about 0.1 s");
    // The bar's middle is at the origin (its own centre is (6, 0.5, 0.5)); it turns about its middle at 1 rad/s, from angle -0.1 so that it is level at t = 0.1.
    let centre = Vector::new(6.0, 0.5, 0.5);
    let bar_start = Pose::from_parts(-centre, rapier3d::prelude::Rotation::IDENTITY);
    let bar_motion = NonlinearRigidMotion::new(bar_start, centre, Vector::ZERO, Vector::new(0.0, 0.0, 1.0));
    // The cube rests with its lower face on the level bar's upper face (y = 0.5 about the middle line), over the tip.
    let cube_motion = NonlinearRigidMotion::constant_position(Pose::from_translation(Vector::new(5.5, 0.5, -0.5)));
    for (label, bar_angle_start) in [("level at start (touching)", 0.0f64), ("0.05 rad below", -0.05), ("0.1 rad below", -0.1), ("0.3 rad below", -0.3)] {
        let (sine, cosine) = small_sin_cos(bar_angle_start / 2.0);
        let start = Pose::from_parts(Vector::ZERO, rapier3d::prelude::Rotation::from_xyzw(0.0, 0.0, sine, cosine));
        let motion = NonlinearRigidMotion::new(start * bar_start, centre, Vector::ZERO, Vector::new(0.0, 0.0, 1.0));
        let _ = &bar_motion;
        let answer = cast_shapes_nonlinear(&motion, &*bar, &cube_motion, &*cube, 0.0, 0.5, true);
        // The same with plain cuboids (the voxel shapes' boxes), to see whether the setup or the voxels are what it does not find.
        let bar_box = rapier3d::prelude::SharedShape::cuboid(6.0, 0.5, 0.5);
        let cube_box = rapier3d::prelude::SharedShape::cuboid(0.5, 0.5, 0.5);
        let box_motion = NonlinearRigidMotion::new(Pose::from_parts(Vector::ZERO, rapier3d::prelude::Rotation::from_xyzw(0.0, 0.0, small_sin_cos(bar_angle_start / 2.0).0, small_sin_cos(bar_angle_start / 2.0).1)), Vector::ZERO, Vector::ZERO, Vector::new(0.0, 0.0, 1.0));
        let box_cube_motion = NonlinearRigidMotion::constant_position(Pose::from_translation(Vector::new(6.0, 1.0, 0.0)));
        let box_answer = cast_shapes_nonlinear(&box_motion, &*bar_box, &box_cube_motion, &*cube_box, 0.0, 0.5, true);
        // Mixed: the voxel bar against a cuboid cube, and a cuboid bar against a voxel cube.
        let mixed_a = cast_shapes_nonlinear(&motion, &*bar, &box_cube_motion_in_voxel_frame(), &*cube_box, 0.0, 0.5, true);
        let mixed_b = cast_shapes_nonlinear(&box_motion, &*bar_box, &box_cube_voxel_motion(), &*cube, 0.0, 0.5, true);
        let say = |answer: &Result<Option<rapier3d::parry::query::ShapeCastHit>, _>| match answer { Ok(Some(hit)) => format!("hit at {:.5}", hit.time_of_impact), Ok(None) => "no touch".to_string(), Err(_) => "unsupported".to_string() };
        let swapped = cast_shapes_nonlinear(&cube_motion, &*cube, &motion, &*bar, 0.0, 0.5, true);
        let swapped_mixed = cast_shapes_nonlinear(&box_cube_voxel_motion(), &*cube, &box_motion, &*bar_box, 0.0, 0.5, true);
        let swapped_other = cast_shapes_nonlinear(&box_cube_motion_in_voxel_frame(), &*cube_box, &motion, &*bar, 0.0, 0.5, true);
        println!("  (voxel bar vs cuboid cube: {}; cuboid bar vs voxel cube: {}; SWAPPED voxel cube first vs voxel bar: {}; voxel cube first vs cuboid bar: {}; cuboid cube first vs voxel bar: {})", say(&mixed_a), say(&mixed_b), say(&swapped), say(&swapped_mixed), say(&swapped_other));
        println!("  (cuboids: {})", match &box_answer { Ok(Some(hit)) => format!("hit at {:.5} ({:?})", hit.time_of_impact, hit.status), Ok(None) => "no touch".to_string(), Err(_) => "unsupported".to_string() });
        let runs = 50;
        let started = std::time::Instant::now();
        for _ in 0..runs {
            let _ = cast_shapes_nonlinear(&motion, &*bar, &cube_motion, &*cube, 0.0, 0.5, true);
        }
        let micros = started.elapsed().as_secs_f64() * 1e6 / runs as f64;
        let said = match &answer {
            Ok(Some(hit)) => format!("hits at {:.5} s ({:?})", hit.time_of_impact, hit.status),
            Ok(None) => "no touch".to_string(),
            Err(_) => "UNSUPPORTED".to_string(),
        };
        println!("  bar {label:<28} {micros:>8.1} us  {said}");
    }
}

fn box_cube_motion_in_voxel_frame() -> rapier3d::parry::query::NonlinearRigidMotion {
    use rapier3d::prelude::{Pose, Vector};
    // The cuboid cube's middle is at (6, 1, 0) in the frame where the bar's middle is the origin.
    rapier3d::parry::query::NonlinearRigidMotion::constant_position(Pose::from_translation(Vector::new(6.0, 1.0, 0.0)))
}

fn box_cube_voxel_motion() -> rapier3d::parry::query::NonlinearRigidMotion {
    use rapier3d::prelude::{Pose, Vector};
    rapier3d::parry::query::NonlinearRigidMotion::constant_position(Pose::from_translation(Vector::new(5.5, 0.5, -0.5)))
}

/// Can parry measure the distance between two voxel shapes, and how fast?
fn distances() {
    use rapier3d::parry::query::distance;
    use rapier3d::prelude::{Pose, Vector};
    let hull = voxel_shape([24, 24, 24], |_, _, _| true);
    let hollow = voxel_shape([24, 24, 24], |x, y, z| x == 0 || y == 0 || z == 0 || x == 23 || y == 23 || z == 23);
    let small = voxel_shape([4, 4, 4], |_, _, _| true);
    let tunnel = voxel_shape([32, 32, 32], |x, y, z| !((2..30).contains(&x) && (2..30).contains(&y) && (2..30).contains(&z)));
    println!("
the distance between voxel shapes (parry query::distance): value and microseconds, by shape and true gap");
    for (name, first, second) in [("solid 24^3 / solid 24^3", &hull, &hull), ("hollow 24^3 / hollow 24^3", &hollow, &hollow), ("solid 24^3 / small 4^3", &hull, &small), ("tunnel 32^3 / solid 24^3 inside", &tunnel, &hull)] {
        for gap in [0.05, 0.5, 5.0, 50.0] {
            let other = if name.starts_with("tunnel") { Pose::from_translation(Vector::new(4.0 - gap + 2.0 - 2.0, 4.0, 4.0)) } else { Pose::from_translation(Vector::new(24.0 + gap, 0.0, 0.0)) };
            let value = distance(&Pose::IDENTITY, &**first, &other, &**second);
            let runs = 20;
            let started = std::time::Instant::now();
            for _ in 0..runs {
                let _ = distance(&Pose::IDENTITY, &**first, &other, &**second);
            }
            let micros = started.elapsed().as_secs_f64() * 1e6 / runs as f64;
            println!("  {name:<34} gap {gap:>5}: {} in {micros:>9.1} us", match value { Ok(d) => format!("distance {d:?}"), Err(_) => "UNSUPPORTED".to_string() });
        }
    }
}

fn spinhit() {
    println!("\na bar of 12 voxels (12,000) turning at w about z touches a cube (1,000) at the same angle whatever moment of the step: the cube's upward speed and its spin after, by moment (12), without the cut / with it");
    for spin in [0.5f64, 1.0, 2.0] {
        for cut in [false, true] {
            let runs: Vec<(f64, f64, u64)> = (0..12).map(|k| spin_hit(spin, k as f64 / 12.0, cut)).collect();
            let low = runs.iter().map(|r| r.0).fold(f64::INFINITY, f64::min);
            let high = runs.iter().map(|r| r.0).fold(f64::NEG_INFINITY, f64::max);
            let cuts: u64 = runs.iter().map(|r| r.2).sum();
            println!("w {spin:>3} {}: speed [{low:>8.4}, {high:>8.4}]  (tip {:.1})  cuts {cuts}   {}", if cut { "cut   " } else { "no cut" }, 6.0 * spin, runs.iter().map(|r| format!("{:.3}", r.0)).collect::<Vec<_>>().join(" "));
        }
    }
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    if which == "castcost" {
        castcost();
    }
    if which == "levers" {
        levers();
    }
    if which == "landing" {
        landing();
    }
    if which == "landingphases" {
        landing_by_phase();
    }
    if which == "chaintrace" {
        let speed: f64 = std::env::args().nth(2).unwrap().parse().unwrap();
        let moment: f64 = std::env::args().nth(3).unwrap().parse().unwrap();
        let settings = BodySettings { substeps: 3, gravity: [0.0; 3], ..BodySettings::default() };
        let step = TICK / steps_for(speed) as f64;
        let gap = speed * step * (6.0 + moment / 12.0);
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
        for k in 0..4 {
            add_cube(&mut world, k as u64 + 1, [k as f64, 10.0, 10.0], [0.0; 3]);
        }
        for k in 0..3 {
            world.couple(CouplingId(k as u64 + 1), BodyId(k as u64 + 1), BodyId(k as u64 + 2), &ball_face(), CouplingOptions { contacts_between_bodies: true }).unwrap();
        }
        add_cube(&mut world, 5, [4.0 + gap, 10.0, 10.0], [-speed, 0.0, 0.0]);
        let mut line = String::new();
        for tick in 0..((gap / speed + 1.0) / TICK).ceil() as usize + 2 {
            world.step();
            if tick as f64 * TICK > gap / speed {
                line.push_str(&format!("{:.4} ", -velocity(&world, 5)[0] / speed));
            }
        }
        println!("{line}");
        for id in 1..=5u64 {
            let state = world.state(BodyId(id)).unwrap();
            println!("  body {id}: vx/v {:+.4} vy {:+.5} vz {:+.5}  spin {:+.5} {:+.5} {:+.5}", -state.linear_velocity[0] / speed, state.linear_velocity[1], state.linear_velocity[2], state.angular_velocity[0], state.angular_velocity[1], state.angular_velocity[2]);
        }
    }
    if which == "pairspin" {
        println!("a lone cube (1000) hits a lone cube at rest, v voxels a second, by moment of the step: the stranger's kept speed (true 0.5) and the largest spin of either after");
        for speed in [2.0f64, 5.0, 20.0, 60.0] {
            let step = TICK / steps_for(speed) as f64;
            let mut line = String::new();
            for k in 0..12 {
                let gap = speed * step * (6.0 + k as f64 / 12.0);
                let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings { substeps: 3, gravity: [0.0; 3], cut_at_first_touch: std::env::var("CUT_OFF").is_err(), solver: if std::env::var("NO_PASSES").is_ok() { SolverDials::default() } else { BodySettings::default().solver }, ..BodySettings::default() });
                add_cube(&mut world, 1, [0.0, 10.0, 10.0], [0.0; 3]);
                add_cube(&mut world, 2, [1.0 + gap, 10.0, 10.0], [-speed, 0.0, 0.0]);
                if let Ok(count) = std::env::var("PASSES") {
                    let dials = SolverDials { extra_substeps: 0, extra_passes: count.parse().unwrap() };
                    world.set_solver_dials(BodyId(1), dials).unwrap();
                    world.set_solver_dials(BodyId(2), dials).unwrap();
                }
                for _ in 0..((gap / speed + 1.0) / TICK).ceil() as usize {
                    world.step();
                }
                let spin = |id: u64| world.state(BodyId(id)).unwrap().angular_velocity.iter().fold(0.0f64, |m, x| m.max(x.abs()));
                line.push_str(&format!("{:.4}/{:.0e} ", -velocity(&world, 2)[0] / speed, spin(1).max(spin(2))));
            }
            println!("v {speed:>3}: {line}");
        }
    }
    if which == "distances" {
        distances();
    }
    if which == "nonlinear" {
        nonlinear();
    }
    if which == "spinhit" {
        spinhit();
    }
    if which == "spinone" {
        let spin: f64 = std::env::args().nth(2).unwrap().parse().unwrap();
        let moment: f64 = std::env::args().nth(3).unwrap().parse().unwrap();
        println!("{:?}", spin_hit(spin, moment / 12.0, true));
    }
    if which == "startone" {
        let speed: f64 = std::env::args().nth(2).unwrap().parse().unwrap();
        let fraction: f64 = std::env::args().nth(3).unwrap().parse().unwrap();
        let settings = BodySettings { substeps: 3, gravity: [0.0; 3], ..BodySettings::default() };
        let step = TICK / steps_for(speed) as f64;
        println!("kept {:.4}", kept_under(&rigid_face(), false, speed, settings, speed * step * (6.0 + fraction)));
    }
    if which == "startphases" {
        println!("the rigid pass-through chain, kept speed by how far into a step (fraction of a step) the touch falls, at 3 steps a tick");
        for speed in [20.0f64, 60.0, 150.0] {
            let settings = BodySettings { substeps: 3, gravity: [0.0; 3], ..BodySettings::default() };
            let step = TICK / steps_for(speed) as f64;
            let mut line = String::new();
            for fraction in [0.0f64, 0.0001, 0.0003, 0.001, 0.003, 0.01, 0.03, 0.1, 0.5] {
                line.push_str(&format!(" {fraction}: {:.4}", kept_under(&rigid_face(), false, speed, settings, speed * step * (6.0 + fraction))));
            }
            println!("v {speed:>3}:{line}");
        }
    }
    if which == "startvsmiddle" {
        let couplings: [(&str, Vec<PivotPair>, bool); 4] = [
            ("rigid pass-through", rigid_face(), false),
            ("rigid meet", rigid_face(), true),
            ("ball meet", ball_face(), true),
            ("hinge", hinge_face(), false),
        ];
        for (label, pivots, contacts) in &couplings {
            let mut line = String::new();
            for speed in [5.0f64, 20.0, 60.0, 150.0] {
                let settings = BodySettings { substeps: 3, gravity: [0.0; 3], ..BodySettings::default() };
                let step = TICK / steps_for(speed) as f64;
                let at = |k: f64| kept_under(pivots, *contacts, speed, settings, speed * step * (6.0 + k / 12.0));
                line.push_str(&format!(" v{speed:>3}: {:.4}/{:.4}", at(0.0), at(6.0)));
            }
            println!("{label:<20}{line}");
        }
    }
    if which == "chainone" {
        let speed: f64 = std::env::args().nth(2).unwrap().parse().unwrap();
        let moment: f64 = std::env::args().nth(3).unwrap().parse().unwrap();
        let settings = BodySettings { substeps: 3, gravity: [0.0; 3], ..BodySettings::default() };
        let step = TICK / steps_for(speed) as f64;
        println!("kept {:.4}", kept_under(&ball_face(), true, speed, settings, speed * step * (6.0 + moment / 12.0)));
    }
    if which == "chainmoments" {
        chain_by_moment();
    }
    if which == "margin" {
        margin();
    }
    if which == "landingspin" {
        landing_spin();
    }
    if which == "landingone" {
        let speed: f64 = std::env::args().nth(2).unwrap().parse().unwrap();
        let moment: f64 = std::env::args().nth(3).unwrap().parse().unwrap();
        println!("{:?}", land(speed, moment / 12.0, true));
    }
    if which == "landingtrace" {
        let speed: f64 = std::env::args().nth(2).unwrap().parse().unwrap();
        let moment: f64 = std::env::args().nth(3).unwrap().parse().unwrap();
        land_trace(speed, moment / 12.0);
    }
    if which == "outcome" || which == "all" {
        outcome();
    }
    if which == "dials" || which == "all" {
        dials();
    }
    if which == "phase" {
        phase();
    }
    if which == "scan" || which == "all" {
        scan();
    }
    if which == "cost" || which == "all" {
        cost();
    }
    if which == "shapecast" || which == "all" {
        shapecast();
    }
}
