use std::fmt;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use voxel_space::{BrickCoordinate, VoxelCoordinate};

use crate::brick::Brick;
use crate::digest::Fnv1a64;
use crate::layout::{ChannelIndex, GridLayout};

/// A brick offered to a grid has a different number of channels than the grid's layout.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("brick has {actual} channels but the grid layout defines {expected}")]
pub struct ChannelCountMismatch {
    pub expected: usize,
    pub actual: usize,
}

/// A sparse, unbounded grid of voxels.
///
/// Bricks are created on first write of a non-default value and never removed implicitly: an
/// absent brick means "every channel at its default", whereas a present brick that happens to hold
/// only defaults is a distinct, deliberate state (callers layering edits over generated terrain
/// rely on that difference). Equality therefore compares which bricks exist as well as their
/// contents.
///
/// Bricks are held in `Arc`s and copied on write, so [`Clone`] is a cheap snapshot that later
/// edits do not disturb.
#[derive(Clone, PartialEq)]
pub struct Grid {
    layout: Arc<GridLayout>,
    bricks: FxHashMap<BrickCoordinate, Arc<Brick>>,
}

impl Grid {
    pub fn new(layout: Arc<GridLayout>) -> Self {
        Self {
            layout,
            bricks: FxHashMap::default(),
        }
    }

    pub fn layout(&self) -> &Arc<GridLayout> {
        &self.layout
    }

    /// The channel's default value wherever no brick exists.
    pub fn get(&self, channel: ChannelIndex, coordinate: VoxelCoordinate) -> u32 {
        let (brick_coordinate, local) = coordinate.split();
        match self.bricks.get(&brick_coordinate) {
            Some(brick) => brick.get(channel, local),
            None => self.layout.default_value(channel),
        }
    }

    /// Stores `value`, creating the brick if needed. Returns whether the stored value changed;
    /// writing the default into an absent brick changes nothing and allocates nothing.
    pub fn set(&mut self, channel: ChannelIndex, coordinate: VoxelCoordinate, value: u32) -> bool {
        let (brick_coordinate, local) = coordinate.split();
        match self.bricks.get_mut(&brick_coordinate) {
            Some(brick) => {
                // Check first so a no-op write does not copy a brick shared with a snapshot.
                if brick.get(channel, local) == value {
                    return false;
                }
                Arc::make_mut(brick).set(channel, local, value)
            }
            None => {
                if value == self.layout.default_value(channel) {
                    return false;
                }
                let mut brick = Brick::new(&self.layout);
                brick.set(channel, local, value);
                self.bricks.insert(brick_coordinate, Arc::new(brick));
                true
            }
        }
    }

    pub fn brick_count(&self) -> usize {
        self.bricks.len()
    }

    pub fn brick(&self, coordinate: BrickCoordinate) -> Option<&Arc<Brick>> {
        self.bricks.get(&coordinate)
    }

    /// Mutable access for batched edits. Copies the brick first if a snapshot shares it.
    pub fn brick_mut(&mut self, coordinate: BrickCoordinate) -> Option<&mut Brick> {
        self.bricks.get_mut(&coordinate).map(Arc::make_mut)
    }

    /// Like [`Grid::brick_mut`], first creating an all-default brick if none exists.
    pub fn brick_mut_or_default(&mut self, coordinate: BrickCoordinate) -> &mut Brick {
        let layout = &self.layout;
        let brick = self
            .bricks
            .entry(coordinate)
            .or_insert_with(|| Arc::new(Brick::new(layout)));
        Arc::make_mut(brick)
    }

    /// Adds or replaces a brick, returning the one it replaced.
    pub fn insert_brick(
        &mut self,
        coordinate: BrickCoordinate,
        brick: Arc<Brick>,
    ) -> Result<Option<Arc<Brick>>, ChannelCountMismatch> {
        if brick.channel_count() != self.layout.channel_count() {
            return Err(ChannelCountMismatch {
                expected: self.layout.channel_count(),
                actual: brick.channel_count(),
            });
        }
        Ok(self.bricks.insert(coordinate, brick))
    }

