//! How a block is turned when a player places it, from where the player is and what they place it against.
//!
//! This is the rule of Minecraft, for the two kinds of block a block's definition can say it is (see `block_models::Placement`):
//!
//! - A block with *one front face*, like a piston, a dispenser or an observer: the front faces the player. It faces the player's side of the block, the
//!   opposite of the way they look along the ground; but a block placed close beside the player (within two voxels, along both horizontal axes) and
//!   more than two voxels under their eyes faces up, and one placed at the height of their eyes or above them faces down.
//! - A block with *a line of radial symmetry*, like a log: the line runs along the face the block is placed against, whichever way the player
//!   looks.
//!
//! The answer is a turn of the lattice, as its number in `LatticeRotation::all`: the number every voxel keeps in its orientation channel, whatever
//! its block (see `base_blocks::orientation_channel_identifier`).

use block_models::Placement;
use voxel_space::{
    BRICK_EDGE_LENGTH, BrickCoordinate, FaceDirection, GridPose, LatticeRotation, Orientation,
    VoxelCoordinate, WorldPosition,
};
use world_storage::GridFrame;

/// Where the player is and which way they look, in the axes of the grid the block is placed in (a ship's own coordinates, for a block placed on a
/// ship).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacerView {
    /// Where their eyes are, in voxels.
    pub eyes: [f64; 3],
    /// The way they look, a unit vector.
    pub forward: [f32; 3],
}

impl PlacerView {
    /// The view of a player whose eyes are at `eyes` in the world and who looks as `view` turns, seen from the grid that sits in the world as `frame`
    /// says: in a ship's own axes for a ship. `None` if that is not a place the world can hold.
    pub fn in_frame(frame: &GridFrame, eyes: WorldPosition, view: Orientation) -> Option<Self> {
        let (eyes, view) = match frame {
            GridFrame::Level(_) => (eyes, view),
            GridFrame::Ship { pose, .. } => pose.place_in_grid(eyes, view)?,
        };
        let edge = BRICK_EDGE_LENGTH as f64;
        let (brick, offset) = (eyes.brick(), eyes.offset());
        let nowhere = WorldPosition::new(BrickCoordinate::ORIGIN, [0.0; 3]);
        Some(Self {
            eyes: [
                brick.x() as f64 * edge + f64::from(offset[0]),
                brick.y() as f64 * edge + f64::from(offset[1]),
                brick.z() as f64 * edge + f64::from(offset[2]),
            ],
            forward: GridPose::new(nowhere, view).direction_to_world([0.0, 0.0, -1.0]),
        })
    }
}

/// The side of the voxel `clicked` that the voxel `placed` is on (the face the player clicked): `None` unless they are next to each other.
pub fn face_between(clicked: VoxelCoordinate, placed: VoxelCoordinate) -> Option<FaceDirection> {
    let step = [
        placed.x() - clicked.x(),
        placed.y() - clicked.y(),
        placed.z() - clicked.z(),
    ];
    FaceDirection::ALL
        .into_iter()
        .find(|face| face.offset() == step)
}

/// The face a block with a front is to have turned to the player: see the module.
fn front_toward(at: VoxelCoordinate, view: &PlacerView) -> FaceDirection {
    let close_in_x = (view.eyes[0] - at.x() as f64).abs() < 2.0;
    let close_in_z = (view.eyes[2] - at.z() as f64).abs() < 2.0;
    if close_in_x && close_in_z {
        if view.eyes[1] - at.y() as f64 > 2.0 {
            return FaceDirection::PositiveY;
        }
        if at.y() as f64 - view.eyes[1] > 0.0 {
            return FaceDirection::NegativeY;
        }
    }
    // The side of the block they are on: the opposite of the way they look along the ground (along x if they look more that way than along z).
    let [forward_x, _, forward_z] = view.forward;
    if forward_x.abs() > forward_z.abs() {
        if forward_x > 0.0 {
            FaceDirection::NegativeX
        } else {
            FaceDirection::PositiveX
        }
    } else if forward_z > 0.0 {
        FaceDirection::NegativeZ
    } else {
        FaceDirection::PositiveZ
    }
}

