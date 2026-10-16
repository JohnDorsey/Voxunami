//! A sparse octree that maps every brick of an unbounded grid to a value.
//!
//! Values are assigned to aligned cubes of bricks ([`BrickCube`]). A cube whose eight children
//! all hold the same value is stored as one leaf, so the tree spends nodes only near boundaries
//! between regions: a world with two terrain generator versions needs a handful of nodes per
//! boundary, not one per brick. That keeps the tree small enough to send to clients.
//!
//! The tree is canonical: the same brick-to-value mapping always produces the same tree shape,
//! whatever order the cubes were assigned in, so equal trees compare equal and
//! [`RegionOctree::leaves`] is a stable serialization order.

use voxel_space::{BRICK_COORDINATE_MAXIMUM, BRICK_COORDINATE_MINIMUM, BrickBox, BrickCoordinate};

/// Level of the root cube, which spans every valid brick coordinate (2^56 bricks per axis).
pub const ROOT_LEVEL: u8 = 56;

/// Bricks are shifted so the valid range starts at zero; all tree arithmetic is unsigned.
fn to_unsigned(coordinate: i64) -> u64 {
    (coordinate - BRICK_COORDINATE_MINIMUM) as u64
}

fn from_unsigned(offset: u64) -> i64 {
    offset as i64 + BRICK_COORDINATE_MINIMUM
}

/// An aligned cube of `2^level` bricks per edge. The origin is a multiple of the edge length,
/// measured from the lowest valid brick coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BrickCube {
    origin: BrickCoordinate,
    level: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidBrickCube {
    #[error("cube level {level} exceeds the maximum {ROOT_LEVEL}")]
    LevelTooLarge { level: u8 },
    #[error("cube origin {origin} is not aligned to its edge length of 2^{level} bricks")]
    Misaligned { origin: BrickCoordinate, level: u8 },
}

impl BrickCube {
    pub fn new(origin: BrickCoordinate, level: u8) -> Result<Self, InvalidBrickCube> {
        if level > ROOT_LEVEL {
            return Err(InvalidBrickCube::LevelTooLarge { level });
        }
        let alignment_mask = (1u64 << level) - 1;
        let aligned = [origin.x(), origin.y(), origin.z()]
            .into_iter()
            .all(|component| to_unsigned(component) & alignment_mask == 0);
        if aligned {
            Ok(Self { origin, level })
        } else {
            Err(InvalidBrickCube::Misaligned { origin, level })
        }
    }

    /// The cube of the given level that contains `brick`. Panics if `level > ROOT_LEVEL`.
    pub fn containing(brick: BrickCoordinate, level: u8) -> Self {
        assert!(
            level <= ROOT_LEVEL,
            "cube level {level} exceeds {ROOT_LEVEL}"
        );
        let align = |component: i64| from_unsigned(to_unsigned(component) >> level << level);
        Self {
            origin: BrickCoordinate::new(align(brick.x()), align(brick.y()), align(brick.z())),
            level,
        }
    }

    pub fn origin(self) -> BrickCoordinate {
        self.origin
    }

    pub fn level(self) -> u8 {
        self.level
    }

    /// Bricks along each edge.
    pub fn edge_length(self) -> u64 {
        1 << self.level
    }

    /// Every cube of the given level that holds at least one brick of `region`, x varying fastest,
    /// then y, then z. Panics if `level > ROOT_LEVEL`.
    pub fn covering(region: BrickBox, level: u8) -> impl Iterator<Item = BrickCube> {
        let first = Self::containing(region.min(), level).origin();
        let last = Self::containing(region.max(), level).origin();
        let edge = 1i64 << level;
        let count = |first: i64, last: i64| ((last - first) >> level) + 1;
        let (columns, rows, layers) = (
            count(first.x(), last.x()),
            count(first.y(), last.y()),
            count(first.z(), last.z()),
        );
        (0..layers).flat_map(move |layer| {
            (0..rows).flat_map(move |row| {
                (0..columns).map(move |column| Self {
                    origin: BrickCoordinate::new(
                        first.x() + column * edge,
                        first.y() + row * edge,
                        first.z() + layer * edge,
                    ),
                    level,
                })
            })
        })
    }

