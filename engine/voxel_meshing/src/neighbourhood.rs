//! The voxels a brick's mesh depends on: its own, and the layer one voxel thick all round it.

use voxel_space::{BRICK_EDGE_LENGTH, LocalVoxelIndex};

use crate::mesher::BrickChannels;

/// How many bricks touch a brick: six across its faces, twelve along its edges, eight at its corners.
pub const NEIGHBOUR_COUNT: usize = 26;

/// Where each of the bricks that touch a brick is, in bricks from it: the six across its faces first, in the order of [`voxel_space::FaceDirection::index`], then the twelve
/// along its edges, then the eight at its corners.
pub const NEIGHBOUR_OFFSETS: [[i8; 3]; NEIGHBOUR_COUNT] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, -1, 0],
    [1, -1, 0],
    [-1, 1, 0],
    [1, 1, 0],
    [-1, 0, -1],
    [1, 0, -1],
    [-1, 0, 1],
    [1, 0, 1],
    [0, -1, -1],
    [0, 1, -1],
    [0, -1, 1],
    [0, 1, 1],
    [-1, -1, -1],
    [1, -1, -1],
    [-1, 1, -1],
    [1, 1, -1],
    [-1, -1, 1],
    [1, -1, 1],
    [-1, 1, 1],
    [1, 1, 1],
];

/// Which of the [`NEIGHBOUR_OFFSETS`] this is, or `None` for the brick itself and for anything farther.
pub fn neighbour_slot(offset: [i8; 3]) -> Option<usize> {
    NEIGHBOUR_OFFSETS.iter().position(|&known| known == offset)
}

/// The voxels a brick's mesh depends on: its own, and the bricks that touch it.
///
/// What a face looks at is in the brick or across a face of it, and the shade of a corner of a face (see [`crate::corner_occlusion`]) reads the cells beside that one, which
/// can be in a brick that touches only along an edge or at a corner of this one. `neighbours` is indexed as [`NEIGHBOUR_OFFSETS`]: so by [`voxel_space::FaceDirection::index`]
/// for the first six. A missing neighbour is treated as empty, so faces next to it are drawn and nothing in it shades; callers that stream bricks should mesh only once all
/// twenty-six exist, or remesh when they arrive.
///
/// Only the blocks, states and orientations of the edge and corner bricks are read, never their light, so a caller need not work it out for them.
#[derive(Clone, Copy, Debug)]
pub struct BrickNeighbourhood<'a> {
    pub center: BrickChannels<'a>,
    pub neighbours: [Option<BrickChannels<'a>>; NEIGHBOUR_COUNT],
}

impl<'a> BrickNeighbourhood<'a> {
    /// A brick with only the bricks across its faces (indexed by [`voxel_space::FaceDirection::index`]), for the callers that look at nothing else: the edge and corner bricks
    /// are missing.
    pub fn with_face_neighbours(
        center: BrickChannels<'a>,
        faces: [Option<BrickChannels<'a>>; 6],
    ) -> Self {
        let mut neighbours = [None; NEIGHBOUR_COUNT];
        neighbours[..6].copy_from_slice(&faces);
        Self { center, neighbours }
    }

    /// The channels of the brick that holds the voxel at `local`, and the voxel's index in it: `local` is in voxels from the brick's lowest corner, and each component is from
    /// -1 (the layer of the brick before it) to the brick's edge length (the layer of the brick after it). `None` if that brick is missing.
    ///
    /// Panics if a component is farther out than that.
    pub fn voxel_at(&self, local: [i32; 3]) -> Option<(&BrickChannels<'a>, LocalVoxelIndex)> {
        let edge = BRICK_EDGE_LENGTH as i32;
        let mut offset = [0i8; 3];
        let mut inside = [0u8; 3];
        for axis in 0..3 {
            let component = local[axis];
            assert!(
                (-1..=edge).contains(&component),
                "the voxel {local:?} is outside the brick and the layer of voxels round it"
            );
            offset[axis] = match component {
                ..0 => -1,
                0..32 => 0,
                _ => 1,
            };
            inside[axis] = component.rem_euclid(edge) as u8;
        }
        let channels = if offset == [0; 3] {
            Some(&self.center)
        } else {
            let slot = neighbour_slot(offset).expect("every offset of a touching brick is known");
            self.neighbours[slot].as_ref()
        };
        channels.map(|channels| {
            (
                channels,
                LocalVoxelIndex::new(inside[0], inside[1], inside[2]),
            )
        })
    }
}

#[cfg(test)]
mod tests;
