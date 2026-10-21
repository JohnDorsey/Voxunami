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
                // (Only a voxel on the surface of the box can have a neighbour that is not in it, and one that is in it is a whole cube, so
                // only the faces of the box are looked at, and no further than the first open one.)
                let mut touches_open =
                    |x: usize, y: usize, z: usize, width: usize, height: usize, depth: usize| {
                        let low = [x, y, z];
                        let high = [x + width, y + height, z + depth];
                        for face in FaceDirection::ALL {
                            let axis = face.axis();
                            let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
                            let layer = if face.is_positive() {
                                high[axis] - 1
                            } else {
                                low[axis]
                            };
                            for a in low[first]..high[first] {
                                for b in low[second]..high[second] {
                                    let mut cell = [0usize; 3];
                                    cell[axis] = layer;
                                    cell[first] = a;
                                    cell[second] = b;
                                    if !full_beside(cell[0], cell[1], cell[2], face) {
                                        return true;
                                    }
                                }
                            }
                        }
                        false
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A random number from a place and a seed: the same everywhere.
    fn mix(seed: u64, x: i64, y: i64, z: i64) -> u64 {
        let mut state = seed
            ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ (y as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
            ^ (z as u64).wrapping_mul(0x1656_67b1_9e37_79f9);
        state = (state ^ (state >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        state = (state ^ (state >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        state ^ (state >> 31)
    }

    /// Terrain of several kinds, for bricks to be made of.
    #[derive(Clone, Copy)]
    enum Kind {
        /// Hills: solid up to a height that varies from column to column.
        Hills,
        /// Mostly solid with random holes.
        Cheese,
        /// Solid but for a room in the middle and a few slabs lying in it.
        RoomWithSlabs,
        /// Solid up to x = 40 and open beyond it: large boxes with one open side.
        Wall,
    }

    struct Terrain {
        seed: u64,
        kind: Kind,
    }

    impl CollisionSource for Terrain {
        fn collision_at(&mut self, voxel: VoxelCoordinate) -> Collision {
            let (x, y, z) = (voxel.x(), voxel.y(), voxel.z());
            let chance = mix(self.seed, x, y, z) % 100;
            match self.kind {
                Kind::Hills => {
                    let height =
                        40 + (mix(self.seed, x.div_euclid(3), 0, z.div_euclid(3)) % 9) as i64;
                    if y <= height {
                        Collision::Full
                    } else {
                        Collision::Empty
                    }
                }
                Kind::Cheese => {
                    if chance < 85 {
                        Collision::Full
                    } else {
                        Collision::Empty
                    }
                }
                Kind::RoomWithSlabs => {
                    let inside = (4..28).contains(&x.rem_euclid(32))
                        && (4..28).contains(&y.rem_euclid(32))
                        && (4..28).contains(&z.rem_euclid(32));
                    if !inside {
                        Collision::Full
                    } else if chance < 6 {
                        Collision::Partial(Arc::from(vec![ShapeBox {
                            min: [0, 0, 0],
                            max: [8, 4, 8],
                        }]))
                    } else {
                        Collision::Empty
                    }
                }
                Kind::Wall => {
                    if x <= 40 {
                        Collision::Full
                    } else {
                        Collision::Empty
                    }
                }
            }
        }
    }

    /// A hash of every box of a brick's collider, bit for bit, in order.
    fn hash_of_boxes(
        source: &mut Terrain,
        brick: BrickCoordinate,
        origin: BrickCoordinate,
    ) -> (u64, usize) {
        let made = build_brick(source, brick, origin);
        let mut state: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |value: u32| {
            for byte in value.to_le_bytes() {
                state ^= u64::from(byte);
                state = state.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        let Some(collider) = made.terrain else {
            return (state, 0);
        };
        for value in collider.position().translation.to_array() {
            feed(value.to_bits());
        }
        let compound = collider.shape().as_compound().expect("a compound of boxes");
        for (pose, shape) in compound.shapes() {
            for value in pose.translation.to_array() {
                feed(value.to_bits());
            }
            for value in shape.as_cuboid().expect("boxes").half_extents.to_array() {
                feed(value.to_bits());
            }
        }
        (state, compound.shapes().len())
    }

    /// The boxes of the bricks of terrain of every kind, taken with the code that merged cubes into boxes one voxel at a time, so that making
    /// bricks quicker is known to make the same boxes in the same order.
    #[test]
    fn the_boxes_of_a_brick_are_what_they_were_for_terrain_of_every_kind() {
        let mut combined: u64 = 0;
        let mut counts = Vec::new();
        for (index, kind) in [Kind::Hills, Kind::Cheese, Kind::RoomWithSlabs, Kind::Wall]
            .into_iter()
            .enumerate()
        {
            for (seed, brick) in [
                (1u64, BrickCoordinate::new(0, 1, 0)),
                (2, BrickCoordinate::new(-1, 1, 2)),
                (3, BrickCoordinate::new(5, 1, -4)),
                (4, BrickCoordinate::new(1, 1, 1)),
                (5, BrickCoordinate::new(1, 0, 0)),
                (6, BrickCoordinate::new(-3, 1, -3)),
            ] {
                let mut terrain = Terrain {
                    seed: seed + 10 * index as u64,
                    kind,
                };
                let (hash, count) =
                    hash_of_boxes(&mut terrain, brick, BrickCoordinate::new(0, 0, 0));
                combined = combined.rotate_left(7) ^ hash;
                counts.push(count);
            }
        }
        println!("boxes per brick: {counts:?}");
        assert_eq!(combined, PINNED_BOXES, "boxes per brick: {counts:?}");
    }

    const PINNED_BOXES: u64 = 17_322_980_413_275_256_541;
}
