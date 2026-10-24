use std::collections::BTreeMap;
use std::sync::Arc;

use protocol::{GridChange, GridState};
use terrain::GeneratorId;
use voxel_grid::{Brick, Grid};
use voxel_space::{BrickBox, GridId, GridPose, LevelIndex};

use crate::error::ClientError;

/// What generates the bricks of a ship that the server does not send, as this client numbers its generators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientGridTerrain {
    pub generator: GeneratorId,
    /// The bricks it makes, in the ship's own coordinates.
    pub extent: BrickBox,
}

/// A ship as a client knows it: where it is and the bricks the server sent for it. For a ship that nothing generates these are all of its
/// bricks; for a ship with terrain they are the edited ones, and the others are what the generator makes.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientGrid {
    pub level: LevelIndex,
    pub pose: GridPose,
    pub voxels: Grid,
    pub terrain: Option<ClientGridTerrain>,
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

    /// Adds the ships the server told of. `translate_generator` turns a number of the server's table of generators into this client's.
    pub(crate) fn spawn(
        &mut self,
        states: Vec<GridState>,
        make_voxels: impl Fn() -> Grid,
        translate_generator: impl Fn(u32) -> Result<GeneratorId, ClientError>,
    ) -> Result<(), ClientError> {
        for state in states {
            if self.grids.contains_key(&state.id) {
                return Err(ClientError::GridAlreadyKnown(state.id));
            }
            let terrain = state
                .terrain
                .map(|terrain| {
                    Ok::<_, ClientError>(ClientGridTerrain {
                        generator: translate_generator(terrain.generator)?,
                        extent: terrain.extent,
                    })
                })
                .transpose()?;
            self.grids.insert(
                state.id,
                ClientGrid {
                    level: state.level,
                    pose: state.pose,
                    voxels: make_voxels(),
                    terrain,
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

#[cfg(test)]
mod tests {
    use protocol::GridTerrainState;
    use voxel_grid::{ChannelDefinition, GridLayout};
    use voxel_space::BrickCoordinate;

    use super::*;

    fn voxels() -> Grid {
        Grid::new(Arc::new(
            GridLayout::new(vec![ChannelDefinition::number(
                "test:marker".parse().unwrap(),
                0,
            )])
            .unwrap(),
        ))
    }

    fn state(id: u64, terrain: Option<GridTerrainState>) -> GridState {
        GridState {
            id: GridId::new(GridId::FIRST_ALLOCATED + id),
            level: LevelIndex::MAIN,
            pose: GridPose::identity(),
            terrain,
        }
    }

    #[test]
    fn the_generator_of_a_ship_is_the_number_this_client_has_for_it_not_the_servers() {
        let extent = BrickBox::around(BrickCoordinate::ORIGIN, 3);
        let mut grids = ClientGrids::default();
        grids
            .spawn(
                vec![
                    state(
                        0,
                        Some(GridTerrainState {
                            generator: 3,
                            extent,
                        }),
                    ),
                    state(1, None),
                ],
                voxels,
                // (This client numbers the generators ten higher than the server.)
                |source| Ok(GeneratorId::from_raw(source + 10)),
            )
            .unwrap();
        let planet = grids.get(GridId::new(GridId::FIRST_ALLOCATED)).unwrap();
        assert_eq!(
            planet.terrain,
            Some(ClientGridTerrain {
                generator: GeneratorId::from_raw(13),
                extent
            })
        );
        assert_eq!(
            grids
                .get(GridId::new(GridId::FIRST_ALLOCATED + 1))
                .unwrap()
                .terrain,
            None
        );
    }

    #[test]
    fn a_ship_whose_generator_cannot_be_numbered_is_not_added() {
        let mut grids = ClientGrids::default();
        let result = grids.spawn(
            vec![state(
                0,
                Some(GridTerrainState {
                    generator: 99,
                    extent: BrickBox::around(BrickCoordinate::ORIGIN, 1),
                }),
            )],
            voxels,
            |_| {
                Err(ClientError::UnexpectedMessage(
                    "a generator it does not have",
                ))
            },
        );
        assert!(result.is_err());
        assert!(grids.is_empty());
    }
}
