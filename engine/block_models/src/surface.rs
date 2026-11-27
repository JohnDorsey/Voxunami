//! Where a ray first meets the surface of a [`Shape`], and which place on a face's picture that is.
//!
//! A voxel's shape is made of boxes on a grid of eighths ([`SUBDIVISIONS`]), so the surface is the sides of filled sub-cells that have an empty one (or the world) beside
//! them. The walk below goes from sub-cell to sub-cell the way a ray goes through voxels, and stops at the first filled one. It is plain arithmetic on `f64` and nothing
//! from a graphics card, so a server can use it (a beam that pushes a ship at a point) as well as a painting tool.

use voxel_space::FaceDirection;

use crate::shape::{SUBDIVISIONS, Shape};
use crate::table::FacePicture;

/// A place on a picture, as the shares of its width and height from the picture's left and top: 0 to 1 each, so that a picture of any size has the same place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PicturePlace {
    /// From the left edge to the right.
    pub across: f64,
    /// From the top edge down.
    pub down: f64,
}

impl PicturePlace {
    /// The pixel that holds this place in a picture `size` pixels across and down, as `[column, row]`; a place on the far edge belongs to the last pixel.
    pub fn pixel(self, size: usize) -> [usize; 2] {
        let find = |share: f64| ((share * size as f64).floor().max(0.0) as usize).min(size - 1);
        [find(self.across), find(self.down)]
    }
}

impl FacePicture {
    /// Where a point of the voxel (its own coordinates, 0 to 1; a point on the face this picture is on) is on the picture. It is the place the renderer samples there: the
    /// picture's coordinates grow along `across` and, downward, against `up`, and a picture repeats on each voxel (a negative `across` runs from the voxel's far side).
    pub fn place_of(self, point: [f64; 3]) -> PicturePlace {
        let along = |direction: FaceDirection| {
            let coordinate = point[direction.axis()];
            if direction.is_positive() {
                coordinate
            } else {
                1.0 - coordinate
            }
        };
        PicturePlace {
            across: along(self.across),
            down: 1.0 - along(self.up),
        }
    }
}

/// Where a ray first meets a shape, in the voxel's own coordinates (the voxel is the cube from 0 to 1 on each axis).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceHit {
    /// The way the surface looks: the outward normal of the side that was hit. A ray going toward +x hits a face that looks toward -x.
    pub face: FaceDirection,
    /// How far along the ray, in voxels, it meets the surface (the ray's direction is taken as a unit vector).
    pub distance: f64,
    /// The point, in the voxel's own coordinates. The part along the face's axis is exactly on the plane of the side (a multiple of an eighth).
    pub point: [f64; 3],
}

impl Shape {
    /// The first place a ray meets this shape, from `origin` along `direction` (the voxel's own coordinates, 0 to 1 across it), within `longest` voxels of travel; `None` if it
    /// misses, or the shape is empty. The direction need not be a unit vector; a zero or non-finite one hits nothing.
    ///
    /// A ray that begins outside the voxel meets the shape where it enters the voxel if the sub-cell there is filled, else at the first filled sub-cell it enters. A ray that
    /// begins inside the voxel ignores the sub-cell it begins in, as the walk through voxels ignores the voxel it begins in, and meets the first it enters.
    pub fn first_surface(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        longest: f64,
    ) -> Option<SurfaceHit> {
        let length = (direction[0] * direction[0]
            + direction[1] * direction[1]
            + direction[2] * direction[2])
            .sqrt();
        if length == 0.0 || !length.is_finite() || origin.iter().any(|value| !value.is_finite()) {
            return None;
        }
        // Work in sub-cells, along a unit direction: a boundary between sub-cells is at a whole number.
        let scale = SUBDIVISIONS as f64;
        let direction = direction.map(|component| component / length);
        let start = origin.map(|component| component * scale);
        let farthest = longest * scale;

        // Where the ray is inside the voxel's cube, from `enter` to `leave`, and through which axis it came in.
        let mut enter = f64::NEG_INFINITY;
        let mut leave = f64::INFINITY;
        let mut entered_through = None;
        for axis in 0..3 {
            if direction[axis] == 0.0 {
                if start[axis] < 0.0 || start[axis] > scale {
                    return None;
                }
                continue;
            }
            let first = (0.0 - start[axis]) / direction[axis];
            let second = (scale - start[axis]) / direction[axis];
            let (low, high) = if first < second {
                (first, second)
            } else {
                (second, first)
            };
            if low > enter {
                enter = low;
                entered_through = Some(axis);
            }
            leave = leave.min(high);
        }
        if enter > leave || leave < 0.0 {
            return None;
        }

        // The sub-cell the walk starts in, and the first boundary of each axis beyond it.
        let begins_outside = enter >= 0.0;
        let from = if begins_outside { enter } else { 0.0 };
        let mut cell = [0usize; 3];
        for axis in 0..3 {
            let place = start[axis] + direction[axis] * from;
            cell[axis] = (place.floor().max(0.0) as usize).min(SUBDIVISIONS - 1);
        }
        if begins_outside && self.fills(cell) {
            let axis = entered_through?;
            if enter > farthest {
                return None;
            }
            let face = face_of(axis, direction[axis] < 0.0);
            return Some(hit_at(start, direction, enter, scale, face));
        }
        let mut next_boundary = [f64::INFINITY; 3];
        let mut spacing = [f64::INFINITY; 3];
        for axis in 0..3 {
            if direction[axis] != 0.0 {
                let boundary = cell[axis] as f64 + if direction[axis] > 0.0 { 1.0 } else { 0.0 };
                next_boundary[axis] = (boundary - start[axis]) / direction[axis];
                spacing[axis] = 1.0 / direction[axis].abs();
            }
        }
        loop {
            let axis = (0..3)
                .min_by(|&first, &second| next_boundary[first].total_cmp(&next_boundary[second]))
                .expect("three axes");
            let at = next_boundary[axis];
            if at > leave || at > farthest {
                return None;
            }
            if direction[axis] > 0.0 {
                cell[axis] += 1;
            } else if cell[axis] == 0 {
                return None;
            } else {
                cell[axis] -= 1;
            }
            if cell[axis] >= SUBDIVISIONS {
                return None;
            }
            next_boundary[axis] += spacing[axis];
            if self.fills(cell) {
                let face = face_of(axis, direction[axis] < 0.0);
                return Some(hit_at(start, direction, at, scale, face));
            }
        }
    }
}

/// The face that looks toward the ray's origin when the ray crosses a plane of `axis`: that of the side it comes from.
fn face_of(axis: usize, going_negative: bool) -> FaceDirection {
    FaceDirection::from_index((axis * 2) as u8 + u8::from(going_negative))
        .expect("an axis of three has a face each way")
}

fn hit_at(
    start: [f64; 3],
    direction: [f64; 3],
    at: f64,
    scale: f64,
    face: FaceDirection,
) -> SurfaceHit {
    let mut point = [0.0; 3];
    for axis in 0..3 {
        point[axis] = (start[axis] + direction[axis] * at) / scale;
    }
    // (The plane that was crossed is a multiple of an eighth, and is put there exactly rather than where the arithmetic landed.)
    let plane = (start[face.axis()] + direction[face.axis()] * at).round();
    point[face.axis()] = plane / scale;
    SurfaceHit {
        face,
        distance: at / scale,
        point,
    }
}

#[cfg(test)]
mod tests;
