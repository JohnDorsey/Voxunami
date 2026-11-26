//! The rules of joints between ships, apart from any ship: when two joint ends pull on each other, when they are coupled, and the pull (`docs/decisions/joints-and-cars.md`,
//! "Coupling, lock, unlock"). The numbers the owner chose are in the first test; the pull is the part that is mine, and it is checked by working a pair of bodies out under it.

use grid_bodies::docking::{Approach, BlockFaceView};
use grid_bodies::joints::{
    Holding, Inertia, JointSettings, LineAxes, effective_inertia, effective_mass,
    inertia_in_the_world, inverse_inertia_along, mass_at_an_end,
};
use grid_bodies::{BlockFace, JointKind};
use voxel_space::{FaceDirection, LatticeRotation, VoxelCoordinate};

type Vec3 = [f64; 3];

/// The tensor of a body that turns as easily every way: `inertia` on the diagonal.
fn isotropic(inertia: f64) -> Inertia {
    diagonal([inertia; 3])
}

/// The tensor of a body with these on the diagonal (axes of its own that are the world's).
fn diagonal(about: Vec3) -> Inertia {
    [
        [about[0], 0.0, 0.0],
        [0.0, about[1], 0.0],
        [0.0, 0.0, about[2]],
    ]
}

fn length(a: Vec3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// An end that looks along `normal` from `centre`: all the rules read of an end is where its face is and which way it looks.
fn end(centre: Vec3, normal: Vec3) -> BlockFaceView {
    BlockFaceView {
        block_face: BlockFace::plain(VoxelCoordinate::new(0, 0, 0), FaceDirection::PositiveX),
        centre,
        normal,
        rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    }
}

fn between(first: &BlockFaceView, second: &BlockFaceView) -> Approach {
    let gap = [0, 1, 2].map(|axis| second.centre[axis] - first.centre[axis]);
    Approach {
        gap,
        distance: length(gap),
        turn: LatticeRotation::IDENTITY,
        twist_axis: [0.0; 3],
        twist_cosine: 1.0,
    }
}

/// A pair of ends `distance` apart along x that face each other, the second turned `degrees` away from exactly opposite.
fn facing_pair(distance: f64, degrees: f64) -> (BlockFaceView, BlockFaceView) {
    let angle = degrees.to_radians();
    let first = end([0.0; 3], [1.0, 0.0, 0.0]);
    let second = end(
        [distance, 0.0, 0.0],
        [-libm::cos(angle), libm::sin(angle), 0.0],
    );
    (first, second)
}

#[test]
fn the_numbers_are_the_ones_the_owner_chose_and_the_pull_is_the_one_he_chose_at_most() {
    let settings = JointSettings::default();
    // The pull is "10 kN each, as for docking", and ends are in range of each other at the distance docking uses; the coupling is within 0.3 m and 45 degrees; the relative speed
    // is 1 m/s and 0.2 rad/s (agreed 2026-11-09); a coupled car gets 4 extra substeps (proposed and agreed the same day).
    assert_eq!(settings.range, 6.0);
    assert_eq!(settings.pull_force, 10_000.0);
    assert_eq!(settings.couple_distance, 0.3);
    assert_eq!(settings.couple_degrees, 45.0);
    assert_eq!(settings.max_relative_speed, 1.0);
    assert_eq!(settings.max_relative_spin, 0.2);
    assert_eq!(settings.extra_substeps, 4);
}

#[test]
fn ends_that_face_each_other_within_range_pull_and_others_do_not() {
    let settings = JointSettings::default();
    let pulls = |first: &BlockFaceView, second: &BlockFaceView| {
        settings.pulls(first, second, &between(first, second))
    };
    let (first, second) = facing_pair(5.9, 0.0);
    assert!(pulls(&first, &second));
    let (first, second) = facing_pair(6.1, 0.0);
    assert!(!pulls(&first, &second), "out of range");
    // Faces that look the same way, or away from each other, do not pull.
    let first = end([0.0; 3], [1.0, 0.0, 0.0]);
    assert!(!pulls(&first, &end([3.0, 0.0, 0.0], [1.0, 0.0, 0.0])));
    assert!(!pulls(&first, &end([-3.0, 0.0, 0.0], [-1.0, 0.0, 0.0])));
    // An end behind the face of the other (the face looks away from it) does not, though the faces oppose.
    assert!(!pulls(&first, &end([-3.0, 0.0, 0.0], [1.0, 0.0, 0.0])));
    assert!(!pulls(&first, &end([-3.0, 0.0, 0.0], [-1.0, 0.0, 0.0])));
}

#[test]
fn ends_are_coupled_only_within_the_distance_the_angle_and_the_speeds() {
    let settings = JointSettings::default();
    let still = [0.0; 3];
    let coupled = |distance: f64, degrees: f64, speed: f64, spin: f64| {
        let (first, second) = facing_pair(distance, degrees);
        settings.can_couple(
            &first,
            &second,
            &between(&first, &second),
            [0.0, speed, 0.0],
            [0.0, 0.0, spin],
        )
    };
    assert!(coupled(0.29, 0.0, 0.0, 0.0));
    assert!(!coupled(0.31, 0.0, 0.0, 0.0), "too far");
    assert!(coupled(0.1, 44.0, 0.0, 0.0));
    assert!(!coupled(0.1, 46.0, 0.0, 0.0), "faces too far from opposite");
    assert!(coupled(0.1, 0.0, 0.99, 0.0));
    assert!(
        !coupled(0.1, 0.0, 1.01, 0.0),
        "going past each other too fast"
    );
    assert!(coupled(0.1, 0.0, 0.0, 0.19));
    assert!(
        !coupled(0.1, 0.0, 0.0, 0.21),
        "turning against each other too fast"
    );
    // Ends that are already together to a centimetre (a group that was saved and loaded, swinging) are coupled whatever the cars' spin against each other: a ball turns
    // however it is, and only the speed of the pivots matters. The speed is still limited.
    assert!(coupled(0.005, 0.0, 0.0, 3.0), "together, turning fast");
    assert!(!coupled(0.02, 0.0, 0.0, 3.0), "not together, turning fast");
    assert!(
        !coupled(0.005, 0.0, 1.5, 0.0),
        "together, but the pivots are fast"
    );
    assert_eq!(settings.spin_waived_within, 0.01);
    // The direction of the relative velocity does not matter, only its size.
    let (first, second) = facing_pair(0.1, 0.0);
    let how = between(&first, &second);
    assert!(settings.can_couple(&first, &second, &how, [0.6, 0.6, 0.0], still));
    assert!(!settings.can_couple(&first, &second, &how, [0.8, 0.8, 0.0], still));
    // The pivots at one place, at once, is the best case.
    assert!(settings.can_couple(&first, &second, &between(&first, &first), still, still));
}

#[test]
fn the_effective_mass_of_a_pair_is_the_reduced_mass_or_the_mass_of_the_one_that_can_move() {
    assert_eq!(effective_mass(Some(100.0), Some(100.0)), Some(50.0));
    let reduced = effective_mass(Some(1.0), Some(1000.0)).unwrap();
    assert!((reduced - 1000.0 / 1001.0).abs() < 1e-12);
    assert_eq!(effective_mass(Some(100.0), None), Some(100.0));
    assert_eq!(effective_mass(None, Some(7.0)), Some(7.0));
    assert_eq!(effective_mass(None, None), None);
    // A body with no mass (or an unusable one) cannot be pulled.
    assert_eq!(effective_mass(Some(0.0), Some(5.0)), None);
    assert_eq!(effective_mass(Some(f64::NAN), None), None);
}

/// The most the pull may ask of one end, for any masses, gaps and speeds: the owner's 10 kN.
#[test]
fn the_pull_is_never_more_than_ten_kilonewtons_whatever_the_masses_the_gaps_and_the_speeds() {
    let settings = JointSettings::default();
    for mass in [1e-3, 1.0, 50.0, 1e3, 1e5, 1e9] {
        for distance in [0.0, 1e-6, 0.05, 0.3, 6.0, 1e3] {
            for speed in [0.0, 0.5, 5.0, 1e3] {
                for direction in [[1.0, 0.0, 0.0], [0.0, 0.6, 0.8], [-0.6, 0.0, 0.8]] {
                    let gap = direction.map(|component| component * distance);
                    let velocity = [speed, -speed, speed / 2.0];
                    let force = settings.pull_on_second(gap, velocity, mass);
                    assert!(
                        length(force) <= settings.pull_force * (1.0 + 1e-12),
                        "mass {mass} gap {distance} speed {speed}: {force:?}"
                    );
                    assert!(force.iter().all(|part| part.is_finite()));
                }
            }
        }
    }
}

#[test]
fn the_pull_is_toward_the_other_end_from_rest_and_nothing_where_they_are_at_one_place_and_together()
{
    let settings = JointSettings::default();
    // The second end is 5 m along +x from the first: pulled toward it is along -x.
    let force = settings.pull_on_second([5.0, 0.0, 0.0], [0.0; 3], 500.0);
    assert!(
        force[0] < 0.0 && force[1] == 0.0 && force[2] == 0.0,
        "{force:?}"
    );
    // Nothing where they are at one place and at rest against each other; and a mass nobody can pull is not pulled.
    assert_eq!(settings.pull_on_second([0.0; 3], [0.0; 3], 500.0), [0.0; 3]);
    assert_eq!(
        settings.pull_on_second([5.0, 0.0, 0.0], [0.0; 3], 0.0),
        [0.0; 3]
    );
    assert_eq!(
        settings.pull_on_second([5.0, 0.0, 0.0], [0.0; 3], f64::NAN),
        [0.0; 3]
    );
    // At one place but going apart (or past), it takes the speed off: the force is against the relative velocity.
    let force = settings.pull_on_second([0.0; 3], [0.0, 1.0, 0.0], 500.0);
    assert!(force[1] < 0.0 && force[0] == 0.0, "{force:?}");
    // Closing too fast for the brakes: the force is against the way it is going, away from the other end, whatever it is at.
    let force = settings.pull_on_second([1.0, 0.0, 0.0], [-20.0, 0.0, 0.0], 500.0);
    assert!(force[0] > 0.0, "{force:?}");
}

/// Two point bodies on a line, the second `gap` metres along +x of the first, pulled by the rule under the explicit steps the server takes. Returns, for the tick at which the pair
/// first would be coupled by `can_couple`'s distance and speed, how many seconds that was and how fast they were closing, and the least gap they ever had.
fn approach_in_one_dimension(
    settings: &JointSettings,
    first_mass: Option<f64>,
    second_mass: Option<f64>,
    gap: f64,
    closing_speed: f64,
) -> (f64, f64, f64) {
    let dt = 0.05;
    let mass = effective_mass(first_mass, second_mass).unwrap();
    // The first is at rest (or not moving at all), the second comes at it.
    let (mut position_second, mut velocity_first, mut velocity_second) = (gap, 0.0, -closing_speed);
    let mut position_first = 0.0;
    let mut least = gap;
    for tick in 0..4_000 {
        let relative = [velocity_second - velocity_first, 0.0, 0.0];
        let apart = [position_second - position_first, 0.0, 0.0];
        let on_second = settings.pull_on_second(apart, relative, mass);
        if apart[0] <= settings.couple_distance && relative[0].abs() <= settings.max_relative_speed
        {
            return (f64::from(tick) * dt, -relative[0], least);
        }
        // Each end gets the same force, the other way; the one that cannot move (a mass of None) keeps still.
        if let Some(first_mass) = first_mass {
            velocity_first -= on_second[0] / first_mass * dt;
        }
        if let Some(second_mass) = second_mass {
            velocity_second += on_second[0] / second_mass * dt;
        }
        position_first += velocity_first * dt;
        position_second += velocity_second * dt;
        least = least.min(position_second - position_first);
    }
    (f64::INFINITY, 0.0, least)
}

#[test]
fn from_rest_six_metres_off_any_pair_comes_to_within_the_coupling_distance_slowly_and_without_passing_through()
 {
    let settings = JointSettings::default();
    let masses = [1.0, 10.0, 500.0, 1e4, 1e5, 1e6];
    for first in masses {
        for second in masses {
            let (seconds, closing, least) =
                approach_in_one_dimension(&settings, Some(first), Some(second), 6.0, 0.0);
            assert!(seconds.is_finite(), "{first} and {second} never came");
            assert!(
                closing <= settings.max_relative_speed,
                "{first} and {second}: closing at {closing}"
            );
            assert!(least > -0.05, "{first} and {second} went through: {least}");
            // (How long it takes is what the pull allows: 10 kN on a pair of 1,000 tonne ships is 0.01 m/s/s, and they take their time.)
            if first.min(second) <= 1e4 {
                assert!(seconds < 30.0, "{first} and {second}: {seconds} s");
            }
        }
    }
}

#[test]
fn a_ship_that_cannot_move_is_come_to_and_one_that_comes_fast_is_slowed_before_it_arrives() {
    let settings = JointSettings::default();
    // The first held (it cannot move): the second comes to it.
    for second in [1.0, 500.0, 1e5] {
        let (seconds, closing, least) =
            approach_in_one_dimension(&settings, None, Some(second), 6.0, 0.0);
        assert!(
            seconds.is_finite() && closing <= 1.0 && least > -0.05,
            "{second}"
        );
    }
    // Coming at 3 m/s from 2 m (a ship that was going that way, as its pilot flew it) is slowed to the limit before it is within the distance, or while it is.
    for second in [10.0, 500.0, 5e3] {
        let (seconds, closing, least) =
            approach_in_one_dimension(&settings, Some(500.0), Some(second), 2.0, 3.0);
        assert!(seconds.is_finite(), "{second}");
        assert!(closing <= 1.0, "{second}: {closing}");
        assert!(least > -0.05, "{second}: {least}");
    }
}

#[test]
fn the_spin_of_the_cars_against_each_other_is_slowed_by_a_torque_against_it_that_is_never_more_than_ten_kilonewton_metres()
 {
    let settings = JointSettings::default();
    assert_eq!(settings.damping_torque_limit, 10_000.0);
    // Against the spin of the second car against the first, and nothing where there is none.
    let torque = settings.spin_damping_on_second([0.0, 0.5, 0.0], isotropic(1_000.0));
    assert!(
        torque[1] < 0.0 && torque[0] == 0.0 && torque[2] == 0.0,
        "{torque:?}"
    );
    assert_eq!(
        settings.spin_damping_on_second([0.0; 3], isotropic(1_000.0)),
        [0.0; 3]
    );
    assert_eq!(
        settings.spin_damping_on_second([0.0, 0.5, 0.0], isotropic(0.0)),
        [0.0; 3]
    );
    assert_eq!(
        settings.spin_damping_on_second([0.0, 0.5, 0.0], isotropic(f64::NAN)),
        [0.0; 3]
    );
    // In proportion to the inertia and the spin, by the response: 1,000 kg m m times 0.5 rad/s over 0.1 s.
    assert!((torque[1] + 1_000.0 * 0.5 / settings.response_seconds).abs() < 1e-9);
    // Cut to the limit, for any inertia and spin.
    for inertia in [1e-3, 1.0, 1e4, 1e9] {
        for spin in [0.0, 0.3, 5.0, 1e3] {
            let torque =
                settings.spin_damping_on_second([spin, -spin, spin / 2.0], isotropic(inertia));
            assert!(
                length(torque) <= settings.damping_torque_limit * (1.0 + 1e-12),
                "{inertia} {spin}"
            );
            assert!(torque.iter().all(|part| part.is_finite()));
        }
    }
}

#[test]
fn a_torque_against_the_spin_takes_it_off_in_the_end_without_turning_the_other_way() {
    // Two cars turning against each other, slowed by the torque the rule gives, equal and opposite, with the tick of the server.
    let settings = JointSettings::default();
    let (first_inertia, second_inertia) = (3_000.0, 9_000.0);
    let inertia = effective_inertia(
        Some(isotropic(first_inertia)),
        Some(isotropic(second_inertia)),
    )
    .unwrap();
    let (mut first_spin, mut second_spin) = (-0.6_f64, 0.6_f64);
    for _ in 0..100 {
        let on_second =
            settings.spin_damping_on_second([0.0, second_spin - first_spin, 0.0], inertia);
        first_spin -= on_second[1] / first_inertia * 0.05;
        second_spin += on_second[1] / second_inertia * 0.05;
        assert!(second_spin - first_spin >= -1e-9, "turned the other way");
    }
    assert!((second_spin - first_spin).abs() < 1e-3);
    // Angular momentum is as it was: -0.6 x 3,000 + 0.6 x 9,000.
    let momentum = first_inertia * first_spin + second_inertia * second_spin;
    assert!((momentum - 3_600.0).abs() < 1e-6, "{momentum}");
}

#[test]
fn the_pull_is_the_same_on_both_ends_but_the_other_way_so_nothing_is_made_from_nothing() {
    let settings = JointSettings::default();
    // The rule gives the force on the second; the plugin gives the first the opposite. Worked out here as the plugin does it: the momentum of the pair does not change, whatever they weigh.
    let (mut first_velocity, mut second_velocity) = (0.0_f64, 0.0_f64);
    let (first_mass, second_mass) = (30.0, 4_000.0);
    let mass = effective_mass(Some(first_mass), Some(second_mass)).unwrap();
    let mut gap = 4.0;
    for _ in 0..100 {
        let on_second = settings.pull_on_second(
            [gap, 0.0, 0.0],
            [second_velocity - first_velocity, 0.0, 0.0],
            mass,
        );
        first_velocity -= on_second[0] / first_mass * 0.05;
        second_velocity += on_second[0] / second_mass * 0.05;
        gap += (second_velocity - first_velocity) * 0.05;
    }
    let momentum = first_mass * first_velocity + second_mass * second_velocity;
    assert!(momentum.abs() < 1e-9, "{momentum}");
    // And the dot product of the two forces is negative: they oppose.
    let on_second = settings.pull_on_second([3.0, 0.0, 0.0], [0.0; 3], 100.0);
    assert!(dot(on_second, on_second.map(|part| -part)) < 0.0);
}

#[test]
fn the_spin_of_two_cars_that_turn_easily_about_one_axis_is_cut_the_same_part_every_tick_on_every_axis()
 {
    // Two cars like long rods: little inertia about the long axis (x), a lot about the others. The old rule slowed them with one number, a third of the trace, which on the long
    // axis took a spin of 1 radian a second to -13.8 in one tick (red, 2026-11-19, with the tick of the server); each axis is cut by its own response now.
    let settings = JointSettings::default();
    let (first, second) = ([50.0, 3_000.0, 3_000.0], [80.0, 5_000.0, 5_000.0]);
    let inertia = effective_inertia(Some(diagonal(first)), Some(diagonal(second))).unwrap();
    let (mut first_spin, mut second_spin) = ([-0.5, 0.2, 0.1], [0.5, -0.2, -0.1]);
    let kept = 1.0 - 0.05 / settings.response_seconds;
    for tick in 0..60 {
        let relative = [0, 1, 2].map(|axis| second_spin[axis] - first_spin[axis]);
        let on_second = settings.spin_damping_on_second(relative, inertia);
        for axis in 0..3 {
            first_spin[axis] -= on_second[axis] / first[axis] * 0.05;
            second_spin[axis] += on_second[axis] / second[axis] * 0.05;
            let after = second_spin[axis] - first_spin[axis];
            assert!(
                (after - relative[axis] * kept).abs() < 1e-9,
                "tick {tick}, axis {axis}: {} then {after}",
                relative[axis]
            );
        }
    }
    let end = [0, 1, 2].map(|axis| second_spin[axis] - first_spin[axis]);
    assert!(length(end) < 1e-6, "{end:?}");
}

#[test]
fn the_spin_of_a_pair_is_slowed_against_the_inertia_of_the_pair_or_of_the_one_that_can_turn_or_not_at_all()
 {
    let settings = JointSettings::default();
    let (first, second) = (diagonal([2.0, 4.0, 6.0]), diagonal([2.0, 12.0, 3.0]));
    let spin = [1.0, -1.0, 0.5];
    // Both can turn: against the reduced tensor, a b / (a + b) on each axis, worked out by hand: 1, 3 and 2.
    let both = settings.spin_damping_between(spin, Some(first), Some(second));
    for (axis, reduced) in [1.0, 3.0, 2.0].into_iter().enumerate() {
        let wanted = -reduced * spin[axis] / settings.response_seconds;
        assert!((both[axis] - wanted).abs() < 1e-9, "{both:?}");
    }
    // The order of the two is not the matter.
    assert_eq!(
        both,
        settings.spin_damping_between(spin, Some(second), Some(first))
    );
    // One that cannot turn (a ship that is held): the other's own tensor.
    let only_first = settings.spin_damping_between(spin, Some(first), None);
    for (axis, own) in [2.0, 4.0, 6.0].into_iter().enumerate() {
        let wanted = -own * spin[axis] / settings.response_seconds;
        assert!((only_first[axis] - wanted).abs() < 1e-9, "{only_first:?}");
    }
    assert_eq!(
        only_first,
        settings.spin_damping_between(spin, None, Some(first))
    );
    // Neither can, or a tensor is not usable: nothing to slow it against.
    assert_eq!(settings.spin_damping_between(spin, None, None), [0.0; 3]);
    assert_eq!(
        settings.spin_damping_between(spin, Some(isotropic(f64::NAN)), None),
        [0.0; 3]
    );
}

#[test]
fn a_tensor_that_is_the_same_every_way_slows_a_spin_as_the_number_did() {
    let settings = JointSettings::default();
    for inertia in [0.5, 40.0, 3_000.0] {
        for spin in [[0.3, -0.1, 0.2], [0.0, 2.0, 0.0], [-1e-3, 0.0, 5.0]] {
            let torque = settings.spin_damping_on_second(spin, isotropic(inertia));
            let size = length(spin.map(|part| -inertia * part / settings.response_seconds));
            let scale = (settings.damping_torque_limit / size).min(1.0);
            for axis in 0..3 {
                let wanted = -inertia * spin[axis] / settings.response_seconds * scale;
                assert!((torque[axis] - wanted).abs() <= 1e-9 * wanted.abs().max(1.0));
            }
        }
    }
}

#[test]
fn a_tensor_with_a_turn_in_it_gives_a_torque_about_the_axis_it_is_turned_to() {
    // A rod along x that is turned a quarter turn about z lies along y, and turns easily about y: a spin about y gets a small torque, a spin about x a large one.
    let quarter = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let turned = inertia_in_the_world(quarter, diagonal([50.0, 3_000.0, 3_000.0]));
    for (row, expected) in [[3_000.0, 0.0, 0.0], [0.0, 50.0, 0.0], [0.0, 0.0, 3_000.0]]
        .iter()
        .enumerate()
    {
        for column in 0..3 {
            assert!(
                (turned[row][column] - expected[column]).abs() < 1e-9,
                "{turned:?}"
            );
        }
    }
    let unturned = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let body = [[10.0, 1.0, 0.5], [1.0, 20.0, 2.0], [0.5, 2.0, 30.0]];
    assert_eq!(inertia_in_the_world(unturned, body), body);
    // Turned any way, the sum of the diagonal (and so the mean the old rule used) is the same, and the tensor stays symmetric.
    let slant = [[0.6, -0.8, 0.0], [0.8, 0.6, 0.0], [0.0, 0.0, 1.0]];
    let slanted = inertia_in_the_world(slant, body);
    let trace = |tensor: Inertia| tensor[0][0] + tensor[1][1] + tensor[2][2];
    assert!((trace(slanted) - trace(body)).abs() < 1e-9);
    for (row, line) in slanted.iter().enumerate() {
        for (column, entry) in line.iter().enumerate() {
            assert!((entry - slanted[column][row]).abs() < 1e-9);
        }
    }
}

#[test]
fn the_inertia_of_a_pair_is_the_reduced_tensor_or_the_tensor_of_the_one_that_can_turn() {
    let (first, second) = (
        diagonal([50.0, 3_000.0, 3_000.0]),
        diagonal([80.0, 5_000.0, 6_000.0]),
    );
    let both = effective_inertia(Some(first), Some(second)).unwrap();
    for axis in 0..3 {
        let (a, b) = (first[axis][axis], second[axis][axis]);
        assert!(
            (both[axis][axis] - a * b / (a + b)).abs() < 1e-6 * a,
            "{both:?}"
        );
    }
    // The same every way, it is the reduced mass as a number.
    let reduced = effective_inertia(Some(isotropic(100.0)), Some(isotropic(100.0))).unwrap();
    assert!((reduced[0][0] - 50.0).abs() < 1e-9 && reduced[0][1].abs() < 1e-12);
    assert_eq!(effective_inertia(Some(first), None), Some(first));
    assert_eq!(effective_inertia(None, Some(second)), Some(second));
    assert_eq!(effective_inertia(None, None), None);
    // A tensor that is nothing, not a number, or not positive is not usable.
    assert_eq!(effective_inertia(Some(isotropic(0.0)), Some(second)), None);
    assert_eq!(effective_inertia(Some(isotropic(f64::NAN)), None), None);
    assert_eq!(
        effective_inertia(Some(diagonal([1.0, -1.0, 1.0])), None),
        None
    );
}

#[test]
fn what_a_force_at_an_end_adds_to_the_answer_of_a_body_is_the_lever_through_the_inverse_tensor() {
    let rod = diagonal([50.0, 3_000.0, 3_000.0]);
    // A torque of 3 about z turns the rod as 1/3,000 does.
    assert!((inverse_inertia_along(rod, [0.0, 0.0, 3.0]).unwrap() - 9.0 / 3_000.0).abs() < 1e-12);
    // The same torque about the long axis turns it 60 times as much.
    assert!((inverse_inertia_along(rod, [3.0, 0.0, 0.0]).unwrap() - 9.0 / 50.0).abs() < 1e-12);
    assert_eq!(inverse_inertia_along(isotropic(0.0), [1.0, 0.0, 0.0]), None);
}

#[test]
fn the_mass_at_the_end_of_a_car_is_its_mass_less_what_its_lever_gives_it_to_answer_with() {
    let rod = diagonal([50.0, 3_000.0, 3_000.0]);
    // A force along the arm has no lever: the mass of the car.
    let along = mass_at_an_end(100.0, rod, [0.0, 2.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
    assert!((along - 100.0).abs() < 1e-9, "{along}");
    // A force across a 2 metre arm turns the car as well: the end answers it as 1/100 + 4/3,000 does, so as a lighter mass.
    let across = mass_at_an_end(100.0, rod, [0.0, 2.0, 0.0], [1.0, 0.0, 0.0]).unwrap();
    assert!(
        (across - 1.0 / (1.0 / 100.0 + 4.0 / 3_000.0)).abs() < 1e-9,
        "{across}"
    );
    assert!(across < along);
    // About the axis the rod turns easily about, the same lever gives the end far more to answer with.
    let easy = mass_at_an_end(100.0, rod, [0.0, 2.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
    assert!(
        (easy - 1.0 / (1.0 / 100.0 + 4.0 / 50.0)).abs() < 1e-9,
        "{easy}"
    );
    // Nothing that is not a mass and an inertia.
    assert_eq!(
        mass_at_an_end(100.0, isotropic(0.0), [0.0, 2.0, 0.0], [1.0, 0.0, 0.0]),
        None
    );
    assert_eq!(mass_at_an_end(0.0, rod, [0.0; 3], [1.0, 0.0, 0.0]), None);
}

/// The turn about an axis (0 for x, 1 for y, 2 for z) by `degrees`, as the matrix a ship's turn is (`rotation[row][column]`).
fn about(axis: usize, degrees: f64) -> [[f64; 3]; 3] {
    let (sine, cosine) = (
        libm::sin(degrees.to_radians()),
        libm::cos(degrees.to_radians()),
    );
    let (first, second) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut matrix = [[0.0; 3]; 3];
    matrix[axis][axis] = 1.0;
    matrix[first][first] = cosine;
    matrix[first][second] = -sine;
    matrix[second][first] = sine;
    matrix[second][second] = cosine;
    matrix
}

fn multiply(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|row| [0, 1, 2].map(|column| (0..3).map(|k| a[row][k] * b[k][column]).sum()))
}

/// An end of a hinge on a ship turned by `rotation`: it looks up (along y) in the ship's own axes, and its hinge is the line along `hinge` in them.
fn hinge_end(rotation: [[f64; 3]; 3], hinge: Option<FaceDirection>) -> BlockFaceView {
    BlockFaceView {
        block_face: BlockFace {
            voxel: VoxelCoordinate::new(0, 0, 0),
            face: FaceDirection::PositiveY,
            line: hinge,
        },
        centre: [0.0; 3],
        normal: [0, 1, 2].map(|row| rotation[row][1]),
        rotation,
    }
}

const NOT_TURNED: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[test]
fn two_hinge_ends_whose_lines_are_one_line_make_a_hinge_and_the_hinge_of_the_second_is_the_end_of_its_line_that_looks_the_way_the_first_does()
 {
    let settings = JointSettings::default();
    let east = FaceDirection::PositiveX;
    let west = FaceDirection::NegativeX;
    let first = hinge_end(NOT_TURNED, Some(east));
    let hinge = |first, second| Some(Holding::Hinge(LineAxes { first, second }));
    // The same line, whichever end of it the second's block names: it is given the end that points as the first's does (the pivots are paired by it).
    for named in [east, west] {
        assert_eq!(
            settings.holding(&first, &hinge_end(NOT_TURNED, Some(named))),
            hinge(east, east),
            "{named:?}"
        );
    }
    // A second ship turned half way round about the bar (y): its east is the world's west, so the end of its line that points the way the first's does is its west.
    let turned_round = about(1, 180.0);
    assert_eq!(
        settings.holding(&first, &hinge_end(turned_round, Some(east))),
        hinge(east, west)
    );
    // The first named by its west end is told as it is named.
    assert_eq!(
        settings.holding(
            &hinge_end(NOT_TURNED, Some(west)),
            &hinge_end(NOT_TURNED, Some(east))
        ),
        hinge(west, west)
    );
}

#[test]
fn two_hinges_hold_however_far_they_are_bent_about_their_line_and_not_at_all_if_the_lines_are_not_one_line()
 {
    let settings = JointSettings::default();
    let east = FaceDirection::PositiveX;
    let first = hinge_end(NOT_TURNED, Some(east));
    let held = |second: BlockFaceView| settings.holding(&first, &second);
    // Bent about its own line by any angle: the lines are still the same.
    for degrees in [0.0, 30.0, 90.0, 179.0] {
        assert!(
            matches!(
                held(hinge_end(about(0, degrees), Some(east))),
                Some(Holding::Hinge(_))
            ),
            "{degrees}"
        );
    }
    // Turned about the bar a quarter turn, the second's line goes across the first's (along z): two hinges at right angles hold nothing, not even a ball.
    assert_eq!(held(hinge_end(about(1, 90.0), Some(east))), None);
    // The line may be off by 10 degrees (a lock's snap) and not by more: the pivots of the two are brought together by the solver, and that is what it takes to.
    assert_eq!(settings.line_degrees, 10.0);
    assert!(held(hinge_end(about(1, 9.9), Some(east))).is_some());
    assert!(held(hinge_end(about(1, -9.9), Some(east))).is_some());
    assert_eq!(held(hinge_end(about(1, 10.1), Some(east))), None);
    // Off by a bend and a turn together: the turn is what counts.
    assert_eq!(
        held(hinge_end(
            multiply(about(1, 20.0), about(0, 40.0)),
            Some(east)
        )),
        None
    );
}

#[test]
fn a_ball_holds_a_ball_and_a_ball_and_a_hinge_hold_nothing() {
    let settings = JointSettings::default();
    let east = FaceDirection::PositiveX;
    let hinge = hinge_end(NOT_TURNED, Some(east));
    let ball = hinge_end(NOT_TURNED, None);
    assert_eq!(settings.holding(&ball, &ball), Some(Holding::Ball));
    assert_eq!(settings.holding(&hinge, &ball), None);
    assert_eq!(settings.holding(&ball, &hinge), None);
}

#[test]
fn ends_of_different_kinds_neither_pull_nor_couple_and_hinges_whose_lines_cross_pull_and_do_not_couple()
 {
    let settings = JointSettings::default();
    let (ball_first, ball_second) = facing_pair(0.1, 0.0);
    let with_line = |view: BlockFaceView, line: Option<FaceDirection>| BlockFaceView {
        block_face: BlockFace {
            line,
            ..view.block_face
        },
        ..view
    };
    let up = Some(FaceDirection::PositiveY);
    let across = Some(FaceDirection::PositiveZ);
    let how = between(&ball_first, &ball_second);
    // A ball and a hinge, either way round.
    for (first, second) in [
        (ball_first, with_line(ball_second, up)),
        (with_line(ball_first, up), ball_second),
    ] {
        assert!(!settings.pulls(&first, &second, &how));
        assert!(!settings.can_couple(&first, &second, &how, [0.0; 3], [0.0; 3]));
    }
    // Two hinges on one line: both; two hinges whose lines cross are drawn together (the player turns one) and are not coupled.
    let (first, second) = (with_line(ball_first, up), with_line(ball_second, up));
    assert!(settings.pulls(&first, &second, &how));
    assert!(settings.can_couple(&first, &second, &how, [0.0; 3], [0.0; 3]));
    let second = with_line(ball_second, across);
    assert!(settings.pulls(&first, &second, &how));
    assert!(!settings.can_couple(&first, &second, &how, [0.0; 3], [0.0; 3]));
}

#[test]
fn a_joint_end_is_a_ball_a_hinge_or_an_axle_by_where_the_line_it_turns_about_runs() {
    let end = |face, line| BlockFace {
        voxel: VoxelCoordinate::new(0, 0, 0),
        face,
        line,
    };
    for face in FaceDirection::ALL {
        assert_eq!(end(face, None).kind(), JointKind::Ball);
        for line in FaceDirection::ALL {
            // The line along the way the end looks (either end of it) is an axle's; across it is a hinge's.
            let expected = if line.axis() == face.axis() {
                JointKind::Axle
            } else {
                JointKind::Hinge
            };
            assert_eq!(end(face, Some(line)).kind(), expected, "{face:?} {line:?}");
        }
    }
}

#[test]
fn two_axles_hold_however_they_are_turned_about_their_line_and_not_if_it_is_not_one_line() {
    let settings = JointSettings::default();
    let up = FaceDirection::PositiveY;
    let down = FaceDirection::NegativeY;
    let first = hinge_end(NOT_TURNED, Some(up));
    let held = |second: BlockFaceView| settings.holding(&first, &second);
    let axle = |first, second| Some(Holding::Axle(LineAxes { first, second }));
    // Whichever end of the line the second names, it is given the end that points as the first's does.
    for named in [up, down] {
        assert_eq!(held(hinge_end(NOT_TURNED, Some(named))), axle(up, up));
    }
    // Turned about the line (the bar) by any angle, and half way round about another axis so that the ends look at each other: still one line.
    for degrees in [0.0, 17.0, 45.0, 90.0, 133.0, 180.0, 271.0] {
        assert!(
            matches!(
                held(hinge_end(about(1, degrees), Some(up))),
                Some(Holding::Axle(_))
            ),
            "{degrees}"
        );
    }
    assert_eq!(
        held(hinge_end(about(0, 180.0), Some(up))),
        axle(up, down),
        "turned over, its up is the world's down"
    );
    // Tilted off the line by more than ten degrees it is not one line.
    assert!(held(hinge_end(about(0, 9.9), Some(up))).is_some());
    assert_eq!(held(hinge_end(about(0, 10.1), Some(up))), None);
    assert_eq!(held(hinge_end(about(2, 14.0), Some(up))), None);
}

#[test]
fn an_axle_holds_only_an_axle_and_pulls_only_one() {
    let settings = JointSettings::default();
    let axle = hinge_end(NOT_TURNED, Some(FaceDirection::PositiveY));
    let hinge = hinge_end(NOT_TURNED, Some(FaceDirection::PositiveX));
    let ball = hinge_end(NOT_TURNED, None);
    for other in [hinge, ball] {
        assert_eq!(settings.holding(&axle, &other), None);
        assert_eq!(settings.holding(&other, &axle), None);
    }
    // Pulls: ends that face each other, an axle and a hinge or a ball, do not pull; two axles do.
    let (first, second) = facing_pair(0.5, 0.0);
    let with_line = |view: BlockFaceView, line: Option<FaceDirection>| BlockFaceView {
        block_face: BlockFace {
            line,
            ..view.block_face
        },
        ..view
    };
    // (The ends of `facing_pair` are on faces east: an axle's line there is along x, a hinge's along z.)
    let along = Some(FaceDirection::PositiveX);
    let across = Some(FaceDirection::PositiveZ);
    let how = between(&first, &second);
    for other in [across, None] {
        assert!(!settings.pulls(&with_line(first, along), &with_line(second, other), &how));
        assert!(!settings.pulls(&with_line(first, other), &with_line(second, along), &how));
    }
    assert!(settings.pulls(&with_line(first, along), &with_line(second, along), &how));
}
