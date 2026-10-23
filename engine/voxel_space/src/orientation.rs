use std::fmt;

/// How far from 1 the length of an orientation's four components may be. Rotating a body many times
/// drifts a little; a value read from a message or a file that is further off than this is damaged.
const LENGTH_TOLERANCE: f64 = 1e-3;

/// A rotation in three dimensions, as a unit quaternion `[x, y, z, w]` of `f64`.
///
/// Walking things need only a yaw and a pitch ([`Orientation::from_yaw_pitch`]); ships and the
/// creatures that hold on to surfaces turn all ways, which is why the stored form is the full rotation.
/// It is `f64` so that a very large ship, a planet, can be turned exactly: an error of one part in ten million
/// of the turn is five metres at fifty thousand kilometres. Two orientations are equal when their components are bit for bit the same, so
/// an orientation that was sent, saved and read back is equal to the one it was.
#[derive(Clone, Copy)]
pub struct Orientation([f64; 4]);

impl Orientation {
    /// No rotation.
    pub const IDENTITY: Self = Self([0.0, 0.0, 0.0, 1.0]);

    /// The rotation of a body turned `yaw` radians about the vertical axis (positive turns toward
    /// -x, as the camera's yaw does), then tipped up by `pitch` radians. Uses `libm`, so the result
    /// is the same on every machine.
    pub fn from_yaw_pitch(yaw: f32, pitch: f32) -> Self {
        let (half_yaw, half_pitch) = (f64::from(yaw) * 0.5, f64::from(pitch) * 0.5);
        let (yaw_sin, yaw_cos) = (libm::sin(half_yaw), libm::cos(half_yaw));
        let (pitch_sin, pitch_cos) = (libm::sin(half_pitch), libm::cos(half_pitch));
        // Yaw about +y, then pitch about the turned x axis: q = yaw * pitch.
        Self([
            yaw_cos * pitch_sin,
            yaw_sin * pitch_cos,
            // Written as a subtraction from zero so that no turn at all gives +0, not -0.
            0.0 - yaw_sin * pitch_sin,
            yaw_cos * pitch_cos,
        ])
    }

    /// The orientation with these components `[x, y, z, w]`, if they are finite and make a unit
    /// quaternion (to within the drift [`Orientation`] allows).
    pub fn from_components(components: [f64; 4]) -> Option<Self> {
        if components.iter().any(|component| !component.is_finite()) {
            return None;
        }
        let length_squared: f64 = components
            .iter()
            .map(|component| component * component)
            .sum();
        ((length_squared - 1.0).abs() <= LENGTH_TOLERANCE).then_some(Self(components))
    }

    /// The orientation with these `f32` components, exactly (every `f32` is an `f64`): what an older file or a graphics or physics library holds.
    pub fn from_single_components(components: [f32; 4]) -> Option<Self> {
        Self::from_components(components.map(f64::from))
    }

    /// The components `[x, y, z, w]`.
    pub fn components(self) -> [f64; 4] {
        self.0
    }

    /// The components rounded to `f32`, for the libraries that work in it (the graphics card, the physics engine). Rounding is the only loss.
    pub fn components_single(self) -> [f32; 4] {
        self.0.map(|component| component as f32)
    }

    /// The rotation that does `inner` first and then `self`: the product of the two quaternions, `self * inner`, as for matrices.
    /// Normalised, so the same two rotations give the same bits on every machine.
    pub fn compose(self, inner: Self) -> Self {
        let [ax, ay, az, aw] = self.0;
        let [bx, by, bz, bw] = inner.0;
        let product = [
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz,
        ];
        let length = libm::sqrt(product.map(|c| c * c).iter().sum::<f64>());
        Self(product.map(|component| component / length))
    }

    /// The rotation that undoes this one (the conjugate: for a unit quaternion that is its inverse).
    pub fn inverse(self) -> Self {
        let [x, y, z, w] = self.0;
        // Written as subtractions from zero so that no turn about an axis gives +0 there, not -0.
        Self([0.0 - x, 0.0 - y, 0.0 - z, w])
    }
}

impl Default for Orientation {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl PartialEq for Orientation {
    fn eq(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(&other.0)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

impl Eq for Orientation {}

impl fmt::Debug for Orientation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Orientation([{}, {}, {}, {}])",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn length_squared(orientation: Orientation) -> f64 {
        orientation.components().iter().map(|c| c * c).sum()
    }

