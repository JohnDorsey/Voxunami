//! Changing what is drawn without a gap or an overlap (`docs/lod-spec.md`, 3.6: a node replaced by its children, or the reverse, swaps in one frame).
//!
//! What is drawn at any moment is a set of cubes that do not overlap: bricks (detail level 0) and nodes. What should be drawn, [`select_nodes`](crate::select_nodes)
//! says, is another such set, and the two differ as the viewer moves. A cube that is to go is replaced by the cubes that are to come in its place, and the
//! two must change together: if it goes before they are ready there is a hole, and if they come before it goes there is a picture drawn twice. So the cubes are
//! taken in **groups**: a cube and the cubes of the other set that it overlaps (they are its halves, or its quarters, or the cube that holds it) change together,
//! and only when every cube that comes in is ready.

use std::collections::{HashMap, HashSet};

use region_octree::BrickCube;

/// Cubes that go and cubes that come, which are to change in one frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Swap {
    pub remove: Vec<BrickCube>,
    pub add: Vec<BrickCube>,
}

/// The order cubes are put in: the biggest first, then by place.
fn order(cube: &BrickCube) -> (std::cmp::Reverse<u8>, [i128; 3]) {
    let origin = cube.origin();
    (
        std::cmp::Reverse(cube.level()),
        [origin.x(), origin.y(), origin.z()],
    )
}

/// The cube of `set` that holds `cube` (and is not it), if there is one. Cubes of a set do not overlap, so there is at most one.
fn holder(cube: BrickCube, set: &HashSet<BrickCube>, highest_level: u8) -> Option<BrickCube> {
    (cube.level() + 1..=highest_level)
        .map(|level| BrickCube::containing(cube.origin(), level))
        .find(|candidate| set.contains(candidate))
}

