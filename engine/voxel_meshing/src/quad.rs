use voxel_space::{BRICK_EDGE_LENGTH, FaceDirection};

/// A rectangle of identical voxel faces, all on one plane of a brick.
///
/// For a face along axis `a`, the quad spans axes `u = (a + 1) % 3` and `v = (a + 2) % 3`
/// (x faces span y and z, y faces span z and x, z faces span x and y). `origin` is the lowest
/// voxel of the run; `width` counts voxels along `u` and `height` along `v`. The face itself lies
/// on the voxel's low side for negative faces and on its high side (`origin[a] + 1`) for positive
/// ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Quad {
    pub origin: [u8; 3],
    /// 1 to 32.
    pub width: u8,
    /// 1 to 32.
    pub height: u8,
    pub face: FaceDirection,
    pub material: u32,
}

/// The two axes a quad on `face` spans: `(u, v)`.
pub fn quad_axes(face: FaceDirection) -> (usize, usize) {
    let axis = face.axis();
    ((axis + 1) % 3, (axis + 2) % 3)
}

/// A [`Quad`] in 8 bytes, the layout the GPU reads (two `u32`s per instance).
///
/// `geometry` bits: 0-4 origin x, 5-9 origin y, 10-14 origin z, 15-19 width minus one, 20-24
/// height minus one, 25-27 face index. `material` is the voxel's material value.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PackedQuad {
    pub geometry: u32,
    pub material: u32,
}

impl Quad {
    /// Panics if a component is out of range for the packed layout.
    pub fn pack(self) -> PackedQuad {
        let edge = BRICK_EDGE_LENGTH as u8;
        assert!(
            self.origin.iter().all(|&c| c < edge),
            "quad origin outside the brick"
        );
        assert!(
            (1..=edge).contains(&self.width) && (1..=edge).contains(&self.height),
            "quad size must be 1 to {edge}"
        );
        let geometry = u32::from(self.origin[0])
            | u32::from(self.origin[1]) << 5
            | u32::from(self.origin[2]) << 10
            | u32::from(self.width - 1) << 15
            | u32::from(self.height - 1) << 20
            | u32::from(self.face.index()) << 25;
        PackedQuad {
            geometry,
            material: self.material,
        }
    }
}

impl PackedQuad {
    /// Panics if the face index is invalid.
    pub fn unpack(self) -> Quad {
        let field = |shift: u32| (self.geometry >> shift & 31) as u8;
        Quad {
            origin: [field(0), field(5), field(10)],
            width: field(15) + 1,
            height: field(20) + 1,
            face: FaceDirection::from_index((self.geometry >> 25 & 7) as u8)
                .expect("packed quad has a valid face index"),
            material: self.material,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_places_each_field_at_its_documented_bits() {
        let quad = Quad {
            origin: [1, 2, 3],
            width: 4,
            height: 5,
            face: FaceDirection::PositiveY,
            material: 0xABCD_EF01,
        };
        let packed = quad.pack();
        assert_eq!(
            packed.geometry,
            1 | 2 << 5 | 3 << 10 | 3 << 15 | 4 << 20 | 3 << 25
        );
        assert_eq!(packed.material, 0xABCD_EF01);
        assert_eq!(packed.geometry, 0b011_00100_00011_00011_00010_00001);
    }

    #[test]
    fn every_extreme_round_trips() {
        for face in FaceDirection::ALL {
            for (origin, width, height) in [
                ([0, 0, 0], 1, 1),
                ([31, 31, 31], 32, 32),
                ([31, 0, 17], 1, 32),
            ] {
                let quad = Quad {
                    origin,
                    width,
                    height,
                    face,
                    material: u32::MAX,
                };
                assert_eq!(quad.pack().unpack(), quad);
            }
        }
    }

    #[test]
    fn a_packed_quad_is_eight_bytes() {
        assert_eq!(size_of::<PackedQuad>(), 8);
    }

    #[test]
    #[should_panic(expected = "outside the brick")]
    fn packing_rejects_an_origin_outside_the_brick() {
        Quad {
            origin: [32, 0, 0],
            width: 1,
            height: 1,
            face: FaceDirection::NegativeX,
            material: 0,
        }
        .pack();
    }

    #[test]
    #[should_panic(expected = "size must be")]
    fn packing_rejects_a_zero_size() {
        Quad {
            origin: [0, 0, 0],
            width: 0,
            height: 1,
            face: FaceDirection::NegativeX,
            material: 0,
        }
        .pack();
    }

    #[test]
    fn faces_span_the_two_axes_after_their_own() {
        assert_eq!(quad_axes(FaceDirection::PositiveX), (1, 2));
        assert_eq!(quad_axes(FaceDirection::NegativeY), (2, 0));
        assert_eq!(quad_axes(FaceDirection::PositiveZ), (0, 1));
    }
}