    pub fn contains(self, brick: BrickCoordinate) -> bool {
        let offset =
            |component: i64, origin: i64| to_unsigned(component).wrapping_sub(to_unsigned(origin));
        [
            offset(brick.x(), self.origin.x()),
            offset(brick.y(), self.origin.y()),
            offset(brick.z(), self.origin.z()),
        ]
        .into_iter()
        .all(|distance| distance < self.edge_length())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node<V> {
    Leaf(V),
    Branch(Box<[Node<V>; 8]>),
}

/// Maps every brick to a value; see the crate documentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionOctree<V> {
    root: Node<V>,
}

/// Which child of a node at `node_level` holds the point `offset` (unsigned, per axis).
fn child_index(offset: [u64; 3], node_level: u8) -> usize {
    let bit = |axis: usize| ((offset[axis] >> (node_level - 1)) & 1) as usize;
    bit(0) | (bit(1) << 1) | (bit(2) << 2)
}

impl<V: Clone + PartialEq> RegionOctree<V> {
    /// Every brick holds `default`.
    pub fn new(default: V) -> Self {
        Self {
            root: Node::Leaf(default),
        }
    }

    pub fn get(&self, brick: BrickCoordinate) -> &V {
        let offset = [
            to_unsigned(brick.x()),
            to_unsigned(brick.y()),
            to_unsigned(brick.z()),
        ];
        let mut node = &self.root;
        let mut level = ROOT_LEVEL;
        while let Node::Branch(children) = node {
            node = &children[child_index(offset, level)];
            level -= 1;
        }
        match node {
            Node::Leaf(value) => value,
            Node::Branch(_) => unreachable!("the loop only ends at a leaf"),
        }
    }

    /// Gives every brick of `cube` the value `value`, replacing whatever was there.
    pub fn set_cube(&mut self, cube: BrickCube, value: V) {
        let origin = cube.origin();
        let offset = [
            to_unsigned(origin.x()),
            to_unsigned(origin.y()),
            to_unsigned(origin.z()),
        ];
        assign(&mut self.root, ROOT_LEVEL, offset, cube.level(), value);
    }

    /// Every leaf cube with its value, in Morton (z-order) order. The leaves tile the whole
    /// coordinate space, and the order is the same for equal trees.
    pub fn leaves(&self) -> Vec<(BrickCube, &V)> {
        let mut leaves = Vec::new();
        collect_leaves(&self.root, ROOT_LEVEL, [0; 3], &mut leaves);
        leaves
    }

    /// Total nodes, a measure of how much the tree costs to store or send.
    pub fn node_count(&self) -> usize {
        fn count<V>(node: &Node<V>) -> usize {
            match node {
                Node::Leaf(_) => 1,
                Node::Branch(children) => 1 + children.iter().map(count).sum::<usize>(),
            }
        }
        count(&self.root)
    }
}

fn assign<V: Clone + PartialEq>(
    node: &mut Node<V>,
    node_level: u8,
    target_offset: [u64; 3],
    target_level: u8,
    value: V,
) {
    if node_level == target_level {
        *node = Node::Leaf(value);
        return;
    }
    if let Node::Leaf(existing) = node {
        if *existing == value {
            return;
        }
        let existing = existing.clone();
        *node = Node::Branch(Box::new(std::array::from_fn(|_| {
            Node::Leaf(existing.clone())
        })));
    }
    let Node::Branch(children) = node else {
        unreachable!("a leaf was just split into a branch");
    };
    assign(
        &mut children[child_index(target_offset, node_level)],
        node_level - 1,
        target_offset,
        target_level,
        value,
    );
    if let Node::Leaf(first) = &children[0]
        && children
            .iter()
            .all(|child| matches!(child, Node::Leaf(other) if other == first))
    {
        *node = Node::Leaf(first.clone());
    }
}

fn collect_leaves<'tree, V>(
    node: &'tree Node<V>,
    level: u8,
    origin_offset: [u64; 3],
    leaves: &mut Vec<(BrickCube, &'tree V)>,
) {
    match node {
        Node::Leaf(value) => {
            let origin = BrickCoordinate::new(
                from_unsigned(origin_offset[0]),
                from_unsigned(origin_offset[1]),
                from_unsigned(origin_offset[2]),
            );
            leaves.push((BrickCube { origin, level }, value));
        }
        Node::Branch(children) => {
            for (index, child) in children.iter().enumerate() {
                let half = 1u64 << (level - 1);
                let child_origin = std::array::from_fn(|axis| {
                    origin_offset[axis] + if index >> axis & 1 == 1 { half } else { 0 }
                });
                collect_leaves(child, level - 1, child_origin, leaves);
            }
        }
    }
}

/// The levels given do not describe a tiling of the whole space; see [`tile_from_levels`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TilingError {
    #[error("the levels end before the whole space is covered")]
    Incomplete,
    #[error("a cube is larger than the space left for it")]
    TooLarge,
    #[error("there are more levels than the cubes that cover the space")]
    Excess,
}

