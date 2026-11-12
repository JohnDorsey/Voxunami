//! The reference for the sun's ray against the boxes of entities (`docs/specs/contact-trace-spec.md`, section 16): an avatar or an item on the ground is not a voxel of any grid,
//! so the walk through the lattice cannot find it, and the shadow map leaves the same lit strip at its foot as at a wall's. The shader tests the ray against a short list of boxes
//! with the slab method; this is the same in `f64`, and its tests are against a ray marched in small steps, which has nothing in common with it.

/// A box the sun does not pass through, with its sides along the axes of the world: the entity's (an avatar's two voxels, or an item's box).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunBox {
    /// The lowest and the highest corner, in voxels, in the same frame as the ray's origin.
    pub low: [f64; 3],
    pub high: [f64; 3],
}

impl SunBox {
    /// A box with this middle and this half the size along each axis.
    pub fn around(middle: [f64; 3], half_size: [f64; 3]) -> Self {
        Self {
            low: std::array::from_fn(|axis| middle[axis] - half_size[axis]),
            high: std::array::from_fn(|axis| middle[axis] + half_size[axis]),
        }
    }
}

/// Whether the sun's ray from `origin`, along `toward_sun` (a vector of any length, not zero), enters one of `boxes` within `longest` voxels of its length. A ray that starts
/// inside a box is blocked (a place inside an entity is in its shade), and one that only touches a box's edge or face is too; a box behind the origin is not in the way.
pub fn sun_blocked_by_boxes(
    origin: [f64; 3],
    toward_sun: [f64; 3],
    longest: f64,
    boxes: &[SunBox],
) -> bool {
    let length = toward_sun
        .iter()
        .map(|part| part * part)
        .sum::<f64>()
        .sqrt();
    assert!(length > 0.0, "the sun has a direction");
    let direction = toward_sun.map(|part| part / length);
    boxes.iter().any(|candidate| {
        // The stretch of the ray that is inside the box, along each axis in turn: where it enters the slab between the two planes of that axis and where it leaves it.
        let (mut near, mut far) = (f64::NEG_INFINITY, f64::INFINITY);
        for axis in 0..3 {
            let (low, high) = (candidate.low[axis], candidate.high[axis]);
            if direction[axis] == 0.0 {
                // (A ray along a plane stays between the two or never is.)
                if origin[axis] < low || origin[axis] > high {
                    return false;
                }
                continue;
            }
            let first = (low - origin[axis]) / direction[axis];
            let second = (high - origin[axis]) / direction[axis];
            near = near.max(first.min(second));
            far = far.min(first.max(second));
        }
        near <= far && far >= 0.0 && near <= longest
    })
}

#[cfg(test)]
mod tests;
