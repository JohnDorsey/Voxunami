//! Is the game's own collision physics (`physics::BodyWorld`, as `voxunami_cave` configures it) accurate? Cases with known answers, and invariants.
//! Run: `cargo run --release --bin collisions -- [head|ratio|oblique|tunnel|rest|friction|seam|all]`.

use std::collections::HashMap;

use physics::{BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld, Collision, CollisionSource};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const EDGE: f64 = 32.0;

#[derive(Clone, Default)]
struct Hull(HashMap<VoxelCoordinate, Collision>);

impl Hull {
    fn block(sx: i128, sy: i128, sz: i128) -> Self {
        let mut hull = Self::default();
        for x in 0..sx {
            for y in 0..sy {
                for z in 0..sz {
                    hull.0.insert(VoxelCoordinate::new(x, y, z), Collision::Full);
                }
            }
        }
        hull
    }

    fn cube(size: i128) -> Self {
        Self::block(size, size, size)
    }

    fn mass(&self, density: f64) -> BodyMass {
        let mut voxels: Vec<&VoxelCoordinate> = self.0.keys().collect();
        voxels.sort();
        physics::MassMoments::sum(voxels.into_iter().map(|v| physics::MassMoments::of_voxel([v.x(), v.y(), v.z()], density)))
            .properties()
            .expect("the hull has mass")
    }
}

impl CollisionSource for &Hull {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        self.0.get(&voxel).cloned().unwrap_or(Collision::Empty)
    }
}

/// Terrain from a function: which voxels are whole cubes.
struct Terrain<F: Fn(i128, i128, i128) -> bool>(F);

impl<F: Fn(i128, i128, i128) -> bool> CollisionSource for Terrain<F> {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if (self.0)(voxel.x(), voxel.y(), voxel.z()) { Collision::Full } else { Collision::Empty }
    }
}

fn at(x: f64, y: f64, z: f64) -> WorldPosition {
    let brick = |value: f64| (value / EDGE).floor() as i128;
    let (bx, by, bz) = (brick(x), brick(y), brick(z));
    WorldPosition::new(BrickCoordinate::new(bx, by, bz), [x - bx as f64 * EDGE, y - by as f64 * EDGE, z - bz as f64 * EDGE])
}

fn coordinates(state: &BodyState) -> [f64; 3] {
    let (brick, offset) = (state.position.brick(), state.position.offset());
    [brick.x() as f64 * EDGE + offset[0], brick.y() as f64 * EDGE + offset[1], brick.z() as f64 * EDGE + offset[2]]
}

fn add(world: &mut BodyWorld, id: u64, hull: &Hull, position: [f64; 3], velocity: [f64; 3], angular: [f64; 3], kind: BodyKind) {
    let state = BodyState { position: at(position[0], position[1], position[2]), orientation: Orientation::IDENTITY, linear_velocity: velocity, angular_velocity: angular };
    world.add_body(BodyId(id), state, kind).unwrap();
    world.set_brick(BodyId(id), BrickCoordinate::ORIGIN, &mut &*hull).unwrap();
    world.set_mass(BodyId(id), hull.mass(1.0)).unwrap();
}

fn zero_gravity(substeps: u32) -> BodySettings {
    BodySettings { gravity: [0.0; 3], substeps, ..BodySettings::default() }
}

fn velocity(world: &BodyWorld, id: u64) -> [f64; 3] {
    world.state(BodyId(id)).unwrap().linear_velocity
}

fn norm(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let all = which == "all";
    if which == "head" || all {
        head_on();
    }
    if which == "ratio" || all {
        mass_ratio();
    }
    if which == "oblique" || all {
        oblique();
    }
    if which == "tunnel" || all {
        tunnel();
    }
    if which == "rest" || all {
        resting();
    }
    if which == "friction" || all {
        friction();
    }
    if which == "seam" || all {
        seam();
    }
    if which == "sleep" || all {
        sleeping();
    }
    if which == "plate" || all {
        plate();
    }
    if which == "thin" || all {
        thin();
    }
    if which == "lock" || all {
        locked();
    }
    if which == "threshold" || all {
        threshold();
    }
    if which == "awake" || all {
        awake();
    }
}

