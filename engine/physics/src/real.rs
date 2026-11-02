//! Where the crate's numbers meet Rapier's.
//!
//! Rapier here works in `f64` (the `rapier3d-f64` package, with `enhanced-determinism`, which fixes the order of its work and its transcendental functions, so the same
//! inputs give the same bits on every machine), and so does the crate's own interface: a speed, a force, a setting, the offset in a
//! [`WorldPosition`](voxel_space::WorldPosition) are `f64`, and nothing is rounded on the way in or out. (Until 2026-11-02 the interface was `f32` and these functions
//! were the places where a number was rounded.)

use rapier3d::prelude::{Matrix, Real, Vector};
use voxel_space::{BRICK_EDGE_LENGTH, BrickCoordinate, WorldPosition};

pub(crate) fn vector(components: [f64; 3]) -> Vector {
    Vector::new(components[0], components[1], components[2])
}

pub(crate) fn array(vector: Vector) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

pub(crate) fn matrix(columns: &[[f64; 3]; 3]) -> Matrix {
    Matrix::from_cols_array_2d(columns)
}

/// The position in the world of a place `local` voxels from the corner of the brick `origin`, or `None` if it is not finite or leaves the
/// world. The whole bricks are carried out, so that only the offset inside a brick is an `f64` (4e-15 voxel at the most), however far from the origin the place is.
pub(crate) fn world_position(origin: BrickCoordinate, local: Vector) -> Option<WorldPosition> {
    if !local.is_finite() {
        return None;
    }
    let edge = BRICK_EDGE_LENGTH as Real;
    let components = [local.x, local.y, local.z];
    let carries = components.map(|component| (component / edge).floor());
    let offset = [0, 1, 2].map(|axis| components[axis] - carries[axis] * edge);
    let brick =
        origin.checked_offset(carries[0] as i128, carries[1] as i128, carries[2] as i128)?;
    WorldPosition::try_new(brick, offset)
}
