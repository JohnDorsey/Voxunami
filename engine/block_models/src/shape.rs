use voxel_space::FaceDirection;

/// How many sub-cells a voxel's edge is divided into when describing the part of the voxel a block
/// fills. Shapes are made of boxes whose corners are on this grid.
pub const SUBDIVISIONS: usize = 8;

const CELL_COUNT: usize = SUBDIVISIONS * SUBDIVISIONS * SUBDIVISIONS;

/// The two axes a face along `face`'s axis spans, `(u, v)`: the axes after its own, wrapping
/// around. This is the order the mesher's quads use for their width and height.
pub fn face_axes(face: FaceDirection) -> (usize, usize) {
    let axis = face.axis();
    ((axis + 1) % 3, (axis + 2) % 3)
}

/// A box inside a voxel, in eighths of a voxel: `min` is below `max` on every axis and every
/// coordinate is from 0 to 8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeBox {
    pub min: [u8; 3],
    pub max: [u8; 3],
}

impl ShapeBox {
    /// The whole voxel.
    pub const FULL: Self = Self {
        min: [0; 3],
        max: [SUBDIVISIONS as u8; 3],
    };
}

/// A rectangle of exposed sub-faces of a shape, all in one layer of sub-cells, facing one way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubRect {
    /// The layer of sub-cells (0 to 7, along the face's axis) the rectangle belongs to. The face
    /// itself is on the layer's high side for a positive face and its low side for a negative one.
    pub layer: u8,
    /// Where the rectangle starts and how big it is, in sub-cells along the face's two axes
    /// ([`face_axes`]).
    pub u: u8,
    pub v: u8,
    pub width: u8,
    pub height: u8,
}

impl SubRect {
    /// Whether the rectangle lies on the boundary of the voxel, where the voxel next door may hide it.
    pub fn on_boundary(&self, face: FaceDirection) -> bool {
        usize::from(self.layer)
            == if face.is_positive() {
                SUBDIVISIONS - 1
            } else {
                0
            }
    }

    /// The bits of a face mask (see [`Shape::face_cover`]) that this rectangle covers.
    pub fn mask(&self) -> u64 {
        let mut mask = 0;
        for v in self.v..self.v + self.height {
            for u in self.u..self.u + self.width {
                mask |= 1 << (usize::from(v) * SUBDIVISIONS + usize::from(u));
            }
        }
        mask
    }
}

/// Which faces of a voxel are joined through the part of it a block leaves empty: light and other
/// fills can travel from one joined face to another through the cell. Symmetric; a face is joined to
/// itself when any of it is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceConnections(u64);

impl FaceConnections {
    /// Nothing is joined: the cell is solid, or every face is covered.
    pub const NONE: Self = Self(0);

    fn bit(first: FaceDirection, second: FaceDirection) -> u64 {
        1 << (usize::from(first.index()) * 6 + usize::from(second.index()))
    }

    /// Everything joined to everything: an empty cell.
    pub fn all() -> Self {
        let mut connections = 0;
        for first in FaceDirection::ALL {
            for second in FaceDirection::ALL {
                connections |= Self::bit(first, second);
            }
        }
        Self(connections)
    }

    pub fn connects(self, first: FaceDirection, second: FaceDirection) -> bool {
        self.0 & Self::bit(first, second) != 0
    }

    /// Whether any of the face is open.
    pub fn is_open(self, face: FaceDirection) -> bool {
        self.connects(face, face)
    }
}

/// The part of a voxel a block fills, with everything the rest of the engine wants to know about it
/// worked out once: which sub-faces show, which faces are covered (so they hide what is behind
/// them), and which faces the empty part joins (so light can pass).
///
/// A shape is described by boxes (which are also what a physics engine collides with) and measured
/// on a grid of [`SUBDIVISIONS`] cubed sub-cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    boxes: Vec<ShapeBox>,
    /// One word per layer of z; bit `y * 8 + x` is whether sub-cell (x, y, z) is filled.
    occupied: [u64; SUBDIVISIONS],
    face_cover: [u64; 6],
    connections: FaceConnections,
    exposed: [Vec<SubRect>; 6],
}

