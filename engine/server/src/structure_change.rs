//! What the operations that change which ship holds which voxels did: [`StructureChange`].
//!
//! A split, a merge, a dock and a detach each end with ships that have gone, ships that have come and voxels that went from one to the other. Anything that
//! has to follow such a change, as opposed to finding it out by comparing the ships before and after, reads the record: a client that is told of it as a
//! rename or a split and not as ships that go and ships that come with all their bricks, a rule that keeps its own state by ship. Nothing reads it yet
//! (replication still compares the ships each client knows with the ships there are, which it must go on doing for a ship that comes into a client's
//! region with no change of structure at all); it is here so that the options kept open in `docs/decisions/joints-and-cars.md` ("The planet's unchanged
//! identifier") cost nothing to take up later.
//!
//! The server keeps the records of a tick until [`crate::ServerWorld::take_structure_changes`] hands them out (the server does it at the end of each
//! tick, into [`crate::StepReport`]), in the order the operations ran.

use std::collections::BTreeSet;

use voxel_space::{GridId, LatticeTransform, VoxelCoordinate};

/// Voxels that went from one ship to another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoxelsCarried {
    /// The ship they were in. (It is gone after a split, a merge and a dock, and still there after a detach.)
    pub from: GridId,
    /// The ship they are in now.
    pub to: GridId,
    /// Where each goes: `transform.apply(voxel)` is the voxel of `to` that a voxel of `from` became. The same map as the one the tombstone of `from` holds, where there is one.
    pub transform: LatticeTransform,
    /// The voxels, in the coordinates of `from`: those with something in them (any channel not at its default) that were carried.
    pub voxels: BTreeSet<VoxelCoordinate>,
}

/// One restructuring of ships, as it was done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructureChange {
    /// `from` was cut into parts (`split_grid`): none keeps its id, every part is in the coordinates of `from` (the transform is the identity), and each holds only the voxels it took.
    Split {
        from: GridId,
        parts: Vec<VoxelsCarried>,
    },
    /// Two ships were made one (`merge_grids`): `into` is new. `base`, the larger, kept its coordinates in it, so none of its voxels is in the record; `carried` is the other ship's,
    /// carried onto the lattice of `base`.
    Merge {
        into: GridId,
        base: GridId,
        carried: VoxelsCarried,
    },
    /// A ship was written into a planet, which kept its id (`dock_into`): `carried.from` is gone, `carried.to` is the planet, and the voxels are stored in the planet's bricks.
    Dock { carried: VoxelsCarried },
    /// Blocks were taken out of a ship, or out of the terrain of a level (`detach_blocks`), into a new ship `carried.to`. `carried.from` stays (it is a level's root grid
    /// for terrain), the voxels are those taken, and the transform is the shift that puts the lowest corner of them at the origin of the new ship.
    Detach { carried: VoxelsCarried },
}
