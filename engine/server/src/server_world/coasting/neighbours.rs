//! Which ships are in the way of a ship, found without looking at every ship (`ServerWorld::near` is asked of every ship that is not pushed, every tick, and looked at every grid
//! for each, so a tick cost the square of the ships: 36 ms of 46 at 800 ships).
//!
//! **The rule** is the one `near` has always had: a grid is in the way of a ship if the gap between them (the distance between their middles less the reach of each) is less than
//! the room, which is `factor` times the margin and the way they would go in a second at the speed they close at ([`is_in_the_way`], the one place that arithmetic is). A ship
//! has the room of `factor` 2 to go on a rail and of 1 to stay on one. What is given is the first of the grids in the order of their numbers, so a search that has found one
//! stops looking at anything with only higher numbers.
//!
//! **How it is found.** The ships of a level are put in a tree once for a tick (a k-d tree over the place and the speed, scaled so that a voxel a second is two voxels, which is
//! what the room counts it as), and each node keeps a box of the places and a box of the speeds of its ships, the greatest reach among them and the lowest number. A search for
//! the ships in the way of one goes down the tree, lower numbers first, and leaves a node alone when
//! - nothing in it has a number lower than the best found, or
//! - the nearest its box comes to the ship, less the two greatest reaches, is not less than the room for the fastest the ship and the node can close at.
//!
//! The room is made of the speed two ships **close** at, and not of the speed they go, so a crowd that goes round together at 8 km/s costs what it has near it and not what it
//! has in all; and in a world where everything is near everything the first leaf gives the answer. What is left is asked the exact question. Nothing is guessed: a node is dropped
//! only when every ship in it is out of the way, with a margin for the rounding of the boxes. The answers are those of looking at every ship (`tests/near_ships.rs` checks it on
//! random worlds, at the edge of the room to the last bit, with speeds that are endless or are not numbers, and in several levels).

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};

use voxel_space::{GridId, LevelIndex, WorldPosition};
use world_storage::MovingGrid;

use super::{NEAR_MARGIN, NEAR_SECONDS, Reach, reach, size};

/// Ships a leaf of the tree holds at most.
const LEAF_SHIPS: usize = 8;

/// The part of a number that the boxes of a node may be wrong by (two roundings of a place worked out from bricks and a rounding of a distance), made a million times larger
/// than it can be, so that what is dropped is dropped for certain.
const ROUNDING: f64 = 1.0e-12;

/// What `near` asks of a grid: where its middle is and how far from it it reaches, how fast it goes, and which level it is in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::server_world) struct Body {
    level: LevelIndex,
    reach: Reach,
    velocity: [f64; 3],
}

impl Body {
    #[cfg(test)]
    pub(in crate::server_world) fn new(
        level: LevelIndex,
        centre: WorldPosition,
        radius: f64,
        velocity: [f64; 3],
    ) -> Body {
        Body {
            level,
            reach: Reach { centre, radius },
            velocity,
        }
    }

    /// What a grid is now. (Its reach is the walk over all its bricks, so a tick takes it once for each grid.)
    pub(in crate::server_world) fn of(grid: &MovingGrid) -> Body {
        Body {
            level: grid.level,
            reach: reach(grid),
            velocity: grid.linear_velocity,
        }
    }

    pub(in crate::server_world) fn level(&self) -> LevelIndex {
        self.level
    }
}

/// Whether `other` is in the way of `ship` with this much room (1 to stay on a rail, 2 to go on one): the gap between their middles, less the reach of each, is less than
/// `factor` times the margin and the way they would go in a second at the speed they close at. (The levels are the caller's to compare.) This is the arithmetic that `near`
/// has always had, in the order it has always had it, and it is asked of the ship that is asking: the other order can round differently by a last bit.
pub(in crate::server_world) fn is_in_the_way(ship: &Body, other: &Body, factor: f64) -> bool {
    let apart = size(
        ship.reach
            .centre
            .displacement_to(other.reach.centre)
            .in_voxels(),
    );
    let velocity: [f64; 3] = [0, 1, 2].map(|axis| other.velocity[axis] - ship.velocity[axis]);
    let gap = apart - ship.reach.radius - other.reach.radius;
    gap < factor * (NEAR_MARGIN + size(velocity) * NEAR_SECONDS)
}

/// How many seconds of the speed they close at the ground of a planet is in the way of a group that has a frame of its own: the bodies look this far ahead for terrain
/// (`grid_bodies`, `TERRAIN_LOOKAHEAD_SECONDS`), so that the ground can touch nothing that is farther than this from the ground.
const GROUND_SECONDS: f64 = 0.25;