impl Shape {
    /// Nothing: air.
    pub fn empty() -> Self {
        Self::from_boxes(&[])
    }

    /// The whole voxel.
    pub fn full() -> Self {
        Self::from_boxes(&[ShapeBox::FULL])
    }

    /// The half of the voxel against `side`: a slab sitting on that side. A slab against
    /// [`FaceDirection::NegativeY`] is the usual bottom slab.
    pub fn half(side: FaceDirection) -> Self {
        let middle = (SUBDIVISIONS / 2) as u8;
        let mut shape_box = ShapeBox::FULL;
        if side.is_positive() {
            shape_box.min[side.axis()] = middle;
        } else {
            shape_box.max[side.axis()] = middle;
        }
        Self::from_boxes(&[shape_box])
    }

    /// A stair: the bottom half of the voxel, and the upper half of the part toward `high_side`,
    /// which must be a horizontal direction.
    ///
    /// Panics if `high_side` is up or down.
    pub fn stair(high_side: FaceDirection) -> Self {
        assert_ne!(high_side.axis(), 1, "a stair rises toward a side");
        let (end, middle) = (SUBDIVISIONS as u8, (SUBDIVISIONS / 2) as u8);
        let mut step = ShapeBox {
            min: [0, middle, 0],
            max: [end, end, end],
        };
        if high_side.is_positive() {
            step.min[high_side.axis()] = middle;
        } else {
            step.max[high_side.axis()] = middle;
        }
        Self::from_boxes(&[
            ShapeBox {
                min: [0; 3],
                max: [end, middle, end],
            },
            step,
        ])
    }

    /// A shape that is the union of `boxes`.
    ///
    /// Panics if a box is empty or leaves the voxel.
    pub fn from_boxes(boxes: &[ShapeBox]) -> Self {
        let mut occupied = [0u64; SUBDIVISIONS];
        for shape_box in boxes {
            for axis in 0..3 {
                assert!(
                    shape_box.min[axis] < shape_box.max[axis]
                        && usize::from(shape_box.max[axis]) <= SUBDIVISIONS,
                    "{shape_box:?} is empty or leaves the voxel"
                );
            }
            for z in shape_box.min[2]..shape_box.max[2] {
                for y in shape_box.min[1]..shape_box.max[1] {
                    for x in shape_box.min[0]..shape_box.max[0] {
                        occupied[usize::from(z)] |=
                            1 << (usize::from(y) * SUBDIVISIONS + usize::from(x));
                    }
                }
            }
        }
        let mut shape = Self {
            boxes: boxes.to_vec(),
            occupied,
            face_cover: [0; 6],
            connections: FaceConnections::NONE,
            exposed: Default::default(),
        };
        shape.measure();
        shape
    }

    fn is_filled(&self, cell: [usize; 3]) -> bool {
        self.occupied[cell[2]] >> (cell[1] * SUBDIVISIONS + cell[0]) & 1 == 1
    }

    fn measure(&mut self) {
        for face in FaceDirection::ALL {
            let (u_axis, v_axis) = face_axes(face);
            let index = usize::from(face.index());
            let boundary_layer = if face.is_positive() {
                SUBDIVISIONS - 1
            } else {
                0
            };
            let mut cover = 0u64;
            for v in 0..SUBDIVISIONS {
                for u in 0..SUBDIVISIONS {
                    let mut cell = [0; 3];
                    cell[face.axis()] = boundary_layer;
                    cell[u_axis] = u;
                    cell[v_axis] = v;
                    if self.is_filled(cell) {
                        cover |= 1 << (v * SUBDIVISIONS + u);
                    }
                }
            }
            self.face_cover[index] = cover;

            for layer in 0..SUBDIVISIONS {
                let mut mask = [false; SUBDIVISIONS * SUBDIVISIONS];
                for v in 0..SUBDIVISIONS {
                    for u in 0..SUBDIVISIONS {
                        let mut cell = [0; 3];
                        cell[face.axis()] = layer;
                        cell[u_axis] = u;
                        cell[v_axis] = v;
                        let mut beyond = cell;
                        let covered_beyond = if face.is_positive() {
                            layer + 1 < SUBDIVISIONS && {
                                beyond[face.axis()] += 1;
                                self.is_filled(beyond)
                            }
                        } else {
                            layer > 0 && {
                                beyond[face.axis()] -= 1;
                                self.is_filled(beyond)
                            }
                        };
                        mask[v * SUBDIVISIONS + u] = self.is_filled(cell) && !covered_beyond;
                    }
                }
                merge_rectangles(layer as u8, &mut mask, &mut self.exposed[index]);
            }
        }
        self.connections = self.measure_connections();
    }

