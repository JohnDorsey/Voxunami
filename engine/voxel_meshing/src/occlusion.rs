//! How much the cubes beside a face shade each of its four corners (ambient occlusion).
//!
//! A corner's **level** is 0 (nothing shades it) to 3 (as dark as it goes). It is worked out from the layer of cells in front of the face, from the 2 by 2 cells that meet at the
//! corner: the cell the face looks at (which is air, or the face would not be drawn), the two cells beside it, and the cell diagonal to it. Each of the other three that is a cube that
//! occludes adds 1, except that when both cells beside it are, the corner is level 3 whatever the diagonal is, because nothing gets round a corner that is shut on both sides.
//!
//! The level belongs to the corner, not to the face: two faces on one plane that share a corner look at the same four cells and give it the same level, so the shade has no seam where
//! quads meet. (A rule that counted the face's own two edges instead would have seams; the tests pin this.)
//!
//! What a level looks like, and how the four corners are blended over the quad, is the renderer's.

use voxel_space::FaceDirection;

use crate::quad::quad_axes;

/// The level of each of the four corners of a quad, in two bits each.
///
/// Corners are numbered `u` fastest, in the quad's own axes ([`quad_axes`]): 0 is the lowest in both, 1 the highest in `u`, 2 the highest in `v`, 3 the highest in both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CornerOcclusion(u8);

impl CornerOcclusion {
    /// No corner is shaded.
    pub const NONE: Self = Self(0);

    /// Panics if a level is more than 3.
    pub fn from_levels(levels: [u8; 4]) -> Self {
        let mut bits = 0;
        for (corner, level) in levels.into_iter().enumerate() {
            assert!(level <= 3, "occlusion level {level} does not fit two bits");
            bits |= level << (2 * corner);
        }
        Self(bits)
    }

    /// The levels as the GPU reads them: two bits each, corner 0 in the lowest bits.
    pub fn bits(self) -> u8 {
        self.0
    }

    /// The levels from [`CornerOcclusion::bits`]. Panics if a bit above the eighth is set (the rest of a packed word is not used).
    pub fn from_bits(bits: u32) -> Self {
        assert!(bits <= 0xFF, "occlusion bits {bits:#x} are more than eight");
        Self(bits as u8)
    }

    pub fn levels(self) -> [u8; 4] {
        std::array::from_fn(|corner| self.0 >> (2 * corner) & 3)
    }

    /// Whether the two corners at the same end of `v` have the same level: a run of such faces along `u` can be one quad, with the same four levels, and be shaded as each
    /// of them is.
    pub fn is_constant_along_u(self) -> bool {
        let [lowest, right, upper, upper_right] = self.levels();
        lowest == right && upper == upper_right
    }

    /// The same along `v`.
    pub fn is_constant_along_v(self) -> bool {
        let [lowest, right, upper, upper_right] = self.levels();
        lowest == upper && right == upper_right
    }
}

/// The corner levels of the face of the voxel at `voxel` that looks `face`. `occludes` says whether the cell at a place is a cube that occludes; the function asks only about
/// the eight cells around the one the face looks at.
pub fn corner_occlusion(
    occludes: impl Fn([i64; 3]) -> bool,
    voxel: [i64; 3],
    face: FaceDirection,
) -> CornerOcclusion {
    let (u_axis, v_axis) = quad_axes(face);
    let mut front = voxel;
    front[face.axis()] += if face.is_positive() { 1 } else { -1 };
    let mut levels = [0; 4];
    for (corner, level) in levels.iter_mut().enumerate() {
        let toward_u = if corner & 1 == 1 { 1 } else { -1 };
        let toward_v = if corner & 2 == 2 { 1 } else { -1 };
        let mut beside_in_u = front;
        beside_in_u[u_axis] += toward_u;
        let mut beside_in_v = front;
        beside_in_v[v_axis] += toward_v;
        let mut diagonal = beside_in_u;
        diagonal[v_axis] += toward_v;
        let (blocked_in_u, blocked_in_v) = (occludes(beside_in_u), occludes(beside_in_v));
        let diagonal_blocked = occludes(diagonal);
        *level = if blocked_in_u && blocked_in_v {
            3
        } else {
            u8::from(blocked_in_u) + u8::from(blocked_in_v) + u8::from(diagonal_blocked)
        };
    }
    CornerOcclusion::from_levels(levels)
}

#[cfg(test)]
mod tests;