/// The turn that carries the face `from` to the face `to`: the first of the 24 that does (so that the same placement always gives the same number).
pub fn turn_carrying(from: FaceDirection, to: FaceDirection) -> LatticeRotation {
    LatticeRotation::all()
        .into_iter()
        .find(|rotation| rotation.apply_face(from) == to)
        .expect("a turn carries any face to any face")
}

/// The orientation (the number of a turn) a block of this placement is given when it is put at `at`, against the face `against` of the block next to it
/// that was clicked (`None` if the request did not say), by a player who is where `view` says.
pub fn orientation_of_placement(
    placement: Placement,
    at: VoxelCoordinate,
    against: Option<FaceDirection>,
    view: &PlacerView,
) -> u32 {
    let turn = match placement {
        Placement::Fixed => LatticeRotation::IDENTITY,
        Placement::FrontToPlayer(front) => turn_carrying(front, front_toward(at, view)),
        Placement::AlongPlacedFace(axis) => match against {
            Some(face) => turn_carrying(axis, face),
            None => LatticeRotation::IDENTITY,
        },
    };
    u32::from(turn.index())
}

#[cfg(test)]
mod tests {
    use block_models::{BlockDefinition, BlockTable};

    use super::*;

    fn at() -> VoxelCoordinate {
        VoxelCoordinate::new(10, 5, 10)
    }

    /// A player at this place looking this way.
    fn looking(eyes: [f64; 3], forward: [f32; 3]) -> PlacerView {
        PlacerView { eyes, forward }
    }

    /// The table of a piston (its front to the east as it is made), a log (its line up and down) and a plain block.
    fn table() -> BlockTable {
        let piston = BlockDefinition::cube([0.5; 3]).with_front_face(FaceDirection::PositiveX);
        let log = BlockDefinition::cube([0.5; 3]).with_axis_face(FaceDirection::PositiveY);
        let plain = BlockDefinition::cube([0.5; 3]);
        BlockTable::from_definitions(&[Some(&piston), Some(&log), Some(&plain)])
    }

    fn front_after_placing(
        view: &PlacerView,
        at: VoxelCoordinate,
        against: Option<FaceDirection>,
    ) -> FaceDirection {
        let table = table();
        let orientation = orientation_of_placement(table.placement(0, 0), at, against, view);
        table.front_face(0, 0, orientation).unwrap()
    }

    #[test]
    fn a_block_with_a_front_faces_the_player_who_looks_along_the_ground_at_the_place_it_goes() {
        // Eyes level with the block, four away, looking toward it: east, west, north (-z) and south.
        let cases = [
            ([6.0, 5.5, 10.5], [1.0, 0.0, 0.0], FaceDirection::NegativeX),
            (
                [14.0, 5.5, 10.5],
                [-1.0, 0.0, 0.0],
                FaceDirection::PositiveX,
            ),
            (
                [10.5, 5.5, 14.0],
                [0.0, 0.0, -1.0],
                FaceDirection::PositiveZ,
            ),
            ([10.5, 5.5, 6.0], [0.0, 0.0, 1.0], FaceDirection::NegativeZ),
        ];
        for (eyes, forward, wanted) in cases {
            assert_eq!(
                front_after_placing(&looking(eyes, forward), at(), None),
                wanted,
                "looking {forward:?}"
            );
        }
        // Looking a little up or down, or a little off the line, makes no difference: the way along the ground that they look most.
        let tilted = looking([6.0, 5.5, 10.5], [0.9, -0.3, 0.3]);
        assert_eq!(
            front_after_placing(&tilted, at(), None),
            FaceDirection::NegativeX
        );
        let mostly_south = looking([10.5, 5.5, 6.0], [0.4, 0.2, 0.9]);
        assert_eq!(
            front_after_placing(&mostly_south, at(), None),
            FaceDirection::NegativeZ
        );
    }

