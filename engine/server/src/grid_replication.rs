//! Telling clients about ships: for each client, which came into the region it watches, which moved, which left, and what was edited in
//! the ones it already has.
//!
//! A client watches a box of bricks of one level. A ship is in its picture if the ship is in that level and the bricks its box touches
//! (the bound [`MovingGrid::world_brick_bounds`] gives) meet the box, or if it is a planet, which is in the picture of every client that watches its level. A
//! client that is told of a ship is sent every brick of it, and is told to
//! forget it all when the ship leaves. **A ship that is not in the picture but looks big enough from where the client is** (the settings say how big and how
//! many: [`FarShipSettings`]) is told of as *far*: no brick is sent and none ever is while it is far, and the nodes of the game's plugins draw it. A ship goes
//! from one tier to the other by being told to be forgotten and told of again in the same tick. A ship with terrain, a planet, is told of with the generator and the extent, and only the bricks that are stored (the
//! edits) are sent: the client makes the others itself, and the bricks edited later arrive as the edits of a brick it has, or as a whole brick when the
//! edit stored it.

use std::collections::BTreeMap;
use std::sync::Arc;

use protocol::{GridChange, GridContent, GridState, GridTerrainState, ServerMessage};
use voxel_space::{BRICK_EDGE_LENGTH, BrickBox, BrickCoordinate, GridId, GridPose, LevelIndex};

use crate::far_ship_settings::FarShipSettings;
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
    /// The bricks the ship has, in its own coordinates (what a far ship is told of with).
    own_bounds: BrickBox,
    /// The bricks that are stored, which are all the client is sent.
    brick_count: u64,
    terrain: Option<GridTerrainState>,
    /// The ship is in a file, not in memory: it has no brick to send, so it is never in a client's picture and can be told of as far only.
    in_a_file: bool,
}

/// The ships that have anything to show: those with bricks, and a place in the world; the ones in memory, and the ones in files that the world can place without reading
/// them (not a planet, which is brought in whatever the distance, nor one that was on a rail, whose place is not where it was put away; and not one put away by a program
/// that did not keep the box of its bricks). In order of id.
pub(crate) fn ship_views(world: &ServerWorld) -> Vec<ShipView> {
    let in_memory = world.data().grids().iter().filter_map(|(id, ship)| {
        Some(ShipView {
            id,
            level: ship.level,
            pose: ship.pose,
            bounds: ship.world_brick_bounds()?,
            own_bounds: ship.own_brick_box()?,
            brick_count: ship.voxels.brick_count() as u64,
            terrain: ship.terrain.map(|terrain| GridTerrainState {
                generator: terrain.generator.raw(),
                extent: terrain.extent,
            }),
            in_a_file: false,
        })
    });
    let in_files = world
        .data()
        .grids()
        .parked()
        .filter(|(_, parked)| !parked.has_terrain && !parked.on_a_rail)
        .filter_map(|(id, parked)| {
            Some(ShipView {
                id,
                level: parked.level,
                pose: parked.pose,
                bounds: parked.bounds?,
                own_bounds: parked.extent?,
                brick_count: 0,
                terrain: None,
                in_a_file: true,
            })
        });
    let mut views: Vec<ShipView> = in_memory.chain(in_files).collect();
    views.sort_by_key(|view| view.id);
    views
}

/// How many ships a client is told of, and how.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShipLimits {
    /// The most bricks of ships in one client's picture.
    pub maximum_bricks: u64,
    /// Whether ships that are not in the picture are told of as far, and which.
    pub far: Option<FarShipSettings>,
}

/// What one client knows of a ship: where it was when it was last told, and whether it is known as far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct KnownGrid {
    pose: GridPose,
    pub far: bool,
}

/// What one client has been told of the ships, by id.
pub(crate) type KnownGrids = BTreeMap<GridId, KnownGrid>;

/// The length of the longest side of a box of bricks, in voxels, and its middle, in voxels along each axis.
fn size_and_middle(bounds: BrickBox) -> (f64, [f64; 3]) {
    let edge = BRICK_EDGE_LENGTH as f64;
    let (low, high) = (bounds.min(), bounds.max());
    let lows = [low.x(), low.y(), low.z()];
    let highs = [high.x(), high.y(), high.z()];
    let mut longest: f64 = 0.0;
    let mut middle = [0.0; 3];
    for axis in 0..3 {
        let length = (highs[axis] - lows[axis] + 1) as f64 * edge;
        longest = longest.max(length);
        // (The first brick starts at its coordinate and the last ends one brick after its own.)
        middle[axis] = (highs[axis] + lows[axis] + 1) as f64 * edge / 2.0;
    }
    (longest, middle)
}

