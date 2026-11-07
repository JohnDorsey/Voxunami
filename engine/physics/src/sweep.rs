//! The bricks a body will reach, for the terrain that has colliders ahead of it.
//!
//! A body is given colliders of the terrain it will meet before it meets it: the bricks within a margin of its bounds, which grows with its speed (the way it goes in a tenth of a second, a
//! quarter of one). A cube of that margin all round the body, as it first was, is a cube of the bricks it will not meet as well: behind it, over it and beside it. At 2 kilometres a second
//! the margin is 500 voxels, a cube of 33 bricks along each edge, 36,000 bricks in a tick, and the work of the tick grew as the cube of the speed (34 of 40 milliseconds at 2.3 kilometres a
//! second, 700 at 6, so that the server ran at two ticks a second and the game with it).
//!
//! So the margin is a cube only up to a cap, and beyond the cap it is that cube *swept* along the way the body goes: a tube of the cube's width, as long as the margin is. Where the margin is
//! under the cap, which is for every body that goes slower than the cap lets (a quarter of the margin of the cap, 400 voxels a second for 128 voxels), the bricks are the cube's, exactly. Beyond it
//! they are always some of the cube's (never a brick the cube had not) and always the ones ahead.

use voxel_space::{BRICK_EDGE_LENGTH, BrickBox, BrickCoordinate};

use rapier3d::prelude::{Real, Vector};

const EDGE: Real = BRICK_EDGE_LENGTH as Real;

/// How far, in voxels, the margins of the terrain a body has colliders of reach every way: the bricks within a margin are a cube of it round the body up to this, and past it that cube carried
/// the way the body goes. A ship that goes slower than a quarter of this less its keeping (about 400 voxels a second) has the cube, exactly; a faster one has the bricks ahead, all of them, and
/// no more of the others than this: a body that is thrown back at more than this in a tick, by a blow, may reach terrain that was let go.
pub const TERRAIN_EVERY_WAY_VOXELS: Real = 128.0;

/// How long, in voxels, one box of a tube is along the way: the tube is made of boxes, each the box round the cube at the start and at the end of its piece of the way. A longer piece is
/// fewer boxes to ask and more bricks held that are not in the tube, where the way is slanted.
const PIECE: Real = 128.0;

/// Bricks, as boxes that may overlap (so that a brick can be in more than one, and `bricks` can say it more than once).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweptBricks {
    boxes: Vec<BrickBox>,
}

impl SweptBricks {
    /// The bricks of one box.
    pub fn of_box(bricks: BrickBox) -> Self {
        Self {
            boxes: vec![bricks],
        }
    }

    /// The bricks of some boxes.
    pub fn of_boxes(boxes: Vec<BrickBox>) -> Self {
        Self { boxes }
    }

    /// The boxes the bricks are in.
    pub fn boxes(&self) -> &[BrickBox] {
        &self.boxes
    }

    pub fn contains(&self, brick: BrickCoordinate) -> bool {
        self.boxes.iter().any(|bricks| bricks.contains(brick))
    }

    /// Every brick of every box, a brick that is in more than one of them once for each.
    pub fn bricks(&self) -> impl Iterator<Item = BrickCoordinate> + '_ {
        self.boxes.iter().flat_map(|bricks| bricks.bricks())
    }

    /// How many bricks there are in all of them, counting a brick once for each box it is in.
    pub fn brick_count(&self) -> u128 {
        self.boxes.iter().map(|bricks| bricks.brick_count()).sum()
    }
}