    #[test]
    fn a_block_with_a_front_put_right_under_the_player_faces_up_and_one_at_or_above_their_eyes_faces_down()
     {
        let near = |eye_height: f64| looking([10.5, eye_height, 10.5], [1.0, 0.0, 0.0]);
        // More than two under the eyes, close beside them in both directions: up.
        assert_eq!(
            front_after_placing(&near(8.0), at(), None),
            FaceDirection::PositiveY
        );
        assert_eq!(
            front_after_placing(&near(7.01), at(), None),
            FaceDirection::PositiveY
        );
        // Within two under: the usual rule, whichever way they look.
        assert_eq!(
            front_after_placing(&near(7.0), at(), None),
            FaceDirection::NegativeX
        );
        // Above the eyes (the block's lower edge is above them): down.
        assert_eq!(
            front_after_placing(&near(4.9), at(), None),
            FaceDirection::NegativeY
        );
        // Level with the lower edge of the block: not above.
        assert_eq!(
            front_after_placing(&near(5.0), at(), None),
            FaceDirection::NegativeX
        );
        // Far off along either ground axis, it is the usual rule, whatever the height.
        let far_in_x = looking([13.0, 9.0, 10.5], [1.0, 0.0, 0.0]);
        assert_eq!(
            front_after_placing(&far_in_x, at(), None),
            FaceDirection::NegativeX
        );
        let far_in_z = looking([10.5, 9.0, 3.0], [0.0, 0.0, 1.0]);
        assert_eq!(
            front_after_placing(&far_in_z, at(), None),
            FaceDirection::NegativeZ
        );
    }

    #[test]
    fn a_block_with_a_line_of_symmetry_has_it_along_the_face_it_is_put_against_whichever_way_the_player_looks()
     {
        let table = table();
        let any_view = looking([6.0, 5.5, 10.5], [1.0, 0.0, 0.0]);
        for face in FaceDirection::ALL {
            let orientation =
                orientation_of_placement(table.placement(1, 0), at(), Some(face), &any_view);
            // The line runs along the face's axis: one end of it is the face or its opposite (here, the end the block was made with, turned).
            assert_eq!(table.axis_face(1, 0, orientation), Some(face), "{face:?}");
        }
        // Put on the top of something, a log stands up: it is as it was made.
        let on_top = orientation_of_placement(
            table.placement(1, 0),
            at(),
            Some(FaceDirection::PositiveY),
            &any_view,
        );
        assert_eq!(on_top, 0);
        // Not told what it was put against, it is as it was made.
        assert_eq!(
            orientation_of_placement(table.placement(1, 0), at(), None, &any_view),
            0
        );
    }

    #[test]
    fn a_block_with_neither_is_not_turned() {
        let table = table();
        let view = looking([6.0, 5.5, 10.5], [1.0, 0.0, 0.0]);
        for against in [None, Some(FaceDirection::NegativeZ)] {
            assert_eq!(
                orientation_of_placement(table.placement(2, 0), at(), against, &view),
                0
            );
        }
    }

    #[test]
    fn the_face_that_was_clicked_is_the_side_the_new_block_is_on() {
        let clicked = VoxelCoordinate::new(3, 4, 5);
        for face in FaceDirection::ALL {
            let step = face.offset();
            let placed = VoxelCoordinate::new(3 + step[0], 4 + step[1], 5 + step[2]);
            assert_eq!(face_between(clicked, placed), Some(face));
        }
        assert_eq!(face_between(clicked, clicked), None);
        assert_eq!(face_between(clicked, VoxelCoordinate::new(5, 4, 5)), None);
        assert_eq!(face_between(clicked, VoxelCoordinate::new(4, 5, 5)), None);
    }
}