/// Whether the ground of `planet` is in the way of `ship` with this much room (`factor` as for [`is_in_the_way`]): the gap between their middles, less the reach of each, is less
/// than `factor` times the margin and the way they would go in [`GROUND_SECONDS`] at the speed they close at. (The levels are the caller's to compare.) A planet is in the way of a
/// ship by the room of [`is_in_the_way`] too, which counts a second of the speed: that is the room of a ship that is not to be hit, and this is the room of a ship that is not to
/// be in the reach of the ground of a planet, which is a quarter of a second of it.
pub(in crate::server_world) fn ground_in_the_way(ship: &Body, planet: &Body, factor: f64) -> bool {
    let apart = size(
        ship.reach
            .centre
            .displacement_to(planet.reach.centre)
            .in_voxels(),
    );
    let velocity: [f64; 3] = [0, 1, 2].map(|axis| planet.velocity[axis] - ship.velocity[axis]);
    let gap = apart - ship.reach.radius - planet.reach.radius;
    gap < factor * (NEAR_MARGIN + size(velocity) * GROUND_SECONDS)
}

/// The ships of a world (grids without terrain), ready to be asked which of them are in the way of one.
pub(in crate::server_world) struct ShipsInTheWay {
    ships: Vec<(GridId, Body)>,
    index: HashMap<GridId, usize>,
    trees: BTreeMap<LevelIndex, Tree>,
    pairs_tested: Cell<u64>,
}

/// What one ship is in a level's tree.
struct Point {
    ship: usize,
    id: GridId,
    place: [f64; 3],
    velocity: [f64; 3],
    radius: f64,
}

/// A node of the tree: the ships `low..high` of the points, the boxes of their places and of their speeds, the greatest reach and the lowest number among them.
struct Node {
    low: usize,
    high: usize,
    place_low: [f64; 3],
    place_high: [f64; 3],
    velocity_low: [f64; 3],
    velocity_high: [f64; 3],
    radius: f64,
    lowest_id: GridId,
    children: Option<(usize, usize)>,
}

/// The ships of one level.
struct Tree {
    /// The place of the first ship: the places of the tree are from it (a whole number of bricks of difference, which is exact, and the offsets).
    origin: WorldPosition,
    /// How far a box of places may be wrong by, in voxels.
    slack: f64,
    points: Vec<Point>,
    nodes: Vec<Node>,
}

/// The ship that is asking, with its place from the origin of the tree it asks.
struct Query<'a> {
    ship: usize,
    body: &'a Body,
    place: [f64; 3],
}

impl ShipsInTheWay {
    /// Puts `ships` (any order) in trees, a tree for each level.
    pub(in crate::server_world) fn find(ships: &[(GridId, Body)]) -> ShipsInTheWay {
        let mut by_level: BTreeMap<LevelIndex, Vec<usize>> = BTreeMap::new();
        for (index, (_, body)) in ships.iter().enumerate() {
            by_level.entry(body.level).or_default().push(index);
        }
        ShipsInTheWay {
            ships: ships.to_vec(),
            index: ships
                .iter()
                .enumerate()
                .map(|(index, (id, _))| (*id, index))
                .collect(),
            trees: by_level
                .into_iter()
                .map(|(level, members)| (level, Tree::new(ships, &members)))
                .collect(),
            pairs_tested: Cell::new(0),
        }
    }

    /// The lowest number among the other ships of `ship`'s level that are in its way with this room, not counting `excluded`: `Some(None)` if there is none, and `None` for a
    /// ship this was not made for.
    pub(in crate::server_world) fn lowest(
        &self,
        ship: GridId,
        factor: f64,
        excluded: &[GridId],
    ) -> Option<Option<GridId>> {
        let &index = self.index.get(&ship)?;
        let body = &self.ships[index].1;
        let tree = self.trees.get(&body.level)?;
        let query = Query {
            ship: index,
            body,
            place: tree.origin.displacement_to(body.reach.centre).in_voxels(),
        };
        let mut best = None;
        self.search(tree, 0, &query, factor, excluded, &mut best);
        Some(best)
    }

    /// How many pairs of ships were asked the exact question, for a test of what a crowd costs and for the world to say.
    pub(in crate::server_world) fn pairs_tested(&self) -> u64 {
        self.pairs_tested.get()
    }

    fn search(
        &self,
        tree: &Tree,
        node: usize,
        query: &Query<'_>,
        factor: f64,
        excluded: &[GridId],
        best: &mut Option<GridId>,
    ) {
        let node = &tree.nodes[node];
        if best.is_some_and(|best| node.lowest_id >= best)
            || tree.cannot_be_in_the_way(node, query, factor)
        {
            return;
        }
        match node.children {
            None => {
                for point in &tree.points[node.low..node.high] {
                    if point.ship == query.ship
                        || best.is_some_and(|best| point.id >= best)
                        || excluded.contains(&point.id)
                    {
                        continue;
                    }
                    self.pairs_tested.set(self.pairs_tested.get() + 1);
                    if is_in_the_way(query.body, &self.ships[point.ship].1, factor) {
                        *best = Some(point.id);
                    }
                }
            }
            Some((first, second)) => {
                // The child with the lower number first, so that what is found first holds the other back.
                let (first, second) = if tree.nodes[first].lowest_id <= tree.nodes[second].lowest_id
                {
                    (first, second)
                } else {
                    (second, first)
                };
                self.search(tree, first, query, factor, excluded, best);
                self.search(tree, second, query, factor, excluded, best);
            }
        }
    }
}