/// The bricks, in coordinates whose origin is the corner of the brick `origin`, that a box from `low` to `high` (voxels, in the frame) comes within `margin` voxels of: the cube of the margin
/// when that is no more than `cap`; otherwise the cube of `cap` swept along `direction` (a way, of any length but none) for as far as the margin is past the cap.
pub fn swept_bricks(
    low: Vector,
    high: Vector,
    margin: Real,
    cap: Real,
    direction: Vector,
    origin: BrickCoordinate,
) -> Option<SweptBricks> {
    let reach = margin - cap;
    let direction = if direction.length() > 1e-9 {
        direction.normalize()
    } else {
        Vector::ZERO
    };
    if reach <= 0.0 || direction == Vector::ZERO {
        // The cube. (A body that does not go anywhere has no way to sweep along: it keeps the cube it had.)
        let margin = Vector::splat(margin);
        return cube(low - margin, high + margin, origin).map(SweptBricks::of_box);
    }
    // The cube of the cap at the start of the way, and at the end of each piece of it: the box round the two is the bricks of the piece. (The pieces end at the same places whatever the
    // margin is, except the last, which ends where the margin does; so a bigger margin has all the boxes of a smaller one, and more.)
    let (low, high) = (low - Vector::splat(cap), high + Vector::splat(cap));
    let mut boxes = Vec::new();
    let mut start = 0.0;
    while start < reach {
        let end = (start + PIECE).min(reach);
        let (from, to) = (direction * start, direction * end);
        boxes.push(cube(low + from.min(to), high + from.max(to), origin)?);
        start = end;
    }
    Some(SweptBricks { boxes })
}

