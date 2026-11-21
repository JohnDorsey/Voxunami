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

use std::collections::HashMap;

use region_octree::BrickCube;
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

/// The way a body goes as the sweep takes it: of length one, or none for a body that goes nowhere (a way too short to have a direction).
fn way_of(direction: Vector) -> Vector {
    if direction.length() > 1e-9 {
        direction.normalize()
    } else {
        Vector::ZERO
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
    let direction = way_of(direction);
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
    BrickBox::new(
        BrickCoordinate::new(
            brick_at(low.x, origin.x()),
            brick_at(low.y, origin.y()),
            brick_at(low.z, origin.z()),
        ),
        BrickCoordinate::new(
            brick_at(high.x, origin.x()),
            brick_at(high.y, origin.y()),
            brick_at(high.z, origin.z()),
        ),
    )
}

/// The brick along one axis that a place in voxels is in, in coordinates whose origin is the brick `origin` along it.
fn brick_at(value: Real, origin: i128) -> i128 {
    origin.saturating_add((value / EDGE).floor() as i128)
}

/// Where along an axis each coordinate of a range of bricks is first in some range that grows as a position in a list goes up: for the ranges at the positions `first` to `last`,
/// of which the one at `last` is the widest and holds every coordinate asked about.
struct FirstPositions {
    lowest: i128,
    positions: Vec<usize>,
}

impl FirstPositions {
    /// `range_at(position)` is the range of coordinates at a position, `(lowest, highest)`, and each range holds the one before.
    fn new(first: usize, last: usize, range_at: impl Fn(usize) -> (i128, i128)) -> Self {
        let (lowest, highest) = range_at(last);
        let positions = (lowest..=highest)
            .map(|coordinate| {
                // The first position whose range has the coordinate, by halving: the ranges only grow, so it is in all the positions from there on.
                let (mut before, mut after) = (first, last);
                while before < after {
                    let middle = before + (after - before) / 2;
                    let (low, high) = range_at(middle);
                    if low <= coordinate && coordinate <= high {
                        after = middle;
                    } else {
                        before = middle + 1;
                    }
                }
                after
            })
            .collect();
        Self { lowest, positions }
    }

    /// Where the coordinate is first in the range, or `None` if it is in none of them.
    fn of(&self, coordinate: i128) -> Option<usize> {
        let index = usize::try_from(coordinate.checked_sub(self.lowest)?).ok()?;
        self.positions.get(index).copied()
    }

    /// The widest range, `(lowest, highest)`, or `None` if there is none.
    fn widest(&self) -> Option<(i128, i128)> {
        let count = i128::try_from(self.positions.len())
            .ok()
            .filter(|&count| count > 0)?;
        Some((self.lowest, self.lowest + count - 1))
    }
}

/// What a sweep is of: the box of a body (`low` to `high`, voxels in the frame) and the way it goes, with the `cap` and the `origin` that [`swept_bricks`] takes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sweep {
    pub low: Vector,
    pub high: Vector,
    pub cap: Real,
    pub direction: Vector,
    pub origin: BrickCoordinate,
}

/// How far a body is to reach, for [`BodyWorld::visit_bricks_ahead`](crate::BodyWorld::visit_bricks_ahead): some `margins` (in order, the least first), the `cap`, and the way it goes (in the axes of the frame).
#[derive(Clone, Copy, Debug)]
pub struct Reach<'a> {
    pub margins: &'a [Real],
    pub cap: Real,
    pub direction: [Real; 3],
}

/// How big the cubes are that the bricks of a box are looked for in to begin with: 64 bricks to an edge, 2,000 voxels, so that a box of a piece of a tube is in one or two of them along
/// each axis, and in empty space the whole of it is one question.
const FIRST_CUBE_LEVEL: u8 = 6;

/// The cubes of this level and above that have been asked about are remembered (those below it are as many as the bricks, and asked once each by the piece that has them).
const REMEMBERED_FROM_LEVEL: u8 = 2;

/// The bricks of boxes that are in none of the cubes that `skip` says need not be visited.
struct Unskipped<S: FnMut(BrickCube) -> bool> {
    skip: S,
    answers: HashMap<BrickCube, bool>,
    found: Vec<BrickCoordinate>,
}

impl<S: FnMut(BrickCube) -> bool> Unskipped<S> {
    fn new(skip: S) -> Self {
        Self {
            skip,
            answers: HashMap::new(),
            found: Vec::new(),
        }
    }

