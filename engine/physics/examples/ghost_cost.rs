//! What the contact ghost costs (step 4c-5-2 of `docs/decisions/joints-and-cars.md`): a mechanism of four cars that are cubes of `SIDE` voxels (10,648 blocks each at 22), coupled end to
//! end as the game couples them, a stranger cube of the same size, and the ghost between them. Run with `cargo run --release -p physics --example ghost_cost`; the numbers are for
//! the machine they are run on, and every row is measured in one run, so under the same load.
//!
//! Rows (microseconds a tick, the average over the ticks of the row): the mechanism's own world and the stranger's, stepped alone (what they cost with no ghost); what the ghost
//! adds (telling it where its bodies are, stepping it, giving back the corrections); and the same while the stranger is far off, near and not touching, and in a hit.
//! `SIDE=n` in the environment changes the size of the cubes.

use std::collections::HashSet;
use std::time::Instant;

use physics::{
    BodyId, BodyKind, BodySettings, BodyState, BodyWorld, Collision, CollisionSource, ContactGhost,
    CouplingId, CouplingOptions, MassMoments, PivotPair, Side,
};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

const CARS: u64 = 4;
const STRANGER: u64 = 10;

struct Cube(HashSet<VoxelCoordinate>);

impl CollisionSource for &Cube {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if self.0.contains(&voxel) {
            Collision::Full
        } else {
            Collision::Empty
        }
    }
}

fn cube(side: i128) -> Cube {
    let mut voxels = HashSet::new();
    for x in 0..side {
        for y in 0..side {
            for z in 0..side {
                voxels.insert(VoxelCoordinate::new(x, y, z));
            }
        }
    }
    Cube(voxels)
}

fn state(position: [f64; 3], velocity: [f64; 3]) -> BodyState {
    BodyState {
        position: WorldPosition::new(BrickCoordinate::ORIGIN, position),
        orientation: Orientation::IDENTITY,
        linear_velocity: velocity,
        angular_velocity: [0.0; 3],
    }
}

fn add_cube(
    world: &mut BodyWorld,
    id: u64,
    shape: &Cube,
    side: i128,
    at: [f64; 3],
    velocity: [f64; 3],
) {
    world
        .add_body(BodyId(id), state(at, velocity), BodyKind::Dynamic)
        .unwrap();
    let mut bricks: Vec<BrickCoordinate> = shape.0.iter().map(|voxel| voxel.split().0).collect();
    bricks.sort();
    bricks.dedup();
    for brick in bricks {
        world.set_brick(BodyId(id), brick, &mut &*shape).unwrap();
    }
    let mut voxels: Vec<&VoxelCoordinate> = shape.0.iter().collect();
    voxels.sort();
    let mass = MassMoments::sum(
        voxels
            .into_iter()
            .map(|voxel| MassMoments::of_voxel([voxel.x(), voxel.y(), voxel.z()], 1.0)),
    )
    .properties()
    .unwrap();
    world.set_mass(BodyId(id), mass).unwrap();
    let _ = side;
}

struct Scene {
    chain: BodyWorld,
    stranger: BodyWorld,
    ghost: ContactGhost,
}

fn scene(side: i128, stranger_at: [f64; 3], stranger_velocity: [f64; 3]) -> Scene {
    let settings = BodySettings {
        gravity: [0.0; 3],
        ..BodySettings::default()
    };
    let shape = cube(side);
    let mut chain = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    for car in 0..CARS {
        add_cube(
            &mut chain,
            car + 1,
            &shape,
            side,
            [car as f64 * side as f64, 40.0, 40.0],
            [0.0; 3],
        );
    }
    let middle = side as f64 / 2.0;
    for car in 0..CARS - 1 {
        chain
            .couple(
                CouplingId(car + 1),
                BodyId(car + 1),
                BodyId(car + 2),
                &[PivotPair {
                    first: [side as f64, middle, middle],
                    second: [0.0, middle, middle],
                }],
                CouplingOptions::default(),
            )
            .unwrap();
    }
    let mut stranger = BodyWorld::new(BrickCoordinate::ORIGIN, settings);
    add_cube(
        &mut stranger,
        STRANGER,
        &shape,
        side,
        stranger_at,
        stranger_velocity,
    );
    let mut ghost = ContactGhost::new(BrickCoordinate::ORIGIN, settings);
    for car in 1..=CARS {
        let at = chain.state(BodyId(car)).unwrap();
        ghost
            .add_body_from(Side(0), &chain, BodyId(car), at)
            .unwrap();
    }
    for car in 1..CARS {
        ghost.add_coupling_from(&chain, CouplingId(car)).unwrap();
    }
    let at = stranger.state(BodyId(STRANGER)).unwrap();
    ghost
        .add_body_from(Side(1), &stranger, BodyId(STRANGER), at)
        .unwrap();
    Scene {
        chain,
        stranger,
        ghost,
    }
}

