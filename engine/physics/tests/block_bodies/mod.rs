//! What the tests of bodies that are cubes share: cubes of voxels, a chain of them coupled end to end, and the ways to couple two (`settled_hit.rs`, `contact_ghost.rs`).

#![allow(dead_code)] // Each test file uses some of it.

use physics::{
    BodyId, BodyKind, BodyMass, BodySettings, BodyState, BodyWorld, Collision, CollisionSource,
    CouplingId, CouplingOptions, MassMoments, PivotPair,
};
use voxel_space::{BrickCoordinate, Orientation, VoxelCoordinate, WorldPosition};

pub const TICK: f64 = 1.0 / 20.0;

/// The mass of a voxel: a cube of one voxel weighs 1000.
pub const DENSITY: f64 = 1000.0;

/// The cars of the chain, and the one that hits them.
pub const CARS: usize = 4;

pub const STRANGER: u64 = CARS as u64 + 1;

/// A source whose one whole cube has `side` voxels along each edge, at the origin of its body (a cube of 1 is the voxel at the origin).
pub struct Block(pub i128);

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

pub fn block_mass(side: i128) -> BodyMass {
    let voxels = (0..side).flat_map(|x| {
        (0..side)
            .flat_map(move |y| (0..side).map(move |z| MassMoments::of_voxel([x, y, z], DENSITY)))
    });
    MassMoments::sum(voxels)
        .properties()
        .expect("a cube has a mass")
}

/// A cube of `side` voxels along each edge (up to 32: one brick) with its origin, its lower corner, at `origin`, going at `velocity`.
pub fn add_block(world: &mut BodyWorld, id: u64, side: i128, origin: [f64; 3], velocity: [f64; 3]) {
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
pub fn add_cube(world: &mut BodyWorld, id: u64, origin: [f64; 3], velocity: [f64; 3]) {
    add_block(world, id, 1, origin, velocity);
}

pub fn velocity(world: &BodyWorld, id: u64) -> [f64; 3] {
    world.state(BodyId(id)).expect("a body").linear_velocity
}

/// How fast a body turns, about its fastest axis.
pub fn spin(world: &BodyWorld, id: u64) -> f64 {
    world
        .state(BodyId(id))
        .expect("a body")
        .angular_velocity
        .iter()
        .fold(0.0, |most: f64, part| most.max(part.abs()))
}

/// How two cubes that touch along x are held together.
pub struct Coupling {
    pub name: &'static str,
    pub pivots: Vec<PivotPair>,
    /// Whether the cubes of a pair meet each other (the game's coupling: they do).
    pub contacts_between_cars: bool,
}

pub fn pivot(y: f64, z: f64) -> PivotPair {
    PivotPair {
        first: [1.0, y, z],
        second: [0.0, y, z],
    }
}

pub fn couplings() -> Vec<Coupling> {
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

/// `CARS` cubes end to end along x, at rest, each coupled to the next as `coupling` says.
pub fn add_chain(world: &mut BodyWorld, coupling: &Coupling) {
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

/// The game's settings with no gravity and a tick cut into `substeps` steps.
pub fn settings_with(substeps: u32) -> BodySettings {
    BodySettings {
        substeps,
        gravity: [0.0; 3],
        ..BodySettings::default()
    }
}

/// An empty world in free space.
pub fn free_space(substeps: u32) -> BodyWorld {
    BodyWorld::new(BrickCoordinate::ORIGIN, settings_with(substeps))
}
