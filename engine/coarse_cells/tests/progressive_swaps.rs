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
    let between = intermediate_cubes(&groups, |_| false);
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
    assert!(intermediate_cubes(&groups, |_| false).is_empty());
}

#[test]
fn the_node_is_replaced_by_its_halves_when_they_are_ready_though_the_cubes_that_come_are_not() {
    let (big, wanted) = a_far_node_and_what_replaces_it();
    let shown = set(&[big]);
    let between: HashSet<BrickCube> = intermediate_cubes(&refinements(&shown, &wanted), |_| false)
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
    let between: HashSet<BrickCube> =
        intermediate_cubes(&refinements(&set(&[big]), &wanted), |_| false)
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

fn halves_of(parent: BrickCube) -> Vec<BrickCube> {
    if parent.level() == 0 {
        Vec::new()
    } else {
        halves(parent)
    }
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
        let between: HashSet<BrickCube> = intermediate_cubes(&refinements(&shown, &wanted), |_| false).into_iter().collect();
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
        let between: HashSet<BrickCube> = intermediate_cubes(&refinements(&set(&[big]), &wanted), |_| false).into_iter().collect();
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

/// The nodes of the far node's replacement are ready, held pictures among them, and the bricks of the corner are not: the shape of a view back from far away over the
/// ground, whose fine ball round the viewer is made by the mesher and is not ready yet (`docs/specs/viewpoints-spec.md` 9d).
fn the_bricks_are_not_ready(
    wanted: &HashSet<BrickCube>,
    between: &HashSet<BrickCube>,
) -> impl Fn(BrickCube) -> bool {
    let (wanted, between) = (wanted.clone(), between.clone());
    move |cube: BrickCube| (wanted.contains(&cube) && cube.level() > 0) || between.contains(&cube)
}

#[test]
fn only_what_holds_a_cube_that_is_not_ready_has_to_be_made_between() {
    // A node of level 4 over sixty-four nodes of level 2, except that one of them is eight nodes of level 1 and one of those is eight bricks.
    let big = cube(0, 0, 0, 4);
    let corner_of_two = cube(0, 0, 0, 2);
    let corner_of_one = cube(0, 0, 0, 1);
    let mut wanted: HashSet<BrickCube> = HashSet::new();
    for half in halves(big) {
        for quarter in halves(half) {
            if quarter != corner_of_two {
                wanted.insert(quarter);
            }
        }
    }
    for node in halves(corner_of_two) {
        if node != corner_of_one {
            wanted.insert(node);
        }
    }
    wanted.extend(halves(corner_of_one));
    let groups = refinements(&set(&[big]), &wanted);
    let everything = intermediate_cubes(&groups, |_| false);
    let bricks_missing = intermediate_cubes(&groups, |cube| cube.level() > 0);
    // What holds the bricks and what a node of level 2 lies in: the eight halves, the node of level 2 at the corner, the node of level 1 at the corner.
    assert_eq!(everything.len(), 10, "{everything:?}");
    // What holds the bricks only: the node of level 1, of level 2 and of level 3, all at the corner.
    assert_eq!(
        set(&bricks_missing),
        set(&[corner_of_one, corner_of_two, cube(0, 0, 0, 3)])
    );
    assert!(set(&bricks_missing).is_subset(&set(&everything)));
    // Everything ready: nothing is to be made between.
    assert!(intermediate_cubes(&groups, |_| true).is_empty());
}

#[test]
fn what_is_ready_is_shown_at_once_and_only_what_holds_a_cube_that_is_not_ready_is_covered_by_the_cubes_between()
 {
    let (big, wanted) = a_far_node_and_what_replaces_it();
    let shown = set(&[big]);
    let between: HashSet<BrickCube> =
        intermediate_cubes(&refinements(&shown, &wanted), |cube| cube.level() > 0)
            .into_iter()
            .collect();
    let ready = the_bricks_are_not_ready(&wanted, &between);

    // The first step: every node of the wanted cubes that is ready and has no brick under it comes in as it is, and the half that has the bricks comes as the cube
    // between (its own picture, made for the purpose).
    let corner = cube(0, 0, 0, 3);
    let swaps = plan_progressive_swaps(&shown, &wanted, &ready);
    assert_eq!(swaps.len(), 1, "{swaps:?}");
    assert_eq!(swaps[0].remove, [big]);
    let mut expected: HashSet<BrickCube> = wanted
        .iter()
        .copied()
        .filter(|cube| !holds(corner, *cube))
        .collect();
    expected.insert(corner);
    assert_eq!(set(&swaps[0].add), expected);
    let mut now = shown.clone();
    apply(&mut now, &swaps);
    bricks_of(&now);

    // Then the corner goes the same way, a level at a time, down to the nodes of level 1 that hold the bricks.
    let mut rounds = 1;
    loop {
        let swaps = plan_progressive_swaps(&now, &wanted, &ready);
        if swaps.is_empty() {
            break;
        }
        apply(&mut now, &swaps);
        bricks_of(&now);
        rounds += 1;
        assert!(rounds <= 3, "{rounds} rounds");
    }
    assert_eq!(rounds, 3, "levels 3, 2 and then the nodes of level 1");
    let parents_of_bricks: HashSet<BrickCube> = wanted
        .iter()
        .filter(|cube| cube.level() == 0)
        .map(|brick| BrickCube::containing(brick.origin(), 1))
        .collect();
    let mut at_rest: HashSet<BrickCube> = wanted
        .iter()
        .copied()
        .filter(|cube| cube.level() > 0)
        .collect();
    at_rest.extend(parents_of_bricks);
    assert_eq!(
        now, at_rest,
        "everything that is ready, and what covers the bricks"
    );

    // The bricks come: they replace the cubes that cover them.
    let swaps = plan_progressive_swaps(&now, &wanted, |_| true);
    apply(&mut now, &swaps);
    assert_eq!(now, wanted);
}

#[test]
fn a_half_whose_cubes_are_all_ready_comes_in_as_those_cubes_though_it_is_not_ready_itself_and_the_half_with_a_brick_comes_in_as_itself()
 {
    // A node of level 3 over six nodes of level 2 and two halves that are cut finer: one is eight nodes of level 1, all ready, and one is the same with a brick in
    // one of the nodes (the brick is not ready).
    let big = cube(0, 0, 0, 3);
    let list = halves(big);
    let (all_nodes, with_a_brick) = (list[0], list[1]);
    let mut wanted: HashSet<BrickCube> = halves(big).into_iter().skip(2).collect();
    wanted.extend(halves(all_nodes));
    let mut nodes_with_a_brick = halves(with_a_brick);
    let node_of_bricks = nodes_with_a_brick.remove(0);
    wanted.extend(nodes_with_a_brick);
    wanted.extend(halves(node_of_bricks));
    let shown = set(&[big]);
    // Ready: every cube that is wanted but a brick, and the half that holds the brick (made between), and not the half that holds nodes only.
    let ready =
        |cube: BrickCube| (wanted.contains(&cube) && cube.level() > 0) || cube == with_a_brick;
    let swaps = plan_progressive_swaps(&shown, &wanted, ready);
    assert_eq!(swaps.len(), 1, "{swaps:?}");
    assert_eq!(swaps[0].remove, [big]);
    let mut expected: HashSet<BrickCube> = halves(big).into_iter().skip(2).collect();
    expected.extend(halves(all_nodes));
    expected.insert(with_a_brick);
    assert_eq!(set(&swaps[0].add), expected);
}

#[test]
fn a_cut_with_nothing_ready_but_the_halves_is_replaced_as_before() {
    // (The case the cold build is in: nothing under the halves is ready, so nothing comes in but the halves.)
    let (big, wanted) = a_far_node_and_what_replaces_it();
    let shown = set(&[big]);
    let swaps = plan_progressive_swaps(&shown, &wanted, |cube| cube.level() == 3);
    assert_eq!(swaps.len(), 1);
    assert_eq!(set(&swaps[0].add), set(&halves(big)));
}

proptest! {
    // (Each case counts every brick of a cube of up to 32,768 after each round, so fewer cases than the default.)
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn whatever_is_ready_and_whatever_has_to_be_made_between_is_made_a_view_comes_to_rest_with_only_what_waits_for_a_cube_that_is_not_ready(
        level in 3u8..=5,
        wanted_seed in 1u64..u64::MAX,
        wanted_splits in 60u64..95,
        ready_seed in 0u64..1000,
        ready_share in 0u64..=100,
    ) {
        let big = cube(0, 0, 0, level);
        let wanted: HashSet<BrickCube> = tiling(level, wanted_seed, wanted_splits).into_iter().collect();
        prop_assume!(!wanted.contains(&big));
        let wanted_ready = |cube: BrickCube| {
            let origin = cube.origin();
            let mixed = (origin.x() * 31 + origin.y() * 17 + origin.z() * 7 + i128::from(cube.level()) * 101 + ready_seed as i128).unsigned_abs();
            mixed % 100 < u128::from(ready_share)
        };
        let groups = refinements(&set(&[big]), &wanted);
        // What has to be made between is what holds a cube that comes and is not ready, and it is ready (it is made).
        let between: HashSet<BrickCube> = intermediate_cubes(&groups, wanted_ready).into_iter().collect();
        let everything: HashSet<BrickCube> = intermediate_cubes(&groups, |_| false).into_iter().collect();
        prop_assert!(between.is_subset(&everything));
        for cube in &between {
            prop_assert!(
                wanted.iter().any(|coming| holds(*cube, *coming) && !wanted_ready(*coming)),
                "{cube:?} is made between and holds nothing that is not ready"
            );
        }
        let ready = |cube: BrickCube| {
            if wanted.contains(&cube) { wanted_ready(cube) } else { between.contains(&cube) }
        };
        let mut shown = set(&[big]);
        let mut rounds = 0;
        loop {
            let swaps = plan_progressive_swaps(&shown, &wanted, ready);
            if swaps.is_empty() {
                break;
            }
            for swap in &swaps {
                for cube in &swap.add {
                    prop_assert!(ready(*cube), "{cube:?} came in and is not ready");
                    prop_assert!(wanted.contains(cube) || between.contains(cube), "{cube:?} is neither wanted nor between");
                }
            }
            apply(&mut shown, &swaps);
            prop_assert_eq!(bricks_of(&shown).len(), 1usize << (3 * u32::from(level)));
            rounds += 1;
            prop_assert!(rounds <= usize::from(level), "does not come to rest in {rounds} rounds");
        }
        // At rest, a cube that is shown and is not wanted holds a cube that is not ready, and a wanted cube that is ready and is under nothing that waits is shown.
        for cube in shown.iter().filter(|cube| !wanted.contains(cube)) {
            prop_assert!(
                wanted.iter().any(|coming| holds(*cube, *coming) && !wanted_ready(*coming)),
                "{cube:?} is shown and all it is replaced by is ready"
            );
            // It is waiting for a reason: a half of it is a cube that is wanted and is not ready, or what comes in it is only its halves (and the plain planner waits for
            // all of them), or it is of level 1 (over bricks).
            let coming: Vec<&BrickCube> = wanted.iter().filter(|coming| holds(*cube, **coming)).collect();
            let only_halves = coming.iter().all(|coming| coming.level() + 1 >= cube.level());
            let a_half_is_not_ready = halves_of(*cube).iter().any(|half| wanted.contains(half) && !wanted_ready(*half));
            prop_assert!(
                cube.level() < 2 || only_halves || a_half_is_not_ready,
                "{cube:?} is shown and could be replaced by what is ready of what comes and its covers"
            );
        }
        for coming in wanted.iter().filter(|coming| wanted_ready(**coming)) {
            let waits = shown.iter().any(|cube| cube != coming && holds(*cube, *coming));
            if waits {
                // It waits under a cube that is shown, which is between and waits for a cube that is not ready that is not under another half that is ready: the cube that is
                // shown holds something not ready.
                let holder = shown.iter().find(|cube| holds(**cube, *coming)).unwrap();
                prop_assert!(wanted.iter().any(|other| holds(*holder, *other) && !wanted_ready(*other)));
            } else {
                prop_assert!(shown.contains(coming), "{coming:?} is ready and is not shown");
            }
        }
        if ready_share == 100 {
            prop_assert_eq!(&shown, &wanted);
        }
    }
}
