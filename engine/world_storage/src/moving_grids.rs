use std::collections::BTreeMap;

use terrain::GeneratorId;
use voxel_grid::Grid;
use voxel_space::{
    BrickBox, BrickCoordinate, GridId, GridPose, LatticeTransform, LevelIndex, WorldPosition,
};

use crate::container::Section;
use crate::rail::Rail;
use crate::ship_files::ShipFileName;

/// What happens to a ship when its pilot logs out (a ship's own wish; a server setting can override it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PilotLogout {
    /// It stays where it is, in the world, as any ship does.
    #[default]
    Stay,
    /// It is saved and put away with the pilot, and comes back, where it was, when they do.
    Vanish,
}

/// What makes the bricks of a ship that nobody has stored: a planet is a ship with terrain. The bricks of the ship inside `extent` are those the generator makes
/// unless the ship's voxels have the brick (an edit, which wins); outside it there is nothing but what is stored. The clients make the same bricks from the same
/// generator, so they are not sent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridTerrain {
    /// The version of the terrain generator that makes the bricks, a number of the world's table of generators like a level's (a file names it).
    pub generator: GeneratorId,
    /// The bricks it makes, in the ship's own coordinates.
    pub extent: BrickBox,
    /// What the ship weighs in kilograms, declared: a ship whose bricks are not stored has no blocks to add up. The centre of mass is the ship's field of that name.
    pub mass: f32,
}

/// A grid of voxels that is not the terrain of a level: a ship. Its voxels are in its own coordinates, and its pose says where
/// those coordinates are in its level.
#[derive(Clone, Debug, PartialEq)]
pub struct MovingGrid {
    /// The level this grid is in.
    pub level: LevelIndex,
    pub pose: GridPose,
    /// How fast the origin of the ship moves, in voxels a second, and how fast it turns (an axis scaled by radians a second). Bodies
    /// write them as they move the ship; a ship made anew has none.
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    /// How much the ship weighs in kilograms and where its centre of mass is in its own coordinates, as the bodies work it out from its blocks
    /// (zero where nothing has). A ship of enough mass pulls things toward its centre of mass (see `voxel_space::gravity_at`).
    pub mass: f32,
    pub centre_of_mass: [f32; 3],
    /// A held ship does not move: gravity, forces and collisions leave it where it is, and everything else collides with it.
    pub held: bool,
    /// A frozen ship is held, and its rules wait, because no player is near it (see `server::ShipSettings`). It is not saved: a ship that
    /// comes out of its file is awake.
    pub frozen: bool,
    /// What its pilot's logout does to it.
    pub on_pilot_logout: PilotLogout,
    /// The voxels that are stored: all of them for a ship someone built, the edits for a ship with terrain. An absent brick is every channel at its default, or
    /// what the terrain generates there.
    pub voxels: Grid,
    /// What generates the bricks nobody has stored, for a planet; `None` for a ship someone built (nothing generates it).
    pub terrain: Option<GridTerrain>,
    /// How the ship moves if it is on rails: its place and turn are a function of the time of the world, put there at the start of each tick (see
    /// `crate::rail`); `None` for a ship that is moved by forces or by hand.
    pub rail: Option<Rail>,
    /// Sections of plugins that the ship's file had and nobody here reads, kept as they were so that a ship saved again loses nothing.
    pub sections: Vec<Section>,
}

impl MovingGrid {
    /// The lowest and the highest brick that has anything in it or that its terrain makes, along each axis, in the ship's own coordinates. `None` for a ship with
    /// no bricks.
    pub fn brick_extent(&self) -> Option<(BrickCoordinate, BrickCoordinate)> {
        let mut bounds: Option<(BrickCoordinate, BrickCoordinate)> = self
            .terrain
            .map(|terrain| (terrain.extent.min(), terrain.extent.max()));
        for (brick, _) in self.voxels.bricks() {
            bounds = Some(match bounds {
                None => (brick, brick),
                Some((low, high)) => (
                    BrickCoordinate::new(
                        low.x().min(brick.x()),
                        low.y().min(brick.y()),
                        low.z().min(brick.z()),
                    ),
                    BrickCoordinate::new(
                        high.x().max(brick.x()),
                        high.y().max(brick.y()),
                        high.z().max(brick.z()),
                    ),
                ),
            });
        }
        bounds
    }

