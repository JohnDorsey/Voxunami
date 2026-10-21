//! Meshes the bricks of ships: the same meshing as the terrain's, from the voxels a client was sent, a few bricks at a time on the thread
//! that asks (a ship is small beside the world around the viewer, and nothing has to be generated).
//!
//! Where [`crate::WorldMesher`] loads bricks around a viewer, this is told which bricks changed ([`GridMesher::brick_changed`]) and meshes
//! those, and their neighbours whose faces against them may have changed, when [`GridMesher::update`] is called. A brick that is not in
//! the ship is empty, which is what a ship's voxels are where nothing was sent, so a face against it is drawn.

use std::collections::BTreeSet;
use std::sync::Arc;

use block_models::BlockTable;
use voxel_grid::{Brick, ChannelStorage, Grid};
use voxel_meshing::{BrickNeighbourhood, mesh_brick};
use voxel_space::{BrickCoordinate, FaceDirection, GridId};

use crate::mesher::{MeshUpdate, ViewChannels, brick_channels};

pub struct GridMesher {
    table: Arc<BlockTable>,
    channels: ViewChannels,
    /// The bricks whose mesh is out of date, in order of ship and brick.
    dirty: BTreeSet<(GridId, BrickCoordinate)>,
    /// The bricks the renderer holds a mesh for.
    displayed: BTreeSet<(GridId, BrickCoordinate)>,
}

impl std::fmt::Debug for GridMesher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GridMesher")
            .field("dirty", &self.dirty.len())
            .field("displayed", &self.displayed.len())
            .finish_non_exhaustive()
    }
}

impl GridMesher {
    pub fn new(channels: ViewChannels, table: Arc<BlockTable>) -> Self {
        Self {
            table,
            channels,
            dirty: BTreeSet::new(),
            displayed: BTreeSet::new(),
        }
    }

    /// Bricks waiting to be meshed.
    pub fn pending(&self) -> usize {
        self.dirty.len()
    }

    /// Tells the mesher a brick of a ship arrived or changed (or went), so it and the bricks around it are meshed again.
    pub fn brick_changed(&mut self, grid: GridId, brick: BrickCoordinate) {
        self.dirty.insert((grid, brick));
        for face in FaceDirection::ALL {
            if let Some(neighbour) = brick.neighbour(face) {
                self.dirty.insert((grid, neighbour));
            }
        }
    }

    /// Forgets a ship that is gone. Nothing is returned for the meshes it had: whoever draws them drops the ship whole.
    pub fn forget_grid(&mut self, grid: GridId) {
        self.dirty.retain(|(held, _)| *held != grid);
        self.displayed.retain(|(held, _)| *held != grid);
    }

    /// Changes what is drawn from (as when a game changes the scales of its light): every brick shown is meshed again.
    pub fn set_channels(&mut self, channels: ViewChannels) {
        if channels == self.channels {
            return;
        }
        self.channels = channels;
        self.dirty.extend(self.displayed.iter().copied());
    }

    /// Meshes up to `budget` of the bricks that are waiting (not counting those that turn out not to be there), in order of ship and brick, reading from the voxels `grids` gives for a
    /// ship. Returns what the renderer must change.
    pub fn update<'g>(
        &mut self,
        grids: impl Fn(GridId) -> Option<&'g Grid>,
        budget: usize,
    ) -> Vec<MeshUpdate> {
        let mut updates = Vec::new();
        let mut meshed = 0;
        while meshed < budget {
            let Some((grid, coordinate)) = self.dirty.pop_first() else {
                break;
            };
            // Only meshing a brick that is there counts against the budget: noticing that one is not is free.
            let mesh = grids(grid)
                .and_then(|voxels| voxels.brick(coordinate).map(|brick| (voxels, brick)))
                .map(|(voxels, brick)| {
                    meshed += 1;
                    self.mesh(voxels, brick, coordinate)
                });
            match mesh {
                Some(mesh) if !mesh.is_empty() => {
                    self.displayed.insert((grid, coordinate));
                    updates.push(MeshUpdate::UploadGridBrick {
                        grid,
                        coordinate,
                        mesh,
                    });
                }
                _ => {
                    if self.displayed.remove(&(grid, coordinate)) {
                        updates.push(MeshUpdate::RemoveGridBrick { grid, coordinate });
                    }
                }
            }
        }
        updates
    }

    fn mesh(
        &self,
        voxels: &Grid,
        brick: &Arc<Brick>,
        coordinate: BrickCoordinate,
    ) -> voxel_meshing::PackedBrickMesh {
        let center_light = self.channels.bulk_brightness(brick, coordinate);
        let neighbours: [Option<(Arc<Brick>, Option<ChannelStorage>)>; 6] =
            std::array::from_fn(|face| {
                let neighbour = coordinate.neighbour(FaceDirection::ALL[face])?;
                let held = voxels.brick(neighbour)?;
                let light = self.channels.bulk_brightness(held, neighbour);
                Some((Arc::clone(held), light))
            });
        let neighbourhood = BrickNeighbourhood {
            center: brick_channels(brick, center_light.as_ref(), &self.channels),
            neighbours: std::array::from_fn(|face| {
                neighbours[face]
                    .as_ref()
                    .map(|(held, light)| brick_channels(held, light.as_ref(), &self.channels))
            }),
        };
        mesh_brick(&neighbourhood, &self.table).pack()
    }
}