/// The leaf cubes of a tree, given only their levels in Morton order, as [`RegionOctree::leaves`]
/// lists them. Leaves tile the whole space, so each one's place follows from the levels before it.
pub fn tile_from_levels(levels: &[u8]) -> Result<Vec<BrickCube>, TilingError> {
    fn tile(
        levels: &mut std::slice::Iter<'_, u8>,
        level: u8,
        origin_offset: [u64; 3],
        cubes: &mut Vec<BrickCube>,
    ) -> Result<(), TilingError> {
        let next = *levels.as_slice().first().ok_or(TilingError::Incomplete)?;
        if next > level {
            return Err(TilingError::TooLarge);
        }
        if next == level {
            levels.next();
            let origin = BrickCoordinate::new(
                from_unsigned(origin_offset[0]),
                from_unsigned(origin_offset[1]),
                from_unsigned(origin_offset[2]),
            );
            cubes.push(BrickCube { origin, level });
            return Ok(());
        }
        let half = 1u64 << (level - 1);
        for index in 0..8 {
            let child_origin = std::array::from_fn(|axis| {
                origin_offset[axis] + if index >> axis & 1 == 1 { half } else { 0 }
            });
            tile(levels, level - 1, child_origin, cubes)?;
        }
        Ok(())
    }
    let mut remaining = levels.iter();
    let mut cubes = Vec::new();
    tile(&mut remaining, ROOT_LEVEL, [0; 3], &mut cubes)?;
    if remaining.next().is_some() {
        return Err(TilingError::Excess);
    }
    Ok(cubes)
}