impl Tree {
    fn new(ships: &[(GridId, Body)], members: &[usize]) -> Tree {
        let origin = ships[members[0]].1.reach.centre;
        let points: Vec<Point> = members
            .iter()
            .map(|&ship| {
                let body = &ships[ship].1;
                Point {
                    ship,
                    id: ships[ship].0,
                    place: origin.displacement_to(body.reach.centre).in_voxels(),
                    velocity: body.velocity,
                    radius: body.reach.radius,
                }
            })
            .collect();
        let farthest = points
            .iter()
            .flat_map(|point| point.place)
            .fold(0.0_f64, |most, coordinate| most.max(coordinate.abs()));
        let count = points.len();
        let mut tree = Tree {
            origin,
            slack: ROUNDING * (1.0 + farthest),
            points,
            nodes: Vec::new(),
        };
        tree.build(0, count);
        tree
    }

    /// The node for the points `low..high`, and the nodes under it.
    fn build(&mut self, low: usize, high: usize) -> usize {
        let mut node = Node {
            low,
            high,
            place_low: [f64::INFINITY; 3],
            place_high: [f64::NEG_INFINITY; 3],
            velocity_low: [f64::INFINITY; 3],
            velocity_high: [f64::NEG_INFINITY; 3],
            radius: 0.0,
            lowest_id: self.points[low].id,
            children: None,
        };
        for point in &self.points[low..high] {
            for axis in 0..3 {
                node.place_low[axis] = node.place_low[axis].min(point.place[axis]);
                node.place_high[axis] = node.place_high[axis].max(point.place[axis]);
                node.velocity_low[axis] = node.velocity_low[axis].min(point.velocity[axis]);
                node.velocity_high[axis] = node.velocity_high[axis].max(point.velocity[axis]);
            }
            node.radius = node.radius.max(point.radius);
            node.lowest_id = node.lowest_id.min(point.id);
        }
        let index = self.nodes.len();
        if high - low > LEAF_SHIPS {
            // Split at the median of the axis the node is longest along, a voxel a second counting as the room of two voxels.
            let scale = 2.0 * NEAR_SECONDS;
            let extents: [f64; 6] = [0, 1, 2, 3, 4, 5].map(|axis| {
                if axis < 3 {
                    node.place_high[axis] - node.place_low[axis]
                } else {
                    scale * (node.velocity_high[axis - 3] - node.velocity_low[axis - 3])
                }
            });
            let axis = (0..6)
                .max_by(|&first, &second| extents[first].total_cmp(&extents[second]))
                .unwrap_or(0);
            let key = |point: &Point| {
                if axis < 3 {
                    point.place[axis]
                } else {
                    point.velocity[axis - 3]
                }
            };
            let middle = low + (high - low) / 2;
            self.points[low..high]
                .select_nth_unstable_by(middle - low, |a, b| key(a).total_cmp(&key(b)));
            self.nodes.push(node);
            let first = self.build(low, middle);
            let second = self.build(middle, high);
            self.nodes[index].children = Some((first, second));
        } else {
            self.nodes.push(node);
        }
        index
    }

    /// Whether no ship of the node can be in the way of the asking ship with this room: the nearest the box of the places comes to it, less its reach and the greatest in the node
    /// and what the rounding may have taken, is not less than the room for the fastest the ship and the box of speeds can close at. (A speed that is endless makes the room
    /// endless, and one that is not a number makes the comparison false: neither drops a node, and a ship with a speed that is not a number is in the way of nothing.)
    fn cannot_be_in_the_way(&self, node: &Node, query: &Query<'_>, factor: f64) -> bool {
        let nearest = size([0, 1, 2].map(|axis| {
            (node.place_low[axis] - query.place[axis])
                .max(query.place[axis] - node.place_high[axis])
                .max(0.0)
        }));
        let fastest = size([0, 1, 2].map(|axis| {
            (node.velocity_high[axis] - query.body.velocity[axis])
                .abs()
                .max((query.body.velocity[axis] - node.velocity_low[axis]).abs())
        })) * (1.0 + ROUNDING);
        let room = factor * (NEAR_MARGIN + fastest * NEAR_SECONDS);
        nearest - self.slack - query.body.reach.radius - node.radius >= room
    }
}
