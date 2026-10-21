//! What it costs to simulate ships: stepping, and making the colliders of a brick again after an edit, for ships of 10,000 and 100,000 blocks,
//! and for a hundred ships. Run with `cargo run --release -p physics --example ship_cost`; the numbers are for the machine they are run on.

use std::collections::HashSet;
use std::time::Instant;

use physics::{
    BodyId, BodyKind, BodySettings, BodyState, BodyWorld, BrickHint, Collision, CollisionSource,
    MassMoments,
};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

struct Ground;

impl CollisionSource for Ground {
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

/// A ship of `size` voxels along each edge, as a set.
struct Hull(HashSet<VoxelCoordinate>);

impl CollisionSource for &Hull {
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if self.0.contains(&voxel) {
            Collision::Full
        } else {
            Collision::Empty
        }
    }
}

fn cube(x: i64, y: i64, z: i64) -> Hull {
    let mut voxels = HashSet::new();
    for a in 0..x {
        for b in 0..y {
            for c in 0..z {
                voxels.insert(VoxelCoordinate::new(a, b, c));
            }
        }
    }
    Hull(voxels)
}

fn mass_of(hull: &Hull) -> physics::BodyMass {
    let mut voxels: Vec<&VoxelCoordinate> = hull.0.iter().collect();
    voxels.sort();
    MassMoments::sum(
        voxels
            .into_iter()
            .map(|v| MassMoments::of_voxel([v.x(), v.y(), v.z()], 1.0)),
    )
    .properties()
    .unwrap()
}

fn bricks_of(hull: &Hull) -> Vec<BrickCoordinate> {
    let mut bricks: Vec<BrickCoordinate> = hull.0.iter().map(|voxel| voxel.split().0).collect();
    bricks.sort();
    bricks.dedup();
    bricks
}

fn start(x: f32, y: f32, z: f32) -> BodyState {
    BodyState {
        position: WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
        orientation: Orientation::IDENTITY,
        linear_velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
    }
}

fn world_with_ground(radius: i64) -> BodyWorld {
    let mut world = BodyWorld::new(BrickCoordinate::ORIGIN, BodySettings::default());
    for x in -radius..=radius {
        for z in -radius..=radius {
            world.set_terrain_brick(BrickCoordinate::new(x, -1, z), &mut Ground);
        }
    }
    world
}

fn milliseconds(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn one_ship(label: &str, x: i64, y: i64, z: i64) {
    let mut world = world_with_ground(4);
    let hull = cube(x, y, z);
    let blocks = hull.0.len();
    world
        .add_body(BodyId(1), start(8.0, 0.5, 8.0), BodyKind::Dynamic)
        .unwrap();
    let started = Instant::now();
    let bricks = bricks_of(&hull);
    for brick in &bricks {
        world.set_brick(BodyId(1), *brick, &mut &hull).unwrap();
    }
    let build = milliseconds(started);
    world.set_mass(BodyId(1), mass_of(&hull)).unwrap();
    // A ship that is moved every tick is awake: a force too small to see keeps it so.
    let mut worst: f64 = 0.0;
    let mut total = 0.0;
    for _ in 0..200 {
        world.apply_force(BodyId(1), [1.0, 0.0, 0.0], None).unwrap();
        let started = Instant::now();
        world.step();
        let took = milliseconds(started);
        worst = worst.max(took);
        total += took;
    }
    println!(
        "{label}: {blocks} blocks in {} bricks: colliders made in {build:.1} ms; awake, a tick of 3 steps takes {:.3} ms on average, {worst:.3} ms at worst",
        bricks.len(),
        total / 200.0
    );
    // One block changes in each tick, as a ship that is being built or shot at: the brick it is in is made again.
    let mut hull = hull;
    let first = bricks[0];
    let target = first.origin();
    let mut worst: f64 = 0.0;
    let mut total = 0.0;
    for tick in 0..100 {
        let voxel = VoxelCoordinate::new(
            target.x() + tick % 30,
            target.y() + tick / 30,
            target.z() + 1,
        );
        if !hull.0.remove(&voxel) {
            hull.0.insert(voxel);
        }
        let started = Instant::now();
        world.set_brick(BodyId(1), first, &mut &hull).unwrap();
        let took = milliseconds(started);
        worst = worst.max(took);
        total += took;
    }
    println!(
        "    one block edited a tick: its brick is made again in {:.3} ms on average, {worst:.3} ms at worst",
        total / 100.0
    );
}

fn many_ships() {
    let mut world = world_with_ground(8);
    let hull = cube(12, 12, 16);
    let mass = mass_of(&hull);
    let brick = BrickCoordinate::ORIGIN;
    for index in 0..100u64 {
        let (column, row) = ((index % 10) as f32, (index / 10) as f32);
        world
            .add_body(
                BodyId(index),
                start(
                    column * 20.0 - 100.0,
                    2.0 + (index % 3) as f32 * 3.0,
                    row * 20.0 - 100.0,
                ),
                BodyKind::Dynamic,
            )
            .unwrap();
        world.set_brick(BodyId(index), brick, &mut &hull).unwrap();
        world.set_mass(BodyId(index), mass).unwrap();
    }
    let mut worst: f64 = 0.0;
    let mut total = 0.0;
    let mut worst_settled: f64 = 0.0;
    for tick in 0..300 {
        let started = Instant::now();
        world.step();
        let took = milliseconds(started);
        if tick < 100 {
            worst = worst.max(took);
        } else {
            worst_settled = worst_settled.max(took);
        }
        total += took;
    }
    println!(
        "100 ships of {} blocks dropped on the ground: {:.3} ms a tick on average over 300 ticks, {worst:.3} ms at worst while landing, {worst_settled:.3} ms at worst afterwards",
        hull.0.len(),
        total / 300.0
    );
}

fn main() {
    one_ship("a ship of about 10,000 blocks", 22, 22, 21);
    one_ship("a ship of about 100,000 blocks", 47, 47, 46);
    many_ships();
}
