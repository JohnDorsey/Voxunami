//! A node that goes is replaced one level at a time when the cubes that replace it are far finer, so that what is on the screen is never more than a level
//! coarser than what has been built (`docs/specs/viewpoints-spec.md` 6.2, item 11; found in phase 0: from 264 km up back to the surface the old giant nodes stayed on the
//! screen for the whole of the build, because a group changes only when all of it is ready).

use std::collections::HashSet;

use coarse_cells::{Swap, intermediate_cubes, plan_progressive_swaps, plan_swaps, refinements};
use proptest::prelude::*;
use region_octree::BrickCube;
use voxel_space::BrickCoordinate;

fn cube(x: i128, y: i128, z: i128, level: u8) -> BrickCube {
    BrickCube::new(BrickCoordinate::new(x, y, z), level).expect("aligned")
}

/// Whether `outer` is `inner` or holds it.
fn holds(outer: BrickCube, inner: BrickCube) -> bool {
    inner.level() <= outer.level() && BrickCube::containing(inner.origin(), outer.level()) == outer
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

/// Every brick of a cube, for the bricks to be counted: two cubes that overlap have a brick in common.
fn bricks_of(cubes: &HashSet<BrickCube>) -> HashSet<BrickCoordinate> {
    let mut bricks = HashSet::new();
    for cube in cubes {
        let edge = cube.edge_length() as i128;
        let origin = cube.origin();
        for x in 0..edge {
            for y in 0..edge {
                for z in 0..edge {
                    let brick =
                        BrickCoordinate::new(origin.x() + x, origin.y() + y, origin.z() + z);
                    assert!(bricks.insert(brick), "{brick:?} is in two cubes");
                }
            }
        }
    }
    bricks
}

fn apply(shown: &mut HashSet<BrickCube>, swaps: &[Swap]) {
    for swap in swaps {
        for cube in &swap.remove {
            assert!(shown.remove(cube), "{cube:?} was not shown");
        }
        for cube in &swap.add {
            assert!(shown.insert(*cube), "{cube:?} was shown already");
        }
    }
}

/// A node of level 4 that goes, and the bricks (level 0) of one corner and the nodes of level 3 of the rest that come: the shape of a view that has jumped from far
/// away to the ground.
fn a_far_node_and_what_replaces_it() -> (BrickCube, HashSet<BrickCube>) {
    let big = cube(0, 0, 0, 4);
    let mut wanted: HashSet<BrickCube> = HashSet::new();
    // Inside the node of level 2 at the corner: bricks in five of its eight nodes of level 1, and three of them nodes.
    for (index, node) in halves(cube(0, 0, 0, 2)).into_iter().enumerate() {
        if (1..4).contains(&index) {
            wanted.insert(node);
        } else {
            wanted.extend(halves(node));
        }
    }
    for octant in halves(cube(0, 0, 0, 3)).into_iter().skip(1) {
        wanted.insert(octant);
    }
    for octant in halves(big).into_iter().skip(1) {
        wanted.insert(octant);
    }
    (big, wanted)
}

#[test]
fn a_node_over_much_finer_cubes_has_the_cubes_between_found_and_nothing_is_found_when_the_gap_is_one_level()
 {
    let (big, wanted) = a_far_node_and_what_replaces_it();
    // (The finest of them are bricks inside a node of level 4: three levels down.)
    let shown = set(&[big]);
    // Nothing in `wanted` holds `big`, so it is a refinement group of its own, with every wanted cube inside it.
    let groups = refinements(&shown, &wanted);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].going, big);
    assert_eq!(
        set(&groups[0].coming),
        wanted
            .iter()
            .copied()
            .filter(|cube| holds(big, *cube))
            .collect()
    );
    let between = intermediate_cubes(&groups);
    assert!(!between.is_empty());
    for cube in &between {
        assert!(
            !wanted.contains(cube),
            "{cube:?} is wanted: it is not between"
        );
        assert!(holds(big, *cube) && cube.level() < big.level());
        assert!(
            groups[0].coming.iter().any(|coming| holds(*cube, *coming)),
            "{cube:?} holds none of what comes"
        );
    }

    // Eight halves in place of the node: one level, nothing between.
    let shown = set(&[big]);
    let wanted = set(&halves(big));
    let groups = refinements(&shown, &wanted);
    assert!(intermediate_cubes(&groups).is_empty());
}