    /// How fast a place in the world moves because the ship carries it, in voxels a second in the world's axes: the speed of the ship's centre
    /// of mass, and what its turning adds, which is the turn (an axis scaled by radians a second) crossed with the vector from the
    /// centre of mass to the place. It is the speed on record: a ship that is held has none until whoever moves it says how fast.
    pub fn velocity_at(&self, place: WorldPosition) -> [f32; 3] {
        let centre = WorldPosition::new(BrickCoordinate::ORIGIN, self.centre_of_mass);
        let Some(centre) = self.pose.local_to_world(centre) else {
            return self.linear_velocity;
        };
        let arm = place.relative_to(centre).map(f64::from);
        let turn = self.angular_velocity.map(f64::from);
        let carried = [
            turn[1] * arm[2] - turn[2] * arm[1],
            turn[2] * arm[0] - turn[0] * arm[2],
            turn[0] * arm[1] - turn[1] * arm[0],
        ];
        [0, 1, 2].map(|axis| (f64::from(self.linear_velocity[axis]) + carried[axis]) as f32)
    }

    /// The bricks of the level (its root grid's, in the level's coordinates) that the ship's bricks are in or touch, as a box: the box
    /// around the turned box of its bricks. `None` for a ship with no bricks, or one whose bricks are outside the supported range in
    /// the world. It is a bound, not a fit: a ship turned half way across the diagonal is covered by a box larger than it.
    pub fn world_brick_bounds(&self) -> Option<BrickBox> {
        let (low, high) = self.brick_extent()?;
        // The corners of the box of voxels, in the ship's own coordinates: from the first voxel of the lowest brick to the voxel
        // after the last of the highest.
        let mut corners = Vec::with_capacity(8);
        for x in [low.x(), high.x() + 1] {
            for y in [low.y(), high.y() + 1] {
                for z in [low.z(), high.z() + 1] {
                    let corner = BrickCoordinate::try_new(x, y, z).ok()?;
                    corners.push(
                        self.pose
                            .local_to_world(WorldPosition::new(corner, [0.0; 3]))?,
                    );
                }
            }
        }
        // The bricks the box touches run from the brick of its lowest corner to the brick of its highest, where a highest corner exactly
        // on the boundary between two bricks touches only the one below.
        let mut min = corners[0].brick();
        let mut max = min;
        for corner in &corners {
            let brick = corner.brick();
            let offset = corner.offset();
            let upper = |value: i64, offset: f32| if offset != 0.0 { value } else { value - 1 };
            min = BrickCoordinate::new(
                min.x().min(brick.x()),
                min.y().min(brick.y()),
                min.z().min(brick.z()),
            );
            max = BrickCoordinate::new(
                max.x().max(upper(brick.x(), offset[0])),
                max.y().max(upper(brick.y(), offset[1])),
                max.z().max(upper(brick.z(), offset[2])),
            );
        }
        BrickBox::new(min, max)
    }
}

/// A ship that is not in memory: its voxels are in a file, and this is what the world knows of it without reading the file, which is enough
/// to say where it is in the world, how near a player is to it, and where what was aboard it is.
#[derive(Clone, Debug, PartialEq)]
pub struct ParkedGrid {
    pub level: LevelIndex,
    /// Where it was when it was put away.
    pub pose: GridPose,
    /// The bricks of the level it touched then (`None` for a ship with no bricks).
    pub bounds: Option<BrickBox>,
    /// Its file, and the checksum the file had when it was written.
    pub file: ShipFileName,
    pub checksum: u32,
    /// It went away with its pilot and comes back with whoever is aboard it, not when a player comes near.
    pub waits_for_those_aboard: bool,
    /// It has terrain: it is a planet, which a client draws from as far as there is anything to see of it, so it is not put in a file for good and is brought back
    /// whatever the distance.
    pub has_terrain: bool,
}

/// The grids that are not the root grids of levels, and the ids they had before they were split or merged.
///
/// Ids come from a counter that only goes up, starting at [`GridId::FIRST_ALLOCATED`], so an id is never given twice in a world and a
/// recording and a reload agree about which grid it names. A grid that is split or merged has its id retired: it leaves a tombstone naming
/// the grids that took its place and the transform from its voxels to theirs, so a saved reference to it still resolves
/// ([`GridStore::resolve`]).
#[derive(Clone, Debug, PartialEq)]
pub struct GridStore {
    next: u64,
    /// The number the next file of a ship will have (see [`ShipFileName`]).
    next_serial: u64,
    grids: BTreeMap<GridId, MovingGrid>,
    /// The ships that are in files and not in memory.
    parked: BTreeMap<GridId, ParkedGrid>,
    tombstones: BTreeMap<GridId, Vec<(GridId, LatticeTransform)>>,
}