    pub fn remove_brick(&mut self, coordinate: BrickCoordinate) -> Option<Arc<Brick>> {
        self.bricks.remove(&coordinate)
    }

    /// Every brick, in no particular order. Use [`Grid::sorted_brick_coordinates`] where order
    /// matters (saving, hashing).
    pub fn bricks(&self) -> impl ExactSizeIterator<Item = (BrickCoordinate, &Arc<Brick>)> {
        self.bricks
            .iter()
            .map(|(&coordinate, brick)| (coordinate, brick))
    }

    pub fn sorted_brick_coordinates(&self) -> Vec<BrickCoordinate> {
        let mut coordinates: Vec<_> = self.bricks.keys().copied().collect();
        coordinates.sort_unstable();
        coordinates
    }

    /// A 64-bit digest of which bricks exist and what they hold: FNV-1a over, for each brick in
    /// coordinate order, its three coordinates (`i64`, little-endian) and its
    /// [`Brick::content_digest`]. Equal grids of the same layout give equal digests on every
    /// platform; the layout itself is not included.
    pub fn content_digest(&self) -> u64 {
        let mut hasher = Fnv1a64::new();
        for coordinate in self.sorted_brick_coordinates() {
            for component in [coordinate.x(), coordinate.y(), coordinate.z()] {
                hasher.write(&component.to_le_bytes());
            }
            hasher.write(&self.bricks[&coordinate].content_digest().to_le_bytes());
        }
        hasher.finish()
    }
}

