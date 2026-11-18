//! The cubes of a ship that is far away. A client that is told of a ship as far is sent none of its bricks, so no brick of it is drawn: the finest cubes are nodes of
//! detail level 1, and the cubes that are drawn are the ones the ordinary selection chooses with no ball of bricks round the viewer. The server and the client both
//! work them out from the brick the viewer is in, in the ship's own coordinates, so that what the server sends is what the client draws.

use region_octree::BrickCube;
use voxel_space::BrickCoordinate;

use crate::selection::{ViewSelection, select_around_brick};

/// The cubes drawn of a far ship for a viewer in `brick` of the ship's own coordinates: those the selection chooses with no fine region, none of them a brick.
/// `view.fine_radius` is not used. A cube `may_hold` says no to is left out with everything inside it.
pub fn select_far_ship(
    brick: BrickCoordinate,
    view: &ViewSelection,
    may_hold: &dyn Fn(BrickCube) -> bool,
) -> Vec<BrickCube> {
    let view = ViewSelection {
        fine_radius: 0,
        ..*view
    };
    select_around_brick(brick, &view, may_hold)
        .into_iter()
        .filter(|cube| cube.level() >= 1)
        .collect()
}

/// The cubes to send a client for a far ship, coarsest first: the ones drawn ([`select_far_ship`]), every cube that holds one of them up to `coarsest_level`, and
/// the eight cubes a level finer inside each of them that is coarser than level 1 and that `may_hold` does not refuse. The holders are what the picture falls
/// back to when the viewer goes away from the ship, and the halves what it goes on to when the viewer comes nearer: both are on the client before the selection
/// changes, so that a ship is not without a picture for as long as it takes the server to answer. Nothing is in the list twice.
pub fn far_ship_nodes_to_send(
    brick: BrickCoordinate,
    view: &ViewSelection,
    may_hold: &dyn Fn(BrickCube) -> bool,
    coarsest_level: u8,
) -> Vec<BrickCube> {
    let drawn = select_far_ship(brick, view, may_hold);
    let mut wanted: std::collections::HashSet<BrickCube> = drawn.iter().copied().collect();
    for cube in &drawn {
        for level in cube.level() + 1..=coarsest_level {
            wanted.insert(BrickCube::containing(cube.origin(), level));
        }
        if cube.level() >= 2 {
            let half = 1i128 << (cube.level() - 1);
            let origin = cube.origin();
            for step in 0..8i128 {
                let offset = |bit: i128| (step >> bit & 1) * half;
                let Some(inner) = origin.checked_offset(offset(0), offset(1), offset(2)) else {
                    continue;
                };
                if let Ok(child) = BrickCube::new(inner, cube.level() - 1)
                    && may_hold(child)
                {
                    wanted.insert(child);
                }
            }
        }
    }
    let mut cubes: Vec<BrickCube> = wanted.into_iter().collect();
    cubes.sort_by_key(|cube| (std::cmp::Reverse(cube.level()), cube.origin()));
    cubes
}