impl Default for GridStore {
    fn default() -> Self {
        Self::new()
    }
}

impl GridStore {
    pub fn new() -> Self {
        Self {
            next: GridId::FIRST_ALLOCATED,
            next_serial: 0,
            grids: BTreeMap::new(),
            parked: BTreeMap::new(),
            tombstones: BTreeMap::new(),
        }
    }

    /// The store a saved world had: no ship in memory, these in files, this counter and these retired ids. The caller has checked that the
    /// ids are below the counter and that no id is both a ship and retired.
    pub(crate) fn from_saved(
        next: u64,
        next_serial: u64,
        parked: Vec<(GridId, ParkedGrid)>,
        tombstones: Vec<(GridId, Vec<(GridId, LatticeTransform)>)>,
    ) -> Self {
        Self {
            next: next.max(GridId::FIRST_ALLOCATED),
            next_serial,
            grids: BTreeMap::new(),
            parked: parked.into_iter().collect(),
            tombstones: tombstones.into_iter().collect(),
        }
    }

    /// The id the next ship will get: what a save keeps, so that an id is never given twice.
    pub fn next_id(&self) -> u64 {
        self.next
    }

    /// The number the next file of a ship will have: what a save keeps, so that a file is never given the name of an earlier one.
    pub fn next_serial(&self) -> u64 {
        self.next_serial
    }

    /// The number for a file that is about to be written, which is not given again.
    pub fn take_serial(&mut self) -> u64 {
        let serial = self.next_serial;
        self.next_serial += 1;
        serial
    }

    /// Whether anything about this store is worth saving: a ship, a retired id, or ids given out.
    pub fn is_worth_saving(&self) -> bool {
        self.next != GridId::FIRST_ALLOCATED
            || self.next_serial != 0
            || !self.grids.is_empty()
            || !self.parked.is_empty()
            || !self.tombstones.is_empty()
    }

    /// The retired ids and what took their places, in order of id.
    pub fn tombstones(
        &self,
    ) -> impl ExactSizeIterator<Item = (GridId, &[(GridId, LatticeTransform)])> {
        self.tombstones
            .iter()
            .map(|(id, successors)| (*id, successors.as_slice()))
    }

    /// The ships that are in files, in order of id.
    pub fn parked(&self) -> impl ExactSizeIterator<Item = (GridId, &ParkedGrid)> {
        self.parked.iter().map(|(id, parked)| (*id, parked))
    }

    pub fn get_parked(&self, id: GridId) -> Option<&ParkedGrid> {
        self.parked.get(&id)
    }

    pub fn get_parked_mut(&mut self, id: GridId) -> Option<&mut ParkedGrid> {
        self.parked.get_mut(&id)
    }

    /// Whether this is a ship of the world, in memory or in a file.
    pub fn knows(&self, id: GridId) -> bool {
        self.grids.contains_key(&id) || self.parked.contains_key(&id)
    }

    /// The level a ship is in and where it is there, whether it is in memory or in a file.
    pub fn frame_of(&self, id: GridId) -> Option<(LevelIndex, GridPose)> {
        match (self.grids.get(&id), self.parked.get(&id)) {
            (Some(ship), _) => Some((ship.level, ship.pose)),
            (None, Some(parked)) => Some((parked.level, parked.pose)),
            (None, None) => None,
        }
    }

    /// Puts a ship in a file: it leaves memory, and `parked` says where it went. Returns the ship, or `None`, changing nothing, if it is
    /// not in memory.
    pub fn park(&mut self, id: GridId, parked: ParkedGrid) -> Option<MovingGrid> {
        let ship = self.grids.remove(&id)?;
        self.parked.insert(id, parked);
        Some(ship)
    }