impl fmt::Debug for Grid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Grid")
            .field("channel_count", &self.layout.channel_count())
            .field("brick_count", &self.bricks.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use proptest::prelude::*;
    use voxel_space::LocalVoxelIndex;
    use voxel_space::test_support::nearby_voxel_coordinate;

    use super::*;
    use crate::test_support::{MATERIAL_DEFAULT, StandardTestLayout, TEMPERATURE_DEFAULT};

    fn voxel(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    #[test]
    fn the_grid_digest_depends_on_where_bricks_are_and_what_they_hold_and_nothing_else() {
        let fixture = StandardTestLayout::default();
        let build = |edits: &[(i64, u32)]| {
            let mut grid = Grid::new(Arc::clone(&fixture.layout));
            for &(x, value) in edits {
                grid.set(fixture.material, voxel(x, 0, 0), value);
            }
            grid
        };
        let empty = build(&[]).content_digest();
        let one = build(&[(1, 5)]).content_digest();
        assert_ne!(empty, one);
        // Insertion order is irrelevant; so is editing a voxel and putting it back.
        assert_eq!(
            build(&[(1, 5), (40, 6)]).content_digest(),
            build(&[(40, 6), (1, 5)]).content_digest()
        );
        assert_eq!(
            build(&[(1, 5), (1, 0)]).content_digest(),
            build(&[(1, 5), (1, 0)]).content_digest()
        );
        // The same contents in a different brick, or a different value, are different.
        assert_ne!(one, build(&[(33, 5)]).content_digest());
        assert_ne!(one, build(&[(1, 6)]).content_digest());
        // Pinned, so a change to the digest definition is noticed.
        assert_eq!(empty, 0xcbf2_9ce4_8422_2325);
    }

    fn brick(x: i64, y: i64, z: i64) -> BrickCoordinate {
        BrickCoordinate::new(x, y, z)
    }

    fn empty_grid() -> (Grid, StandardTestLayout) {
        let fixture = StandardTestLayout::default();
        (Grid::new(fixture.layout.clone()), fixture)
    }

    #[test]
    fn reading_where_no_brick_exists_returns_the_channel_default() {
        let (grid, fixture) = empty_grid();
        let far_away = voxel(1_000_000_000_000, -5, 77);
        assert_eq!(grid.get(fixture.material, far_away), MATERIAL_DEFAULT);
        assert_eq!(grid.get(fixture.temperature, far_away), TEMPERATURE_DEFAULT);
        assert_eq!(grid.brick_count(), 0);
    }

    #[test]
    fn writing_a_default_value_into_an_absent_brick_allocates_nothing() {
        let (mut grid, fixture) = empty_grid();
        assert!(!grid.set(fixture.material, voxel(5, 5, 5), MATERIAL_DEFAULT));
        assert!(!grid.set(fixture.temperature, voxel(5, 5, 5), TEMPERATURE_DEFAULT));
        assert_eq!(grid.brick_count(), 0);
    }

    #[test]
    fn writes_read_back_across_brick_boundaries_and_negative_coordinates() {
        let (mut grid, fixture) = empty_grid();
        let writes = [
            (voxel(0, 0, 0), brick(0, 0, 0), 1),
            (voxel(31, 31, 31), brick(0, 0, 0), 2),
            (voxel(32, 0, 0), brick(1, 0, 0), 3),
            (voxel(-1, -1, -1), brick(-1, -1, -1), 4),
            (voxel(-33, 5, 100), brick(-2, 0, 3), 5),
        ];
        for (coordinate, _, value) in writes {
            assert!(grid.set(fixture.material, coordinate, value));
        }

        for (coordinate, expected_brick, value) in writes {
            assert_eq!(grid.get(fixture.material, coordinate), value);
            assert!(grid.brick(expected_brick).is_some(), "{expected_brick}");
        }
        assert_eq!(grid.brick_count(), 4);
        // A neighbour of a written voxel is untouched.
        assert_eq!(grid.get(fixture.material, voxel(1, 0, 0)), MATERIAL_DEFAULT);
        assert_eq!(
            grid.get(fixture.material, voxel(-2, -1, -1)),
            MATERIAL_DEFAULT
        );
    }

    #[test]
    fn set_reports_a_change_only_when_the_value_differs() {
        let (mut grid, fixture) = empty_grid();
        assert!(grid.set(fixture.material, voxel(1, 2, 3), 8));
        assert!(!grid.set(fixture.material, voxel(1, 2, 3), 8));
        assert!(grid.set(fixture.material, voxel(1, 2, 3), 9));
        // Writing the default into an existing brick is a real change.
        assert!(grid.set(fixture.material, voxel(1, 2, 3), MATERIAL_DEFAULT));
        assert_eq!(grid.brick_count(), 1);
    }

    #[test]
    fn channels_do_not_interfere() {
        let (mut grid, fixture) = empty_grid();
        grid.set(fixture.material, voxel(1, 2, 3), 8);
        assert_eq!(
            grid.get(fixture.temperature, voxel(1, 2, 3)),
            TEMPERATURE_DEFAULT
        );
        grid.set(fixture.temperature, voxel(1, 2, 3), 300);
        assert_eq!(grid.get(fixture.material, voxel(1, 2, 3)), 8);
    }

    #[test]
    fn cloning_makes_a_snapshot_that_later_edits_do_not_disturb() {
        let (mut grid, fixture) = empty_grid();
        grid.set(fixture.material, voxel(1, 1, 1), 5);
        grid.set(fixture.material, voxel(100, 1, 1), 6);
        let snapshot = grid.clone();

        grid.set(fixture.material, voxel(1, 1, 1), 9);

        assert_eq!(snapshot.get(fixture.material, voxel(1, 1, 1)), 5);
        assert_eq!(grid.get(fixture.material, voxel(1, 1, 1)), 9);
        // The edited brick was copied; the untouched one is still shared.
        let edited = brick(0, 0, 0);
        let untouched = brick(3, 0, 0);
        assert!(!Arc::ptr_eq(
            snapshot.brick(edited).unwrap(),
            grid.brick(edited).unwrap()
        ));
        assert!(Arc::ptr_eq(
            snapshot.brick(untouched).unwrap(),
            grid.brick(untouched).unwrap()
        ));
    }

    #[test]
    fn writing_a_value_a_voxel_already_holds_does_not_unshare_a_snapshotted_brick() {
        let (mut grid, fixture) = empty_grid();
        grid.set(fixture.material, voxel(1, 1, 1), 5);
        let snapshot = grid.clone();

        assert!(!grid.set(fixture.material, voxel(1, 1, 1), 5));

        assert!(Arc::ptr_eq(
            snapshot.brick(brick(0, 0, 0)).unwrap(),
            grid.brick(brick(0, 0, 0)).unwrap()
        ));
    }

    #[test]
    fn batched_edits_through_brick_mut_also_copy_on_write() {
        let (mut grid, fixture) = empty_grid();
        grid.set(fixture.material, voxel(1, 1, 1), 5);
        let snapshot = grid.clone();

        grid.brick_mut(brick(0, 0, 0)).expect("brick exists").set(
            fixture.material,
            LocalVoxelIndex::new(1, 1, 1),
            7,
        );

        assert_eq!(grid.get(fixture.material, voxel(1, 1, 1)), 7);
        assert_eq!(snapshot.get(fixture.material, voxel(1, 1, 1)), 5);
    }

    #[test]
    fn brick_mut_does_not_create_bricks_but_the_or_default_variant_does() {
        let (mut grid, fixture) = empty_grid();
        assert!(grid.brick_mut(brick(2, 2, 2)).is_none());
        assert_eq!(grid.brick_count(), 0);

        grid.brick_mut_or_default(brick(2, 2, 2)).set(
            fixture.material,
            LocalVoxelIndex::new(0, 0, 0),
            3,
        );

        assert_eq!(grid.brick_count(), 1);
        assert_eq!(grid.get(fixture.material, voxel(64, 64, 64)), 3);
        // Calling it again returns the existing brick rather than replacing it.
        grid.brick_mut_or_default(brick(2, 2, 2));
        assert_eq!(grid.get(fixture.material, voxel(64, 64, 64)), 3);
    }

    #[test]
    fn insert_brick_replaces_and_returns_the_previous_brick() {
        let (mut grid, fixture) = empty_grid();
        let mut first = Brick::new(&fixture.layout);
        first.set(fixture.material, LocalVoxelIndex::new(0, 0, 0), 1);
        let second = Brick::new(&fixture.layout);

        assert_eq!(
            grid.insert_brick(brick(0, 0, 0), Arc::new(first.clone())),
            Ok(None)
        );
        let replaced = grid
            .insert_brick(brick(0, 0, 0), Arc::new(second))
            .expect("channel counts match")
            .expect("a brick was replaced");

        assert_eq!(*replaced, first);
        assert_eq!(grid.get(fixture.material, voxel(0, 0, 0)), MATERIAL_DEFAULT);
    }

    #[test]
    fn insert_brick_rejects_a_brick_with_the_wrong_channel_count() {
        let (mut grid, _) = empty_grid();
        let one_channel = Brick::from_channels(vec![crate::ChannelStorage::uniform(0)]);

        assert_eq!(
            grid.insert_brick(brick(0, 0, 0), Arc::new(one_channel)),
            Err(ChannelCountMismatch {
                expected: 2,
                actual: 1
            })
        );
        assert_eq!(grid.brick_count(), 0);
    }

    #[test]
    fn remove_brick_returns_it_and_reverts_the_area_to_defaults() {
        let (mut grid, fixture) = empty_grid();
        grid.set(fixture.material, voxel(1, 1, 1), 5);

        let removed = grid.remove_brick(brick(0, 0, 0)).expect("brick existed");

        assert_eq!(
            removed.get(fixture.material, LocalVoxelIndex::new(1, 1, 1)),
            5
        );
        assert_eq!(grid.get(fixture.material, voxel(1, 1, 1)), MATERIAL_DEFAULT);
        assert!(grid.remove_brick(brick(0, 0, 0)).is_none());
    }

    #[test]
    fn sorted_brick_coordinates_do_not_depend_on_insertion_order() {
        let (mut grid, fixture) = empty_grid();
        let positions = [
            (50, 0, 0),
            (-40, 0, 0),
            (0, 0, 0),
            (0, 70, 0),
            (0, -70, 5),
            (0, 0, 33),
        ];
        for (x, y, z) in positions {
            grid.set(fixture.material, voxel(x, y, z), 1);
        }

        let sorted = grid.sorted_brick_coordinates();

        assert_eq!(
            sorted,
            [
                brick(-2, 0, 0),
                brick(0, -3, 0),
                brick(0, 0, 0),
                brick(0, 0, 1),
                brick(0, 2, 0),
                brick(1, 0, 0),
            ]
        );
        assert_eq!(sorted.len(), grid.bricks().len());
    }

    #[test]
    fn grids_with_equal_bricks_are_equal_regardless_of_write_order() {
        let (mut left, fixture) = empty_grid();
        let mut right = Grid::new(fixture.layout.clone());
        left.set(fixture.material, voxel(1, 1, 1), 5);
        left.set(fixture.material, voxel(100, 0, 0), 6);
        right.set(fixture.material, voxel(100, 0, 0), 6);
        right.set(fixture.material, voxel(1, 1, 1), 5);
        assert_eq!(left, right);

        right.set(fixture.temperature, voxel(1, 1, 1), 99);
        assert_ne!(left, right);
    }

    #[test]
    fn an_explicit_default_brick_is_not_equal_to_an_absent_one() {
        let (empty, fixture) = empty_grid();
        let mut with_default_brick = Grid::new(fixture.layout.clone());
        with_default_brick.brick_mut_or_default(brick(0, 0, 0));
        assert_ne!(empty, with_default_brick);
    }

    #[test]
    fn debug_output_summarises_instead_of_dumping_voxels() {
        let (mut grid, fixture) = empty_grid();
        grid.set(fixture.material, voxel(1, 1, 1), 5);
        assert_eq!(
            format!("{grid:?}"),
            "Grid { channel_count: 2, brick_count: 1 }"
        );
    }

    #[derive(Debug, Clone)]
    struct Write {
        channel_choice: usize,
        coordinate: VoxelCoordinate,
        value: u32,
    }

    fn writes() -> impl Strategy<Value = Vec<Write>> {
        let write = (
            0usize..2,
            nearby_voxel_coordinate(),
            prop_oneof![Just(0u32), Just(1), Just(20), Just(21)],
        )
            .prop_map(|(channel_choice, coordinate, value)| Write {
                channel_choice,
                coordinate,
                value,
            });
        proptest::collection::vec(write, 0..300)
    }

    proptest! {
        /// A grid must agree with a plain map from (channel, coordinate) to value, and must
        /// allocate a brick exactly when a non-default value is first written into it.
        #[test]
        fn behaves_like_a_map_of_voxel_values(
            writes in writes(),
            probes in proptest::collection::vec(nearby_voxel_coordinate(), 0..50),
        ) {
            let (mut grid, fixture) = empty_grid();
            let channels = [fixture.material, fixture.temperature];
            let defaults = [MATERIAL_DEFAULT, TEMPERATURE_DEFAULT];
            let mut model: HashMap<(usize, VoxelCoordinate), u32> = HashMap::new();
            let mut bricks_ever_given_a_non_default_value = BTreeSet::new();

            for write in &writes {
                let channel = channels[write.channel_choice];
                let default = defaults[write.channel_choice];
                let previous = model
                    .get(&(write.channel_choice, write.coordinate))
                    .copied()
                    .unwrap_or(default);
                prop_assert_eq!(
                    grid.set(channel, write.coordinate, write.value),
                    previous != write.value
                );
                model.insert((write.channel_choice, write.coordinate), write.value);
                if write.value != default {
                    bricks_ever_given_a_non_default_value.insert(write.coordinate.split().0);
                }
            }

            for (choice, channel) in channels.iter().enumerate() {
                for write in &writes {
                    let expected = model[&(write.channel_choice, write.coordinate)];
                    if write.channel_choice == choice {
                        prop_assert_eq!(grid.get(*channel, write.coordinate), expected);
                    }
                }
                for probe in &probes {
                    let expected = model.get(&(choice, *probe)).copied().unwrap_or(defaults[choice]);
                    prop_assert_eq!(grid.get(*channel, *probe), expected);
                }
            }
            prop_assert_eq!(
                grid.sorted_brick_coordinates(),
                bricks_ever_given_a_non_default_value.into_iter().collect::<Vec<_>>()
            );
        }
    }
}