/// A cube drifts alone in space at a small speed. Physics says it never stops.
fn sleeping() {
    println!("== SLEEP. One cube of 4 drifting alone in space at speed v for 20 s. It must still be going at v. ==");
    println!("{:<10} {:>14} {:>14} {:>14} {:>16}", "v m/s", "distance", "expected", "final speed", "asleep at 20 s");
    for v in [0.05f64, 0.2, 0.35, 0.45, 0.6, 1.0, 5.0] {
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(3));
        add(&mut world, 1, &Hull::cube(4), [0.0, 10.0, 10.0], [v, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
        for _ in 0..400 {
            world.step();
        }
        let state = world.state(BodyId(1)).unwrap();
        println!("{v:<10} {:>14.3} {:>14.3} {:>14.4} {:>16}", coordinates(&state)[0], v * 20.0, norm(state.linear_velocity), world.is_asleep(BodyId(1)));
    }
    println!("-- the same with a slow spin (rad/s) --");
    for w in [0.05f64, 0.3, 0.6, 1.0] {
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(3));
        add(&mut world, 1, &Hull::cube(4), [0.0, 10.0, 10.0], [0.0; 3], [0.0, w, 0.0], BodyKind::Dynamic);
        for _ in 0..400 {
            world.step();
        }
        let state = world.state(BodyId(1)).unwrap();
        println!("spin {w:<5} final spin {:>8.4} asleep {}", norm(state.angular_velocity), world.is_asleep(BodyId(1)));
    }
}

/// Why does a cube pass a dynamic plate at 50 m/s? Positions tick by tick.
fn plate() {
    for v in [50.0f64, 5.0] {
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(3));
        add(&mut world, 1, &Hull::cube(2), [0.0, 14.0, 14.0], [v, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
        add(&mut world, 2, &Hull::block(1, 8, 8), [30.0, 10.0, 10.0], [0.0; 3], [0.0; 3], BodyKind::Dynamic);
        println!("v = {v}: tick, cube x, cube v, plate x, plate v");
        for tick in 0..((40.0 / v / 0.05) as usize + 40) {
            world.step();
            let (c, p) = (world.state(BodyId(1)).unwrap(), world.state(BodyId(2)).unwrap());
            if tick % 2 == 0 || tick < 40 {
                println!("  {tick:>3} {:>9.3} {:>9.3} {:>9.3} {:>9.3}", coordinates(&c)[0], c.linear_velocity[0], coordinates(&p)[0], p.linear_velocity[0]);
            }
        }
    }
}

/// Resting contact with sleeping defeated (a tiny push every tick keeps the bodies awake): what is the jitter underneath?
fn awake() {
    println!("== AWAKE. As REST, but a force of 1e-6 N wakes the bodies every tick. Speed noise and height drift over the last 20 s. ==");
    println!("{:<34} {:<10} {:>14} {:>14} {:>14} {:>14}", "scene", "substeps", "rms speed", "max speed", "height change", "height wander");
    for (name, stacked) in [("cube of 4 on the floor", false), ("cube of 2 on a cube of 6 on the floor", true)] {
        for substeps in [3u32, 12] {
            let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings { substeps, ..BodySettings::default() });
            for x in -1..=1 {
                for z in -1..=1 {
                    world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Terrain(|_, y, _| y < 0));
                }
            }
            let (big, small) = if stacked { (Hull::cube(6), Hull::cube(2)) } else { (Hull::cube(4), Hull::cube(0)) };
            add(&mut world, 1, &big, [10.0, 0.01, 10.0], [0.0; 3], [0.0; 3], BodyKind::Dynamic);
            if stacked {
                add(&mut world, 2, &small, [12.0, 6.02, 12.0], [0.0; 3], [0.0; 3], BodyKind::Dynamic);
            }
            let id = if stacked { 2 } else { 1 };
            let mut speeds = Vec::new();
            let mut heights = Vec::new();
            for tick in 0..800 {
                for body in 1..=(if stacked { 2 } else { 1 }) {
                    world.apply_force(BodyId(body), [1e-6, 0.0, 0.0], None).unwrap();
                }
                world.step();
                if tick >= 400 {
                    speeds.push(norm(velocity(&world, id)));
                    heights.push(coordinates(&world.state(BodyId(id)).unwrap())[1]);
                }
            }
            let rms = (speeds.iter().map(|s| s * s).sum::<f64>() / speeds.len() as f64).sqrt();
            let max = speeds.iter().cloned().fold(0.0, f64::max);
            let wander = heights.iter().cloned().fold(f64::MIN, f64::max) - heights.iter().cloned().fold(f64::MAX, f64::min);
            println!("{name:<34} {substeps:<10} {rms:>14.2e} {max:>14.2e} {:>14.2e} {wander:>14.2e}", heights[heights.len() - 1] - heights[0]);
        }
    }
}

