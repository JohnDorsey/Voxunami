//! Following a ship whose id changed.
//!
//! A ship that is split, merged, docked with a planet or given a new id has its old id retired, and the world remembers what took its place (`GridStore::resolve`, from the
//! tombstones). What the game keeps of a ship by its id (the frame a walker's body is simulated in, the ship a walker is buckled to, the place a walker left a ship, how the ship went
//! when the walker was last stepped) is stale after it. This is the one place that says what such an id has become, so that no holder of an id works it out for itself, and none forgets.
//!
//! The rule for a body that is aboard: what it does (where it is on the blocks, how it goes against them) is not changed by the ship's name. Where it is comes from the world's record
//! of the player, which the world moved to the new ship with the transform; how it goes in the ship is its speed turned by the rotation of that transform.

use voxel_space::{GridId, LatticeTransform};
use world_storage::GridStore;

/// What the ship `old` has become, if it is not a ship of the world any more but something took its place: the id to follow, and the transform from the old ship's coordinates
/// to the new one's. Of the parts of a ship that was split, the one `prefer` is if it is one of them (the part the world put what we follow on), else the first; the parts are in
/// the old ship's coordinates (the transform is the identity), so a place in the old ship is a place in each part.
///
/// `None` if `old` is still a ship (there is nothing to follow), and also if nothing took its place (it is gone), which the holder of the id deals with as it always has.
pub fn successor(
    grids: &GridStore,
    old: GridId,
    prefer: Option<GridId>,
) -> Option<(GridId, LatticeTransform)> {
    if grids.knows(old) {
        return None;
    }
    let successors = grids.resolve(old);
    prefer
        .and_then(|wanted| successors.iter().find(|(grid, _)| *grid == wanted))
        .or_else(|| successors.first())
        .copied()
}