/// The changes that can be made now, from what is `shown` to what is `wanted`: the groups whose cubes that come are all `ready`, and the cubes that go and
/// have nothing coming (they are out of what is wanted). Biggest first. Neither set has two cubes that overlap, and after the changes `shown` has none either,
/// and every brick that was covered by a shown cube and is covered by a wanted one is covered still (by the cube that was shown, or by the ones that replaced it).
pub fn plan_swaps(
    shown: &HashSet<BrickCube>,
    wanted: &HashSet<BrickCube>,
    ready: impl Fn(BrickCube) -> bool,
) -> Vec<Swap> {
    let going: HashSet<BrickCube> = shown.difference(wanted).copied().collect();
    let coming: HashSet<BrickCube> = wanted.difference(shown).copied().collect();
    let highest = |set: &HashSet<BrickCube>| set.iter().map(|cube| cube.level()).max().unwrap_or(0);
    let (highest_going, highest_coming) = (highest(&going), highest(&coming));

    // A group is named by its biggest cube. A cube that something in the other set holds is in that cube's group, and one that nothing holds is the name of
    // its own (and of the cubes it holds, if it holds any).
    let mut groups: HashMap<BrickCube, Swap> = HashMap::new();
    for &cube in &going {
        let name = holder(cube, &coming, highest_coming).unwrap_or(cube);
        groups
            .entry(name)
            .or_insert_with(|| Swap {
                remove: Vec::new(),
                add: Vec::new(),
            })
            .remove
            .push(cube);
    }
    for &cube in &coming {
        let name = holder(cube, &going, highest_going).unwrap_or(cube);
        groups
            .entry(name)
            .or_insert_with(|| Swap {
                remove: Vec::new(),
                add: Vec::new(),
            })
            .add
            .push(cube);
    }

    let mut chosen: Vec<(BrickCube, Swap)> = groups
        .into_iter()
        .filter(|(_, swap)| swap.add.iter().all(|&cube| ready(cube)))
        .collect();
    chosen.sort_by_key(|(name, _)| order(name));
    chosen
        .into_iter()
        .map(|(_, mut swap)| {
            swap.remove.sort_by_key(order);
            swap.add.sort_by_key(order);
            swap
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_space::BrickCoordinate;

    use super::*;

    fn cube(x: i128, y: i128, z: i128, level: u8) -> BrickCube {
        BrickCube::new(BrickCoordinate::new(x, y, z), level).expect("aligned")
    }

    fn set(cubes: &[BrickCube]) -> HashSet<BrickCube> {
        cubes.iter().copied().collect()
    }

    /// The eight halves of a cube.
    fn halves(parent: BrickCube) -> Vec<BrickCube> {
        let half = 1i128 << (parent.level() - 1);
        let origin = parent.origin();
        (0..8)
            .map(|octant| {
                cube(
                    origin.x() + (octant & 1) * half,
                    origin.y() + ((octant >> 1) & 1) * half,
                    origin.z() + (octant >> 2) * half,
                    parent.level() - 1,
                )
            })
            .collect()
    }

    #[test]
    fn nothing_changes_when_what_is_shown_is_what_is_wanted() {
        let cubes = set(&[cube(0, 0, 0, 2), cube(4, 0, 0, 1)]);
        assert!(plan_swaps(&cubes, &cubes, |_| true).is_empty());
        assert!(plan_swaps(&HashSet::new(), &HashSet::new(), |_| true).is_empty());
    }

    #[test]
    fn a_node_is_replaced_by_its_halves_in_one_swap_when_all_of_them_are_ready_and_not_before() {
        let parent = cube(0, 0, 0, 2);
        let children = halves(parent);
        let shown = set(&[parent]);
        let wanted: HashSet<BrickCube> = children.iter().copied().collect();
        assert!(
            plan_swaps(&shown, &wanted, |cube| cube != children[5]).is_empty(),
            "one of the eight is not ready: the node stays"
        );
        let swaps = plan_swaps(&shown, &wanted, |_| true);
        assert_eq!(swaps.len(), 1);
        assert_eq!(swaps[0].remove, [parent]);
        assert_eq!(set(&swaps[0].add), wanted);
    }

    #[test]
    fn eight_halves_are_replaced_by_their_node_in_one_swap_when_it_is_ready() {
        let parent = cube(8, 0, 8, 3);
        let children = halves(parent);
        let shown: HashSet<BrickCube> = children.iter().copied().collect();
        let wanted = set(&[parent]);
        assert!(plan_swaps(&shown, &wanted, |_| false).is_empty());
        let swaps = plan_swaps(&shown, &wanted, |_| true);
        assert_eq!(swaps.len(), 1);
        assert_eq!(set(&swaps[0].remove), shown);
        assert_eq!(swaps[0].add, [parent]);
    }

    #[test]
    fn a_node_over_bricks_changes_with_all_the_bricks_in_one_swap() {
        let node = cube(2, 4, 6, 1);
        let bricks = halves(node);
        let swaps = plan_swaps(&set(&bricks), &set(&[node]), |_| true);
        assert_eq!(swaps.len(), 1);
        assert_eq!(swaps[0].remove.len(), 8);
        assert_eq!(swaps[0].add, [node]);
    }

    #[test]
    fn a_cube_that_goes_with_nothing_to_take_its_place_goes_at_once_and_one_that_comes_where_nothing_is_waits_to_be_ready()
     {
        let leaving = cube(0, 0, 0, 3);
        let arriving = cube(64, 0, 0, 3);
        let shown = set(&[leaving]);
        let wanted = set(&[arriving]);
        let swaps = plan_swaps(&shown, &wanted, |_| false);
        assert_eq!(
            swaps,
            [Swap {
                remove: vec![leaving],
                add: Vec::new()
            }]
        );
        let swaps = plan_swaps(&shown, &wanted, |_| true);
        assert_eq!(swaps.len(), 2);
        assert!(swaps.iter().any(|swap| swap.add == [arriving]));
    }

    #[test]
    fn groups_are_independent_and_come_biggest_first() {
        let big = cube(0, 0, 0, 4);
        let small = cube(32, 0, 0, 1);
        let shown = set(&[big, small]);
        let mut wanted: HashSet<BrickCube> = halves(big).into_iter().collect();
        wanted.extend(halves(small));
        let one_not_ready = halves(small)[0];
        let swaps = plan_swaps(&shown, &wanted, |cube| cube != one_not_ready);
        assert_eq!(swaps.len(), 1, "the big one changes, the small one waits");
        assert_eq!(swaps[0].remove, [big]);
        let swaps = plan_swaps(&shown, &wanted, |_| true);
        let levels: Vec<u8> = swaps.iter().map(|swap| swap.remove[0].level()).collect();
        assert_eq!(levels, [4, 1]);
    }

    /// A tiling of the cube of `level` at the origin by random splitting: each cube is split in eight with the chance given, down to level 0.
    fn tiling(level: u8, mut seed: u64, split_in: u64) -> Vec<BrickCube> {
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut done = Vec::new();
        let mut todo = vec![cube(0, 0, 0, level)];
        while let Some(current) = todo.pop() {
            if current.level() > 0 && next() % 100 < split_in {
                todo.extend(halves(current));
            } else {
                done.push(current);
            }
        }
        done
    }

    fn bricks_of(cubes: &HashSet<BrickCube>) -> HashSet<BrickCoordinate> {
        let mut bricks = HashSet::new();
        for cube in cubes {
            let edge = cube.edge_length() as i128;
            for x in 0..edge {
                for y in 0..edge {
                    for z in 0..edge {
                        let origin = cube.origin();
                        let brick =
                            BrickCoordinate::new(origin.x() + x, origin.y() + y, origin.z() + z);
                        assert!(bricks.insert(brick), "{brick:?} is in two cubes");
                    }
                }
            }
        }
        bricks
    }

    proptest! {
        #[test]
        fn whatever_is_ready_nothing_is_ever_drawn_twice_or_dropped_and_all_ready_gives_exactly_what_is_wanted(
            level in 1u8..=4,
            shown_seed in 1u64..u64::MAX,
            wanted_seed in 1u64..u64::MAX,
            shown_splits in 10u64..90,
            wanted_splits in 10u64..90,
            ready_seed in 0u64..1000,
            ready_share in 0u64..=100,
        ) {
            let shown: HashSet<BrickCube> = tiling(level, shown_seed, shown_splits).into_iter().collect();
            let wanted: HashSet<BrickCube> = tiling(level, wanted_seed, wanted_splits).into_iter().collect();
            let ready = |cube: BrickCube| {
                let origin = cube.origin();
                let mixed = (origin.x() * 31 + origin.y() * 17 + origin.z() * 7 + i128::from(cube.level()) * 101
                    + ready_seed as i128).unsigned_abs();
                mixed % 100 < u128::from(ready_share)
            };
            let swaps = plan_swaps(&shown, &wanted, ready);

            let mut after = shown.clone();
            for swap in &swaps {
                for cube in &swap.remove {
                    prop_assert!(after.remove(cube), "{cube:?} was not shown");
                }
                for cube in &swap.add {
                    prop_assert!(ready(*cube), "{cube:?} came in before it was ready");
                    prop_assert!(wanted.contains(cube));
                    prop_assert!(after.insert(*cube), "{cube:?} was shown already");
                }
            }
            // No two cubes overlap, and the same bricks are covered as before (both tile the same cube).
            prop_assert_eq!(bricks_of(&after), bricks_of(&shown));
            // Every cube is shown or wanted.
            for cube in &after {
                prop_assert!(shown.contains(cube) || wanted.contains(cube));
            }
            if ready_share == 100 {
                prop_assert_eq!(&after, &wanted);
            }
        }

        #[test]
        fn when_the_two_cover_different_places_what_both_cover_stays_covered_and_what_only_was_shown_goes(
            level in 1u8..=4,
            shown_seed in 1u64..u64::MAX,
            wanted_seed in 1u64..u64::MAX,
            drop_seed in 0u64..1000,
            ready_seed in 0u64..1000,
            ready_share in 0u64..=100,
        ) {
            let keep = |cube: &BrickCube, seed: u64, share: u64| {
                let origin = cube.origin();
                let mixed = (origin.x() * 13 + origin.y() * 29 + origin.z() * 3 + i128::from(cube.level()) * 7
                    + seed as i128).unsigned_abs();
                mixed % 100 < u128::from(share)
            };
            let shown: HashSet<BrickCube> = tiling(level, shown_seed, 50)
                .into_iter()
                .filter(|cube| keep(cube, drop_seed, 70))
                .collect();
            let wanted: HashSet<BrickCube> = tiling(level, wanted_seed, 50)
                .into_iter()
                .filter(|cube| keep(cube, drop_seed + 1, 70))
                .collect();
            let ready = |cube: BrickCube| keep(&cube, ready_seed, ready_share);
            let swaps = plan_swaps(&shown, &wanted, ready);

            let mut after = shown.clone();
            for swap in &swaps {
                for cube in &swap.remove {
                    prop_assert!(after.remove(cube));
                }
                for cube in &swap.add {
                    prop_assert!(ready(*cube));
                    prop_assert!(after.insert(*cube));
                }
            }
            let covered_after = bricks_of(&after);
            let covered_before = bricks_of(&shown);
            let covered_wanted = bricks_of(&wanted);
            for brick in covered_before.intersection(&covered_wanted) {
                prop_assert!(covered_after.contains(brick), "{brick:?} was covered and is to stay so");
            }
            for brick in &covered_after {
                prop_assert!(covered_before.contains(brick) || covered_wanted.contains(brick));
            }
            if ready_share == 100 {
                prop_assert_eq!(&after, &wanted);
            }
        }
    }
}
