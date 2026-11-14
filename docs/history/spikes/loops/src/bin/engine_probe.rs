//! The third round of the ghost study, in the game's own `physics::BodyWorld` (its real step rule, its real couplings, its voxel colliders): does what a hit does depend on how fast
//! the closing is (the world cuts its tick into 3 to 24 steps by its fastest body), and do the solver dials (`SolverDials`) take that dependence away?
//!
//! Run: `cargo run --release --bin engine_probe -- outcome|dials|pop|all`.

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
    let state = BodyState { position: WorldPosition::from_voxels(origin), orientation: Orientation::IDENTITY, linear_velocity: velocity, angular_velocity: [0.0; 3] };
    world.add_body(BodyId(id), state, BodyKind::Dynamic).unwrap();
    world.set_brick(BodyId(id), BrickCoordinate::ORIGIN, &mut Solid(1)).unwrap();
    world.set_mass(BodyId(id), cube_mass(1)).unwrap();
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

/// Microseconds a tick costs while two cubes of `side` voxels meet at `speed` (for 1 s after they touch), or while one rests on the ground.
fn tick_cost(side: i128, speed: f64, dials: Option<SolverDials>, base: u32, resting: bool) -> f64 {
    let gravity = if resting { [0.0, -5.0, 0.0] } else { [0.0; 3] };
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings { substeps: base, gravity, ..BodySettings::default() });
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
    if let Some(dials) = dials {
        world.set_solver_dials(BodyId(1), dials).unwrap();
        if !resting {
            world.set_solver_dials(BodyId(2), dials).unwrap();
        }
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
the real BodyWorld: what a tick costs, microseconds, for two cubes of 24 voxels (13,824 voxels each, one voxel collider each) that meet at 10 voxels/s, and for one that rests on the ground; this machine, one thread");
    println!("{:<34} {:>10} {:>10}", "settings", "meeting", "resting");
    for (label, dial, base) in [
        ("today: 3 steps, no dials", None, 3u32),
        ("3 steps, passes 15", Some(SolverDials { extra_substeps: 0, extra_passes: 15 }), 3),
        ("3 steps, substeps 4, passes 15", Some(SolverDials { extra_substeps: 4, extra_passes: 15 }), 3),
        ("6 steps, passes 15", Some(SolverDials { extra_substeps: 0, extra_passes: 15 }), 6),
        ("12 steps, passes 15", Some(SolverDials { extra_substeps: 0, extra_passes: 15 }), 12),
        ("12 steps, substeps 4, passes 15", Some(SolverDials { extra_substeps: 4, extra_passes: 15 }), 12),
    ] {
        let meeting = tick_cost(24, 10.0, dial, base, false);
        let resting = tick_cost(24, 10.0, dial, base, true);
        println!("{label:<34} {meeting:>10.0} {resting:>10.0}");
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

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
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
