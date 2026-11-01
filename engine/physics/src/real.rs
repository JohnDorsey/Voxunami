//! Where the crate's numbers cross into Rapier.
//!
//! Rapier here works in `f64` (the `rapier3d-f64` package, with `enhanced-determinism`, which fixes the order of its work and its transcendental
//! functions, so the same inputs give the same bits on every machine). The crate's own interface keeps `f32` for what is small and was
//! always `f32` (speeds, forces, settings, the offset in a [`WorldPosition`](voxel_space::WorldPosition)); these functions are the only
//! places that change one into the other, so a number is never rounded by accident.

use rapier3d::prelude::{Matrix, Real, Vector};
use voxel_space::{BRICK_EDGE_LENGTH, BrickCoordinate, WorldPosition};

/// An `f32` as Rapier's number: exact.
pub(crate) fn real(value: f32) -> Real {
    Real::from(value)
}

/// Rapier's number as an `f32`: rounded to the nearest, which is the only loss.
pub(crate) fn single(value: Real) -> f32 {
    value as f32
}

pub(crate) fn vector(components: [f32; 3]) -> Vector {
    Vector::new(
        real(components[0]),
        real(components[1]),
        real(components[2]),
    )
}

pub(crate) fn array(vector: Vector) -> [f32; 3] {
    [single(vector.x), single(vector.y), single(vector.z)]
}

pub(crate) fn matrix(columns: &[[f32; 3]; 3]) -> Matrix {
    Matrix::from_cols_array_2d(&columns.map(|column| column.map(real)))
}

/// The position in the world of a place `local` voxels from the corner of the brick `origin`, or `None` if it is not finite or leaves the
/// world. The whole bricks are carried out in `Real`, so that only the offset inside a brick is rounded to `f32` (a couple of micrometres),
/// however far from the origin the place is.
pub(crate) fn world_position(origin: BrickCoordinate, local: Vector) -> Option<WorldPosition> {
    if !local.is_finite() {
        return None;
    }
    let edge = BRICK_EDGE_LENGTH as Real;
    let components = [local.x, local.y, local.z];
    let carries = components.map(|component| (component / edge).floor());
    let offset = [0, 1, 2].map(|axis| single(components[axis] - carries[axis] * edge));
    let brick =
        origin.checked_offset(carries[0] as i128, carries[1] as i128, carries[2] as i128)?;
    WorldPosition::try_new(brick, offset)
}