    #[test]
    fn identity_is_no_rotation_and_the_default() {
        assert_eq!(Orientation::IDENTITY.components(), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(Orientation::default(), Orientation::IDENTITY);
        assert_eq!(Orientation::from_yaw_pitch(0.0, 0.0), Orientation::IDENTITY);
    }

    #[test]
    fn a_quarter_turn_of_yaw_is_a_half_root_two_about_the_vertical_axis() {
        let turned = Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0).components();
        let half_root_two = std::f64::consts::FRAC_1_SQRT_2;
        assert!((turned[1] - half_root_two).abs() < 1e-7, "{turned:?}");
        assert!((turned[3] - half_root_two).abs() < 1e-7, "{turned:?}");
        assert_eq!((turned[0].to_bits(), turned[2].to_bits()), (0, 0));
    }

    #[test]
    fn components_that_are_not_a_rotation_are_refused() {
        assert!(Orientation::from_components([0.0, 0.0, 0.0, 1.0]).is_some());
        assert!(Orientation::from_components([0.5, 0.5, 0.5, 0.5]).is_some());
        for bad in [
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 2.0],
            [0.0, 0.0, 0.0, 1.01],
            [f64::NAN, 0.0, 0.0, 1.0],
            [0.0, f64::INFINITY, 0.0, 1.0],
        ] {
            assert!(Orientation::from_components(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn equality_is_by_bits_so_a_value_equals_itself_and_zero_signs_differ() {
        let turned = Orientation::from_yaw_pitch(1.0, 0.5);
        assert_eq!(turned, turned);
        let positive = Orientation::from_components([0.0, 0.0, 0.0, 1.0]).unwrap();
        let negative = Orientation::from_components([-0.0, 0.0, 0.0, 1.0]).unwrap();
        assert_ne!(positive, negative);
    }

    #[test]
    fn turns_about_one_axis_add_when_composed() {
        let (first, second) = (0.7f32, -1.9f32);
        let composed = Orientation::from_yaw_pitch(first, 0.0)
            .compose(Orientation::from_yaw_pitch(second, 0.0));
        let expected = Orientation::from_yaw_pitch(first + second, 0.0).components();
        for (component, wanted) in composed.components().into_iter().zip(expected) {
            assert!(
                (component - wanted).abs() < 1e-6,
                "{composed:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn composing_is_not_commutative_and_does_the_inner_turn_first() {
        // Tipped up a quarter turn about x first, then turned a quarter about the vertical axis: the product (yaw about y) * (pitch about
        // x), worked out by hand for two quarter turns, is the quaternion (1/2, 1/2, -1/2, 1/2). The other order is another turn.
        let quarter = std::f32::consts::FRAC_PI_2;
        let tip = Orientation::from_yaw_pitch(0.0, quarter);
        let turn = Orientation::from_yaw_pitch(quarter, 0.0);
        let [x, y, z, w] = turn.compose(tip).components();
        for (component, wanted) in [x, y, z, w].into_iter().zip([0.5, 0.5, -0.5, 0.5]) {
            assert!((component - wanted).abs() < 1e-6, "{x} {y} {z} {w}");
        }
        assert_ne!(turn.compose(tip), tip.compose(turn));
    }

    #[test]
    fn the_inverse_of_no_turn_is_no_turn_with_no_negative_zeros() {
        assert_eq!(Orientation::IDENTITY.inverse(), Orientation::IDENTITY);
    }

    proptest! {
        #[test]
        fn a_turn_composed_with_its_inverse_is_no_turn_in_either_order(
            yaw in -20.0f32..20.0,
            pitch in -1.5f32..1.5,
        ) {
            let turn = Orientation::from_yaw_pitch(yaw, pitch);
            for composed in [turn.compose(turn.inverse()), turn.inverse().compose(turn)] {
                let [x, y, z, w] = composed.components();
                prop_assert!(x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12);
                prop_assert!((w.abs() - 1.0).abs() < 1e-12);
            }
        }

        #[test]
        fn any_yaw_and_pitch_make_a_unit_orientation_that_is_accepted_back(
            yaw in -20.0f32..20.0,
            pitch in -3.2f32..3.2,
        ) {
            let orientation = Orientation::from_yaw_pitch(yaw, pitch);
            prop_assert!((length_squared(orientation) - 1.0).abs() < 1e-12);
            prop_assert_eq!(Orientation::from_components(orientation.components()), Some(orientation));
        }
    }
}
