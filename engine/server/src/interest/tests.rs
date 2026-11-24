use std::collections::BTreeSet;

use super::*;

fn coordinate(x: i128, y: i128, z: i128) -> BrickCoordinate {
    BrickCoordinate::new(x, y, z)
}

/// Every box of bricks whose corners are in the lattice of `edge` bricks along each axis, from 0.
fn boxes_in_a_lattice(edge: i128) -> Vec<BrickBox> {
    let mut boxes = Vec::new();
    let range = || 0..edge;
    for (x0, y0, z0) in
        range().flat_map(|x| range().flat_map(move |y| range().map(move |z| (x, y, z))))
    {
        for x1 in x0..edge {
            for y1 in y0..edge {
                for z1 in z0..edge {
                    boxes.push(
                        BrickBox::new(coordinate(x0, y0, z0), coordinate(x1, y1, z1)).unwrap(),
                    );
                }
            }
        }
    }
    boxes
}

fn bricks_of(boxes: &[BrickBox]) -> BTreeSet<BrickCoordinate> {
    boxes.iter().flat_map(|region| region.bricks()).collect()
}

#[test]
fn the_bricks_newly_covered_are_those_of_the_regions_now_that_were_not_before_each_in_one_piece() {
    // Every pair of regions now against every pair before, in a lattice of 2 by 2 by 2 bricks: 27 boxes, a little over half a million cases.
    let boxes = boxes_in_a_lattice(2);
    assert_eq!(boxes.len(), 27);
    let lists: Vec<Vec<BrickBox>> = [vec![]]
        .into_iter()
        .chain(boxes.iter().map(|&one| vec![one]))
        .chain(
            boxes
                .iter()
                .flat_map(|&one| boxes.iter().map(move |&other| vec![one, other])),
        )
        .collect();
    for now in &lists {
        for before in &lists {
            let pieces = newly_covered(now, before);
            let expected: BTreeSet<BrickCoordinate> = bricks_of(now)
                .difference(&bricks_of(before))
                .copied()
                .collect();
            let given: usize = pieces.iter().map(|piece| piece.bricks().count()).sum();
            assert_eq!(
                bricks_of(&pieces),
                expected,
                "now {now:?}, before {before:?}"
            );
            assert_eq!(
                given,
                expected.len(),
                "a brick is in two pieces: now {now:?}, before {before:?}"
            );
        }
    }
}

#[test]
fn a_region_too_big_to_count_is_cut_into_pieces_without_visiting_its_bricks() {
    // (A box of this size could not be counted brick by brick.)
    let huge = BrickBox::new(
        coordinate(0, 0, 0),
        coordinate(1 << 100, 1 << 100, 1 << 100),
    )
    .unwrap();
    let small = BrickBox::around(coordinate(5, 5, 5), 1);
    let pieces = newly_covered(&[huge, small], &[small]);
    assert_eq!(pieces.len(), 6, "{pieces:?}");
    assert!(
        pieces
            .iter()
            .all(|piece| piece.intersection(small).is_none())
    );
}

#[test]
fn a_watch_covers_the_bricks_of_its_level_that_are_in_any_of_its_regions() {
    let main = LevelIndex::MAIN;
    let other = LevelIndex::new(1);
    let watch = Watch {
        level: main,
        region: BrickBox::around(coordinate(0, 0, 0), 1),
        free: vec![
            BrickBox::around(coordinate(100, 0, 0), 1),
            BrickBox::around(coordinate(0, 100, 0), 0),
        ],
    };
    assert_eq!(
        watch.body(),
        (main, BrickBox::around(coordinate(0, 0, 0), 1))
    );
    for covered in [
        coordinate(1, 1, 1),
        coordinate(99, -1, 1),
        coordinate(0, 100, 0),
    ] {
        assert!(watch.covers(main, covered), "{covered}");
        assert!(!watch.covers(other, covered), "{covered} in another level");
    }
    for uncovered in [
        coordinate(2, 0, 0),
        coordinate(0, 101, 0),
        coordinate(50, 0, 0),
    ] {
        assert!(!watch.covers(main, uncovered), "{uncovered}");
    }
    assert_eq!(watch.regions().len(), 3);
    assert_eq!(watch.regions()[0], watch.region);
}
