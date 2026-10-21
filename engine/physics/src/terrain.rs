use std::sync::Arc;

use block_models::ShapeBox;
use rapier3d::prelude::*;
use voxel_space::{BRICK_EDGE_LENGTH, BrickCoordinate, FaceDirection, VoxelCoordinate};

/// What a voxel is to something moving through it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Collision {
    /// Bodies pass through (air, flames).
    Empty,
    /// A whole cube.
    Full,
    /// Boxes inside the voxel, in eighths of it (a slab, a stair).
    Partial(Arc<[ShapeBox]>),
}

/// What a source knows about a whole brick without looking at its voxels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrickHint {
    /// Nothing known: every voxel is asked.
    Unknown,
    /// Nothing in it for a body to touch.
    Empty,
    /// Every voxel of it is a whole cube.
    Solid,
}

/// Where the terrain a body moves through comes from.
pub trait CollisionSource {
    /// What the voxel is. Where there is nothing to read (terrain not available, the edge of the
    /// world) answer [`Collision::Full`], so that bodies stop there instead of falling into the void.
    fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision;

    /// What is known of a whole brick, to skip the work of asking about every voxel of an empty one.
    fn brick_hint(&mut self, _brick: BrickCoordinate) -> BrickHint {
        BrickHint::Unknown
    }
}

const EDGE: usize = BRICK_EDGE_LENGTH;

/// How far, in voxels, the boxes of a brick reach into the next brick where the terrain is solid on both
/// sides of the seam.
const SEAM_OVERLAP: f32 = 0.05;

/// The colliders of one brick of terrain, in a frame whose origin is the corner of `origin`.
pub(crate) struct BrickColliders {
    /// Every box of the brick, as one compound.
    pub terrain: Option<Collider>,
}

#[derive(Clone)]
enum Cell {
    Empty,
    Full,
    Partial(usize),
}