    /// Joins faces through the empty sub-cells, which are linked to the ones next to them.
    fn measure_connections(&self) -> FaceConnections {
        let mut seen = [false; CELL_COUNT];
        let mut connections = 0u64;
        for start in 0..CELL_COUNT {
            let start_cell = cell_of(start);
            if seen[start] || self.is_filled(start_cell) {
                continue;
            }
            // Everything empty that can be reached from here, and which faces of the voxel it touches.
            let mut touched = 0u8;
            let mut pending = vec![start_cell];
            seen[start] = true;
            while let Some(cell) = pending.pop() {
                for face in FaceDirection::ALL {
                    let edge = if face.is_positive() {
                        SUBDIVISIONS - 1
                    } else {
                        0
                    };
                    if cell[face.axis()] == edge {
                        touched |= 1 << face.index();
                    }
                    let mut next = cell;
                    if face.is_positive() {
                        if cell[face.axis()] + 1 == SUBDIVISIONS {
                            continue;
                        }
                        next[face.axis()] += 1;
                    } else {
                        if cell[face.axis()] == 0 {
                            continue;
                        }
                        next[face.axis()] -= 1;
                    }
                    let index = index_of(next);
                    if !seen[index] && !self.is_filled(next) {
                        seen[index] = true;
                        pending.push(next);
                    }
                }
            }
            for first in FaceDirection::ALL {
                for second in FaceDirection::ALL {
                    if touched >> first.index() & 1 == 1 && touched >> second.index() & 1 == 1 {
                        connections |= FaceConnections::bit(first, second);
                    }
                }
            }
        }
        FaceConnections(connections)
    }

    /// The boxes the shape was made of: what something solid collides with.
    pub fn boxes(&self) -> &[ShapeBox] {
        &self.boxes
    }

    pub fn is_empty(&self) -> bool {
        self.occupied.iter().all(|layer| *layer == 0)
    }

    /// Whether the shape fills the whole voxel.
    pub fn is_full(&self) -> bool {
        self.occupied.iter().all(|layer| *layer == u64::MAX)
    }

    /// How many of the voxel's 512 sub-cells are filled.
    pub fn filled_cells(&self) -> u32 {
        self.occupied.iter().map(|layer| layer.count_ones()).sum()
    }

    /// Whether a sub-cell is filled. Coordinates are 0 to 7.
    pub fn fills(&self, cell: [usize; 3]) -> bool {
        self.is_filled(cell)
    }

    /// Which sub-faces of `face`, on the voxel's boundary, the shape covers: bit `v * 8 + u` for
    /// the sub-face at (u, v) along the face's axes ([`face_axes`]).
    pub fn face_cover(&self, face: FaceDirection) -> u64 {
        self.face_cover[usize::from(face.index())]
    }

    /// Whether the shape covers the whole of `face`, so that whatever is against that face of the
    /// voxel cannot be seen from the other side.
    pub fn covers_face(&self, face: FaceDirection) -> bool {
        self.face_cover(face) == u64::MAX
    }

    /// The parts of the surface that face `face`, as rectangles of sub-faces. Rectangles on the
    /// voxel's boundary are shown unless what is next door covers them.
    pub fn exposed(&self, face: FaceDirection) -> &[SubRect] {
        &self.exposed[usize::from(face.index())]
    }

    /// The faces joined through the empty part of the voxel.
    pub fn connections(&self) -> FaceConnections {
        self.connections
    }
}

fn index_of(cell: [usize; 3]) -> usize {
    (cell[2] * SUBDIVISIONS + cell[1]) * SUBDIVISIONS + cell[0]
}

