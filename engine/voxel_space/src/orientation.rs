use std::fmt;

/// How far from 1 the length of an orientation's four components may be. Rotating a body many times
/// drifts a little; a value read from a message or a file that is further off than this is damaged.
const LENGTH_TOLERANCE: f32 = 1e-3;

/// A rotation in three dimensions, as a unit quaternion `[x, y, z, w]`.
///
/// Walking things need only a yaw and a pitch ([`Orientation::from_yaw_pitch`]); ships and the
/// creatures that hold on to surfaces turn all ways, which is why the stored form is the full rotation.
/// Two orientations are equal when their components are bit for bit the same, so an orientation that
/// was sent, saved and read back is equal to the one it was.
#[derive(Clone, Copy)]
pub struct Orientation([f32; 4]);

impl Orientation {
    /// No rotation.
    pub const IDENTITY: Self = Self([0.0, 0.0, 0.0, 1.0]);

    /// The rotation of a body turned `yaw` radians about the vertical axis (positive turns toward
    /// -x, as the camera's yaw does), then tipped up by `pitch` radians. Uses `libm`, so the result
    /// is the same on every machine.
    pub fn from_yaw_pitch(yaw: f32, pitch: f32) -> Self {
        let (half_yaw, half_pitch) = (yaw * 0.5, pitch * 0.5);
        let (yaw_sin, yaw_cos) = (libm::sinf(half_yaw), libm::cosf(half_yaw));
        let (pitch_sin, pitch_cos) = (libm::sinf(half_pitch), libm::cosf(half_pitch));
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
    pub fn from_components(components: [f32; 4]) -> Option<Self> {
        if components.iter().any(|component| !component.is_finite()) {
            return None;
        }
        let length_squared: f32 = components
            .iter()
            .map(|component| component * component)
            .sum();
        ((length_squared - 1.0).abs() <= LENGTH_TOLERANCE).then_some(Self(components))
    }

    /// The components `[x, y, z, w]`.
    pub fn components(self) -> [f32; 4] {
        self.0
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

    fn length_squared(orientation: Orientation) -> f32 {
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
        let half_root_two = std::f32::consts::FRAC_1_SQRT_2;
        assert!((turned[1] - half_root_two).abs() < 1e-6, "{turned:?}");
        assert!((turned[3] - half_root_two).abs() < 1e-6, "{turned:?}");
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
            [f32::NAN, 0.0, 0.0, 1.0],
            [0.0, f32::INFINITY, 0.0, 1.0],
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

    proptest! {
        #[test]
        fn any_yaw_and_pitch_make_a_unit_orientation_that_is_accepted_back(
            yaw in -20.0f32..20.0,
            pitch in -3.2f32..3.2,
        ) {
            let orientation = Orientation::from_yaw_pitch(yaw, pitch);
            prop_assert!((length_squared(orientation) - 1.0).abs() < 1e-5);
            prop_assert_eq!(Orientation::from_components(orientation.components()), Some(orientation));
        }
    }
}