pub(crate) fn build_brick(
    source: &mut (impl CollisionSource + ?Sized),
    brick: BrickCoordinate,
    origin: BrickCoordinate,
) -> BrickColliders {
    let hint = source.brick_hint(brick);
    if hint == BrickHint::Empty {
        return BrickColliders { terrain: None };
    }
    let first = brick.origin();
    let mut shapes: Vec<Arc<[ShapeBox]>> = Vec::new();
    let solid = hint == BrickHint::Solid;
    let mut cells = vec![if solid { Cell::Full } else { Cell::Empty }; EDGE * EDGE * EDGE];
    let index = |x: usize, y: usize, z: usize| (z * EDGE + y) * EDGE + x;
    for z in 0..EDGE {
        for y in 0..EDGE {
            for x in 0..EDGE {
                if solid {
                    continue; // every voxel is known to be a whole cube
                }
                let voxel = first
                    .checked_offset(x as i64, y as i64, z as i64)
                    .expect("the voxels of a brick are within range");
                cells[index(x, y, z)] = match source.collision_at(voxel) {
                    Collision::Empty => Cell::Empty,
                    Collision::Full => Cell::Full,
                    Collision::Partial(boxes) => {
                        let position = shapes.iter().position(|known| *known == boxes);
                        Cell::Partial(position.unwrap_or_else(|| {
                            shapes.push(boxes);
                            shapes.len() - 1
                        }))
                    }
                };
            }
        }
    }
    // Whether the voxel next to a local one is a whole cube, looking outside the brick if it has to.
    let mut full_beside = |x: usize, y: usize, z: usize, face: FaceDirection| -> bool {
        let [dx, dy, dz] = face.offset();
        let (nx, ny, nz) = (x as i64 + dx, y as i64 + dy, z as i64 + dz);
        if (0..EDGE as i64).contains(&nx)
            && (0..EDGE as i64).contains(&ny)
            && (0..EDGE as i64).contains(&nz)
        {
            matches!(
                cells[index(nx as usize, ny as usize, nz as usize)],
                Cell::Full
            )
        } else {
            first
                .checked_offset(nx, ny, nz)
                .is_none_or(|voxel| matches!(source.collision_at(voxel), Collision::Full))
        }
    };
    let mut boxes = Vec::new();
    // Whole cubes are merged into as few boxes as possible, so that the floor under a body is one flat
    // surface and not a grid of tops with seams between them for a body to catch on.
    let mut taken = vec![false; EDGE * EDGE * EDGE];
    for z in 0..EDGE {
        for y in 0..EDGE {
            for x in 0..EDGE {
                let free = |x: usize, y: usize, z: usize, taken: &[bool]| {
                    matches!(cells[index(x, y, z)], Cell::Full) && !taken[index(x, y, z)]
                };
                if !free(x, y, z, &taken) {
                    continue;
                }
                let mut width = 1;
                while x + width < EDGE && free(x + width, y, z, &taken) {
                    width += 1;
                }
                let mut height = 1;
                'up: while y + height < EDGE {
                    for dx in 0..width {
                        if !free(x + dx, y + height, z, &taken) {
                            break 'up;
                        }
                    }
                    height += 1;
                }
                let mut depth = 1;
                'back: while z + depth < EDGE {
                    for dy in 0..height {
                        for dx in 0..width {
                            if !free(x + dx, y + dy, z + depth, &taken) {
                                break 'back;
                            }
                        }
                    }
                    depth += 1;
                }
                for dz in 0..depth {
                    for dy in 0..height {
                        for dx in 0..width {
                            taken[index(x + dx, y + dy, z + dz)] = true;
                        }
                    }
                }
                // A box that is buried inside the terrain, touching nothing open, is not needed.
                let mut touches_open =
                    |x: usize, y: usize, z: usize, width: usize, height: usize, depth: usize| {
                        let mut open = false;
                        for dz in 0..depth {
                            for dy in 0..height {
                                for dx in 0..width {
                                    open |= FaceDirection::ALL
                                        .iter()
                                        .any(|&face| !full_beside(x + dx, y + dy, z + dz, face));
                                }
                            }
                        }
                        open
                    };
                if touches_open(x, y, z, width, height, depth) {
                    // Where the box reaches the edge of the brick and the terrain goes on solid in the
                    // next one, it reaches a little way into it, so that the boxes of two bricks overlap
                    // and a body crossing from one to the other does not catch on the seam.
                    let start = [x, y, z];
                    let size = [width, height, depth];
                    let mut low = [0.0f32; 3];
                    let mut high = [0.0f32; 3];
                    for axis in 0..3 {
                        for positive in [false, true] {
                            let on_edge = if positive {
                                start[axis] + size[axis] == EDGE
                            } else {
                                start[axis] == 0
                            };
                            if !on_edge {
                                continue;
                            }
                            let face = FaceDirection::ALL
                                .into_iter()
                                .find(|face| face.axis() == axis && face.is_positive() == positive)
                                .expect("every axis has both faces");
                            let layer = if positive { EDGE - 1 } else { 0 };
                            let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
                            let mut solid = true;
                            'face: for a in 0..size[first] {
                                for b in 0..size[second] {
                                    let mut cell = [0usize; 3];
                                    cell[axis] = layer;
                                    cell[first] = start[first] + a;
                                    cell[second] = start[second] + b;
                                    if !full_beside(cell[0], cell[1], cell[2], face) {
                                        solid = false;
                                        break 'face;
                                    }
                                }
                            }
                            if solid {
                                if positive {
                                    high[axis] = SEAM_OVERLAP;
                                } else {
                                    low[axis] = SEAM_OVERLAP;
                                }
                            }
                        }
                    }
                    let min = [0, 1, 2].map(|axis| start[axis] as f32 - low[axis]);
                    let max = [0, 1, 2].map(|axis| (start[axis] + size[axis]) as f32 + high[axis]);
                    boxes.push((
                        Pose::from_translation(Vector::new(
                            (min[0] + max[0]) / 2.0,
                            (min[1] + max[1]) / 2.0,
                            (min[2] + max[2]) / 2.0,
                        )),
                        SharedShape::cuboid(
                            (max[0] - min[0]) / 2.0,
                            (max[1] - min[1]) / 2.0,
                            (max[2] - min[2]) / 2.0,
                        ),
                    ));
                }
            }
        }
    }
    for z in 0..EDGE {
        for y in 0..EDGE {
            for x in 0..EDGE {
                if let Cell::Partial(shape) = cells[index(x, y, z)].clone() {
                    for part in shapes[shape].iter() {
                        let eighths = |value: u8| f32::from(value) / 8.0;
                        let min = [
                            eighths(part.min[0]),
                            eighths(part.min[1]),
                            eighths(part.min[2]),
                        ];
                        let max = [
                            eighths(part.max[0]),
                            eighths(part.max[1]),
                            eighths(part.max[2]),
                        ];
                        boxes.push((
                            Pose::from_translation(Vector::new(
                                x as f32 + (min[0] + max[0]) / 2.0,
                                y as f32 + (min[1] + max[1]) / 2.0,
                                z as f32 + (min[2] + max[2]) / 2.0,
                            )),
                            SharedShape::cuboid(
                                (max[0] - min[0]) / 2.0,
                                (max[1] - min[1]) / 2.0,
                                (max[2] - min[2]) / 2.0,
                            ),
                        ));
                    }
                }
            }
        }
    }
    let at = |brick: BrickCoordinate| {
        Vector::new(
            ((brick.x() - origin.x()) * BRICK_EDGE_LENGTH as i64) as f32,
            ((brick.y() - origin.y()) * BRICK_EDGE_LENGTH as i64) as f32,
            ((brick.z() - origin.z()) * BRICK_EDGE_LENGTH as i64) as f32,
        )
    };
    BrickColliders {
        terrain: (!boxes.is_empty()).then(|| {
            ColliderBuilder::compound(boxes)
                .translation(at(brick))
                .build()
        }),
    }
}