/// Microseconds, the average of each part over `ticks` ticks: the chain's step, the stranger's step, the ghost's refresh, the ghost's step, the corrections given back; and how
/// many of the ticks the sides touched in.
fn run(scene: &mut Scene, ticks: usize) -> ([f64; 5], usize) {
    let mut total = [0.0f64; 5];
    let mut touching = 0;
    for _ in 0..ticks {
        let started = Instant::now();
        for car in 1..=CARS {
            let at = scene.chain.state(BodyId(car)).unwrap();
            scene.ghost.refresh(BodyId(car), &scene.chain, at).unwrap();
        }
        let at = scene.stranger.state(BodyId(STRANGER)).unwrap();
        scene
            .ghost
            .refresh(BodyId(STRANGER), &scene.stranger, at)
            .unwrap();
        total[2] += started.elapsed().as_secs_f64();
        let started = Instant::now();
        scene.ghost.step(scene.chain.steps_this_tick());
        total[3] += started.elapsed().as_secs_f64();
        let started = Instant::now();
        scene.chain.step();
        total[0] += started.elapsed().as_secs_f64();
        let started = Instant::now();
        scene.stranger.step();
        total[1] += started.elapsed().as_secs_f64();
        let started = Instant::now();
        if scene.ghost.touched() {
            touching += 1;
        }
        for id in (1..=CARS).chain([STRANGER]) {
            if let Some(correction) = scene.ghost.correction(BodyId(id)) {
                let world = if id == STRANGER {
                    &mut scene.stranger
                } else {
                    &mut scene.chain
                };
                world.apply_correction(BodyId(id), &correction).unwrap();
            }
        }
        total[4] += started.elapsed().as_secs_f64();
    }
    (total.map(|seconds| seconds * 1e6 / ticks as f64), touching)
}

fn row(name: &str, scene: &mut Scene, ticks: usize) {
    let (cost, touching) = run(scene, ticks);
    println!(
        "{name:<46} chain {:>7.1}  stranger {:>7.1}  refresh {:>7.1}  ghost step {:>8.1}  corrections {:>6.1}   touched in {touching} of {ticks}",
        cost[0], cost[1], cost[2], cost[3], cost[4]
    );
}

fn main() {
    let side: i128 = std::env::var("SIDE")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(22);
    println!(
        "four cars of {side}x{side}x{side} voxels, coupled end to end, and a stranger of the same size; microseconds a tick (the average over the ticks of the row)"
    );
    // Far off, at rest: the ghost has nothing to do.
    let mut far = scene(side, [400.0, 40.0, 40.0], [0.0; 3]);
    row("stranger 300 voxels off, at rest", &mut far, 200);
    // Near and not touching: a gap of three voxels at the end of the chain, at rest.
    let mut near = scene(
        side,
        [(CARS as i128 * side) as f64 + 3.0, 40.0, 40.0],
        [0.0; 3],
    );
    row("stranger 3 voxels off the end, at rest", &mut near, 200);
    // A hit: the stranger comes at 20 voxels a second from 40 voxels off, and the row is the 60 ticks over which it arrives and the chain takes it.
    let mut hit = scene(
        side,
        [(CARS as i128 * side) as f64 + 40.0, 40.0, 40.0],
        [-20.0, 0.0, 0.0],
    );
    row("stranger at 20 voxels a second, its hit", &mut hit, 60);
    // Afterwards: the two go on together, touching, for two seconds.
    row("the same, 40 ticks later (in contact)", &mut hit, 40);
}