/// The bricks of a box of voxels in the frame.
fn cube(low: Vector, high: Vector, origin: BrickCoordinate) -> Option<BrickBox> {
    let brick = |value: Real, origin: i128| origin.saturating_add((value / EDGE).floor() as i128);
    BrickBox::new(
        BrickCoordinate::new(
            brick(low.x, origin.x()),
            brick(low.y, origin.y()),
            brick(low.z, origin.z()),
        ),
        BrickCoordinate::new(
            brick(high.x, origin.x()),
            brick(high.y, origin.y()),
            brick(high.z, origin.z()),
        ),
    )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const CAP: Real = 128.0;

    /// The ship is 8 voxels across, at the origin, and goes `way`.
    fn near_a_small_ship(margin: Real, way: Vector) -> SweptBricks {
        let low = Vector::new(100.0, 200.0, 300.0);
        swept_bricks(
            low,
            low + Vector::splat(8.0),
            margin,
            CAP,
            way,
            BrickCoordinate::ORIGIN,
        )
        .expect("a box is some bricks")
    }

    fn the_cube(margin: Real, low: Vector, high: Vector, origin: BrickCoordinate) -> BrickBox {
        let margin = Vector::splat(margin);
        cube(low - margin, high + margin, origin).expect("a box is some bricks")
    }

    #[test]
    fn a_margin_within_the_cap_is_the_cube_whatever_way_the_body_goes() {
        for margin in [0.0, 2.0, 24.0, 127.9, 128.0] {
            for way in [
                Vector::ZERO,
                Vector::X,
                Vector::new(0.3, -0.8, 0.52).normalize(),
            ] {
                let low = Vector::new(100.0, 200.0, 300.0);
                let expected = the_cube(
                    margin,
                    low,
                    low + Vector::splat(8.0),
                    BrickCoordinate::ORIGIN,
                );
                let bricks = near_a_small_ship(margin, way);
                assert_eq!(bricks.boxes(), &[expected], "margin {margin}, way {way:?}");
            }
        }
    }

    #[test]
    fn a_fast_body_has_a_tube_of_bricks_and_not_a_cube_the_size_of_its_speed() {
        // 6,000 voxels a second: the margin of a quarter of a second, and of the 16 voxels that bricks are kept past it.
        let margin = 8.0 + 0.25 * 6000.0 + 16.0;
        let cube_bricks = the_cube(
            margin,
            Vector::new(100.0, 200.0, 300.0),
            Vector::new(108.0, 208.0, 308.0),
            BrickCoordinate::ORIGIN,
        )
        .brick_count();
        assert!(cube_bricks > 800_000, "{cube_bricks}");
        // Along an axis: the cube of the cap, 9 bricks across, and 1,500 voxels, 47 bricks, of it.
        let along_x = near_a_small_ship(margin, Vector::X).brick_count();
        assert!(along_x < 14_000, "{along_x} bricks along an axis");
        // Slanted, the boxes of the pieces of the way hold some more.
        let slanted =
            near_a_small_ship(margin, Vector::new(1.0, 1.0, 1.0).normalize()).brick_count();
        assert!(slanted < 20_000, "{slanted} bricks slanted");
    }

    proptest! {
        /// A margin within the cap is the cube; any other is boxes inside the cube.
        #[test]
        fn the_bricks_are_never_more_than_the_cube_of_the_margin(
            low in proptest::array::uniform3(-5000.0f64..5000.0),
            size in proptest::array::uniform3(0.5f64..60.0),
            margin in 0.0f64..800.0,
            way in proptest::array::uniform3(-1.0f64..1.0),
            origin in proptest::array::uniform3(-40i128..40),
        ) {
            let (low, high) = (Vector::from_array(low), Vector::from_array(low) + Vector::from_array(size));
            let way = Vector::from_array(way);
            let origin = BrickCoordinate::new(origin[0], origin[1], origin[2]);
            let way = if way.length() > 1e-3 { way.normalize() } else { Vector::ZERO };
            let bricks = swept_bricks(low, high, margin, CAP, way, origin).unwrap();
            let all = the_cube(margin, low, high, origin);
            for each in bricks.boxes() {
                prop_assert!(all.contains(each.min()) && all.contains(each.max()), "{each:?} not in {all:?}");
            }
            if margin <= CAP || way == Vector::ZERO {
                prop_assert_eq!(bricks.boxes(), &[all]);
            }
        }

        /// The body, as it goes the way, is inside bricks of the tube all the way to the end of the margin.
        #[test]
        fn the_body_is_among_the_bricks_all_the_way_it_goes(
            low in proptest::array::uniform3(-5000.0f64..5000.0),
            size in proptest::array::uniform3(0.5f64..60.0),
            margin in 0.0f64..1500.0,
            way in proptest::array::uniform3(-1.0f64..1.0),
        ) {
            let (low, high) = (Vector::from_array(low), Vector::from_array(low) + Vector::from_array(size));
            let way = Vector::from_array(way);
            prop_assume!(way.length() > 1e-3);
            let way = way.normalize();
            let bricks = swept_bricks(low, high, margin, CAP, way, BrickCoordinate::ORIGIN).unwrap();
            let reach = (margin - CAP).max(0.0);
            let near = Vector::splat(margin.min(CAP));
            for step in 0..=20 {
                let along = reach * f64::from(step) / 20.0;
                let there = way * along;
                // The body's box at that place, widened by the margin the cube has: all of its bricks are among the bricks.
                let asked = cube(low + there - near, high + there + near, BrickCoordinate::ORIGIN).unwrap();
                prop_assert!(bricks.contains(asked.min()) && bricks.contains(asked.max()), "{asked:?} at {along} of {reach}");
            }
        }

        /// A bigger margin has the bricks of a smaller one: the rings of bricks that are wanted nearest first are nested.
        #[test]
        fn a_bigger_margin_has_every_brick_a_smaller_one_has(
            low in proptest::array::uniform3(-5000.0f64..5000.0),
            size in proptest::array::uniform3(0.5f64..60.0),
            small in 0.0f64..1500.0,
            more in 0.0f64..600.0,
            way in proptest::array::uniform3(-1.0f64..1.0),
        ) {
            let (low, high) = (Vector::from_array(low), Vector::from_array(low) + Vector::from_array(size));
            let way = Vector::from_array(way);
            prop_assume!(way.length() > 1e-3);
            let way = way.normalize();
            let origin = BrickCoordinate::ORIGIN;
            let inner = swept_bricks(low, high, small, CAP, way, origin).unwrap();
            let outer = swept_bricks(low, high, small + more, CAP, way, origin).unwrap();
            for each in inner.boxes() {
                prop_assert!(outer.contains(each.min()) && outer.contains(each.max()));
                // (A box is a box of bricks: both corners in the outer bricks, and the outer boxes are boxes.)
                prop_assert!(outer.boxes().iter().any(|bigger| bigger.contains(each.min()) && bigger.contains(each.max())), "{each:?}");
            }
        }
    }
}
