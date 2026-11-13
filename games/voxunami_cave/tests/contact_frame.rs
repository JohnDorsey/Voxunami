//! The contact frame (the rest of step 4c-4 of `docs/decisions/joints-and-cars.md`): promise (a) of the joint plan with something near. **A mechanism does not work differently because
//! a ship is near**, or because of where it is or how fast it goes. These tests are about a chain of three cars (`common/chains.rs`) with a stranger (a ship of one stone) near it.
//!
//! Two kinds. The tests about *nearness* say that a stranger that is near and does not touch changes nothing of what the chain does, to the last digits that the chain alone and
//! the chain on a rail agree to. The tests about *contact* are the controls that any change to that must keep: a stranger that does come in does hit the chain, however fast, and the
//! chain is pushed by the share of the stranger's momentum that physics says and stays coupled.

mod common;

use common::chains::{
    Setting, a_chain, a_chain_in_orbit, a_swing, centre_of, how_the_chain_turns_after, mass_of,
    most_apart, origins_of, pivot_gap, set_going,
};
use common::docking::{PLAIN, Placed, make_all_going, stone};
use common::{Scene, circular_speed, length, pose_at, ship_id};

/// Ticks after the chain is set swinging at which two runs are compared: before its swing turns back (about the 130th), where a difference of the rounding of a few ticks before
/// is made large (`how_the_chain_turns_after`).
const TICKS: u32 = 100;

/// How alike the facings and turns of two chains have to be to be the same: what the chain alone at two distances, and alone and on a rail, agree to (`group_off_a_rail.rs`).
const THE_SAME: f64 = 1e-7;

const DISTANCE: f64 = 70_016.0;

#[test]
#[ignore = "the contact frame is not built (docs/decisions/joints-and-cars.md, step 4c-5)"]
fn a_stranger_that_is_near_and_does_not_touch_changes_nothing_of_what_a_chain_does() {
    let alone = how_the_chain_turns_after(DISTANCE, Setting::Alone, TICKS);
    let near = how_the_chain_turns_after(DISTANCE, Setting::WithAStranger, TICKS);
    let apart = most_apart(&alone, &near);
    assert!(
        apart < THE_SAME,
        "with a stranger a hundred voxels off the chain differs from the one alone by {apart}"
    );
    // (They did swing.)
    assert!(alone.iter().any(|(_, spin)| length(*spin) > 0.05));
}

#[test]
#[ignore = "the contact frame is not built (docs/decisions/joints-and-cars.md, step 4c-5)"]
fn with_a_stranger_near_what_a_chain_does_does_not_depend_on_where_it_is_or_how_fast_it_goes() {
    let near = how_the_chain_turns_after(70_016.0, Setting::WithAStranger, TICKS);
    let far = how_the_chain_turns_after(140_000.0, Setting::WithAStranger, TICKS);
    let apart = most_apart(&near, &far);
    assert!(
        apart < THE_SAME,
        "with a stranger near, the chain at 70 and at 140 kilometres differs by {apart}"
    );
}

/// A wall of stone, 13 by 13 voxels and one thick, its middle voxel at the origin of its ship: a stranger that a few metres of aim cannot make miss. (A ship at orbital speed in the
/// frame everything is in climbs by the gain of energy that its steps are known to have, `future-goals.md`, "Forces over solver steps": twenty metres in the seven seconds that a
/// stranger closing at 20 metres a second needs to cover a hundred and fifty voxels, which a single stone went over.)
fn a_wall() -> Vec<Placed> {
    (-6..=6)
        .flat_map(|y| (-6..=6).map(move |z| ([0, y, z], stone(), PLAIN)))
        .collect()
}

/// What a stranger (the wall) that closed on the east end of a chain at `closing` metres a second, from a hundred and fifty voxels off along the chain, did in the ticks asked for.
struct Hit {
    /// How much the speed of the chain's centre of mass along the chain changed.
    chain_change: f64,
    /// The same for the wall.
    wall_change: f64,
    /// The momentum that both gained along the chain, together (nothing but a pull a few hundredths of a metre a second a second across the planet's orbit acts along it).
    momentum_gained: f64,
    /// The momentum the chain gained, to say what a part of it is.
    momentum_of_the_chain: f64,
    /// The joints' worst gap, in voxels.
    worst_gap: f64,
    coupled: bool,
}

fn a_stranger_closing_on_the_chain(closing: f64, ticks: usize) -> Hit {
    let (mut scene, report) = Scene::reporting_in_a_new_world();
    let speed = circular_speed(&scene, 0, DISTANCE);
    let ships = a_chain_in_orbit(&mut scene, a_chain(), DISTANCE, speed);
    set_going(&mut scene, &ships, speed, a_swing());
    scene.ticks(5);
    let end = {
        let origin = origins_of(&scene, &ships)[2];
        [origin[0] + 5.0 + 150.0, origin[1], origin[2]]
    };
    make_all_going(
        &mut scene,
        vec![(pose_at(end[0], end[1], end[2]), a_wall())],
        [-closing, 0.0, speed],
    );
    scene.ticks(2);
    let wall = ship_id(4);
    let before = (centre_of(&scene, &ships).1, centre_of(&scene, &[wall]).1);
    let (chain_mass, wall_mass) = (mass_of(&scene, &ships), mass_of(&scene, &[wall]));
    let mut worst_gap: f64 = 0.0;
    for _ in 0..ticks {
        scene.ticks(1);
        worst_gap = worst_gap
            .max(pivot_gap(&scene, (ships[0], 7.0), ships[1]))
            .max(pivot_gap(&scene, (ships[1], 6.0), ships[2]));
    }
    let after = (centre_of(&scene, &ships).1, centre_of(&scene, &[wall]).1);
    let chain_change = after.0[0] - before.0[0];
    let wall_change = after.1[0] - before.1[0];
    Hit {
        chain_change,
        wall_change,
        momentum_gained: chain_mass * chain_change + wall_mass * wall_change,
        momentum_of_the_chain: chain_mass * chain_change,
        worst_gap,
        coupled: report().couplings == 2,
    }
}

#[test]
fn a_stranger_that_closes_on_the_chain_hits_it_whatever_the_speed_it_closes_at() {
    // Metres a second the stranger closes at, and the ticks to run: it covers a hundred and fifty voxels and hits within them, and the slowest takes longest.
    for (closing, ticks) in [(50.0, 100), (150.0, 50), (300.0, 40)] {
        let hit = a_stranger_closing_on_the_chain(closing, ticks);
        // It hit: the chain went the way the stranger went (along -x), and the stranger lost speed. (A part of the share a plastic hit gives it, which the joints and the turn of the
        // chain take their share of, and the exact amount is the chain's: the control that it is not changed is the conservation below.)
        assert!(
            hit.chain_change < -0.1 * closing && hit.wall_change > 0.0,
            "closing at {closing}: the chain's speed along it changed by {} and the wall's by {}",
            hit.chain_change,
            hit.wall_change
        );
        // Momentum was kept: what one gained the other lost, to a hundredth of what the chain gained.
        assert!(
            hit.momentum_gained.abs() < 0.01 * hit.momentum_of_the_chain.abs(),
            "closing at {closing}: together they gained {} of momentum and the chain {}",
            hit.momentum_gained,
            hit.momentum_of_the_chain
        );
        assert!(
            hit.coupled,
            "closing at {closing}: the chain is coupled still"
        );
        assert!(
            hit.worst_gap < 0.5,
            "closing at {closing}: the joints' worst gap was {} voxels",
            hit.worst_gap
        );
    }
}
