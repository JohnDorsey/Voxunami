//! What a body in a frame that accelerates and turns (the room of a ship) is pulled by: the pull of the world where it is, and the terms of the frame,
//!
//! `pull = field - acceleration - spin x (spin x arm) - spin_change x arm`
//!
//! with the acceleration of the frame's centre of mass, the spin of the frame (an axis scaled by radians a second), how fast the spin changes, and the arm from the centre of mass
//! to the body, all in the frame's own axes. The third is the centrifugal pull (away from the axis through the centre of mass, `spin^2` times as far as the body is from
//! it), the fourth the Euler pull (against the way the floor is being got going). The fifth term of a frame that turns is the Coriolis pull, `-2 spin x velocity`, which is not a
//! pull in this sense but a turn of the body's velocity, at twice the spin of the frame, the other way: `physics::CharacterBody::turn_velocity` does it exactly, as a turn each
//! step and not a push, which would grow the speed.

use voxel_space::GridPose;

/// The motion of a frame at a moment, in the frame's own axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameMotion {
    /// How fast the centre of mass of the frame is accelerating, in voxels a second a second.
    pub acceleration: [f64; 3],
    /// How fast the frame turns: an axis scaled by radians a second.
    pub spin: [f64; 3],
    /// How fast the spin is changing, in radians a second a second.
    pub spin_change: [f64; 3],
}

impl FrameMotion {
    /// A frame that neither accelerates nor turns.
    pub const STILL: Self = Self {
        acceleration: [0.0; 3],
        spin: [0.0; 3],
        spin_change: [0.0; 3],
    };
}

/// The cross product of two vectors.
pub fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

/// What a body that is `arm` from the centre of mass of a frame that moves as `motion` says is pulled by, where the pull of the world is `field`: see the module.
pub fn pull_in_a_frame(field: [f64; 3], motion: &FrameMotion, arm: [f64; 3]) -> [f64; 3] {
    let centrifugal = cross(motion.spin, cross(motion.spin, arm));
    let euler = cross(motion.spin_change, arm);
    [0, 1, 2].map(|axis| field[axis] - motion.acceleration[axis] - centrifugal[axis] - euler[axis])
}