/// Two equal cubes of 4 (mass 64) meet head on at relative speed `u`. With no bounce (the colliders are made with restitution 0) both must end up at
/// the common velocity, here 0: all the relative energy is lost, none gained, and momentum is exactly what it was.
fn head_on() {
    println!("== HEAD-ON. Two cubes of 4 (64 kg each) meet at relative speed u, in space, restitution 0. Expected: both stop (the common velocity). ==");
    println!("{:<8} {:<10} {:>14} {:>14} {:>12} {:>12} {:>12}", "u m/s", "substeps", "final v A", "final v B", "|dP|/P_ref", "max overlap", "KE gained?");
    for u in [0.1f64, 1.0, 10.0, 100.0, 400.0] {
        for substeps in [3u32, 12] {
            let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(substeps));
            let cube = Hull::cube(4);
            add(&mut world, 1, &cube, [0.0, 10.0, 10.0], [u / 2.0, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
            add(&mut world, 2, &cube, [7.0, 10.0, 10.0], [-u / 2.0, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
            let ticks = ((3.0 / (u / 2.0)) / 0.05).ceil() as usize + 40;
            let mut max_overlap = 0.0f64;
            let mut max_dp = 0.0f64;
            let mut max_ke_ratio = 0.0f64;
            let ke0 = 0.5 * 64.0 * 2.0 * (u / 2.0).powi(2);
            for _ in 0..ticks {
                world.step();
                let (a, b) = (world.state(BodyId(1)).unwrap(), world.state(BodyId(2)).unwrap());
                let overlap = coordinates(&a)[0] + 4.0 - coordinates(&b)[0];
                max_overlap = max_overlap.max(overlap);
                let p = 64.0 * (a.linear_velocity[0] + b.linear_velocity[0]);
                max_dp = max_dp.max(p.abs());
                let ke = 0.5 * 64.0 * (norm(a.linear_velocity).powi(2) + norm(b.linear_velocity).powi(2));
                max_ke_ratio = max_ke_ratio.max(ke / ke0);
            }
            let (va, vb) = (velocity(&world, 1), velocity(&world, 2));
            println!("{u:<8} {substeps:<10} {:>14.3e} {:>14.3e} {:>12.1e} {max_overlap:>12.4} {:>12}", va[0], vb[0], max_dp / (64.0 * u / 2.0), if max_ke_ratio > 1.0 + 1e-9 { format!("yes x{max_ke_ratio:.3}") } else { "no".to_string() });
        }
    }
}

/// A light cube meets a heavy one at 10 m/s: the common velocity is known (momentum), and nothing may bounce.
fn mass_ratio() {
    println!("== MASS RATIO. A cube of 1 (1 kg) and a cube of 8 (512 kg), one hits the other at rest at 10 m/s. Both must leave at the common velocity m1 v / (m1 + m2). ==");
    println!("{:<22} {:<10} {:>14} {:>14} {:>14} {:>12}", "case", "substeps", "expected v", "final v light", "final v heavy", "KE gained?");
    for (name, light_moves) in [("light hits heavy", true), ("heavy hits light", false)] {
        for substeps in [3u32, 12] {
            let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(substeps));
            let (small, big) = (Hull::cube(1), Hull::cube(8));
            let (m_small, m_big) = (1.0f64, 512.0f64);
            let (v_small, v_big, expected) = if light_moves { (10.0, 0.0, 10.0 * m_small / (m_small + m_big)) } else { (0.0, 10.0, 10.0 * m_big / (m_small + m_big)) };
            // The big cube spans x 20..28, the small one starts 3 to its left, moving or not.
            add(&mut world, 1, &small, [14.0, 14.0, 14.0], [v_small, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
            add(&mut world, 2, &big, [20.0, 10.0, 10.0], [v_big, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
            // The small cube is aimed at the middle of the big one's face; when the big one moves it starts to its right instead.
            if !light_moves {
                world.set_state(BodyId(1), BodyState { position: at(36.0, 14.0, 14.0), orientation: Orientation::IDENTITY, linear_velocity: [0.0; 3], angular_velocity: [0.0; 3] }).unwrap();
                world.set_state(BodyId(2), BodyState { position: at(20.0 - 20.0, 10.0, 10.0), orientation: Orientation::IDENTITY, linear_velocity: [v_big, 0.0, 0.0], angular_velocity: [0.0; 3] }).unwrap();
            }
            let ke0 = 0.5 * m_small * v_small.powi(2) + 0.5 * m_big * v_big.powi(2);
            let mut max_ke = 0.0f64;
            for _ in 0..200 {
                world.step();
                let (a, b) = (velocity(&world, 1), velocity(&world, 2));
                max_ke = max_ke.max(0.5 * m_small * norm(a).powi(2) + 0.5 * m_big * norm(b).powi(2));
            }
            let (a, b) = (velocity(&world, 1), velocity(&world, 2));
            println!("{name:<22} {substeps:<10} {expected:>14.5} {:>14.5} {:>14.5} {:>12}", a[0], b[0], if max_ke > ke0 * (1.0 + 1e-9) { format!("yes x{:.3}", max_ke / ke0) } else { "no".to_string() });
        }
    }
}

/// Off-centre hits: no exact answer, but momentum and angular momentum must be conserved and energy must not rise. The answer should settle as the substeps rise.
fn oblique() {
    println!("== OBLIQUE. A cube of 4 hits another at rest 1.5 m off its centre line (a glancing face hit), 10 m/s. ==");
    println!("{:<10} {:>12} {:>12} {:>12} {:>34} {:>34}", "substeps", "dP/P", "dL/L", "KE after/before", "final v of the struck cube", "final spin of the struck cube");
    let mut previous: Option<[f64; 3]> = None;
    for substeps in [3u32, 6, 12, 24, 48, 96] {
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(substeps));
        let cube = Hull::cube(4);
        add(&mut world, 1, &cube, [0.0, 10.0, 10.0], [10.0, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
        add(&mut world, 2, &cube, [8.0, 11.5, 10.0], [0.0; 3], [0.0; 3], BodyKind::Dynamic);
        // Momentum and angular momentum about the origin from the states, centre of mass at (2,2,2) of each body (no turn yet).
        let measure = |world: &BodyWorld| {
            let (mut p, mut l, mut ke) = ([0.0f64; 3], [0.0f64; 3], 0.0f64);
            for id in [1u64, 2] {
                let state = world.state(BodyId(id)).unwrap();
                let c = coordinates(&state);
                // Rotation is small enough over the run for the centre to be taken at the origin plus (2,2,2) rotated; for the sums, use the quaternion.
                let q = state.orientation.components();
                let centre_offset = rotate(q, [2.0, 2.0, 2.0]);
                let x = [c[0] + centre_offset[0], c[1] + centre_offset[1], c[2] + centre_offset[2]];
                let v = state.linear_velocity;
                let w = state.angular_velocity;
                let inertia = 64.0 * (16.0 + 16.0) / 12.0 + 64.0 * 0.0; // cube of 4: m (a^2 + a^2) / 12, same on every axis
                for k in 0..3 {
                    p[k] += 64.0 * v[k];
                }
                let orbital = [x[1] * v[2] - x[2] * v[1], x[2] * v[0] - x[0] * v[2], x[0] * v[1] - x[1] * v[0]];
                for k in 0..3 {
                    l[k] += 64.0 * orbital[k] + inertia * w[k];
                }
                ke += 0.5 * 64.0 * norm(v).powi(2) + 0.5 * inertia * norm(w).powi(2);
            }
            (p, l, ke)
        };
        let (p0, l0, ke0) = measure(&world);
        for _ in 0..120 {
            world.step();
        }
        let (p1, l1, ke1) = measure(&world);
        let dp = norm([p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]]) / norm(p0);
        let dl = norm([l1[0] - l0[0], l1[1] - l0[1], l1[2] - l0[2]]) / norm(l0);
        let v2 = velocity(&world, 2);
        let w2 = world.state(BodyId(2)).unwrap().angular_velocity;
        let change = previous.map_or(String::new(), |p| format!("   (struck cube's v moved {:.1e} from the last row)", norm([v2[0] - p[0], v2[1] - p[1], v2[2] - p[2]])));
        println!("{substeps:<10} {dp:>12.2e} {dl:>12.2e} {:>12.4} {:>34} {:>34}{change}", ke1 / ke0, format!("({:.3}, {:.3}, {:.3})", v2[0], v2[1], v2[2]), format!("({:.3}, {:.3}, {:.3})", w2[0], w2[1], w2[2]));
        previous = Some(v2);
    }
}

fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [v[0] + w * t[0] + (y * t[2] - z * t[1]), v[1] + w * t[1] + (z * t[0] - x * t[2]), v[2] + w * t[2] + (x * t[1] - y * t[0])]
}

/// How fast can a cube hit a wall one voxel thick before it goes through? Against terrain, and against another body (a dynamic plate).
fn tunnel() {
    println!("== TUNNEL. A cube of 2 flies at a wall 1 voxel thick (8 x 8) at speed v. 'through' = it ended beyond the wall. ==");
    println!("{:<10} {:<14} {:<22} {:<22}", "v m/s", "substeps", "terrain wall", "dynamic plate (64 kg)");
    for v in [50.0f64, 150.0, 300.0, 380.0, 400.0, 800.0, 3000.0] {
        for substeps in [3u32] {
            let mut cells = Vec::new();
            for dynamic in [false, true] {
                let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(substeps));
                let cube = Hull::cube(2);
                add(&mut world, 1, &cube, [0.0, 13.0, 13.0], [v, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
                if dynamic {
                    let plate = Hull::block(1, 8, 8);
                    add(&mut world, 2, &plate, [30.0, 10.0, 10.0], [0.0; 3], [0.0; 3], BodyKind::Dynamic);
                } else {
                    for bz in 0..1 {
                        world.set_terrain_brick(BrickCoordinate::new(0, 0, bz), &mut Terrain(|x, y, z| x == 30 && (10..18).contains(&y) && (10..18).contains(&z)));
                    }
                }
                let ticks = (80.0 / v / 0.05).ceil() as usize + 20;
                for _ in 0..ticks {
                    world.step();
                }
                let x = coordinates(&world.state(BodyId(1)).unwrap())[0];
                let plate_x = if dynamic { Some(coordinates(&world.state(BodyId(2)).unwrap())[0]) } else { None };
                let through = x > 31.0 && plate_x.is_none_or(|p| x > p + 1.0);
                cells.push(format!("{} (cube x={x:.1})", if through { "THROUGH" } else { "held" }));
            }
            println!("{v:<10} {substeps:<14} {:<22} {:<22}", cells[0], cells[1]);
        }
    }
}

/// A cube sits on the floor, and a small one on that: do they rest (no jitter, no sinking, asleep)?
fn resting() {
    println!("== REST. Cubes at rest under gravity 5 m/s^2 for 40 s: speed noise in the last 20 s, height drift, whether it sleeps. ==");
    println!("{:<34} {:<10} {:>14} {:>14} {:>14} {:>8}", "scene", "substeps", "rms speed", "max speed", "height change", "asleep");
    for (name, stacked) in [("cube of 4 on the floor", false), ("cube of 2 on a cube of 6 on the floor", true)] {
        for substeps in [3u32, 12] {
            let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings { substeps, ..BodySettings::default() });
            for x in -1..=1 {
                for z in -1..=1 {
                    world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Terrain(|_, y, _| y < 0));
                }
            }
            let (big, small) = if stacked { (Hull::cube(6), Hull::cube(2)) } else { (Hull::cube(4), Hull::cube(0)) };
            add(&mut world, 1, &big, [10.0, 0.01, 10.0], [0.0; 3], [0.0; 3], BodyKind::Dynamic);
            let top = if stacked { 6.02 } else { 0.0 };
            if stacked {
                add(&mut world, 2, &small, [12.0, top, 12.0], [0.0; 3], [0.0; 3], BodyKind::Dynamic);
            }
            let id = if stacked { 2 } else { 1 };
            let mut speeds = Vec::new();
            let mut start_height = 0.0;
            for tick in 0..800 {
                world.step();
                if tick == 399 {
                    start_height = coordinates(&world.state(BodyId(id)).unwrap())[1];
                }
                if tick >= 400 {
                    speeds.push(norm(velocity(&world, id)));
                }
            }
            let rms = (speeds.iter().map(|s| s * s).sum::<f64>() / speeds.len() as f64).sqrt();
            let max = speeds.iter().cloned().fold(0.0, f64::max);
            let end_height = coordinates(&world.state(BodyId(id)).unwrap())[1];
            println!("{name:<34} {substeps:<10} {rms:>14.2e} {max:>14.2e} {:>14.2e} {:>8}", end_height - start_height, world.is_asleep(BodyId(id)));
        }
    }
}

/// A cube on a floor with gravity tilted by an angle: stick below the friction angle, slide above it with a = g (sin t - mu cos t).
fn friction() {
    // Rapier averages the two colliders' coefficients: a ship's 0.6 and the terrain's default 0.5.
    let mu = 0.55f64;
    let threshold = mu.atan().to_degrees();
    println!("== FRICTION. A cube of 2 on a floor, gravity 5 m/s^2 tilted by an angle. Combined mu = (0.6 + 0.5) / 2 = {mu}, so it should slide above {threshold:.2} degrees. ==");
    println!("{:<10} {:<10} {:>16} {:>16} {:>10}", "angle", "substeps", "expected accel.", "measured accel.", "slides?");
    for angle in [20.0f64, 27.0, 28.5, 29.2, 31.0, 40.0, 60.0] {
        for substeps in [3u32, 12] {
            let a = angle.to_radians();
            let gravity = [5.0 * a.sin(), -5.0 * a.cos(), 0.0];
            let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings { gravity, substeps, ..BodySettings::default() });
            for x in -1..=1 {
                for z in -1..=1 {
                    world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Terrain(|_, y, _| y < 0));
                }
            }
            add(&mut world, 1, &Hull::cube(2), [10.0, 0.01, 10.0], [0.0; 3], [0.0; 3], BodyKind::Dynamic);
            for _ in 0..40 {
                world.step();
            }
            let v_before = velocity(&world, 1)[0];
            for _ in 0..40 {
                world.step();
            }
            let measured = (velocity(&world, 1)[0] - v_before) / 2.0;
            let expected = (5.0 * (a.sin() - mu * a.cos())).max(0.0);
            println!("{angle:<10} {substeps:<10} {expected:>16.4} {measured:>16.4} {:>10}", measured > 0.05);
        }
    }
}

/// A cube slides over a flat floor made of several bricks: does the seam between bricks snag it? Compare the stopping distance with the one friction predicts.
fn seam() {
    let mu = 0.55f64;
    println!("== SEAM. A cube of 2 slides at 8 m/s on a flat floor from x = 24, across the seam between two bricks at x = 32. Friction alone stops it after v^2 / (2 mu g). ==");
    println!("{:<10} {:>16} {:>16} {:>14} {:>14}", "substeps", "predicted stop x", "measured stop x", "max height", "min height");
    for substeps in [3u32, 12] {
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings { substeps, ..BodySettings::default() });
        for x in -1..=2 {
            for z in -1..=1 {
                world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Terrain(|_, y, _| y < 0));
            }
        }
        add(&mut world, 1, &Hull::cube(2), [24.0, 0.01, 10.0], [8.0, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
        let (mut high, mut low) = (f64::MIN, f64::MAX);
        for tick in 0..100 {
            world.step();
            if tick >= 5 {
                let y = coordinates(&world.state(BodyId(1)).unwrap())[1];
                high = high.max(y);
                low = low.min(y);
            }
        }
        let stop = coordinates(&world.state(BodyId(1)).unwrap())[0];
        println!("{substeps:<10} {:>16.3} {:>16.3} {high:>14.5} {low:>14.5}", 24.0 + 64.0 / (2.0 * mu * 5.0), stop);
    }
}

fn add_dense(world: &mut BodyWorld, id: u64, hull: &Hull, density: f64, position: [f64; 3], velocity: [f64; 3], kind: BodyKind) {
    let state = BodyState { position: at(position[0], position[1], position[2]), orientation: Orientation::IDENTITY, linear_velocity: velocity, angular_velocity: [0.0; 3] };
    world.add_body(BodyId(id), state, kind).unwrap();
    world.set_brick(BodyId(id), BrickCoordinate::ORIGIN, &mut &*hull).unwrap();
    world.set_mass(BodyId(id), hull.mass(density)).unwrap();
}

/// At what speed does a cube of 2 (8 kg) pass through a hull plate one voxel thick (8 x 8), and through one of two voxels? For plates that are light, heavy, held fixed.
fn thin() {
    println!("== THIN. A cube of 2 (8 kg) flies at a plate of 8x8 voxels. 'x' = it ended beyond the plate (tunnelled), '.' = it was stopped or pushed it. Speeds 5 10 20 30 40 50 75 100 200 m/s. ==");
    let speeds = [5.0f64, 10.0, 20.0, 30.0, 40.0, 50.0, 75.0, 100.0, 200.0];
    println!("{:<44} {:<10} {}", "plate", "substeps", "5 10 20 30 40 50 75 100 200");
    for (thickness, density, kind, name) in [
        (1i128, 1.0f64, BodyKind::Dynamic, "1 thick, 64 kg, free"),
        (1, 100.0, BodyKind::Dynamic, "1 thick, 6400 kg, free"),
        (1, 1.0, BodyKind::Fixed, "1 thick, fixed"),
        (2, 1.0, BodyKind::Dynamic, "2 thick, 128 kg, free"),
        (2, 1.0, BodyKind::Fixed, "2 thick, fixed"),
    ] {
        for substeps in [3u32, 12, 24] {
            let mut marks = String::new();
            for v in speeds {
                let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(substeps));
                add(&mut world, 1, &Hull::cube(2), [0.0, 13.0, 13.0], [v, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
                add_dense(&mut world, 2, &Hull::block(thickness, 8, 8), density, [30.0, 10.0, 10.0], [0.0; 3], kind);
                let ticks = (60.0 / v / 0.05).ceil() as usize + 30;
                for _ in 0..ticks {
                    world.step();
                }
                let cube = coordinates(&world.state(BodyId(1)).unwrap())[0];
                let plate = coordinates(&world.state(BodyId(2)).unwrap())[0];
                let through = cube > plate + thickness as f64 - 0.05;
                marks.push_str(&format!("{:>4}", if through { "x" } else { "." }));
            }
            println!("{name:<44} {substeps:<10} {marks}");
        }
    }
}

/// A cube hits a hull plate whose rotation is locked (a huge tensor of inertia), aimed off its centre, through the game's `BodyWorld` with its own choice of steps.
/// Judged in the plate's frame, which does not turn: the cube's centre beyond the plate's far face and inside its footprint at any tick.
fn locked() {
    println!("== LOCK. As the game steps it (speed-dependent steps, cap 24 per tick = 384 voxels/s). A cube of 2 (8 kg) hits a plate of t x 8 x 8 whose rotation is locked. THROUGH = the cube's centre got beyond the far face inside the footprint. ==");
    println!("{:<8} {:<10} {:<8} {}", "plate", "mass", "offset", "v = 20 / 50 / 100 / 200 / 300 / 380 / 500 m/s");
    for (thickness, density) in [(1i128, 1.0f64), (1, 100.0), (2, 1.0), (4, 1.0)] {
        for offset in [0.0f64, 0.5, 1.5, 3.0] {
            let mut cells = Vec::new();
            for v in [20.0f64, 50.0, 100.0, 200.0, 300.0, 380.0, 500.0] {
                let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(3));
                add(&mut world, 1, &Hull::cube(2), [0.0, 13.0 + offset, 13.0 + offset], [v, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
                let plate = Hull::block(thickness, 8, 8);
                let state = BodyState { position: at(30.0, 10.0, 10.0), orientation: Orientation::IDENTITY, linear_velocity: [0.0; 3], angular_velocity: [0.0; 3] };
                world.add_body(BodyId(2), state, BodyKind::Dynamic).unwrap();
                world.set_brick(BodyId(2), BrickCoordinate::ORIGIN, &mut &plate).unwrap();
                let mut mass = plate.mass(density);
                for row in 0..3 {
                    for column in 0..3 {
                        mass.inertia[row][column] *= 1.0e6;
                    }
                }
                world.set_mass(BodyId(2), mass).unwrap();
                let mut through = false;
                let ticks = (60.0 / v / 0.05).ceil() as usize + 60;
                for _ in 0..ticks {
                    world.step();
                    let (c, p) = (world.state(BodyId(1)).unwrap(), world.state(BodyId(2)).unwrap());
                    let (cc, pc) = (coordinates(&c), coordinates(&p));
                    // Centres: cube origin + (1,1,1), plate origin + (t/2, 4, 4); neither turns much.
                    let local = [cc[0] + 1.0 - (pc[0] + thickness as f64 / 2.0), cc[1] + 1.0 - (pc[1] + 4.0), cc[2] + 1.0 - (pc[2] + 4.0)];
                    if local[0] > thickness as f64 / 2.0 && local[1].abs() < 4.0 && local[2].abs() < 4.0 {
                        through = true;
                    }
                }
                cells.push(if through { "THROUGH" } else { "ok" });
            }
            println!("{:<8} {:<10} {offset:<8} {}", format!("{thickness} thick"), format!("{} kg", 8 * thickness * 8 * density as i128), cells.join(" / "));
        }
    }
}

/// At what speed does a body alone in space stop dead? Scan, and also the cube-hits-heavy-cube case that showed it.
fn threshold() {
    println!("== THRESHOLD. A cube alone in space at v: does it still move after 20 s? ==");
    for v in [0.02f64, 0.05, 0.08, 0.1, 0.12, 0.15, 0.18, 0.2] {
        let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(3));
        add(&mut world, 1, &Hull::cube(4), [0.0, 10.0, 10.0], [v, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
        let mut slept_at = None;
        for tick in 0..400 {
            world.step();
            if slept_at.is_none() && world.is_asleep(BodyId(1)) {
                slept_at = Some(tick as f64 * 0.05);
            }
        }
        println!("  v = {v:<5} final speed {:.4}  fell asleep at {}", norm(velocity(&world, 1)), slept_at.map_or("never".to_string(), |t| format!("{t:.2} s")));
    }
    println!("== a body of 512 kg given 10 kg m/s (v = 0.0195 m/s), and a body of 64 kg pushed to 0.15 m/s then left alone ==");
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, zero_gravity(3));
    add(&mut world, 1, &Hull::cube(8), [0.0, 10.0, 10.0], [0.0195, 0.0, 0.0], [0.0; 3], BodyKind::Dynamic);
    for _ in 0..100 {
        world.step();
    }
    println!("  512 kg at 0.0195 m/s after 5 s: speed {:.5}, asleep {}", norm(velocity(&world, 1)), world.is_asleep(BodyId(1)));
}