/// The messages that bring a client's picture of the ships up to date with the world, for a client watching `watched`: those that left,
/// then those that came in (each followed by all its bricks), then those that moved, then the edits made this tick to those the client
/// already had. `known` becomes what the client now knows. At most `budget` bricks of ships are in a client's picture: a ship that
/// would take it past that, in order of id, is left out, and so is every later one that does not fit; it is told of as far instead if `far` says it looks big
/// enough. The exception is `aboard`, the ship the client's own body is aboard (if
/// it is in the level the client watches): it is in the picture wherever the region is, and takes its bricks of the budget first, because a client that does not know it cannot place its own body.
pub(crate) fn grid_messages(
    world: &ServerWorld,
    views: &[ShipView],
    known: &mut KnownGrids,
    watched: Option<(LevelIndex, BrickBox)>,
    aboard: Option<GridId>,
    limits: ShipLimits,
    edited: &[EditedBrick],
) -> Vec<ServerMessage> {
    // The ships to be known, each near (with its bricks) or far.
    let mut wanted: Vec<(&ShipView, bool)> = Vec::new();
    if let Some((level, region)) = watched {
        // (The ship the client's body is aboard takes its bricks of the budget first, and is where its id puts it among the others.)
        let forced = views
            .iter()
            .find(|view| Some(view.id) == aboard && view.level == level && !view.in_a_file)
            .map(|view| view.id);
        let mut used = views
            .iter()
            .find(|view| Some(view.id) == forced)
            .map_or(0, |view| view.brick_count);
        for view in views {
            if Some(view.id) == forced {
                wanted.push((view, false));
                continue;
            }
            // (A planet is known from anywhere in its level: it is drawn from as far as there is anything to see of it, and a client has to know it to draw it.)
            if view.level == level
                && !view.in_a_file
                && (view.terrain.is_some() || view.bounds.intersection(region).is_some())
                && used + view.brick_count <= limits.maximum_bricks
            {
                used += view.brick_count;
                wanted.push((view, false));
            }
        }
        if let Some(settings) = limits.far {
            let viewer = size_and_middle(region).1;
            let mut seen: Vec<(f64, &ShipView)> = views
                .iter()
                .filter(|view| {
                    view.level == level
                        && view.terrain.is_none()
                        && !wanted.iter().any(|(wanted, _)| wanted.id == view.id)
                })
                .filter_map(|view| {
                    let (size, middle) = size_and_middle(view.bounds);
                    let distance_squared: f64 = (0..3)
                        .map(|axis| (middle[axis] - viewer[axis]) * (middle[axis] - viewer[axis]))
                        .sum();
                    let known = known.get(&view.id).is_some_and(|known| known.far);
                    settings
                        .is_seen(size, distance_squared, known)
                        .then_some((distance_squared, view))
                })
                .collect();
            // The nearest are kept, and of equally near ones the lower id.
            seen.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
            seen.truncate(settings.maximum_ships);
            wanted.extend(seen.into_iter().map(|(_, view)| (view, true)));
            wanted.sort_by_key(|(view, _)| view.id);
        }
    }

    // A ship that is not wanted any more, or is wanted in the other tier, is forgotten (and in the second case told of again below).
    let gone: Vec<GridId> = known
        .iter()
        .filter(|(id, before)| {
            !wanted
                .iter()
                .any(|(view, far)| view.id == **id && *far == before.far)
        })
        .map(|(id, _)| *id)
        .collect();
    let mut spawned: Vec<(&ShipView, bool)> = Vec::new();
    let mut changed = Vec::new();
    let mut kept = Vec::new();
    for &(view, far) in &wanted {
        match known.get(&view.id) {
            Some(before) if before.far == far => {
                // (A far ship has no bricks, so no edit of one is sent.)
                if !far {
                    kept.push(view.id);
                }
                if before.pose != view.pose {
                    changed.push(GridChange {
                        id: view.id,
                        pose: view.pose,
                    });
                }
            }
            _ => spawned.push((view, far)),
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
                .map(|&(view, far)| GridState {
                    id: view.id,
                    level: view.level,
                    pose: view.pose,
                    content: if far {
                        GridContent::Far {
                            bounds: view.own_bounds,
                        }
                    } else {
                        view.terrain
                            .map_or(GridContent::Bricks, GridContent::Generated)
                    },
                })
                .collect(),
        ));
        // Their bricks are in their final state for this tick already, so the edits of the tick are not sent for them. (A far ship has none.)
        for (view, _) in spawned.iter().filter(|(_, far)| !far) {
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

    *known = wanted
        .iter()
        .map(|&(view, far)| {
            (
                view.id,
                KnownGrid {
                    pose: view.pose,
                    far,
                },
            )
        })
        .collect();
    messages
}