#[cfg(test)]
mod tests {
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_meshing::PackedBrickMesh;
    use voxel_space::VoxelCoordinate;

    use super::*;

    const SHIP: GridId = GridId::new(GridId::FIRST_ALLOCATED);
    const OTHER: GridId = GridId::new(GridId::FIRST_ALLOCATED + 1);

    struct Fixture {
        mesher: GridMesher,
        ship: Grid,
        material: voxel_grid::ChannelIndex,
    }

    fn fixture() -> Fixture {
        let layout = StandardTestLayout::default();
        let mut opaque = vec![true; 8];
        opaque[0] = false;
        Fixture {
            mesher: GridMesher::new(
                ViewChannels::new(layout.material, None, None),
                Arc::new(BlockTable::of_cubes(&opaque)),
            ),
            ship: Grid::new(Arc::clone(&layout.layout)),
            material: layout.material,
        }
    }

    impl Fixture {
        fn set(&mut self, at: [i64; 3], value: u32) {
            let voxel = VoxelCoordinate::new(at[0], at[1], at[2]);
            self.ship.set(self.material, voxel, value);
            self.mesher.brick_changed(SHIP, voxel.split().0);
        }

        fn update(&mut self, budget: usize) -> Vec<MeshUpdate> {
            let ship = &self.ship;
            self.mesher
                .update(|grid| (grid == SHIP).then_some(ship), budget)
        }

        /// The mesh of a brick of the ship as it should be: meshed alone with the neighbours the ship has.
        fn expected(&self, coordinate: BrickCoordinate) -> PackedBrickMesh {
            let fresh =
                GridMesher::new(self.mesher.channels.clone(), Arc::clone(&self.mesher.table));
            fresh.mesh(&self.ship, self.ship.brick(coordinate).unwrap(), coordinate)
        }
    }