const _: () = assert!(BRICK_COORDINATE_MAXIMUM - BRICK_COORDINATE_MINIMUM + 1 == 1 << ROOT_LEVEL);

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use proptest::prelude::*;

    use super::*;

    fn brick(x: i64, y: i64, z: i64) -> BrickCoordinate {
        BrickCoordinate::new(x, y, z)
    }

    fn cube(x: i64, y: i64, z: i64, level: u8) -> BrickCube {
        BrickCube::new(brick(x, y, z), level).expect("test cubes are aligned")
    }

    #[test]
    fn covering_lists_the_cubes_that_touch_a_region() {
        let region = BrickBox::new(brick(-1, 0, 0), brick(8, 3, 1)).unwrap();
        // Cubes of 4 bricks: x spans -4..-1, 0..3, 4..7, 8..11; y and z need one cube each row.
        let level_two: Vec<_> = BrickCube::covering(region, 2).collect();
        assert_eq!(level_two.len(), 4);
        assert_eq!(level_two[0], cube(-4, 0, 0, 2));
        assert_eq!(level_two[3], cube(8, 0, 0, 2));
        let single = BrickBox::new(brick(5, 5, 5), brick(5, 5, 5)).unwrap();
        assert_eq!(
            BrickCube::covering(single, 0).collect::<Vec<_>>(),
            [cube(5, 5, 5, 0)]
        );
        // A cube so large it holds the whole region is the only one.
        assert_eq!(BrickCube::covering(region, ROOT_LEVEL).count(), 1);
        // Cubes of 8 bricks: x runs over -8..-1, 0..7 and 8..15; y and z fit in one cube each.
        assert_eq!(BrickCube::covering(region, 3).count(), 3);
    }

    proptest! {
        #[test]
        fn covering_is_exactly_the_cubes_containing_the_regions_bricks(
            corner in (-12i64..12, -12i64..12, -12i64..12),
            extent in (0i64..10, 0i64..10, 0i64..10),
            level in 0u8..4,
        ) {
            let region = BrickBox::new(
                brick(corner.0, corner.1, corner.2),
                brick(corner.0 + extent.0, corner.1 + extent.1, corner.2 + extent.2),
            ).unwrap();
            let listed: Vec<_> = BrickCube::covering(region, level).collect();
            let expected: std::collections::BTreeSet<_> = region
                .bricks()
                .map(|candidate| {
                    let origin = BrickCube::containing(candidate, level).origin();
                    (origin.z(), origin.y(), origin.x())
                })
                .collect();
            let actual: Vec<_> = listed
                .iter()
                .map(|found| (found.origin().z(), found.origin().y(), found.origin().x()))
                .collect();
            prop_assert_eq!(actual.len(), listed.len());
            prop_assert_eq!(actual.iter().copied().collect::<std::collections::BTreeSet<_>>(), expected);
            // Listed once each, in z, y, x order.
            prop_assert!(actual.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }

    #[test]
    fn a_new_tree_is_one_leaf_holding_the_default_everywhere() {
        let tree = RegionOctree::new('a');
        assert_eq!(tree.node_count(), 1);
        for probe in [
            brick(0, 0, 0),
            brick(-1, 5, 9),
            brick(BRICK_COORDINATE_MINIMUM, 0, 0),
            brick(
                BRICK_COORDINATE_MAXIMUM,
                BRICK_COORDINATE_MAXIMUM,
                BRICK_COORDINATE_MAXIMUM,
            ),
        ] {
            assert_eq!(tree.get(probe), &'a');
        }
    }

    #[test]
    fn assigning_one_brick_changes_only_that_brick() {
        let mut tree = RegionOctree::new('a');
        tree.set_cube(cube(3, -2, 7, 0), 'b');
        assert_eq!(tree.get(brick(3, -2, 7)), &'b');
        for neighbour in [
            brick(4, -2, 7),
            brick(2, -2, 7),
            brick(3, -1, 7),
            brick(3, -2, 6),
        ] {
            assert_eq!(tree.get(neighbour), &'a');
        }
    }

    #[test]
    fn isolating_a_single_brick_costs_seven_extra_leaves_per_level() {
        let mut tree = RegionOctree::new('a');
        tree.set_cube(cube(0, 0, 0, 0), 'b');
        // Each of the 56 levels above the brick splits into 8 children, one of which is split further.
        assert_eq!(tree.node_count(), 1 + 8 * usize::from(ROOT_LEVEL));
        assert_eq!(tree.leaves().len(), 1 + 7 * usize::from(ROOT_LEVEL));
    }

    #[test]
    fn a_coarse_cube_costs_far_fewer_nodes_than_a_fine_one() {
        let mut coarse = RegionOctree::new('a');
        coarse.set_cube(cube(0, 0, 0, 40), 'b');
        let mut fine = RegionOctree::new('a');
        fine.set_cube(cube(0, 0, 0, 0), 'b');
        assert_eq!(coarse.node_count(), 1 + 8 * usize::from(ROOT_LEVEL - 40));
        assert!(coarse.node_count() < fine.node_count() / 3);
    }

    #[test]
    fn filling_all_eight_children_collapses_the_parent_back_into_a_leaf() {
        let mut tree = RegionOctree::new('a');
        for index in 0..8i64 {
            tree.set_cube(cube(index & 1, index >> 1 & 1, index >> 2 & 1, 0), 'b');
        }
        let mut expected = RegionOctree::new('a');
        expected.set_cube(cube(0, 0, 0, 1), 'b');
        assert_eq!(tree, expected);
        assert!(tree.node_count() < 1 + 8 * usize::from(ROOT_LEVEL));
    }

    #[test]
    fn restoring_the_default_everywhere_returns_to_a_single_leaf() {
        let mut tree = RegionOctree::new('a');
        tree.set_cube(cube(5, 5, 5, 0), 'b');
        tree.set_cube(BrickCube::containing(brick(0, 0, 0), ROOT_LEVEL), 'a');
        assert_eq!(tree.node_count(), 1);
    }

    #[test]
    fn assigning_a_value_a_region_already_holds_changes_nothing() {
        let mut tree = RegionOctree::new('a');
        tree.set_cube(cube(0, 0, 0, 10), 'b');
        let before = tree.clone();
        tree.set_cube(cube(4, 4, 4, 2), 'b');
        assert_eq!(tree, before);
    }

    #[test]
    fn the_shape_does_not_depend_on_assignment_order() {
        // Disjoint cubes, so the final mapping is the same whatever the order.
        let cubes = [
            (cube(0, 0, 0, 3), 'b'),
            (cube(8, 0, 0, 3), 'c'),
            (cube(16, 1, 1, 0), 'd'),
            (cube(-8, -8, -8, 3), 'b'),
        ];
        let mut forward = RegionOctree::new('a');
        for &(cube, value) in &cubes {
            forward.set_cube(cube, value);
        }
        let mut backward = RegionOctree::new('a');
        for &(cube, value) in cubes.iter().rev() {
            backward.set_cube(cube, value);
        }
        assert_eq!(forward, backward);
        assert_eq!(forward.get(brick(16, 1, 1)), &'d');
        assert_eq!(forward.get(brick(16, 1, 2)), &'a');
    }

    #[test]
    fn extreme_coordinates_and_the_root_cube_work() {
        let mut tree = RegionOctree::new(0u8);
        let lowest = brick(
            BRICK_COORDINATE_MINIMUM,
            BRICK_COORDINATE_MINIMUM,
            BRICK_COORDINATE_MINIMUM,
        );
        let highest = brick(
            BRICK_COORDINATE_MAXIMUM,
            BRICK_COORDINATE_MAXIMUM,
            BRICK_COORDINATE_MAXIMUM,
        );
        tree.set_cube(BrickCube::containing(lowest, 0), 1);
        tree.set_cube(BrickCube::containing(highest, 0), 2);
        assert_eq!(tree.get(lowest), &1);
        assert_eq!(tree.get(highest), &2);
        assert_eq!(tree.get(brick(0, 0, 0)), &0);

        tree.set_cube(BrickCube::containing(brick(0, 0, 0), ROOT_LEVEL), 9);
        assert_eq!(tree.node_count(), 1);
        assert_eq!(tree.get(highest), &9);
    }

    #[test]
    fn cubes_must_be_aligned_and_no_larger_than_the_root() {
        assert!(BrickCube::new(brick(8, 0, 0), 3).is_ok());
        assert_eq!(
            BrickCube::new(brick(4, 0, 0), 3),
            Err(InvalidBrickCube::Misaligned {
                origin: brick(4, 0, 0),
                level: 3
            })
        );
        assert_eq!(
            BrickCube::new(brick(-4, 0, 0), 3),
            Err(InvalidBrickCube::Misaligned {
                origin: brick(-4, 0, 0),
                level: 3
            })
        );
        assert!(BrickCube::new(brick(-8, 0, 0), 3).is_ok());
        assert_eq!(
            BrickCube::new(brick(0, 0, 0), ROOT_LEVEL + 1),
            Err(InvalidBrickCube::LevelTooLarge {
                level: ROOT_LEVEL + 1
            })
        );
        // Only the root itself is aligned at the root level.
        assert!(
            BrickCube::new(
                brick(
                    BRICK_COORDINATE_MINIMUM,
                    BRICK_COORDINATE_MINIMUM,
                    BRICK_COORDINATE_MINIMUM
                ),
                ROOT_LEVEL
            )
            .is_ok()
        );
        assert!(BrickCube::new(brick(0, 0, 0), ROOT_LEVEL).is_err());
    }

    #[test]
    fn containing_finds_the_aligned_cube_including_for_negative_bricks() {
        let around_negative = BrickCube::containing(brick(-1, -9, 0), 3);
        assert_eq!(around_negative.origin(), brick(-8, -16, 0));
        assert_eq!(around_negative.edge_length(), 8);
        assert!(around_negative.contains(brick(-1, -9, 0)));
        assert!(around_negative.contains(brick(-8, -16, 0)));
        assert!(around_negative.contains(brick(-1, -9, 7)));
        assert!(!around_negative.contains(brick(0, -9, 0)));
        assert!(!around_negative.contains(brick(-1, -17, 0)));
        assert!(!around_negative.contains(brick(-9, -9, 0)));
    }

    #[test]
    fn levels_alone_give_back_the_leaf_cubes() {
        let mut tree = RegionOctree::new('a');
        tree.set_cube(cube(0, 0, 0, 4), 'b');
        tree.set_cube(cube(-16, 0, 8, 2), 'c');
        tree.set_cube(cube(-3, 7, 1, 0), 'd');
        let leaves = tree.leaves();
        let levels: Vec<u8> = leaves.iter().map(|(leaf, _)| leaf.level()).collect();
        let cubes: Vec<BrickCube> = leaves.iter().map(|(leaf, _)| *leaf).collect();
        assert_eq!(tile_from_levels(&levels), Ok(cubes));
        assert_eq!(
            tile_from_levels(&[ROOT_LEVEL]),
            Ok(vec![BrickCube::containing(brick(0, 0, 0), ROOT_LEVEL)])
        );
    }

    #[test]
    fn levels_that_do_not_tile_the_space_are_refused() {
        let mut tree = RegionOctree::new('a');
        tree.set_cube(cube(0, 0, 0, 4), 'b');
        let levels: Vec<u8> = tree.leaves().iter().map(|(leaf, _)| leaf.level()).collect();
        assert_eq!(tile_from_levels(&[]), Err(TilingError::Incomplete));
        assert_eq!(
            tile_from_levels(&levels[..levels.len() - 1]),
            Err(TilingError::Incomplete)
        );
        let mut too_many = levels.clone();
        too_many.push(0);
        assert_eq!(tile_from_levels(&too_many), Err(TilingError::Excess));
        assert_eq!(
            tile_from_levels(&[ROOT_LEVEL + 1]),
            Err(TilingError::TooLarge)
        );
        // A cube bigger than the space left in its parent.
        let mut too_big = levels;
        too_big[1] = 20;
        assert_eq!(tile_from_levels(&too_big), Err(TilingError::TooLarge));
        // Only the root cube can be a lone level 56; the next level cannot appear after it.
        assert_eq!(
            tile_from_levels(&[ROOT_LEVEL, ROOT_LEVEL]),
            Err(TilingError::Excess)
        );
    }

    proptest! {
        #[test]
        fn the_levels_of_any_tree_give_back_its_cubes(
            assignments in proptest::collection::vec((-20i64..20, -20i64..20, -20i64..20, 0u8..5, 0u8..3), 0..12),
        ) {
            let mut tree = RegionOctree::new(0u8);
            for (x, y, z, level, value) in assignments {
                tree.set_cube(BrickCube::containing(brick(x, y, z), level), value);
            }
            let leaves = tree.leaves();
            let levels: Vec<u8> = leaves.iter().map(|(leaf, _)| leaf.level()).collect();
            let cubes: Vec<BrickCube> = leaves.iter().map(|(leaf, _)| *leaf).collect();
            prop_assert_eq!(tile_from_levels(&levels), Ok(cubes));
        }
    }

    #[test]
    fn leaves_tile_space_in_a_fixed_order_and_rebuild_the_same_tree() {
        let mut tree = RegionOctree::new('a');
        tree.set_cube(cube(0, 0, 0, 4), 'b');
        tree.set_cube(cube(16, 0, 0, 2), 'c');
        tree.set_cube(cube(-3, 7, 1, 0), 'd');

        let leaves = tree.leaves();
        let mut rebuilt = RegionOctree::new('a');
        for (leaf_cube, value) in &leaves {
            rebuilt.set_cube(*leaf_cube, **value);
        }
        assert_eq!(rebuilt, tree);
        assert_eq!(rebuilt.leaves(), leaves);
        for (leaf_cube, value) in &leaves {
            assert_eq!(tree.get(leaf_cube.origin()), *value);
        }
        // Morton order: comparing the per-level child indices from the top level down, as digits,
        // must be strictly increasing from one leaf to the next.
        let keys: Vec<Vec<u8>> = leaves
            .iter()
            .map(|(leaf_cube, _)| {
                let offsets = [
                    leaf_cube.origin().x(),
                    leaf_cube.origin().y(),
                    leaf_cube.origin().z(),
                ]
                .map(|component| (component - BRICK_COORDINATE_MINIMUM) as u64);
                (0..ROOT_LEVEL)
                    .rev()
                    .map(|bit| {
                        (0..3).fold(0u8, |digit, axis| {
                            digit | ((offsets[axis] >> bit & 1) as u8) << axis
                        })
                    })
                    .collect()
            })
            .collect();
        assert!(
            keys.windows(2).all(|pair| pair[0] < pair[1]),
            "leaves are not in Morton order"
        );
    }

    #[derive(Debug, Clone)]
    struct Assignment {
        origin: [i64; 3],
        level: u8,
        value: u8,
    }

    fn assignments() -> impl Strategy<Value = Vec<Assignment>> {
        // Cubes of up to 16 bricks per edge, aligned within a 32-brick domain centred on the origin.
        let assignment =
            (0u8..=4, [0i64..32, 0..32, 0..32], 0u8..4).prop_map(|(level, cell, value)| {
                let size = 1i64 << level;
                Assignment {
                    origin: cell.map(|component| (component / size) * size - 16),
                    level,
                    value,
                }
            });
        proptest::collection::vec(assignment, 0..40)
    }

    proptest! {
        /// The tree must agree with a plain map over the domain, and must stay canonical:
        /// rebuilding it from its own leaves gives an identical tree.
        #[test]
        fn behaves_like_a_plain_map_and_stays_canonical(assignments in assignments()) {
            let mut tree = RegionOctree::new(0u8);
            let mut model: HashMap<[i64; 3], u8> = HashMap::new();
            for assignment in &assignments {
                let origin = brick(assignment.origin[0], assignment.origin[1], assignment.origin[2]);
                tree.set_cube(BrickCube::new(origin, assignment.level).unwrap(), assignment.value);
                let size = 1i64 << assignment.level;
                for dx in 0..size { for dy in 0..size { for dz in 0..size {
                    model.insert(
                        [assignment.origin[0] + dx, assignment.origin[1] + dy, assignment.origin[2] + dz],
                        assignment.value,
                    );
                }}}
            }
            for x in -16..16 { for y in -16..16 { for z in -16..16 {
                let expected = model.get(&[x, y, z]).copied().unwrap_or(0);
                prop_assert_eq!(*tree.get(brick(x, y, z)), expected);
            }}}

            let mut rebuilt = RegionOctree::new(0u8);
            for (leaf_cube, value) in tree.leaves() {
                rebuilt.set_cube(leaf_cube, *value);
            }
            prop_assert_eq!(&rebuilt, &tree);
            // No branch may hold eight equal leaves.
            fn assert_canonical(node: &Node<u8>) -> bool {
                match node {
                    Node::Leaf(_) => true,
                    Node::Branch(children) => {
                        let all_equal_leaves = matches!(&children[0], Node::Leaf(first)
                            if children.iter().all(|child| matches!(child, Node::Leaf(v) if v == first)));
                        !all_equal_leaves && children.iter().all(assert_canonical)
                    }
                }
            }
            prop_assert!(assert_canonical(&tree.root));
        }
    }
}
