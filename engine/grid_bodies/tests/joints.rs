//! The rules of joints between ships, apart from any ship: when two joint ends pull on each other, when they are coupled, and the pull (`docs/decisions/joints-and-cars.md`,
//! "Coupling, lock, unlock"). The numbers the owner chose are in the first test; the pull is the part that is mine, and it is checked by working a pair of bodies out under it.

use grid_bodies::BlockFace;
use grid_bodies::docking::{Approach, BlockFaceView};
use grid_bodies::joints::{JointSettings, effective_mass};
use voxel_space::{FaceDirection, LatticeRotation, VoxelCoordinate};

type Vec3 = [f64; 3];

fn length(a: Vec3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// An end that looks along `normal` from `centre`: all the rules read of an end is where its face is and which way it looks.
fn end(centre: Vec3, normal: Vec3) -> BlockFaceView {
    BlockFaceView {
        block_face: BlockFace {
            voxel: VoxelCoordinate::new(0, 0, 0),
            face: FaceDirection::PositiveX,
        },
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
    let torque = settings.spin_damping_on_second([0.0, 0.5, 0.0], 1_000.0);
    assert!(
        torque[1] < 0.0 && torque[0] == 0.0 && torque[2] == 0.0,
        "{torque:?}"
    );
    assert_eq!(settings.spin_damping_on_second([0.0; 3], 1_000.0), [0.0; 3]);
    assert_eq!(
        settings.spin_damping_on_second([0.0, 0.5, 0.0], 0.0),
        [0.0; 3]
    );
    assert_eq!(
        settings.spin_damping_on_second([0.0, 0.5, 0.0], f64::NAN),
        [0.0; 3]
    );
    // In proportion to the inertia and the spin, by the response: 1,000 kg m m times 0.5 rad/s over 0.1 s.
    assert!((torque[1] + 1_000.0 * 0.5 / settings.response_seconds).abs() < 1e-9);
    // Cut to the limit, for any inertia and spin.
    for inertia in [1e-3, 1.0, 1e4, 1e9] {
        for spin in [0.0, 0.3, 5.0, 1e3] {
            let torque = settings.spin_damping_on_second([spin, -spin, spin / 2.0], inertia);
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
    let inertia = effective_mass(Some(first_inertia), Some(second_inertia)).unwrap();
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
