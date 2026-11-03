//! Where a body that goes round is from its centre, as the world places it: whole bricks and a rest.

use wide_float::WideFloat;

/// What a body that goes round is from its centre, exact: whole bricks of 32 voxels and a rest below a brick, in steps of 2^-32 voxel. (An `f64` of voxels is as
/// far from the next as 2^-52 of itself, which is a hundred voxels at 10^18.)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExactOffset {
    pub bricks: [i128; 3],
    /// In voxels, from 0 up to a brick, a whole number of 2^-32.
    pub rest: [f64; 3],
}

/// Steps of 2^-32 voxel in a brick of 32 voxels.
const STEPS_IN_A_BRICK: i128 = 1 << 37;

/// The step of the rest, as an `f64`: 2^-32.
const STEP: f64 = 1.0 / 4_294_967_296.0;

impl ExactOffset {
    /// As plain numbers, in voxels: as near as an `f64` can say it, to a rounding or two.
    pub fn in_voxels(self) -> [f64; 3] {
        [0, 1, 2].map(|axis| self.bricks[axis] as f64 * 32.0 + self.rest[axis])
    }

    /// A place of voxels from the centre (numbers of the width the place was worked out in, which must hold the bits of a brick count and the step: 128 bits do for every place
    /// an orbit has), rounded to the nearest step of 2^-32 voxel, half a step up. The bricks of a place are an `i128` for every orbit a rail can have (a radius under 2^121
    /// voxels).
    pub(crate) fn of_voxels(place: &[WideFloat; 3]) -> Self {
        let parts = place.each_ref().map(bricks_and_rest);
        Self {
            bricks: parts.map(|(bricks, _)| bricks),
            rest: parts.map(|(_, rest)| rest),
        }
    }
}

/// `voxels` as whole bricks (rounded down, so that the rest is never below nothing) and a rest below a brick, in steps of 2^-32 voxel.
fn bricks_and_rest(voxels: &WideFloat) -> (i128, f64) {
    // (Eight bits more than the place was worked out in hold the sum below: it is below 2^38.)
    let precision = voxels.precision().widened_by(8);
    let value = voxels.rounded_to(precision);
    let bricks = value
        .times_power_of_two(-5)
        .expect("a number of voxels divided by 32 is a number")
        .floor_to_i128()
        .expect("a place of an orbit is under 2^117 bricks from its centre");
    let before = &WideFloat::from_i128(bricks, precision)
        .times_power_of_two(5)
        .expect("a number of bricks times 32 is a number");
    let rest = (&value - before)
        .times_power_of_two(32)
        .expect("a rest times 2^32 is a number");
    let half = WideFloat::from_i64(1, precision)
        .times_power_of_two(-1)
        .expect("a half is a number");
    let steps = (&rest + &half)
        .floor_to_i128()
        .expect("a rest is below a brick, which is below 2^38 steps");
    // (A rest that rounds up to a whole brick is the next brick and nothing.)
    if steps == STEPS_IN_A_BRICK {
        (bricks + 1, 0.0)
    } else {
        (bricks, steps as f64 * STEP)
    }
}

#[cfg(test)]
mod tests;