fn cell_of(index: usize) -> [usize; 3] {
    [
        index % SUBDIVISIONS,
        index / SUBDIVISIONS % SUBDIVISIONS,
        index / (SUBDIVISIONS * SUBDIVISIONS),
    ]
}

/// Covers the set cells of `mask` (indexed `v * 8 + u`) with rectangles, clearing it as it goes.
fn merge_rectangles(layer: u8, mask: &mut [bool], rectangles: &mut Vec<SubRect>) {
    for v0 in 0..SUBDIVISIONS {
        let mut u0 = 0;
        while u0 < SUBDIVISIONS {
            if !mask[v0 * SUBDIVISIONS + u0] {
                u0 += 1;
                continue;
            }
            let mut width = 1;
            while u0 + width < SUBDIVISIONS && mask[v0 * SUBDIVISIONS + u0 + width] {
                width += 1;
            }
            let mut height = 1;
            'grow: while v0 + height < SUBDIVISIONS {
                for u in u0..u0 + width {
                    if !mask[(v0 + height) * SUBDIVISIONS + u] {
                        break 'grow;
                    }
                }
                height += 1;
            }
            for v in v0..v0 + height {
                for u in u0..u0 + width {
                    mask[v * SUBDIVISIONS + u] = false;
                }
            }
            rectangles.push(SubRect {
                layer,
                u: u0 as u8,
                v: v0 as u8,
                width: width as u8,
                height: height as u8,
            });
            u0 += width;
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const DOWN: FaceDirection = FaceDirection::NegativeY;
    const UP: FaceDirection = FaceDirection::PositiveY;

    #[test]
    fn nothing_and_everything() {
        let empty = Shape::empty();
        assert!(empty.is_empty() && !empty.is_full());
        assert_eq!(empty.filled_cells(), 0);
        assert!(
            FaceDirection::ALL
                .iter()
                .all(|&face| empty.exposed(face).is_empty())
        );
        assert!(
            FaceDirection::ALL
                .iter()
                .all(|&face| empty.face_cover(face) == 0)
        );
        assert_eq!(empty.connections(), FaceConnections::all());

        let full = Shape::full();
        assert!(full.is_full() && !full.is_empty());
        assert_eq!(full.filled_cells(), 512);
        for face in FaceDirection::ALL {
            assert!(full.covers_face(face));
            // One rectangle the size of a face, on the boundary, per direction.
            assert_eq!(
                full.exposed(face),
                [SubRect {
                    layer: if face.is_positive() { 7 } else { 0 },
                    u: 0,
                    v: 0,
                    width: 8,
                    height: 8
                }]
            );
            assert!(!full.connections().is_open(face));
        }
        assert_eq!(full.connections(), FaceConnections::NONE);
    }

    #[test]
    fn a_bottom_slab_fills_half_covers_only_its_bottom_and_opens_the_rest_to_each_other() {
        let slab = Shape::half(DOWN);
        assert_eq!(slab.filled_cells(), 256);
        assert!(slab.covers_face(DOWN));
        for face in [UP, FaceDirection::PositiveX, FaceDirection::NegativeZ] {
            assert!(!slab.covers_face(face), "{face:?}");
        }
        // The side faces are covered in their lower half only: rows v = 0..4 of an x face's (y, z).
        // On an x face u is y and v is z, so the covered sub-faces are the low half of every row.
        assert_eq!(
            slab.face_cover(FaceDirection::PositiveX),
            0x0F0F_0F0F_0F0F_0F0F
        );
        // Light can get in and out everywhere but through the bottom, and the open faces all join.
        let connections = slab.connections();
        assert!(!connections.is_open(DOWN));
        let open: Vec<FaceDirection> = FaceDirection::ALL
            .into_iter()
            .filter(|&face| face != DOWN)
            .collect();
        for &first in &open {
            for &second in &open {
                assert!(connections.connects(first, second), "{first:?} {second:?}");
            }
        }
        assert!(!connections.connects(DOWN, UP));
        // What shows: the top of the slab sits in the middle of the voxel, facing up.
        assert_eq!(
            slab.exposed(UP),
            [SubRect {
                layer: 3,
                u: 0,
                v: 0,
                width: 8,
                height: 8
            }]
        );
        assert!(!slab.exposed(UP)[0].on_boundary(UP));
        assert!(slab.exposed(DOWN)[0].on_boundary(DOWN));
    }

    #[test]
    fn a_top_slab_is_the_bottom_slab_upside_down() {
        let (top, bottom) = (Shape::half(UP), Shape::half(DOWN));
        assert!(top.covers_face(UP) && !top.covers_face(DOWN));
        assert_eq!(top.filled_cells(), bottom.filled_cells());
        assert!(!top.connections().is_open(UP));
        assert!(top.connections().connects(DOWN, FaceDirection::PositiveZ));
    }

    #[test]
    fn a_stair_covers_its_bottom_and_the_back_and_leaves_the_top_front_open() {
        let stair = Shape::stair(FaceDirection::PositiveX);
        assert_eq!(stair.filled_cells(), 256 + 128);
        assert!(stair.covers_face(DOWN));
        // The high end is the back of the stair, a wall; the low end is only the bottom half.
        assert!(stair.covers_face(FaceDirection::PositiveX));
        assert_eq!(stair.face_cover(FaceDirection::NegativeX).count_ones(), 32);
        assert!(!stair.covers_face(UP));
        // The top shows in two places: the step, and the tread.
        let tops = stair.exposed(UP);
        assert_eq!(tops.len(), 2);
        assert!(
            tops.iter()
                .all(|top| top.width as usize * top.height as usize == 32)
        );
        // The riser faces the way the stair descends and is half a voxel tall.
        let risers = stair.exposed(FaceDirection::NegativeX);
        assert_eq!(
            risers
                .iter()
                .map(|rectangle| usize::from(rectangle.width) * usize::from(rectangle.height))
                .sum::<usize>(),
            32 + 32
        );
        // Nothing is open to the bottom; the rest of the faces are joined through the empty corner.
        assert!(!stair.connections().is_open(DOWN));
        assert!(!stair.connections().is_open(FaceDirection::PositiveX));
        assert!(stair.connections().connects(UP, FaceDirection::NegativeX));
        assert!(!stair.connections().connects(UP, DOWN));
    }

    #[test]
    #[should_panic(expected = "rises toward a side")]
    fn a_stair_cannot_rise_toward_the_top() {
        Shape::stair(UP);
    }

    #[test]
    fn a_hollow_cell_joins_nothing_to_its_outside_and_a_tunnel_joins_two_faces() {
        // Four walls around a passage: a tunnel with only its two ends open.
        let end = SUBDIVISIONS as u8;
        let walls = [
            ShapeBox {
                min: [0, 0, 0],
                max: [end, 2, end],
            },
            ShapeBox {
                min: [0, 6, 0],
                max: [end, end, end],
            },
            ShapeBox {
                min: [0, 2, 0],
                max: [2, 6, end],
            },
            ShapeBox {
                min: [6, 2, 0],
                max: [end, 6, end],
            },
        ];
        let tunnel = Shape::from_boxes(&walls);
        let (north, south) = (FaceDirection::NegativeZ, FaceDirection::PositiveZ);
        assert!(tunnel.connections().connects(north, south));
        for blocked in [UP, DOWN, FaceDirection::PositiveX, FaceDirection::NegativeX] {
            assert!(!tunnel.connections().is_open(blocked), "{blocked:?}");
        }
        // A sealed hollow joins nothing.
        let mut sealed = walls.to_vec();
        sealed.push(ShapeBox {
            min: [0, 0, 0],
            max: [end, end, 2],
        });
        sealed.push(ShapeBox {
            min: [0, 0, 6],
            max: [end, end, end],
        });
        let sealed = Shape::from_boxes(&sealed);
        assert_eq!(sealed.connections(), FaceConnections::NONE);
        assert_eq!(
            sealed.filled_cells(),
            512 - 4 * 4 * 4,
            "only the cavity inside is empty"
        );
    }

    #[test]
    #[should_panic(expected = "empty or leaves the voxel")]
    fn a_box_that_leaves_the_voxel_is_refused() {
        Shape::from_boxes(&[ShapeBox {
            min: [0, 0, 0],
            max: [9, 1, 1],
        }]);
    }

    #[test]
    #[should_panic(expected = "empty or leaves the voxel")]
    fn an_empty_box_is_refused() {
        Shape::from_boxes(&[ShapeBox {
            min: [2, 0, 0],
            max: [2, 1, 1],
        }]);
    }

    #[test]
    fn face_axes_follow_the_meshers_convention() {
        assert_eq!(face_axes(FaceDirection::PositiveX), (1, 2));
        assert_eq!(face_axes(FaceDirection::NegativeY), (2, 0));
        assert_eq!(face_axes(FaceDirection::PositiveZ), (0, 1));
    }

    fn box_strategy() -> impl Strategy<Value = ShapeBox> {
        (0u8..8, 0u8..8, 0u8..8, 1u8..=8, 1u8..=8, 1u8..=8).prop_map(|(x, y, z, a, b, c)| {
            ShapeBox {
                min: [x, y, z],
                max: [
                    (x + a).min(8).max(x + 1),
                    (y + b).min(8).max(y + 1),
                    (z + c).min(8).max(z + 1),
                ],
            }
        })
    }

    proptest! {
        /// The rectangles of every direction cover exactly the sub-cells that are filled and have
        /// nothing filled behind them in that direction, each once.
        #[test]
        fn exposed_rectangles_are_exactly_the_filled_cells_with_nothing_behind(
            boxes in proptest::collection::vec(box_strategy(), 0..5),
        ) {
            let shape = Shape::from_boxes(&boxes);
            for face in FaceDirection::ALL {
                let (u_axis, v_axis) = face_axes(face);
                let mut covered = std::collections::HashMap::new();
                for rectangle in shape.exposed(face) {
                    for v in rectangle.v..rectangle.v + rectangle.height {
                        for u in rectangle.u..rectangle.u + rectangle.width {
                            let mut cell = [0usize; 3];
                            cell[face.axis()] = usize::from(rectangle.layer);
                            cell[u_axis] = usize::from(u);
                            cell[v_axis] = usize::from(v);
                            *covered.entry(cell).or_insert(0) += 1;
                        }
                    }
                }
                let [dx, dy, dz] = face.offset();
                for z in 0..8usize {
                    for y in 0..8usize {
                        for x in 0..8usize {
                            let cell = [x, y, z];
                            let behind = [x as i64 + dx, y as i64 + dy, z as i64 + dz];
                            let behind_filled = behind.iter().all(|&c| (0..8).contains(&c))
                                && shape.fills([behind[0] as usize, behind[1] as usize, behind[2] as usize]);
                            let expected = shape.fills(cell) && !behind_filled;
                            prop_assert_eq!(covered.get(&cell).copied().unwrap_or(0), usize::from(expected), "{:?} {:?}", face, cell);
                        }
                    }
                }
            }
        }

        /// A face is covered where a filled sub-cell lies on it, and the connections are symmetric.
        #[test]
        fn covers_and_connections_follow_the_filled_cells(boxes in proptest::collection::vec(box_strategy(), 0..5)) {
            let shape = Shape::from_boxes(&boxes);
            for face in FaceDirection::ALL {
                let (u_axis, v_axis) = face_axes(face);
                let layer = if face.is_positive() { 7 } else { 0 };
                for v in 0..8 {
                    for u in 0..8 {
                        let mut cell = [0usize; 3];
                        cell[face.axis()] = layer;
                        cell[u_axis] = u;
                        cell[v_axis] = v;
                        prop_assert_eq!(shape.face_cover(face) >> (v * 8 + u) & 1 == 1, shape.fills(cell));
                    }
                }
                for other in FaceDirection::ALL {
                    prop_assert_eq!(
                        shape.connections().connects(face, other),
                        shape.connections().connects(other, face)
                    );
                }
                // A face that is not fully covered has an empty sub-cell on it, which belongs to some
                // empty region that touches the face: so it is open.
                prop_assert_eq!(shape.connections().is_open(face), !shape.covers_face(face));
            }
        }
    }
}
