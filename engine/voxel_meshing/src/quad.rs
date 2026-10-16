use voxel_space::{BRICK_EDGE_LENGTH, FaceDirection};

/// The brightness of a face that nothing dims: the light value of games that have no lighting.
pub const FULL_LIGHT: u8 = 255;

/// The largest block id a quad can carry; the rest of the word holds the light.
pub const MAXIMUM_MATERIAL: u32 = (1 << 24) - 1;

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
    /// The block the faces belong to, which is also the colour's index in the palette.
    pub material: u32,
    /// How bright the faces are, from the light of the voxel in front of them.
    pub light: u8,
}

/// The two axes a quad on `face` spans: `(u, v)`.
///
/// This is the same pair as `block_models::face_axes`; the two are kept in step by a test.
pub fn quad_axes(face: FaceDirection) -> (usize, usize) {
    let axis = face.axis();
    ((axis + 1) % 3, (axis + 2) % 3)
}

/// A [`Quad`] in 8 bytes, the layout the GPU reads (two `u32`s per instance).
///
/// `geometry` bits: 0-4 origin x, 5-9 origin y, 10-14 origin z, 15-19 width minus one, 20-24
/// height minus one, 25-27 face index. `material` bits: 0-23 the block id, 24-31 the light.
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
        assert!(
            self.material <= MAXIMUM_MATERIAL,
            "block id {} does not fit a quad",
            self.material
        );
        let geometry = u32::from(self.origin[0])
            | u32::from(self.origin[1]) << 5
            | u32::from(self.origin[2]) << 10
            | u32::from(self.width - 1) << 15
            | u32::from(self.height - 1) << 20
            | u32::from(self.face.index()) << 25;
        PackedQuad {
            geometry,
            material: self.material | u32::from(self.light) << 24,
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
            material: self.material & MAXIMUM_MATERIAL,
            light: (self.material >> 24) as u8,
        }
    }
}

/// A rectangle of one face of a block that does not fill its cell: a slab's top, a stair's tread.
/// Positions are in eighths of a voxel from the brick's lowest corner, so a quad can sit at any
/// eighth and be any number of eighths across, which a [`Quad`] cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DetailedQuad {
    /// The lowest corner of the rectangle, 0 to 256. Along the face's own axis this is where the
    /// face's plane is, whichever way the face looks.
    pub origin: [u16; 3],
    /// The extent along the first and second axis the face spans (see [`Quad`]), 1 to 256.
    pub width: u16,
    pub height: u16,
    pub face: FaceDirection,
    pub material: u32,
    pub light: u8,
}

/// A [`DetailedQuad`] in 16 bytes (four `u32`s per instance).
///
/// Word 0: origin x (bits 0-15) and y (16-31). Word 1: origin z and width. Word 2: height (0-15),
/// face index (16-23), light (24-31). Word 3: the block id.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PackedDetailedQuad {
    pub words: [u32; 4],
}

/// The most eighths of a voxel along a brick's edge.
const BRICK_EDGE_EIGHTHS: u16 = (BRICK_EDGE_LENGTH * 8) as u16;

impl DetailedQuad {
    /// Panics if a component is out of range for the packed layout.
    pub fn pack(self) -> PackedDetailedQuad {
        assert!(
            self.origin.iter().all(|&c| c <= BRICK_EDGE_EIGHTHS),
            "quad origin outside the brick"
        );
        assert!(
            (1..=BRICK_EDGE_EIGHTHS).contains(&self.width)
                && (1..=BRICK_EDGE_EIGHTHS).contains(&self.height),
            "quad size must be 1 to {BRICK_EDGE_EIGHTHS}"
        );
        assert!(
            self.material <= MAXIMUM_MATERIAL,
            "block id {} does not fit a quad",
            self.material
        );
        PackedDetailedQuad {
            words: [
                u32::from(self.origin[0]) | u32::from(self.origin[1]) << 16,
                u32::from(self.origin[2]) | u32::from(self.width) << 16,
                u32::from(self.height)
                    | u32::from(self.face.index()) << 16
                    | u32::from(self.light) << 24,
                self.material,
            ],
        }
    }
}

impl PackedDetailedQuad {
    /// Panics if the face index is invalid.
    pub fn unpack(self) -> DetailedQuad {
        let [first, second, third, material] = self.words;
        DetailedQuad {
            origin: [
                (first & 0xFFFF) as u16,
                (first >> 16) as u16,
                (second & 0xFFFF) as u16,
            ],
            width: (second >> 16) as u16,
            height: (third & 0xFFFF) as u16,
            face: FaceDirection::from_index((third >> 16 & 0xFF) as u8)
                .expect("packed quad has a valid face index"),
            light: (third >> 24) as u8,
            material,
        }
    }
}

/// What meshing a brick produced: quads that are rectangles of whole voxel faces, which cost 8
/// bytes each and merge into big rectangles, and detailed quads for blocks that do not fill their
/// cell, which are few and cost 16.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrickMesh {
    pub quads: Vec<Quad>,
    pub detailed: Vec<DetailedQuad>,
}

