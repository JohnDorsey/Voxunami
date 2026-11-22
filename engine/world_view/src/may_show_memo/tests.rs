use super::*;
use std::cell::RefCell;
use voxel_space::BrickCoordinate;

fn cube(x: i128) -> BrickCube {
    BrickCube::new(BrickCoordinate::new(x, 0, 0), 0).unwrap()
}

/// Asks the memo about the cube at `x`, noting in `asked` that the terrain was asked, and giving `answer` if it was.
fn ask(memo: &mut MayShowMemo, asked: &RefCell<Vec<i128>>, x: i128, answer: bool) -> bool {
    memo.get_or_ask(cube(x), || {
        asked.borrow_mut().push(x);
        answer
    })
}

/// A round in which the cubes at `from..to` were asked about (each answered `true` if the terrain was asked), ended.
fn round(memo: &mut MayShowMemo, asked: &RefCell<Vec<i128>>, from: i128, to: i128) {
    for x in from..to {
        ask(memo, asked, x, true);
    }
    memo.finish_round();
}

#[test]
fn an_answer_is_asked_for_once_and_kept_for_the_rounds_that_follow() {
    let mut memo = MayShowMemo::default();
    let asked = RefCell::new(Vec::new());
    assert!(ask(&mut memo, &asked, 1, true));
    assert!(!ask(&mut memo, &asked, 2, false));
    // Again in the same round it is the answer it had, whatever the terrain would say now.
    assert!(ask(&mut memo, &asked, 1, false));
    assert!(!ask(&mut memo, &asked, 2, true));
    assert_eq!(*asked.borrow(), [1, 2]);

    // In the rounds after, nothing is asked: a cube that was not asked about for a long time is still kept (the memory is not full), both answers.
    for _ in 0..50 {
        memo.finish_round();
    }
    assert!(ask(&mut memo, &asked, 1, false));
    assert!(!ask(&mut memo, &asked, 2, true));
    assert_eq!(*asked.borrow(), [1, 2]);
}

#[test]
fn answers_are_kept_for_one_version_of_the_terrain_and_forgotten_when_it_is_another() {
    let mut memo = MayShowMemo::default();
    let asked = RefCell::new(Vec::new());
    memo.keep_for(0);
    assert!(ask(&mut memo, &asked, 1, true));
    memo.finish_round();
    memo.keep_for(0);
    assert!(ask(&mut memo, &asked, 1, false), "the same version: kept");
    assert_eq!(*asked.borrow(), [1]);
    memo.keep_for(1);
    assert!(
        !ask(&mut memo, &asked, 1, false),
        "another version: asked again"
    );
    assert_eq!(*asked.borrow(), [1, 1]);
    memo.keep_for(1);
    assert!(!ask(&mut memo, &asked, 1, true), "and kept for that");
    assert_eq!(*asked.borrow(), [1, 1]);
    memo.clear();
    assert!(
        ask(&mut memo, &asked, 1, true),
        "a clear forgets everything"
    );
}

#[test]
fn over_its_memory_the_answers_not_used_for_longest_are_forgotten_first_and_what_was_used_in_the_last_round_is_not()
 {
    let mut memo = MayShowMemo::with_capacity(20);
    let asked = RefCell::new(Vec::new());
    // Four rounds of ten cubes each: after the third there are 30 kept, over the memory of 20, and the oldest round goes.
    round(&mut memo, &asked, 0, 10);
    round(&mut memo, &asked, 10, 20);
    assert_eq!(memo.len(), 20, "at the memory, not over it");
    round(&mut memo, &asked, 20, 30);
    assert!(
        memo.len() <= 20,
        "over the memory, so some are forgotten ({} kept)",
        memo.len()
    );
    asked.borrow_mut().clear();
    for x in 20..30 {
        ask(&mut memo, &asked, x, false);
    }
    assert_eq!(
        *asked.borrow(),
        Vec::<i128>::new(),
        "the answers of the last round are all kept"
    );
    for x in 0..10 {
        ask(&mut memo, &asked, x, false);
    }
    assert_eq!(
        *asked.borrow(),
        (0..10).collect::<Vec<i128>>(),
        "the oldest round is asked again"
    );
}

#[test]
fn an_answer_that_is_used_again_is_as_new_as_the_round_it_was_used_in() {
    let mut memo = MayShowMemo::with_capacity(20);
    let asked = RefCell::new(Vec::new());
    round(&mut memo, &asked, 0, 10);
    round(&mut memo, &asked, 10, 20);
    // The first ten are used again (a viewpoint that was left is come back to) before ten more come in: the ten in the middle are the oldest now.
    round(&mut memo, &asked, 0, 10);
    round(&mut memo, &asked, 20, 30);
    asked.borrow_mut().clear();
    for x in (0..10).chain(20..30) {
        ask(&mut memo, &asked, x, false);
    }
    assert_eq!(*asked.borrow(), Vec::<i128>::new(), "kept: used lately");
    for x in 10..20 {
        ask(&mut memo, &asked, x, false);
    }
    assert_eq!(
        *asked.borrow(),
        (10..20).collect::<Vec<i128>>(),
        "forgotten: not used for the longest"
    );
}

#[test]
fn when_it_forgets_it_forgets_enough_for_several_rounds_and_not_just_one_answer_over_the_memory() {
    // Forgetting walks all the answers, so it is not done at every round: it goes under the memory, not to the edge of it.
    let mut memo = MayShowMemo::with_capacity(100);
    let asked = RefCell::new(Vec::new());
    for round_number in 0..3 {
        round(&mut memo, &asked, round_number * 30, round_number * 30 + 30);
    }
    assert_eq!(memo.len(), 90, "under the memory of 100: nothing forgotten");
    round(&mut memo, &asked, 90, 120);
    assert!(
        memo.len() < 100,
        "120 are over the memory of 100, and what is kept is under it ({} kept)",
        memo.len()
    );
}

#[test]
fn a_round_bigger_than_the_memory_is_kept_whole_so_that_the_view_just_worked_out_is_never_asked_again()
 {
    let mut memo = MayShowMemo::with_capacity(10);
    let asked = RefCell::new(Vec::new());
    round(&mut memo, &asked, 0, 5);
    round(&mut memo, &asked, 100, 140);
    asked.borrow_mut().clear();
    for x in 100..140 {
        ask(&mut memo, &asked, x, false);
    }
    assert_eq!(
        *asked.borrow(),
        Vec::<i128>::new(),
        "all 40 of the last round are kept, over the memory of 10"
    );
}

#[test]
fn the_memory_stays_bounded_over_a_long_flight_that_asks_about_new_cubes_every_round() {
    let mut memo = MayShowMemo::with_capacity(100);
    let asked = RefCell::new(Vec::new());
    let mut most = 0;
    for step in 0..500 {
        round(&mut memo, &asked, step * 7, step * 7 + 30);
        most = most.max(memo.len());
    }
    assert!(
        most <= 100 + 30,
        "never more than the memory and a round: {most}"
    );
    assert!(
        memo.len() >= 50,
        "and the memory is used, not emptied: {}",
        memo.len()
    );
}

#[test]
fn the_default_memory_holds_two_cuts_of_a_view_far_from_the_ground() {
    // About 33,000 cubes are asked about in a view from 264 km up (measured), and a cut of a hundred thousand in one to 100 million voxels: two of them, the ship's and a
    // map's, are kept (what is spent on it is its size, in 64 bytes or so each).
    const {
        assert!(MAY_SHOW_MEMORY >= 2 * 100_000);
        assert!(MAY_SHOW_MEMORY <= 1 << 20);
    }
}