#[test]
fn the_node_is_replaced_by_its_halves_when_they_are_ready_though_the_cubes_that_come_are_not() {
    let (big, wanted) = a_far_node_and_what_replaces_it();
    let shown = set(&[big]);
    let between: HashSet<BrickCube> = intermediate_cubes(&refinements(&shown, &wanted))
        .into_iter()
        .collect();
    let only_the_halves = |cube: BrickCube| cube.level() == 3;

    // Nothing is ready: the node stays (as before).
    assert!(plan_progressive_swaps(&shown, &wanted, |_| false).is_empty());
    // The wanted cubes are not ready, and the halves of the node are: it is replaced by the halves that hold something that comes.
    let swaps = plan_progressive_swaps(&shown, &wanted, only_the_halves);
    assert_eq!(swaps.len(), 1, "{swaps:?}");
    assert_eq!(swaps[0].remove, [big]);
    assert!(swaps[0].add.iter().all(|cube| cube.level() == 3));
    assert!(
        swaps[0]
            .add
            .iter()
            .all(|cube| wanted.contains(cube) || between.contains(cube))
    );
    // And not when one of those halves is not ready: that would leave a hole.
    let one_missing = swaps[0].add[0];
    assert!(
        plan_progressive_swaps(&shown, &wanted, |cube| cube.level() == 3
            && cube != one_missing)
        .is_empty()
    );
}

#[test]
fn it_goes_one_level_each_time_and_ends_at_what_is_wanted() {
    let (big, wanted) = a_far_node_and_what_replaces_it();
    let between: HashSet<BrickCube> = intermediate_cubes(&refinements(&set(&[big]), &wanted))
        .into_iter()
        .collect();
    // Every cube that is between is ready, and of what is wanted only those of the levels from 2 up: the bricks and the nodes of level 1 are still to come.
    let ready =
        |cube: BrickCube| between.contains(&cube) || (wanted.contains(&cube) && cube.level() >= 2);
    let mut shown = set(&[big]);
    let mut highest_levels = vec![big.level()];
    for _ in 0..8 {
        let swaps = plan_progressive_swaps(&shown, &wanted, ready);
        if swaps.is_empty() {
            break;
        }
        apply(&mut shown, &swaps);
        bricks_of(&shown);
        highest_levels.push(shown.iter().map(|cube| cube.level()).max().unwrap());
    }
    // It did not stop at the node nor jump over the levels: the coarsest on the screen came down, and what is left is what has to wait.
    assert!(highest_levels.len() >= 2, "{highest_levels:?}");
    assert!(highest_levels.windows(2).all(|pair| pair[1] <= pair[0]));
    assert!(
        shown
            .iter()
            .all(|cube| cube.level() >= 2 || wanted.contains(cube) || between.contains(cube))
    );
    assert!(!shown.contains(&big));
    // Now the rest is ready.
    for _ in 0..8 {
        let swaps = plan_progressive_swaps(&shown, &wanted, |_| true);
        if swaps.is_empty() {
            break;
        }
        apply(&mut shown, &swaps);
    }
    assert_eq!(shown, wanted);
}

#[test]
fn a_swap_that_is_ready_is_as_it_was() {
    let (big, wanted) = a_far_node_and_what_replaces_it();
    let shown = set(&[big]);
    assert_eq!(
        plan_progressive_swaps(&shown, &wanted, |_| true),
        plan_swaps(&shown, &wanted, |_| true)
    );
}

/// A tiling of the cube of `level` at the origin by random splitting.
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