    fn uploaded(updates: &[MeshUpdate]) -> Vec<(GridId, BrickCoordinate, usize)> {
        updates
            .iter()
            .filter_map(|update| match update {
                MeshUpdate::UploadGridBrick {
                    grid,
                    coordinate,
                    mesh,
                } => Some((*grid, *coordinate, mesh.quads.len())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_brick_that_changed_is_meshed_from_what_it_holds_and_only_when_asked_for() {
        let mut fixture = fixture();
        fixture.set([1, 2, 3], 1);
        assert_eq!(
            fixture.mesher.pending(),
            7,
            "the brick and its six neighbours"
        );
        let updates = fixture.update(100);
        // Only the brick that exists has a mesh; its neighbours have nothing to draw.
        assert_eq!(uploaded(&updates), [(SHIP, BrickCoordinate::ORIGIN, 6)]);
        let MeshUpdate::UploadGridBrick { mesh, .. } = &updates[0] else {
            panic!("{updates:?}");
        };
        assert_eq!(*mesh, fixture.expected(BrickCoordinate::ORIGIN));
        // Nothing is waiting any more, so nothing is done again.
        assert_eq!(fixture.mesher.pending(), 0);
        assert!(fixture.update(100).is_empty());
    }

    #[test]
    fn the_faces_between_two_bricks_of_a_ship_are_hidden_and_a_change_next_door_meshes_a_brick_again()
     {
        let mut fixture = fixture();
        // Two blocks that touch across the boundary between two bricks.
        fixture.set([31, 5, 5], 1);
        let alone = uploaded(&fixture.update(100));
        assert_eq!(alone, [(SHIP, BrickCoordinate::ORIGIN, 6)]);
        fixture.set([32, 5, 5], 1);
        let joined = uploaded(&fixture.update(100));
        // Both bricks are meshed, and each lacks the face against the other: five quads.
        assert_eq!(
            joined,
            [
                (SHIP, BrickCoordinate::ORIGIN, 5),
                (SHIP, BrickCoordinate::new(1, 0, 0), 5)
            ]
        );
        // Taking the second away meshes the first again, with its face back.
        fixture.set([32, 5, 5], 0);
        let separate = uploaded(&fixture.update(100));
        assert_eq!(separate, [(SHIP, BrickCoordinate::ORIGIN, 6)]);
    }

    #[test]
    fn at_most_the_budget_is_meshed_at_a_time_in_order_and_the_rest_waits() {
        let mut fixture = fixture();
        for x in 0..5 {
            fixture.set([x * 32 + 1, 1, 1], 1);
        }
        // The 5 bricks, and a brick before the first and after the last that are not in the ship, are waiting.
        let waiting = fixture.mesher.pending();
        assert!(waiting >= 5);
        let first = uploaded(&fixture.update(2));
        assert_eq!(first.len(), 2);
        // The three bricks of the ship that are left are still waiting (and so are some that are not in it).
        assert!(fixture.mesher.pending() >= 3 && fixture.mesher.pending() < waiting);
        let rest = uploaded(&fixture.update(100));
        let all: Vec<BrickCoordinate> = first
            .iter()
            .chain(&rest)
            .map(|(_, brick, _)| *brick)
            .collect();
        assert_eq!(
            all,
            (0..5)
                .map(|x| BrickCoordinate::new(x, 0, 0))
                .collect::<Vec<_>>(),
            "in order, none twice"
        );
    }

    #[test]
    fn a_brick_that_empties_or_goes_is_removed_once_and_one_never_shown_is_not() {
        let mut fixture = fixture();
        fixture.set([1, 1, 1], 1);
        fixture.update(100);
        // The brick stays but has nothing in it.
        fixture.set([1, 1, 1], 0);
        let updates = fixture.update(100);
        assert_eq!(
            updates,
            [MeshUpdate::RemoveGridBrick {
                grid: SHIP,
                coordinate: BrickCoordinate::ORIGIN
            }]
        );
        // And a brick that is no longer in the ship at all, once it was shown.
        fixture.set([1, 1, 1], 1);
        fixture.update(100);
        fixture.ship.remove_brick(BrickCoordinate::ORIGIN);
        fixture.mesher.brick_changed(SHIP, BrickCoordinate::ORIGIN);
        assert_eq!(fixture.update(100).len(), 1);
        fixture.mesher.brick_changed(SHIP, BrickCoordinate::ORIGIN);
        assert!(
            fixture.update(100).is_empty(),
            "nothing was shown to remove"
        );
    }

    #[test]
    fn a_ship_that_is_gone_or_forgotten_leaves_nothing_to_remove_or_mesh() {
        let mut fixture = fixture();
        fixture.set([1, 1, 1], 1);
        fixture.update(100);
        // Another ship's bricks are not this ship's.
        fixture.mesher.brick_changed(OTHER, BrickCoordinate::ORIGIN);
        fixture.mesher.forget_grid(OTHER);
        assert_eq!(fixture.mesher.pending(), 0);
        // A ship asked about that no longer exists: its shown bricks are removed.
        fixture.mesher.brick_changed(SHIP, BrickCoordinate::ORIGIN);
        let updates = fixture.mesher.update(|_| None, 100);
        assert_eq!(
            updates,
            [MeshUpdate::RemoveGridBrick {
                grid: SHIP,
                coordinate: BrickCoordinate::ORIGIN
            }]
        );
        // Forgetting a ship drops what was waiting and what was shown, without telling the renderer brick by brick.
        fixture.set([1, 1, 1], 1);
        fixture.update(100);
        fixture.set([2, 2, 2], 1);
        fixture.mesher.forget_grid(SHIP);
        assert_eq!(fixture.mesher.pending(), 0);
        assert!(fixture.update(100).is_empty());
    }

    #[test]
    fn changing_what_is_drawn_from_meshes_every_shown_brick_again_and_the_same_thing_does_not() {
        let mut fixture = fixture();
        fixture.set([1, 1, 1], 1);
        fixture.set([40, 1, 1], 1);
        fixture.update(100);
        let same = fixture.mesher.channels.clone();
        fixture.mesher.set_channels(same);
        assert_eq!(fixture.mesher.pending(), 0);
        let layout = StandardTestLayout::default();
        fixture.mesher.set_channels(ViewChannels::new(
            layout.material,
            None,
            Some(layout.temperature),
        ));
        let again = uploaded(&fixture.update(100));
        assert_eq!(again.len(), 2);
    }
}
