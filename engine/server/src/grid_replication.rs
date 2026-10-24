//! Telling clients about ships: for each client, which came into the region it watches, which moved, which left, and what was edited in
//! the ones it already has.
//!
//! A client watches a box of bricks of one level. A ship is in its picture if the ship is in that level and the bricks its box touches
//! (the bound [`MovingGrid::world_brick_bounds`] gives) meet the box. A client that is told of a ship is sent every brick of it, and is told to
//! forget it all when the ship leaves. A ship with terrain, a planet, is told of with the generator and the extent, and only the bricks that are stored (the
//! edits) are sent: the client makes the others itself, and the bricks edited later arrive as the edits of a brick it has, or as a whole brick when the
//! edit stored it.

use std::collections::BTreeMap;
use std::sync::Arc;

use protocol::{GridChange, GridState, GridTerrainState, ServerMessage};
use voxel_space::{BrickBox, BrickCoordinate, GridId, GridPose, LevelIndex};

use crate::server_world::ServerWorld;

/// What one edited brick needs sent to the clients that already hold it.
pub(crate) struct EditedBrick {
    pub grid: GridId,
    pub brick: BrickCoordinate,
    pub update: ServerMessage,
}

/// A ship as the replication sees it this tick, made once for all clients.
#[derive(Debug)]
pub(crate) struct ShipView {
    id: GridId,
    level: LevelIndex,
    pose: GridPose,
    bounds: BrickBox,
    /// The bricks that are stored, which are all the client is sent.
    brick_count: u64,
    terrain: Option<GridTerrainState>,
}

/// The ships that have anything to show: those with bricks, and a place in the world. In order of id.
pub(crate) fn ship_views(world: &ServerWorld) -> Vec<ShipView> {
    world
        .data()
        .grids()
        .iter()
        .filter_map(|(id, ship)| {
            Some(ShipView {
                id,
                level: ship.level,
                pose: ship.pose,
                bounds: ship.world_brick_bounds()?,
                brick_count: ship.voxels.brick_count() as u64,
                terrain: ship.terrain.map(|terrain| GridTerrainState {
                    generator: terrain.generator.raw(),
                    extent: terrain.extent,
                }),
            })
        })
        .collect()
}

/// What one client has been told of the ships, by id: where each was when it was last told.
pub(crate) type KnownGrids = BTreeMap<GridId, GridPose>;

/// The messages that bring a client's picture of the ships up to date with the world, for a client watching `watched`: those that left,
/// then those that came in (each followed by all its bricks), then those that moved, then the edits made this tick to those the client
/// already had. `known` becomes what the client now knows. At most `budget` bricks of ships are in a client's picture: a ship that
/// would take it past that, in order of id, is left out, and so is every later one that does not fit.
pub(crate) fn grid_messages(
    world: &ServerWorld,
    views: &[ShipView],
    known: &mut KnownGrids,
    watched: Option<(LevelIndex, BrickBox)>,
    budget: u64,
    edited: &[EditedBrick],
) -> Vec<ServerMessage> {
    let mut wanted: Vec<&ShipView> = Vec::new();
    if let Some((level, region)) = watched {
        let mut used = 0u64;
        for view in views {
            if view.level == level
                && view.bounds.intersection(region).is_some()
                && used + view.brick_count <= budget
            {
                used += view.brick_count;
                wanted.push(view);
            }
        }
    }

    let gone: Vec<GridId> = known
        .keys()
        .filter(|id| !wanted.iter().any(|view| view.id == **id))
        .copied()
        .collect();
    let mut spawned = Vec::new();
    let mut changed = Vec::new();
    let mut kept = Vec::new();
    for view in &wanted {
        match known.get(&view.id) {
            None => spawned.push(*view),
            Some(before) => {
                kept.push(view.id);
                if *before != view.pose {
                    changed.push(GridChange {
                        id: view.id,
                        pose: view.pose,
                    });
                }
            }
        }
    }

    let mut messages = Vec::new();
    if !gone.is_empty() {
        messages.push(ServerMessage::GridsDespawned(gone));
    }
    if !spawned.is_empty() {
        messages.push(ServerMessage::GridsSpawned(
            spawned
                .iter()
                .map(|view| GridState {
                    id: view.id,
                    level: view.level,
                    pose: view.pose,
                    terrain: view.terrain,
                })
                .collect(),
        ));
        // Their bricks are in their final state for this tick already, so the edits of the tick are not sent for them.
        for view in &spawned {
            let voxels = &world
                .data()
                .grids()
                .get(view.id)
                .expect("a ship that has a view exists")
                .voxels;
            for brick in voxels.sorted_brick_coordinates() {
                messages.push(ServerMessage::BrickContents {
                    grid: view.id,
                    brick,
                    contents: Arc::clone(voxels.brick(brick).expect("the brick was just found")),
                });
            }
        }
    }
    if !changed.is_empty() {
        messages.push(ServerMessage::GridsChanged(changed));
    }
    messages.extend(
        edited
            .iter()
            .filter(|entry| kept.contains(&entry.grid))
            .map(|entry| entry.update.clone()),
    );

    *known = wanted.iter().map(|view| (view.id, view.pose)).collect();
    messages
}