    /// The bricks of the box from `low` to `high` (each corner included) that are in no cube that `skip` says need not be visited, in `self.found`: of the cubes of
    /// [`FIRST_CUBE_LEVEL`] that have a brick of the box, each is dropped if it is said to be, and otherwise is looked into by its eight halves, down to a brick.
    fn find(&mut self, low: [i128; 3], high: [i128; 3]) {
        self.found.clear();
        let Some(region) = BrickBox::new(
            BrickCoordinate::new(low[0], low[1], low[2]),
            BrickCoordinate::new(high[0], high[1], high[2]),
        ) else {
            return;
        };
        for cube in BrickCube::covering(region, FIRST_CUBE_LEVEL) {
            self.look_into(cube, region);
        }
    }

    fn look_into(&mut self, cube: BrickCube, region: BrickBox) {
        if self.skipped(cube) {
            return;
        }
        if cube.level() == 0 {
            self.found.push(cube.origin());
            return;
        }
        let half = 1i128 << (cube.level() - 1);
        let origin = cube.origin();
        for index in 0..8i128 {
            let child = BrickCube::new(
                BrickCoordinate::new(
                    origin.x() + (index & 1) * half,
                    origin.y() + (index >> 1 & 1) * half,
                    origin.z() + (index >> 2 & 1) * half,
                ),
                cube.level() - 1,
            )
            .expect("the half of an aligned cube is aligned");
            if child.meets(region) {
                self.look_into(child, region);
            }
        }
    }

    fn skipped(&mut self, cube: BrickCube) -> bool {
        if cube.level() < REMEMBERED_FROM_LEVEL {
            return (self.skip)(cube);
        }
        let Self { skip, answers, .. } = self;
        *answers.entry(cube).or_insert_with(|| skip(cube))
    }
}

