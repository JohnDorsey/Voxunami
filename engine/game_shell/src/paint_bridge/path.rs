//! A pointer's places on a picture made continuous: a stroke that goes over the edge of one face onto the next is one stroke.
//!
//! The picture of a face repeats on every voxel (the renderer takes the fraction of the coordinate), and on a flat run of voxels that all face one way the coordinate goes on
//! from one voxel to the next. The path counts pixels in that coordinate, from the first voxel of its segment, so that the tool can draw a line across the join and wrap it
//! onto the picture. Where the surface turns (another face, another plane, another picture or turn of it, another grid) the coordinates do not go on: the path starts a new
//! segment, which the tool treats as a new stroke.

use block_models::{FacePicture, TileRef};
use voxel_space::{FaceDirection, GridId, VoxelCoordinate};

/// One place of the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathPoint {
    pub x: i64,
    pub y: i64,
    pub segment: u64,
}

/// What must stay the same for two places to be on one run of voxels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Surface {
    grid: GridId,
    face: FaceDirection,
    /// The voxel's coordinate along the face's axis: the plane the face is in.
    plane: i128,
    tile: TileRef,
    across: FaceDirection,
    up: FaceDirection,
}

#[derive(Debug, Default)]
pub struct PaintPath {
    segment: u64,
    current: Option<(Surface, VoxelCoordinate)>,
}

fn voxel_axes(voxel: VoxelCoordinate) -> [i128; 3] {
    [voxel.x(), voxel.y(), voxel.z()]
}

impl PaintPath {
    /// The pointer is on `voxel` of `grid`, at `point` of it (its own coordinates, 0 to 1), on the face `face` that has `picture`, whose pixels are `size` across.
    pub fn place(
        &mut self,
        grid: GridId,
        voxel: VoxelCoordinate,
        face: FaceDirection,
        point: [f64; 3],
        picture: FacePicture,
        size: u32,
    ) -> PathPoint {
        let surface = Surface {
            grid,
            face,
            plane: voxel_axes(voxel)[face.axis()],
            tile: picture.tile,
            across: picture.across,
            up: picture.up,
        };
        let anchor = match self.current {
            Some((current, anchor)) if current == surface => anchor,
            _ => {
                self.segment += 1;
                self.current = Some((surface, voxel));
                voxel
            }
        };
        let (voxel, anchor) = (voxel_axes(voxel), voxel_axes(anchor));
        // How far along a direction the point is, from the anchor voxel's own corner, in voxels: the coordinate the renderer takes the fraction of.
        let along = |direction: FaceDirection| {
            let axis = direction.axis();
            let distance = (voxel[axis] - anchor[axis]) as f64 + point[axis];
            if direction.is_positive() {
                distance
            } else {
                -distance
            }
        };
        let size = f64::from(size);
        PathPoint {
            x: (along(picture.across) * size).floor() as i64,
            y: (-along(picture.up) * size).floor() as i64,
            segment: self.segment,
        }
    }

    /// The pointer is over nothing that can be painted: the next place starts a new segment.
    pub fn lift(&mut self) {
        self.current = None;
    }
}
