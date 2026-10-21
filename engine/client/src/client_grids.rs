use std::collections::BTreeMap;
use std::sync::Arc;

use protocol::{GridChange, GridState};
use voxel_grid::{Brick, Grid};
use voxel_space::{GridId, GridPose, LevelIndex};

use crate::error::ClientError;

/// A ship as a client knows it: where it is and the bricks the server sent for it. Nothing generates a ship's voxels, so these are all
/// of them.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientGrid {
    pub level: LevelIndex,
    pub pose: GridPose,
    pub voxels: Grid,
}

/// The ships a client has been told of, built only from the stream.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientGrids {
    grids: BTreeMap<GridId, ClientGrid>,
}

impl ClientGrids {
    pub fn get(&self, id: GridId) -> Option<&ClientGrid> {
        self.grids.get(&id)
    }

    pub fn len(&self) -> usize {
        self.grids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.grids.is_empty()
    }

    /// Every ship, in order of id.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (GridId, &ClientGrid)> {
        self.grids.iter().map(|(id, grid)| (*id, grid))
    }

    pub(crate) fn spawn(
        &mut self,
        states: Vec<GridState>,
        make_voxels: impl Fn() -> Grid,
    ) -> Result<(), ClientError> {
        for state in states {
            if self.grids.contains_key(&state.id) {
                return Err(ClientError::GridAlreadyKnown(state.id));
            }
            self.grids.insert(
                state.id,
                ClientGrid {
                    level: state.level,
                    pose: state.pose,
                    voxels: make_voxels(),
                },
            );
        }
        Ok(())
    }

    pub(crate) fn change(&mut self, changes: Vec<GridChange>) -> Result<(), ClientError> {
        for change in changes {
            self.grids
                .get_mut(&change.id)
                .ok_or(ClientError::UnknownGrid(change.id))?
                .pose = change.pose;
        }
        Ok(())
    }

    pub(crate) fn despawn(&mut self, ids: Vec<GridId>) -> Result<(), ClientError> {
        for id in ids {
            self.grids.remove(&id).ok_or(ClientError::UnknownGrid(id))?;
        }
        Ok(())
    }

    pub(crate) fn get_mut(&mut self, id: GridId) -> Result<&mut ClientGrid, ClientError> {
        self.grids.get_mut(&id).ok_or(ClientError::UnknownGrid(id))
    }
}

impl ClientGrid {
    pub(crate) fn insert_brick(&mut self, brick: voxel_space::BrickCoordinate, contents: Brick) {
        self.voxels
            .insert_brick(brick, Arc::new(contents))
            .expect("the translated brick has one storage per layout channel");
    }
}