/// The first of some margins, in the sense of [`swept_bricks`], whose bricks hold each brick of the last of them, without making the bricks of the margins before the last. Calls `visit` with each brick
/// of the last margin and the position in `margins` of the first margin whose bricks have it; a brick that is in more than one box of the tube is visited once for each, each time with the first
/// position for that box, so the least of the positions it was visited with is the first of all of them.
///
/// Where this is for: a ship's terrain is wanted nearest first, in rings of margins 8 voxels apart, and at 400,000 voxels a second there are 8,000 of them, each a tube of a thousand boxes, so that to make
/// each ring's bricks was the square of the speed in boxes (4 million of them, 400 megabytes, in each tick) before one brick was asked about. The boxes of a tube are the same pieces whatever the margin, but the
/// last, which is as long as the margin; so each piece is looked at once, and for each of the three axes the first margin in which each coordinate of it is in the box is found by halving (a brick is in a
/// box when all three of its coordinates are, so its first margin is the last of the three).
///
/// `skip` says of a cube of bricks that nothing in it need be visited (it holds no ground that anything could meet): the bricks of a cube it says that of are not visited, and one is
/// visited if no cube that has it is said so. It is asked of aligned cubes, from those of 64 bricks to an edge that have a brick of a piece of the tube, down to single bricks, and what it
/// says of a cube of more than one brick is remembered while the bricks are visited; so a tube through empty space is a question for each cube it goes through, and not for each brick.
/// It is asked of whole cubes, which may reach past the bricks that are asked for, and must be said of all of the cube.
///
/// `margins` are in order, the least first, and none is less than the one before it (a panic if one is: what is found of margins that are not in order is wrong, and nothing says so); with none
/// there is nothing to visit. A box whose `low` is not below its `high` has no bricks.
pub(crate) fn visit_bricks_by_first_margin(
    sweep: Sweep,
    margins: &[Real],
    skip: impl FnMut(BrickCube) -> bool,
    mut visit: impl FnMut(BrickCoordinate, usize),
) {
    let Sweep {
        low,
        high,
        cap,
        direction,
        origin,
    } = sweep;
    let mut unskipped = Unskipped::new(skip);
    let Some(&outermost) = margins.last() else {
        return;
    };
    assert!(
        margins.windows(2).all(|pair| pair[0] <= pair[1]),
        "margins that are not in order: {margins:?}"
    );
    let direction = way_of(direction);
    let origins = [origin.x(), origin.y(), origin.z()];
    // The margins that have the cube (as `swept_bricks` has it when `reach <= 0`, or a body that goes nowhere) come first, since a margin that has it is one that a smaller margin has.
    let cubes = if direction == Vector::ZERO {
        margins.len()
    } else {
        margins.partition_point(|margin| margin - cap <= 0.0)
    };
    // The cube of each of those: `low - margin` to `high + margin`.
    let cube_range = |position: usize, axis: usize| {
        let margin = margins[position];
        (
            brick_at(low[axis] - margin, origins[axis]),
            brick_at(high[axis] + margin, origins[axis]),
        )
    };
    let cube_firsts = (cubes > 0).then(|| {
        [0, 1, 2]
            .map(|axis| FirstPositions::new(0, cubes - 1, |position| cube_range(position, axis)))
    });
    let first_of_cube = |brick: BrickCoordinate| {
        let firsts = cube_firsts.as_ref()?;
        Some(
            firsts[0]
                .of(brick.x())?
                .max(firsts[1].of(brick.y())?)
                .max(firsts[2].of(brick.z())?),
        )
    };
    if let (true, Some(firsts)) = (cubes == margins.len(), &cube_firsts) {
        // All of them are the cube: the bricks of the last, each at the first margin that has it.
        let widest = firsts.each_ref().map(FirstPositions::widest);
        if let [Some((x0, x1)), Some((y0, y1)), Some((z0, z1))] = widest {
            unskipped.find([x0, y0, z0], [x1, y1, z1]);
            for &brick in &unskipped.found {
                if let Some(position) = first_of_cube(brick) {
                    visit(brick, position);
                }
            }
        }
        return;
    }
    // The tubes: pieces of the way of the same length for every margin, but the last of each, which ends where the margin does.
    let (low, high) = (low - Vector::splat(cap), high + Vector::splat(cap));
    let last = margins.len() - 1;
    let mut start = 0.0;
    while start < outermost - cap {
        // The margins that have this piece (those whose reach is past where it starts), and of them the ones that have it whole.
        let first = cubes + margins[cubes..].partition_point(|margin| margin - cap <= start);
        let whole = first + margins[first..].partition_point(|margin| margin - cap < start + PIECE);
        // A margin from there on has this piece as it is, so the first margin of a brick of the piece is no later than that.
        let last_that_changes = whole.min(last);
        let piece_range = |position: usize, axis: usize| {
            let end = (start + PIECE).min(margins[position] - cap);
            let (from, to) = (direction[axis] * start, direction[axis] * end);
            (
                brick_at(low[axis] + from.min(to), origins[axis]),
                brick_at(high[axis] + from.max(to), origins[axis]),
            )
        };
        // The bricks of the piece as the last margin that changes it has it, which are all of them, that are not in cubes that need not be visited. (Where a tube goes through empty space
        // this is all there is to a piece: the table of where each coordinate is first is made only for a piece that has bricks.)
        let widest = [0, 1, 2].map(|axis| piece_range(last_that_changes, axis));
        unskipped.find(widest.map(|range| range.0), widest.map(|range| range.1));
        if !unskipped.found.is_empty() {
            let firsts = [0, 1, 2].map(|axis| {
                FirstPositions::new(first, last_that_changes, |position| {
                    piece_range(position, axis)
                })
            });
            for &brick in &unskipped.found {
                let position = [brick.x(), brick.y(), brick.z()]
                    .into_iter()
                    .zip(&firsts)
                    .map(|(coordinate, positions)| {
                        positions
                            .of(coordinate)
                            .expect("a brick of the widest box is in the table")
                    })
                    .max()
                    .expect("there are three axes");
                // A brick of the cube of a smaller margin is in the bricks of that margin and of the tubes after it.
                visit(
                    brick,
                    first_of_cube(brick).map_or(position, |cube| cube.min(position)),
                );
            }
        }
        start += PIECE;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

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

    /// The first margin that has each brick of the last, as it was first found: by making the bricks of every margin and looking at each.
    fn first_margins_by_making_every_region(
        low: Vector,
        high: Vector,
        margins: &[Real],
        way: Vector,
        origin: BrickCoordinate,
    ) -> BTreeMap<BrickCoordinate, usize> {
        let mut first = BTreeMap::new();
        for (position, &margin) in margins.iter().enumerate() {
            let region = swept_bricks(low, high, margin, CAP, way, origin).unwrap();
            for brick in region.bricks() {
                // (The positions go up, so the first time a brick is seen is the first margin that has it.)
                first.entry(brick).or_insert(position);
            }
        }
        // Only the bricks of the last margin are asked about: the margins hold each other, which another test says.
        let last = swept_bricks(low, high, *margins.last().unwrap(), CAP, way, origin).unwrap();
        first.retain(|brick, _| last.contains(*brick));
        first
    }

    fn sweep_of(low: Vector, high: Vector, way: Vector, origin: BrickCoordinate) -> Sweep {
        Sweep {
            low,
            high,
            cap: CAP,
            direction: way,
            origin,
        }
    }

    fn first_margins_in_one_pass(
        low: Vector,
        high: Vector,
        margins: &[Real],
        way: Vector,
        origin: BrickCoordinate,
    ) -> BTreeMap<BrickCoordinate, usize> {
        first_margins_skipping(low, high, margins, way, origin, |_| false)
    }

    /// The first margin of each brick that is visited, the least of the margins it is visited with, when `skip` says of cubes that they need not be.
    fn first_margins_skipping(
        low: Vector,
        high: Vector,
        margins: &[Real],
        way: Vector,
        origin: BrickCoordinate,
        skip: impl FnMut(BrickCube) -> bool,
    ) -> BTreeMap<BrickCoordinate, usize> {
        let mut first: BTreeMap<BrickCoordinate, usize> = BTreeMap::new();
        visit_bricks_by_first_margin(
            sweep_of(low, high, way, origin),
            margins,
            skip,
            |brick, position| {
                first
                    .entry(brick)
                    .and_modify(|seen| *seen = (*seen).min(position))
                    .or_insert(position);
            },
        );
        first
    }

    fn way_from(components: [f64; 3]) -> Vector {
        let way = Vector::from_array(components);
        if way.length() > 1e-3 {
            way.normalize()
        } else {
            Vector::ZERO
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(160))]

        /// What the pass finds is what making every margin's bricks finds, for margins in the cube, past it, and both.
        #[test]
        fn the_first_margin_of_every_brick_is_the_first_whose_bricks_have_it(
            low in proptest::array::uniform3(-5000.0f64..5000.0),
            size in proptest::array::uniform3(0.5f64..60.0),
            mut margins in proptest::collection::vec(0.0f64..1500.0, 1..14),
            way in proptest::array::uniform3(-1.0f64..1.0),
            origin in proptest::array::uniform3(-40i128..40),
        ) {
            margins.sort_by(f64::total_cmp);
            let (low, high) = (Vector::from_array(low), Vector::from_array(low) + Vector::from_array(size));
            let way = way_from(way);
            let origin = BrickCoordinate::new(origin[0], origin[1], origin[2]);
            prop_assert_eq!(
                first_margins_in_one_pass(low, high, &margins, way, origin),
                first_margins_by_making_every_region(low, high, &margins, way, origin)
            );
        }

        /// The margins of a ship's rings: 8 voxels apart, from the cube into the tube, along the axes and slanted.
        #[test]
        fn the_first_margin_of_every_brick_is_right_for_rings_that_are_eight_voxels_apart(
            low in proptest::array::uniform3(-5000.0f64..5000.0),
            size in proptest::array::uniform3(0.5f64..60.0),
            first in 90.0f64..400.0,
            rings in 1usize..40,
            way in proptest::array::uniform3(-1.0f64..1.0),
            along_an_axis in proptest::bool::ANY,
        ) {
            let margins: Vec<Real> = (0..rings).map(|ring| first + 8.0 * ring as f64).collect();
            let (low, high) = (Vector::from_array(low), Vector::from_array(low) + Vector::from_array(size));
            let way = if along_an_axis { Vector::X } else { way_from(way) };
            prop_assert_eq!(
                first_margins_in_one_pass(low, high, &margins, way, BrickCoordinate::ORIGIN),
                first_margins_by_making_every_region(low, high, &margins, way, BrickCoordinate::ORIGIN)
            );
        }
    }

    #[test]
    fn a_hundred_thousand_rings_are_one_pass_over_the_bricks_of_the_tube_and_not_a_hundred_thousand_tubes()
     {
        // A ship at 3 million voxels a second would have ring after ring of tube a thousand boxes long: the rings made first took the square of that, and this takes the boxes
        // of the last once. The bricks it visits are those of the tube, nearest the ship first.
        let rings = 100_000usize;
        let margins: Vec<Real> = (0..rings).map(|ring| 130.0 + 8.0 * ring as f64).collect();
        let (low, high) = (
            Vector::new(100.0, 200.0, 300.0),
            Vector::new(108.0, 208.0, 308.0),
        );
        let mut visits = 0u64;
        let mut along_the_way = BTreeMap::new();
        let (row_y, row_z) = (6, 9);
        visit_bricks_by_first_margin(
            sweep_of(low, high, Vector::X, BrickCoordinate::ORIGIN),
            &margins,
            |_| false,
            |brick, position| {
                visits += 1;
                if brick.y() == row_y && brick.z() == row_z {
                    along_the_way
                        .entry(brick.x())
                        .and_modify(|seen: &mut usize| *seen = (*seen).min(position))
                        .or_insert(position);
                }
            },
        );
        assert!(visits > 5_000_000, "{visits}");
        // The farther along the way, the later the margin that reaches it, and the last of them reaches the end.
        let positions: Vec<usize> = along_the_way.values().copied().collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] <= pair[1]),
            "{positions:?}"
        );
        assert_eq!(positions[0], 0);
        assert!(*positions.last().unwrap() >= rings - 5, "{positions:?}");
    }

    #[test]
    fn no_margins_have_no_bricks_and_a_body_that_goes_nowhere_has_the_cube_for_every_margin() {
        let (low, high) = (
            Vector::new(100.0, 200.0, 300.0),
            Vector::new(108.0, 208.0, 308.0),
        );
        let mut none = 0;
        visit_bricks_by_first_margin(
            sweep_of(low, high, Vector::X, BrickCoordinate::ORIGIN),
            &[],
            |_| false,
            |_, _| none += 1,
        );
        assert_eq!(none, 0);
        // The cube of the margin, though the margin is past the cap: there is no way to sweep along.
        let margins = [20.0, 200.0, 900.0];
        assert_eq!(
            first_margins_in_one_pass(low, high, &margins, Vector::ZERO, BrickCoordinate::ORIGIN),
            first_margins_by_making_every_region(
                low,
                high,
                &margins,
                Vector::ZERO,
                BrickCoordinate::ORIGIN
            )
        );
    }

    #[test]
    #[should_panic(expected = "margins that are not in order")]
    fn margins_that_are_not_in_order_are_refused_and_not_answered_wrongly() {
        let (low, high) = (
            Vector::new(100.0, 200.0, 300.0),
            Vector::new(108.0, 208.0, 308.0),
        );
        visit_bricks_by_first_margin(
            sweep_of(low, high, Vector::X, BrickCoordinate::ORIGIN),
            &[200.0, 100.0],
            |_| false,
            |_, _| {},
        );
    }

    /// Whether the cube of 4 bricks to an edge that holds a brick is one of the half of them, like the black squares of a board, that are air.
    fn in_an_even_cube(brick: BrickCoordinate) -> bool {
        let along = |coordinate: i128| coordinate.div_euclid(4);
        (along(brick.x()) + along(brick.y()) + along(brick.z())).rem_euclid(2) == 0
    }

    /// Whether a brick is one of the third of them that are air in the last of the oracles, which says nothing of cubes of more than one brick.
    fn is_a_scattered_air_brick(brick: BrickCoordinate) -> bool {
        let mixed = (brick.x().wrapping_mul(73_856_093))
            ^ (brick.y().wrapping_mul(19_349_663))
            ^ (brick.z().wrapping_mul(83_492_791));
        mixed.rem_euclid(3) == 0
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(160))]

        /// What is skipped is the bricks of the cubes that are said to be air, and no others: the first margin of every other brick is what making every margin's bricks gives.
        #[test]
        fn the_bricks_that_are_visited_are_all_of_those_that_are_in_no_cube_that_is_skipped(
            low in proptest::array::uniform3(-5000.0f64..5000.0),
            size in proptest::array::uniform3(0.5f64..60.0),
            mut margins in proptest::collection::vec(0.0f64..1500.0, 1..14),
            way in proptest::array::uniform3(-1.0f64..1.0),
            origin in proptest::array::uniform3(-40i128..40),
            // Which of the bricks that are air, and where: a height among the rows of the bricks there are, and a side of it, so that the air is across the middle of them.
            pick in 0usize..10_000,
            nudge in 0i128..2,
            which in 0u8..4,
        ) {
            margins.sort_by(f64::total_cmp);
            let (low, high) = (Vector::from_array(low), Vector::from_array(low) + Vector::from_array(size));
            let way = way_from(way);
            let origin = BrickCoordinate::new(origin[0], origin[1], origin[2]);
            let everything = first_margins_by_making_every_region(low, high, &margins, way, origin);
            let mut rows: Vec<i128> = everything.keys().map(|brick| brick.y()).collect();
            rows.sort_unstable();
            rows.dedup();
            let height = rows[pick % rows.len()] + nudge;
            let kept = |keep: &dyn Fn(BrickCoordinate) -> bool| -> BTreeMap<BrickCoordinate, usize> {
                everything.iter().filter(|(brick, _)| keep(**brick)).map(|(brick, first)| (*brick, *first)).collect()
            };
            let (found, expected) = match which {
                // Every brick from a height up is air: a cube is, if it begins at or over it.
                0 => (
                    first_margins_skipping(low, high, &margins, way, origin, |cube| cube.origin().y() >= height),
                    kept(&|brick| brick.y() < height),
                ),
                // Half of the cubes of 4 to an edge are air, like the black squares of a board: no cube of more than that is, and one of less is if the cube that holds it is.
                1 => (
                    first_margins_skipping(low, high, &margins, way, origin, |cube| {
                        cube.level() <= 2 && in_an_even_cube(cube.origin())
                    }),
                    kept(&|brick| !in_an_even_cube(brick)),
                ),
                // A third of the bricks are air, here and there, and no cube of more than one brick has only them.
                2 => (
                    first_margins_skipping(low, high, &margins, way, origin, |cube| {
                        cube.level() == 0 && is_a_scattered_air_brick(cube.origin())
                    }),
                    kept(&|brick| !is_a_scattered_air_brick(brick)),
                ),
                // All of it is air.
                _ => (
                    first_margins_skipping(low, high, &margins, way, origin, |_| true),
                    BTreeMap::new(),
                ),
            };
            prop_assert_eq!(found, expected);
        }
    }

    #[test]
    fn a_tube_through_empty_space_is_a_question_for_each_cube_it_goes_through_and_not_for_each_brick()
     {
        // A ship at 420,000 voxels a second: rings 8 voxels apart to a lookahead of 105,008 voxels, a tube of 820 pieces and 800,000 bricks that is all air.
        let margins: Vec<Real> = (0..7_900)
            .map(|ring| 42_002.0 + 8.0 * ring as f64)
            .collect();
        let (low, high) = (
            Vector::new(100.0, 200.0, 300.0),
            Vector::new(108.0, 208.0, 308.0),
        );
        for way in [Vector::Y, Vector::new(1.0, 2.0, 3.0).normalize()] {
            let mut questions = 0u64;
            let mut visits = 0u64;
            visit_bricks_by_first_margin(
                sweep_of(low, high, way, BrickCoordinate::ORIGIN),
                &margins,
                |cube| {
                    questions += 1;
                    assert!(cube.level() <= FIRST_CUBE_LEVEL);
                    true
                },
                |_, _| visits += 1,
            );
            assert_eq!(visits, 0);
            // (A cube is asked once however many pieces have a brick in it; the cubes are 64 bricks, 2,000 voxels, to an edge, and the tube goes through 105,000 voxels of them.)
            assert!(questions < 1_000, "{questions} questions");
        }
    }

    #[test]
    fn where_the_tube_goes_through_ground_only_the_cubes_that_have_some_are_looked_into() {
        // The ground is the bricks under a plane across the way; air above. A ship going up through the plane at 6,000 voxels a second.
        let margins: Vec<Real> = (0..60).map(|ring| 1_000.0 + 8.0 * ring as f64).collect();
        let (low, high) = (
            Vector::new(100.0, 200.0, 300.0),
            Vector::new(108.0, 208.0, 308.0),
        );
        let ground_top = 20i128;
        let mut questions = 0u64;
        let mut visited = BTreeMap::new();
        visit_bricks_by_first_margin(
            sweep_of(low, high, Vector::Y, BrickCoordinate::ORIGIN),
            &margins,
            |cube| {
                questions += 1;
                cube.origin().y() >= ground_top
            },
            |brick, position| {
                visited.entry(brick).or_insert(position);
            },
        );
        // Every brick under the plane is visited, and none over it.
        assert!(visited.keys().all(|brick| brick.y() < ground_top));
        let everything = first_margins_by_making_every_region(
            low,
            high,
            &margins,
            Vector::Y,
            BrickCoordinate::ORIGIN,
        );
        let under = everything
            .keys()
            .filter(|brick| brick.y() < ground_top)
            .count();
        assert_eq!(visited.len(), under);
        assert!(
            questions < 10 * under as u64,
            "{questions} questions for {under} bricks"
        );
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