proptest! {
    #[test]
    fn whatever_is_ready_nothing_is_drawn_twice_or_dropped_and_only_wanted_or_between_cubes_come_in(
        level in 2u8..=5,
        shown_seed in 1u64..u64::MAX,
        wanted_seed in 1u64..u64::MAX,
        shown_splits in 0u64..40,
        wanted_splits in 40u64..95,
        ready_seed in 0u64..1000,
        ready_share in 0u64..=100,
    ) {
        let shown: HashSet<BrickCube> = tiling(level, shown_seed, shown_splits).into_iter().collect();
        let wanted: HashSet<BrickCube> = tiling(level, wanted_seed, wanted_splits).into_iter().collect();
        let between: HashSet<BrickCube> = intermediate_cubes(&refinements(&shown, &wanted)).into_iter().collect();
        let ready = |cube: BrickCube| {
            let origin = cube.origin();
            let mixed = (origin.x() * 31 + origin.y() * 17 + origin.z() * 7 + i128::from(cube.level()) * 101 + ready_seed as i128).unsigned_abs();
            mixed % 100 < u128::from(ready_share)
        };
        let swaps = plan_progressive_swaps(&shown, &wanted, ready);
        let mut after = shown.clone();
        for swap in &swaps {
            for cube in &swap.remove {
                prop_assert!(after.remove(cube), "{cube:?} was not shown");
            }
            for cube in &swap.add {
                prop_assert!(ready(*cube), "{cube:?} came in before it was ready");
                prop_assert!(wanted.contains(cube) || between.contains(cube), "{cube:?} is neither wanted nor between");
                prop_assert!(after.insert(*cube), "{cube:?} was shown already");
            }
        }
        // No two cubes overlap and the same bricks are covered (both tile the same cube).
        prop_assert_eq!(bricks_of(&after), bricks_of(&shown));
        if ready_share == 100 {
            prop_assert_eq!(&after, &wanted);
        }
        // Where the plain planner would have changed something, this changes it too (it only adds the steps between).
        let plain = plan_swaps(&shown, &wanted, ready);
        for swap in &plain {
            for cube in &swap.add {
                prop_assert!(after.contains(cube), "{cube:?} came in by the plain planner and not by this one");
            }
        }
    }

    #[test]
    fn when_the_cubes_between_are_ready_a_view_gets_to_what_is_wanted_one_level_at_a_time(
        level in 3u8..=5,
        wanted_seed in 1u64..u64::MAX,
        wanted_splits in 60u64..95,
        threshold in 0u8..=5,
    ) {
        let big = cube(0, 0, 0, level);
        let wanted: HashSet<BrickCube> = tiling(level, wanted_seed, wanted_splits).into_iter().collect();
        prop_assume!(!wanted.contains(&big));
        let between: HashSet<BrickCube> = intermediate_cubes(&refinements(&set(&[big]), &wanted)).into_iter().collect();
        // Everything between is ready, and the wanted cubes of a level from the threshold up.
        let ready = |cube: BrickCube| between.contains(&cube) || (wanted.contains(&cube) && cube.level() >= threshold);
        let mut shown = set(&[big]);
        let mut rounds = 0;
        loop {
            let swaps = plan_progressive_swaps(&shown, &wanted, ready);
            if swaps.is_empty() {
                break;
            }
            apply(&mut shown, &swaps);
            prop_assert_eq!(bricks_of(&shown).len(), 1usize << (3 * u32::from(level)));
            rounds += 1;
            prop_assert!(rounds <= usize::from(level) + 1, "does not come to rest in {rounds} rounds");
        }
        // At rest, what is shown is wanted or is waiting for the cubes of a finer level than the threshold, and none of it is coarser than it need be: no cube that is
        // shown and not wanted holds a cube that is wanted, ready, and one level or more below... it holds something that is not ready.
        for cube in shown.iter().filter(|cube| !wanted.contains(cube)) {
            let coming: Vec<&BrickCube> = wanted.iter().filter(|wanted| holds(*cube, **wanted)).collect();
            prop_assert!(!coming.is_empty());
            prop_assert!(coming.iter().any(|wanted| !ready(**wanted)), "{cube:?} is shown, and all it is replaced by is ready");
        }
    }
}

#[test]
fn even_a_node_of_level_two_over_bricks_is_replaced_by_its_halves_when_they_are_ready() {
    let big = cube(0, 0, 0, 2);
    let nodes = halves(big);
    // Bricks in the first node of level 1, and the other seven are nodes.
    let mut wanted: HashSet<BrickCube> = halves(nodes[0]).into_iter().collect();
    wanted.extend(nodes.iter().skip(1).copied());
    let shown = set(&[big]);
    let swaps = plan_progressive_swaps(&shown, &wanted, |cube| cube.level() == 1);
    assert_eq!(swaps.len(), 1, "{swaps:?}");
    assert_eq!(swaps[0].remove, [big]);
    assert_eq!(set(&swaps[0].add), set(&nodes));
}