/// The motion of a frame in the frame's own axes, from what was seen of the ship in the world's: how its centre of mass accelerates (`acceleration`, in the world's axes, less
/// the acceleration of what carries it, if anything does), how it turns now and how it turned `duration` seconds ago. `pose` is where the frame stands in the world.
pub fn frame_motion(
    pose: &GridPose,
    acceleration: [f64; 3],
    spin: [f64; 3],
    spin_before: [f64; 3],
    duration: f64,
) -> FrameMotion {
    let spin_change = [0, 1, 2].map(|axis| (spin[axis] - spin_before[axis]) / duration);
    FrameMotion {
        acceleration: pose.direction_to_local(acceleration),
        spin: pose.direction_to_local(spin),
        spin_change: pose.direction_to_local(spin_change),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELD: [f64; 3] = [0.0, -5.0, 0.0];

    fn motion(acceleration: [f64; 3], spin: [f64; 3], spin_change: [f64; 3]) -> FrameMotion {
        FrameMotion {
            acceleration,
            spin,
            spin_change,
        }
    }

    #[test]
    fn the_cross_product_is_right_handed() {
        assert_eq!(cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]);
        assert_eq!(cross([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]);
        assert_eq!(cross([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]);
        assert_eq!(cross([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]);
    }

    #[test]
    fn a_frame_that_does_not_move_pulls_by_the_field_to_the_bit() {
        assert_eq!(
            pull_in_a_frame(FIELD, &FrameMotion::STILL, [3.0, 4.0, 5.0]),
            FIELD
        );
    }

    #[test]
    fn the_acceleration_of_the_frame_is_taken_off() {
        // A frame falling at the field's own pull is weightless; one that goes up at five weighs twice.
        assert_eq!(
            pull_in_a_frame(FIELD, &motion(FIELD, [0.0; 3], [0.0; 3]), [1.0, 2.0, 3.0]),
            [0.0; 3]
        );
        assert_eq!(
            pull_in_a_frame(
                FIELD,
                &motion([0.0, 5.0, 0.0], [0.0; 3], [0.0; 3]),
                [0.0; 3]
            ),
            [0.0, -10.0, 0.0]
        );
    }

    #[test]
    fn the_centrifugal_pull_is_away_from_the_axis_and_spin_squared_times_as_far() {
        // A spin of two about z, a body three along x from the centre: 4 * 3 = 12 outward, along +x.
        let pull = pull_in_a_frame(
            [0.0; 3],
            &motion([0.0; 3], [0.0, 0.0, 2.0], [0.0; 3]),
            [3.0, 0.0, 0.0],
        );
        assert_eq!(pull, [12.0, 0.0, 0.0]);
        // The part of the arm along the axis is no distance from it: a body on the axis weighs nothing.
        let pull = pull_in_a_frame(
            [0.0; 3],
            &motion([0.0; 3], [0.0, 0.0, 2.0], [0.0; 3]),
            [0.0, 0.0, 9.0],
        );
        assert_eq!(pull, [0.0; 3]);
        // The sign of the spin does not matter, it is squared.
        let pull = pull_in_a_frame(
            [0.0; 3],
            &motion([0.0; 3], [0.0, 0.0, -2.0], [0.0; 3]),
            [3.0, 0.0, 0.0],
        );
        assert_eq!(pull, [12.0, 0.0, 0.0]);
        // On a floor 2.62 above the centre that spins at one about z the floor is held down 2.62 less: up is outward.
        let pull = pull_in_a_frame(
            FIELD,
            &motion([0.0; 3], [0.0, 0.0, 1.0], [0.0; 3]),
            [0.0, 2.62, 0.0],
        );
        assert_eq!(pull, [0.0, -5.0 + 2.62, 0.0]);
    }

    #[test]
    fn the_euler_pull_is_against_the_way_the_floor_is_got_going() {
        // The spin gains one radian a second a second about z, a body two along +y: the floor there is being got going toward -x (spin x arm = (-2, 0, 0)), so the body is pulled
        // toward +x, the way it would stay.
        let pull = pull_in_a_frame(
            [0.0; 3],
            &motion([0.0; 3], [0.0; 3], [0.0, 0.0, 1.0]),
            [0.0, 2.0, 0.0],
        );
        assert_eq!(pull, [2.0, 0.0, 0.0]);
        // A spin that is slowing is the other way.
        let pull = pull_in_a_frame(
            [0.0; 3],
            &motion([0.0; 3], [0.0; 3], [0.0, 0.0, -1.0]),
            [0.0, 2.0, 0.0],
        );
        assert_eq!(pull, [-2.0, 0.0, 0.0]);
    }

    #[test]
    fn the_terms_add() {
        let all = motion([0.5, 0.0, -0.25], [0.0, 0.0, 2.0], [0.0, 0.0, 1.0]);
        let arm = [3.0, 2.0, 0.0];
        // field - acceleration = (-0.5, -5, 0.25); centrifugal, outward = 4 * (3, 2, 0) = (12, 8, 0); Euler = -(z x (3, 2, 0)) = -(-2, 3, 0) = (2, -3, 0).
        assert_eq!(
            pull_in_a_frame(FIELD, &all, arm),
            [-0.5 + 12.0 + 2.0, -5.0 + 8.0 - 3.0, 0.25]
        );
    }

    /// A frame that stands rolled a quarter about z: its x is along the world's +y, its y along the world's -x, and the world's down is its -x.
    fn rolled_a_quarter() -> GridPose {
        let half_root_two = std::f64::consts::FRAC_1_SQRT_2;
        GridPose::new(
            voxel_space::WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, [0.0; 3]),
            voxel_space::Orientation::from_components([0.0, 0.0, half_root_two, half_root_two])
                .unwrap(),
        )
    }

    fn near(actual: [f64; 3], expected: [f64; 3]) {
        for axis in 0..3 {
            assert!(
                (actual[axis] - expected[axis]).abs() < 1e-12,
                "{actual:?} against {expected:?}"
            );
        }
    }

    #[test]
    fn the_motion_of_a_frame_that_stands_as_the_world_does_is_what_was_seen() {
        let pose = GridPose::new(
            voxel_space::WorldPosition::new(voxel_space::BrickCoordinate::ORIGIN, [0.0; 3]),
            voxel_space::Orientation::IDENTITY,
        );
        let motion = frame_motion(
            &pose,
            [1.0, 2.0, 3.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.5],
            0.25,
        );
        near(motion.acceleration, [1.0, 2.0, 3.0]);
        near(motion.spin, [0.0, 0.0, 1.0]);
        // The spin went from a half to one in a quarter of a second: two radians a second a second.
        near(motion.spin_change, [0.0, 0.0, 2.0]);
    }

    #[test]
    fn the_acceleration_the_spin_and_its_change_are_all_turned_into_the_axes_of_the_frame() {
        let pose = rolled_a_quarter();
        // The world's down, (0, -5, 0), is the frame's -x; the world's +x is the frame's -y... and a spin about the world's y is a spin about the frame's x.
        let motion = frame_motion(
            &pose,
            [0.0, -5.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 1.0, 0.0],
            0.5,
        );
        near(motion.acceleration, [-5.0, 0.0, 0.0]);
        near(motion.spin, [2.0, 0.0, 0.0]);
        near(motion.spin_change, [2.0, 0.0, 0.0]);
        // And one about the world's x is one about the frame's -y.
        let motion = frame_motion(&pose, [5.0, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0; 3], 1.0);
        near(motion.acceleration, [0.0, -5.0, 0.0]);
        near(motion.spin, [0.0, -3.0, 0.0]);
        near(motion.spin_change, [0.0, -3.0, 0.0]);
    }
}
