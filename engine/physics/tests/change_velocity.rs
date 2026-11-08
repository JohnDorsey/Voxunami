//! A change of a character's speed with nothing else changed: for a character whose speed is measured from something that has changed (a planet that carried it let it go, and
//! the speed it had against the planet is the speed it has in the world less the planet's).

mod common;

use common::{STILL, Scene, eye, run, spawn};

#[test]
fn a_change_of_speed_adds_to_the_speed_and_changes_nothing_else() {
    let mut scene = Scene::with_floor(9);
    let mut body = spawn(5.0, 60.0, 5.0);
    // Falling, a few steps.
    run(&mut body, &mut scene, STILL, 3);
    let (place, speed) = (eye(&body), body.velocity());
    body.change_velocity([3.0, -1.5, 2.0]);
    assert_eq!(eye(&body), place, "it is where it was");
    assert_eq!(
        body.velocity(),
        [speed[0] + 3.0, speed[1] - 1.5, speed[2] + 2.0]
    );
    assert!(!body.is_grounded() && !body.is_flying());
    // And it goes on from there: the next step takes it along x by the speed it was given.
    run(&mut body, &mut scene, STILL, 1);
    assert!(
        eye(&body)[0] > place[0] + 0.02,
        "{:?} then {:?}",
        place,
        eye(&body)
    );
}

#[test]
fn a_change_of_nothing_is_to_the_bit_no_change() {
    let mut scene = Scene::with_floor(9);
    let (mut changed, mut control) = (spawn(5.0, 60.0, 5.0), spawn(5.0, 60.0, 5.0));
    run(&mut changed, &mut scene, STILL, 5);
    run(&mut control, &mut scene, STILL, 5);
    changed.change_velocity([0.0; 3]);
    run(&mut changed, &mut scene, STILL, 5);
    run(&mut control, &mut scene, STILL, 5);
    assert_eq!(
        changed.velocity().map(f64::to_bits),
        control.velocity().map(f64::to_bits)
    );
    assert_eq!(
        eye(&changed).map(f64::to_bits),
        eye(&control).map(f64::to_bits)
    );
}