impl BrickMesh {
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty() && self.detailed.is_empty()
    }

    pub fn pack(&self) -> PackedBrickMesh {
        PackedBrickMesh {
            quads: self.quads.iter().map(|quad| quad.pack()).collect(),
            detailed: self.detailed.iter().map(|quad| quad.pack()).collect(),
        }
    }
}

/// A [`BrickMesh`] in the layouts the GPU reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackedBrickMesh {
    pub quads: Vec<PackedQuad>,
    pub detailed: Vec<PackedDetailedQuad>,
}

impl PackedBrickMesh {
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty() && self.detailed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(origin: [u8; 3], width: u8, height: u8, face: FaceDirection) -> Quad {
        Quad {
            origin,
            width,
            height,
            face,
            material: 0,
            light: FULL_LIGHT,
        }
    }

    #[test]
    fn packing_places_each_field_at_its_documented_bits() {
        let packed = Quad {
            origin: [1, 2, 3],
            width: 4,
            height: 5,
            face: FaceDirection::PositiveY,
            material: 0xAB_CDEF,
            light: 0x7E,
        }
        .pack();
        assert_eq!(
            packed.geometry,
            1 | 2 << 5 | 3 << 10 | 3 << 15 | 4 << 20 | 3 << 25
        );
        assert_eq!(packed.material, 0x7EAB_CDEF);
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
                for (material, light) in [(0, 0), (MAXIMUM_MATERIAL, 255), (77, 1)] {
                    let quad = Quad {
                        origin,
                        width,
                        height,
                        face,
                        material,
                        light,
                    };
                    assert_eq!(quad.pack().unpack(), quad);
                }
            }
        }
    }

    #[test]
    fn a_packed_quad_is_eight_bytes_and_a_detailed_one_sixteen() {
        assert_eq!(size_of::<PackedQuad>(), 8);
        assert_eq!(size_of::<PackedDetailedQuad>(), 16);
    }

    #[test]
    #[should_panic(expected = "outside the brick")]
    fn packing_rejects_an_origin_outside_the_brick() {
        quad([32, 0, 0], 1, 1, FaceDirection::NegativeX).pack();
    }

    #[test]
    #[should_panic(expected = "size must be")]
    fn packing_rejects_a_zero_size() {
        quad([0, 0, 0], 0, 1, FaceDirection::NegativeX).pack();
    }

    #[test]
    #[should_panic(expected = "does not fit a quad")]
    fn packing_rejects_a_block_id_that_would_spill_into_the_light() {
        Quad {
            material: MAXIMUM_MATERIAL + 1,
            ..quad([0, 0, 0], 1, 1, FaceDirection::NegativeX)
        }
        .pack();
    }

    #[test]
    fn faces_span_the_two_axes_after_their_own_as_block_models_say_too() {
        assert_eq!(quad_axes(FaceDirection::PositiveX), (1, 2));
        assert_eq!(quad_axes(FaceDirection::NegativeY), (2, 0));
        assert_eq!(quad_axes(FaceDirection::PositiveZ), (0, 1));
        for face in FaceDirection::ALL {
            assert_eq!(quad_axes(face), block_models::face_axes(face), "{face:?}");
        }
    }

    #[test]
    fn detailed_packing_places_each_field_at_its_documented_bits() {
        let packed = DetailedQuad {
            origin: [1, 2, 3],
            width: 4,
            height: 5,
            face: FaceDirection::PositiveY,
            material: 0xABCD,
            light: 0x7E,
        }
        .pack();
        assert_eq!(
            packed.words,
            [1 | 2 << 16, 3 | 4 << 16, 5 | 3 << 16 | 0x7E << 24, 0xABCD]
        );
    }

    #[test]
    fn detailed_extremes_round_trip() {
        for face in FaceDirection::ALL {
            for (origin, width, height) in [
                ([0, 0, 0], 1, 1),
                ([256, 256, 256], 256, 256),
                ([255, 0, 129], 1, 256),
            ] {
                let quad = DetailedQuad {
                    origin,
                    width,
                    height,
                    face,
                    material: MAXIMUM_MATERIAL,
                    light: 255,
                };
                assert_eq!(quad.pack().unpack(), quad);
            }
        }
    }

    #[test]
    #[should_panic(expected = "size must be")]
    fn a_detailed_quad_larger_than_a_brick_is_refused() {
        DetailedQuad {
            origin: [0; 3],
            width: 257,
            height: 1,
            face: FaceDirection::PositiveX,
            material: 0,
            light: 0,
        }
        .pack();
    }

    #[test]
    fn a_mesh_packs_both_kinds_and_knows_when_it_is_empty() {
        assert!(BrickMesh::default().is_empty() && BrickMesh::default().pack().is_empty());
        let mesh = BrickMesh {
            quads: vec![quad([1, 2, 3], 4, 5, FaceDirection::NegativeZ)],
            detailed: vec![DetailedQuad {
                origin: [8, 8, 8],
                width: 8,
                height: 4,
                face: FaceDirection::PositiveY,
                material: 3,
                light: 9,
            }],
        };
        assert!(!mesh.is_empty());
        let packed = mesh.pack();
        assert_eq!(packed.quads[0].unpack(), mesh.quads[0]);
        assert_eq!(packed.detailed[0].unpack(), mesh.detailed[0]);
    }
}