    /// Brings a parked ship into memory under its own id. Returns what the world knew of it, or `None`, changing nothing, if no ship of
    /// that id is parked (or one is already in memory).
    pub fn unpark(&mut self, id: GridId, ship: MovingGrid) -> Option<ParkedGrid> {
        if self.grids.contains_key(&id) {
            return None;
        }
        let parked = self.parked.remove(&id)?;
        self.grids.insert(id, ship);
        Some(parked)
    }

    /// Forgets a ship that is in a file (its file is the caller's to remove), leaving no tombstone.
    pub fn remove_parked(&mut self, id: GridId) -> Option<ParkedGrid> {
        self.parked.remove(&id)
    }

    fn allocate(&mut self) -> GridId {
        let id = GridId::new(self.next);
        self.next += 1;
        id
    }

    /// Adds a grid under a new id.
    pub fn create(&mut self, grid: MovingGrid) -> GridId {
        let id = self.allocate();
        self.grids.insert(id, grid);
        id
    }

    pub fn get(&self, id: GridId) -> Option<&MovingGrid> {
        self.grids.get(&id)
    }

    pub fn get_mut(&mut self, id: GridId) -> Option<&mut MovingGrid> {
        self.grids.get_mut(&id)
    }

    /// The grids, in order of id.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (GridId, &MovingGrid)> {
        self.grids.iter().map(|(id, grid)| (*id, grid))
    }

    pub fn len(&self) -> usize {
        self.grids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.grids.is_empty()
    }

    /// Removes a grid for good (a ship that was destroyed), leaving no tombstone: nothing took its place.
    pub fn remove(&mut self, id: GridId) -> Option<MovingGrid> {
        self.grids.remove(&id)
    }

    /// Replaces a grid by others, as a split or a merge does: each of `parts` is added under a new id (so none keeps the id of
    /// the grid, and every one is greater than it), the grid is removed, and its id leaves a tombstone naming the parts with the
    /// transform from the old grid's voxels to each. Returns the old grid and the new ids in order, or `None`, changing nothing, if
    /// there is no such grid.
    pub fn retire(
        &mut self,
        id: GridId,
        parts: Vec<(MovingGrid, LatticeTransform)>,
    ) -> Option<(MovingGrid, Vec<GridId>)> {
        let old = self.grids.remove(&id)?;
        let mut successors = Vec::with_capacity(parts.len());
        for (grid, transform) in parts {
            successors.push((self.create(grid), transform));
        }
        let ids = successors.iter().map(|(id, _)| *id).collect();
        self.tombstones.insert(id, successors);
        Some((old, ids))
    }

    /// Replaces several grids by one, as a merge does: `merged` is added under a new id (so it keeps none of theirs, and the id is
    /// greater than all of them), the grids are removed, and each id leaves a tombstone naming the merged grid with the transform from
    /// that grid's voxels to its. Returns the old grids in the order given and the new id, or `None`, changing nothing, if an id is
    /// not a grid or is named twice.
    pub fn merge(
        &mut self,
        grids: &[(GridId, LatticeTransform)],
        merged: MovingGrid,
    ) -> Option<(Vec<MovingGrid>, GridId)> {
        let distinct = grids
            .iter()
            .enumerate()
            .all(|(index, (id, _))| !grids[..index].iter().any(|(earlier, _)| earlier == id));
        if !distinct || !grids.iter().all(|(id, _)| self.grids.contains_key(id)) {
            return None;
        }
        let old = grids
            .iter()
            .map(|(id, _)| self.grids.remove(id).expect("the grid exists"))
            .collect();
        let merged_id = self.create(merged);
        for &(id, transform) in grids {
            self.tombstones.insert(id, vec![(merged_id, transform)]);
        }
        Some((old, merged_id))
    }

    /// Removes a grid whose blocks were written into another, which keeps its own id: the removed id leaves a tombstone naming `into` with the transform from
    /// its voxels to the other's. Returns the removed grid, or `None`, changing nothing, if either is not a grid or they are one.
    pub fn absorb(
        &mut self,
        id: GridId,
        into: GridId,
        transform: LatticeTransform,
    ) -> Option<MovingGrid> {
        if id == into || !self.grids.contains_key(&into) {
            return None;
        }
        let old = self.grids.remove(&id)?;
        self.tombstones.insert(id, vec![(into, transform)]);
        Some(old)
    }

    /// The grids that now hold what the grid with this id held, each with the transform from the old id's voxels to its own: the
    /// grid itself (with no transform) if it still exists, the parts it was split into or the grid it was merged into (following
    /// further splits and merges), or nothing if the grid is gone.
    pub fn resolve(&self, id: GridId) -> Vec<(GridId, LatticeTransform)> {
        if self.knows(id) {
            return vec![(id, LatticeTransform::IDENTITY)];
        }
        let Some(successors) = self.tombstones.get(&id) else {
            return Vec::new();
        };
        successors
            .iter()
            .flat_map(|&(successor, first)| {
                self.resolve(successor)
                    .into_iter()
                    .filter_map(move |(grid, next)| Some((grid, first.then(next)?)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use voxel_grid::{ChannelDefinition, GridLayout};
    use voxel_space::{LatticeRotation, Orientation, VoxelCoordinate};

    use super::*;

    fn grid(marker: u32) -> MovingGrid {
        let layout = Arc::new(
            GridLayout::new(vec![ChannelDefinition::number(
                "test:marker".parse().unwrap(),
                0,
            )])
            .unwrap(),
        );
        let mut voxels = Grid::new(layout);
        let channel = voxels.layout().channel_indices().next().unwrap();
        voxels.set(channel, VoxelCoordinate::new(1, 2, 3), marker);
        MovingGrid {
            level: LevelIndex::MAIN,
            pose: GridPose::identity(),
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
            mass: 0.0,
            centre_of_mass: [0.0; 3],
            held: false,
            frozen: false,
            on_pilot_logout: PilotLogout::Stay,
            voxels,
            terrain: None,
            rail: None,
            sections: Vec::new(),
        }
    }

    #[test]
    fn a_place_on_a_ship_moves_with_the_ship_and_a_turning_ship_carries_places_around_its_centre_of_mass()
     {
        let mut ship = grid(1);
        ship.pose = GridPose::new(
            WorldPosition::new(BrickCoordinate::new(2, 0, 0), [1.0, 2.0, 3.0]),
            // Turned a quarter about the vertical axis: its x is the world's -z.
            Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
        );
        ship.centre_of_mass = [4.0, 1.0, 4.0];
        ship.linear_velocity = [1.0, 2.0, 3.0];
        let there =
            |x: f32, y: f32, z: f32| WorldPosition::new(BrickCoordinate::new(2, 0, 0), [x, y, z]);
        // Not turning: every place moves at the ship's speed.
        assert_eq!(ship.velocity_at(there(7.0, 2.0, 9.0)), [1.0, 2.0, 3.0]);
        // Turning at 2 radians a second about the vertical axis. The centre of mass is the ship's (4, 1, 4) turned: from the origin
        // at (1, 2, 3), 4 along the ship's x is 4 along the world's -z and 4 along its z is 4 along the world's x, so it is at
        // (5, 3, -1) from the brick's corner. A place 3 along the world's +x from it goes round: omega x r = (0, 2, 0) x (3, 0, 0) =
        // (0, 0, -6).
        ship.angular_velocity = [0.0, 2.0, 0.0];
        let centre = there(5.0, 3.0, -1.0);
        assert_eq!(ship.velocity_at(centre), [1.0, 2.0, 3.0]);
        let moved = ship.velocity_at(there(8.0, 3.0, -1.0));
        assert!(
            (moved[0] - 1.0).abs() < 1e-5
                && (moved[1] - 2.0).abs() < 1e-5
                && (moved[2] - (3.0 - 6.0)).abs() < 1e-5,
            "{moved:?}"
        );
        // A place above it, 2 up, with the turn about the world's x axis (1 radian a second): (1, 0, 0) x (0, 2, 0) = (0, 0, 2).
        ship.angular_velocity = [1.0, 0.0, 0.0];
        let above = ship.velocity_at(there(5.0, 5.0, -1.0));
        assert!(
            (above[0] - 1.0).abs() < 1e-5
                && (above[1] - 2.0).abs() < 1e-5
                && (above[2] - 5.0).abs() < 1e-5,
            "{above:?}"
        );
        // A ship is as fast as is on record, held or not: a held ship that something moves goes at the speed it was told.
        ship.held = true;
        assert_eq!(ship.velocity_at(there(5.0, 5.0, -1.0)), above);
    }

    #[test]
    fn a_ship_turning_about_no_particular_axis_carries_a_place_by_the_cross_product_in_all_three_components()
     {
        // Turning (1, 2, 3) radians a second, 4 along x, 5 up and 6 along z from the centre of mass, which is the origin of an unturned ship:
        // (1, 2, 3) x (4, 5, 6) = (2 * 6 - 3 * 5, 3 * 4 - 1 * 6, 1 * 5 - 2 * 4) = (-3, 6, -3). Every term of every component is not zero.
        let mut ship = grid(1);
        ship.angular_velocity = [1.0, 2.0, 3.0];
        let place = WorldPosition::new(BrickCoordinate::ORIGIN, [4.0, 5.0, 6.0]);
        assert_eq!(ship.velocity_at(place), [-3.0, 6.0, -3.0]);
        // With the ship's own speed added.
        ship.linear_velocity = [10.0, 20.0, 30.0];
        assert_eq!(ship.velocity_at(place), [7.0, 26.0, 27.0]);
    }

    fn shift(x: i64) -> LatticeTransform {
        LatticeTransform {
            offset: [x, 0, 0],
            ..LatticeTransform::IDENTITY
        }
    }

    /// A ship with a brick at each of these places and this pose.
    fn ship_with_bricks(bricks: &[[i64; 3]], pose: GridPose) -> MovingGrid {
        let mut ship = grid(1);
        ship.voxels = Grid::new(Arc::clone(ship.voxels.layout()));
        let channel = ship.voxels.layout().channel_indices().next().unwrap();
        for brick in bricks {
            let voxel = BrickCoordinate::new(brick[0], brick[1], brick[2]).origin();
            ship.voxels.set(channel, voxel, 1);
        }
        ship.pose = pose;
        ship
    }

    fn bounds(ship: &MovingGrid) -> Option<([i64; 3], [i64; 3])> {
        ship.world_brick_bounds().map(|bounds| {
            let (min, max) = (bounds.min(), bounds.max());
            ([min.x(), min.y(), min.z()], [max.x(), max.y(), max.z()])
        })
    }

    #[test]
    fn the_extent_of_a_ship_with_terrain_is_the_terrains_and_whatever_is_stored_beyond_it() {
        let terrain = |low: [i64; 3], high: [i64; 3]| GridTerrain {
            generator: terrain::GeneratorId::from_raw(3),
            extent: BrickBox::new(
                BrickCoordinate::new(low[0], low[1], low[2]),
                BrickCoordinate::new(high[0], high[1], high[2]),
            )
            .unwrap(),
            mass: 1.0e20,
        };
        let extent = |ship: &MovingGrid| {
            ship.brick_extent()
                .map(|(low, high)| ([low.x(), low.y(), low.z()], [high.x(), high.y(), high.z()]))
        };
        // A planet whose edits are not stored is all its terrain, and its bounds in the world are that.
        let mut planet = ship_with_bricks(&[], GridPose::identity());
        planet.terrain = Some(terrain([-20, -20, -20], [19, 19, 19]));
        assert_eq!(extent(&planet), Some(([-20; 3], [19; 3])));
        assert_eq!(bounds(&planet), Some(([-20; 3], [19; 3])));
        // A brick stored inside it changes nothing; one beyond it (a block built in the sky over the planet) widens it.
        let mut edited = ship_with_bricks(&[[0, 5, 0]], GridPose::identity());
        edited.terrain = planet.terrain;
        assert_eq!(extent(&edited), Some(([-20; 3], [19; 3])));
        let mut built = ship_with_bricks(&[[0, 25, 0]], GridPose::identity());
        built.terrain = planet.terrain;
        assert_eq!(extent(&built), Some(([-20; 3], [19, 25, 19])));
    }

    #[test]
    fn the_bricks_a_ship_touches_in_the_world_are_found_for_a_move_a_turn_and_a_ship_with_none() {
        let at = |x: i64, offset: [f32; 3]| {
            GridPose::new(
                WorldPosition::new(BrickCoordinate::new(x, 0, 0), offset),
                voxel_space::Orientation::IDENTITY,
            )
        };
        let bricks = [[0, 0, 0], [2, 1, 0]];
        assert_eq!(bounds(&ship_with_bricks(&[], at(0, [0.0; 3]))), None);
        // Where the bricks of the ship are is where the bricks of the world are, on the lattice ...
        assert_eq!(
            bounds(&ship_with_bricks(&bricks, at(0, [0.0; 3]))),
            Some(([0, 0, 0], [2, 1, 0]))
        );
        assert_eq!(
            bounds(&ship_with_bricks(&bricks, at(10, [0.0; 3]))),
            Some(([10, 0, 0], [12, 1, 0]))
        );
        // ... and half a brick off it, the ship reaches into one more brick.
        assert_eq!(
            bounds(&ship_with_bricks(&bricks, at(0, [16.0, 0.0, 0.0]))),
            Some(([0, 0, 0], [3, 1, 0]))
        );
        // A quarter turn about the vertical axis takes x onto -z and z onto x: bricks 0 to 1 along x become -2 to -1 along z.
        let quarter = LatticeRotation::all()
            .into_iter()
            .find(|rotation| {
                rotation.apply([1, 0, 0]) == [0, 0, -1] && rotation.apply([0, 1, 0]) == [0, 1, 0]
            })
            .unwrap();
        let turned = GridPose::new(
            WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]),
            quarter.orientation(),
        );
        let (low, high) = bounds(&ship_with_bricks(&[[0, 0, 0], [1, 0, 0]], turned)).unwrap();
        // (The quaternion of a quarter turn is not exact in 32 bits, so a face that should be on a boundary may be a hair over it: the
        // bound may be a brick wider, never narrower.)
        assert!(
            (-1..=0).contains(&low[0]) && (0..=1).contains(&high[0]),
            "{low:?} {high:?}"
        );
        assert!(
            (-1..=0).contains(&low[1]) && (0..=1).contains(&high[1]),
            "{low:?} {high:?}"
        );
        assert!(
            (-3..=-2).contains(&low[2]) && (-1..=0).contains(&high[2]),
            "{low:?} {high:?}"
        );
        // At an angle the box of the world is larger than the ship.
        let slanted = GridPose::new(
            WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]),
            voxel_space::Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_4, 0.0),
        );
        let (low, high) = bounds(&ship_with_bricks(&[[0, 0, 0]], slanted)).unwrap();
        assert!(low[0] <= 0 && high[0] >= 1 && low[2] <= -1 && high[2] >= 0);
    }

    #[test]
    fn ids_start_after_the_root_grids_and_are_never_given_twice() {
        let mut store = GridStore::new();
        let first = store.create(grid(1));
        let second = store.create(grid(2));
        assert_eq!(first.get(), GridId::FIRST_ALLOCATED);
        assert_eq!(second.get(), GridId::FIRST_ALLOCATED + 1);
        assert_eq!(first.root_level(), None);
        // A removed grid's id is not reused.
        assert!(store.remove(first).is_some());
        assert_eq!(store.create(grid(3)).get(), GridId::FIRST_ALLOCATED + 2);
        assert_eq!(store.len(), 2);
        assert_eq!(
            store.iter().map(|(id, _)| id.get()).collect::<Vec<_>>(),
            [GridId::FIRST_ALLOCATED + 1, GridId::FIRST_ALLOCATED + 2]
        );
    }

    #[test]
    fn a_removed_grid_is_gone_and_resolves_to_nothing() {
        let mut store = GridStore::new();
        let id = store.create(grid(1));
        assert_eq!(store.resolve(id), [(id, LatticeTransform::IDENTITY)]);
        assert_eq!(store.remove(id), Some(grid(1)));
        assert_eq!(store.remove(id), None);
        assert_eq!(store.get(id), None);
        assert_eq!(store.resolve(id), []);
        assert_eq!(store.resolve(GridId::MAIN), []);
    }

    #[test]
    fn splitting_gives_every_part_a_new_id_and_the_old_one_resolves_to_all_of_them() {
        let mut store = GridStore::new();
        let ship = store.create(grid(1));
        let (old, parts) = store
            .retire(
                ship,
                vec![
                    (grid(2), shift(0)),
                    (grid(3), shift(5)),
                    (grid(4), shift(9)),
                ],
            )
            .unwrap();
        assert_eq!(old, grid(1));
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|part| *part > ship));
        assert!(!parts.contains(&ship));
        assert_eq!(store.get(ship), None);
        assert_eq!(
            store.resolve(ship),
            [
                (parts[0], shift(0)),
                (parts[1], shift(5)),
                (parts[2], shift(9))
            ]
        );
        // An id that was never a grid still resolves to nothing, and retiring a missing grid changes nothing.
        assert_eq!(store.retire(ship, vec![(grid(9), shift(0))]), None);
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn a_merge_gives_the_result_a_new_id_and_both_old_ids_resolve_to_it() {
        let mut store = GridStore::new();
        let (left, right) = (store.create(grid(1)), store.create(grid(2)));
        let (old, merged) = store
            .merge(&[(left, shift(0)), (right, shift(7))], grid(3))
            .unwrap();
        assert_eq!(old, [grid(1), grid(2)]);
        assert!(merged > left && merged > right);
        assert_eq!(store.len(), 1);
        assert_eq!(store.resolve(left), [(merged, shift(0))]);
        assert_eq!(store.resolve(right), [(merged, shift(7))]);
        // The merged grid is split later: both old ids now lead to the parts, with the transforms of both steps.
        let (_, parts) = store
            .retire(merged, vec![(grid(5), shift(100)), (grid(6), shift(200))])
            .unwrap();
        assert_eq!(
            store.resolve(left),
            [(parts[0], shift(100)), (parts[1], shift(200))]
        );
        let voxel = VoxelCoordinate::new(1, 2, 3);
        let (found, transform) = store.resolve(right)[1];
        assert_eq!(found, parts[1]);
        assert_eq!(
            transform.apply(voxel),
            Some(VoxelCoordinate::new(208, 2, 3))
        );
    }

    #[test]
    fn a_grid_absorbed_into_another_leaves_a_tombstone_naming_the_other_which_keeps_its_id() {
        let mut store = GridStore::new();
        let (planet, ship) = (store.create(grid(1)), store.create(grid(2)));
        let before = store.clone();
        let missing = GridId::new(GridId::FIRST_ALLOCATED + 50);
        // Into itself, from a grid that is not there, into one that is not there: nothing changes.
        assert_eq!(store.absorb(ship, ship, shift(0)), None);
        assert_eq!(store.absorb(missing, planet, shift(0)), None);
        assert_eq!(store.absorb(ship, missing, shift(0)), None);
        assert_eq!(store, before);
        // The ship is written into the planet, which is the lower id: the planet is still there, and the ship's id leads to it.
        assert_eq!(store.absorb(ship, planet, shift(7)), Some(grid(2)));
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(planet), Some(&grid(1)));
        assert_eq!(store.resolve(ship), [(planet, shift(7))]);
        assert_eq!(
            store.resolve(planet),
            [(planet, LatticeTransform::IDENTITY)]
        );
        // It cannot be absorbed twice.
        assert_eq!(store.absorb(ship, planet, shift(0)), None);
    }

    #[test]
    fn a_merge_that_names_a_missing_grid_or_the_same_grid_twice_changes_nothing() {
        let mut store = GridStore::new();
        let (left, right) = (store.create(grid(1)), store.create(grid(2)));
        let before = store.clone();
        let missing = GridId::new(GridId::FIRST_ALLOCATED + 50);
        assert_eq!(
            store.merge(&[(left, shift(0)), (missing, shift(0))], grid(3)),
            None
        );
        assert_eq!(
            store.merge(&[(left, shift(0)), (left, shift(1))], grid(3)),
            None
        );
        assert_eq!(store, before);
        assert!(store.merge(&[(right, shift(0))], grid(3)).is_some());
    }

    #[test]
    fn turns_along_a_chain_compose_in_order() {
        let mut store = GridStore::new();
        let first = store.create(grid(1));
        // A turn that moves the x axis, so that the order of the two steps shows.
        let turn = LatticeRotation::all()
            .into_iter()
            .find(|rotation| rotation.apply([1, 0, 0]) != [1, 0, 0])
            .unwrap();
        let there = LatticeTransform {
            rotation: turn,
            offset: [10, 0, 0],
        };
        let (_, second) = store.retire(first, vec![(grid(2), there)]).unwrap();
        let (_, third) = store
            .retire(second[0], vec![(grid(3), shift(1000))])
            .unwrap();
        let (found, transform) = store.resolve(first)[0];
        assert_eq!(found, third[0]);
        let voxel = VoxelCoordinate::new(4, 5, 6);
        let expected = shift(1000).apply(there.apply(voxel).unwrap());
        assert_eq!(transform.apply(voxel), expected);
    }
}
