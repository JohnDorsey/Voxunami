use crate::{GridId, Orientation, WorldPosition};

/// Where an entity is and which way it faces: in the frame of a grid, whose coordinates it uses.
///
/// The frame is the root grid of a level for anything in the world, and will be a ship's grid for a
/// player aboard or a cart on a rail: the pose is always local to its frame, so it does not change as
/// the frame moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityPose {
    pub grid: GridId,
    pub position: WorldPosition,
    pub orientation: Orientation,
}

impl EntityPose {
    pub fn new(grid: GridId, position: WorldPosition, orientation: Orientation) -> Self {
        Self {
            grid,
            position,
            orientation,
        }
    }
}
