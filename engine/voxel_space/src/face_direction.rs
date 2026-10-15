/// One of the six axis-aligned directions a voxel face can point.
///
/// The discriminants are the face indices used by meshes and shaders: `index = 2 * axis + (1 if
/// positive)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum FaceDirection {
    NegativeX = 0,
    PositiveX = 1,
    NegativeY = 2,
    PositiveY = 3,
    NegativeZ = 4,
    PositiveZ = 5,
}

impl FaceDirection {
    pub const ALL: [Self; 6] = [
        Self::NegativeX,
        Self::PositiveX,
        Self::NegativeY,
        Self::PositiveY,
        Self::NegativeZ,
        Self::PositiveZ,
    ];

    /// `None` unless `index < 6`.
    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }

    pub fn index(self) -> u8 {
        self as u8
    }

    /// 0 for x, 1 for y, 2 for z.
    pub fn axis(self) -> usize {
        usize::from(self as u8 / 2)
    }

    pub fn is_positive(self) -> bool {
        self as u8 % 2 == 1
    }

    pub fn opposite(self) -> Self {
        Self::ALL[usize::from(self as u8 ^ 1)]
    }

    /// The unit step to the neighbouring voxel this face looks at.
    pub fn offset(self) -> [i64; 3] {
        let mut offset = [0; 3];
        offset[self.axis()] = if self.is_positive() { 1 } else { -1 };
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_follow_axis_then_sign() {
        let expected = [
            (FaceDirection::NegativeX, 0, 0, false),
            (FaceDirection::PositiveX, 1, 0, true),
            (FaceDirection::NegativeY, 2, 1, false),
            (FaceDirection::PositiveY, 3, 1, true),
            (FaceDirection::NegativeZ, 4, 2, false),
            (FaceDirection::PositiveZ, 5, 2, true),
        ];
        for (face, index, axis, positive) in expected {
            assert_eq!(face.index(), index);
            assert_eq!(face.axis(), axis);
            assert_eq!(face.is_positive(), positive);
            assert_eq!(FaceDirection::from_index(index), Some(face));
        }
        assert_eq!(FaceDirection::from_index(6), None);
        assert_eq!(FaceDirection::ALL.len(), 6);
    }

    #[test]
    fn offsets_are_unit_steps_and_opposites_cancel() {
        assert_eq!(FaceDirection::NegativeX.offset(), [-1, 0, 0]);
        assert_eq!(FaceDirection::PositiveY.offset(), [0, 1, 0]);
        assert_eq!(FaceDirection::PositiveZ.offset(), [0, 0, 1]);
        for face in FaceDirection::ALL {
            let (a, b) = (face.offset(), face.opposite().offset());
            assert_eq!([a[0] + b[0], a[1] + b[1], a[2] + b[2]], [0, 0, 0]);
            assert_eq!(face.opposite().opposite(), face);
            assert_ne!(face.opposite(), face);
        }
    }
}
