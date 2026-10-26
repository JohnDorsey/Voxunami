//! Where a ship lands: which cells of it are inside something solid at a pose, and the nearest pose where none is.
//!
//! A ship is its *hull*, the cells that something can bump into, each taken as the place of its centre. A cell is blocked at a pose when
//! its centre is inside terrain or inside a block of another ship. That is not the whole of what overlapping is (two cubes can overlap
//! a little at their corners without either's centre being in the other), but it is deterministic, quick, and finds every overlap that
//! is deeper than half a voxel, which are the ones that have to be asked about; the bodies push the rest apart.

use std::collections::{BTreeSet, HashMap};

use grid_bodies::collision::{BlockCollisions, GeneratedHull, ShipHull, SolidAt};
use physics::Collision;
use server::{GridView, ServerWorld};
use voxel_grid::{ChannelIndex, Grid};
use voxel_space::{GridId, GridPose, LevelIndex, LocalVoxelIndex, VoxelCoordinate, WorldPosition};
use world_storage::MovingGrid;

/// How far a ship is moved to find a place where it fits, at most, in voxels.
pub const MOVE_RADIUS: u32 = 32;

/// The cells of a ship that something can bump into: for each, the voxel it is in (in the ship's own coordinates) and the place of its
/// centre there. In order of height, then of z and x, so that a search that stops at the first cell that is blocked reaches the ones
/// at the bottom, which meet the ground, first.
#[derive(Clone, Debug, PartialEq)]
pub struct Hull {
    cells: Vec<(VoxelCoordinate, WorldPosition)>,
}

impl Hull {
    /// The cells of `ship` whose blocks are not empty to a body, by what `collisions` says of the block and state in each.
    pub fn of(
        ship: &MovingGrid,
        collisions: &BlockCollisions,
        block: ChannelIndex,
        state: ChannelIndex,
        orientation: Option<ChannelIndex>,
    ) -> Self {
        let mut cells = Vec::new();
        for brick in ship.voxels.sorted_brick_coordinates() {
            let contents = ship.voxels.brick(brick).expect("a listed brick exists");
            for local in LocalVoxelIndex::all() {
                let (found_block, found_state) =
                    (contents.get(block, local), contents.get(state, local));
                let found_orientation =
                    orientation.map_or(0, |channel| contents.get(channel, local));
                if collisions.of(found_block, found_state, found_orientation) == Collision::Empty {
                    continue;
                }
                let voxel = brick.voxel_at(local);
                let centre = WorldPosition::new(
                    brick,
                    [
                        f32::from(local.x()) + 0.5,
                        f32::from(local.y()) + 0.5,
                        f32::from(local.z()) + 0.5,
                    ],
                );
                cells.push((voxel, centre));
            }
        }
        cells.sort_by_key(|(voxel, _)| (voxel.y(), voxel.z(), voxel.x()));
        Self { cells }
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The voxel of the cell in this place of the hull, in the ship's own coordinates.
    pub fn voxel(&self, cell: usize) -> VoxelCoordinate {
        self.cells[cell].0
    }

    /// Where the centre of a cell is in the world when the ship is at `pose`, or `None` if that is not a place the world can hold.
    fn centre_at(&self, cell: usize, pose: &GridPose) -> Option<WorldPosition> {
        pose.local_to_world(self.cells[cell].1)
    }

    /// The cells that are blocked at `pose`: those whose centre `solid_at` says is in something solid, and those whose centre is not
    /// a place in the world (nothing is there to rest in, so it counts as a wall). In the order of the hull.
    pub fn blocked_cells(
        &self,
        pose: &GridPose,
        solid_at: &mut impl FnMut(WorldPosition) -> bool,
    ) -> Vec<usize> {
        (0..self.cells.len())
            .filter(|&cell| self.centre_at(cell, pose).is_none_or(&mut *solid_at))
            .collect()
    }

    /// Whether no cell is blocked at `pose`. Stops at the first that is.
    pub fn is_free(
        &self,
        pose: &GridPose,
        solid_at: &mut impl FnMut(WorldPosition) -> bool,
    ) -> bool {
        (0..self.cells.len()).all(|cell| {
            self.centre_at(cell, pose)
                .is_some_and(|place| !solid_at(place))
        })
    }

    /// The nearest pose to `pose`, no farther than `radius` voxels, where the ship is free, turned the way it is: moved by whole voxels
    /// along the world's axes, the least first; of two as far, the higher; of two as far and as high, the one with the lower x, then z.
    /// `None` if there is none (it is not found by looking at the others, so it is the same wherever the search starts from).
    pub fn nearest_free_pose(
        &self,
        pose: &GridPose,
        radius: u32,
        solid_at: &mut impl FnMut(WorldPosition) -> bool,
    ) -> Option<GridPose> {
        offsets_within(radius).into_iter().find_map(|[x, y, z]| {
            let moved = GridPose::new(
                pose.position.translated([x as f32, y as f32, z as f32]),
                pose.orientation,
            );
            self.is_free(&moved, solid_at).then_some(moved)
        })
    }
}

/// The voxel a place of the world is in.
pub fn voxel_of(place: WorldPosition) -> VoxelCoordinate {
    let offset = place.offset();
    // (An offset is in [0, 32); a float that rounded up to 32 is the last voxel.)
    let index = |value: f32| value.floor().clamp(0.0, 31.0) as u8;
    place.brick().voxel_at(LocalVoxelIndex::new(
        index(offset[0]),
        index(offset[1]),
        index(offset[2]),
    ))
}

/// Every whole-voxel move of at most `radius` voxels, nearest first; of two as near, the higher first; of two as near and as high, the one
/// with the lower x, then the lower z.
fn offsets_within(radius: u32) -> Vec<[i32; 3]> {
    let reach = radius as i32;
    let limit = i64::from(radius) * i64::from(radius);
    let mut offsets = Vec::new();
    for x in -reach..=reach {
        for y in -reach..=reach {
            for z in -reach..=reach {
                let squared = i64::from(x).pow(2) + i64::from(y).pow(2) + i64::from(z).pow(2);
                if squared <= limit {
                    offsets.push((squared, [x, y, z]));
                }
            }
        }
    }
    offsets.sort_by_key(|&(squared, [x, y, z])| (squared, std::cmp::Reverse(y), x, z));
    offsets.into_iter().map(|(_, offset)| offset).collect()
}

/// Whether a place inside a voxel (as fractions of it, from 0 to 1 along each axis) is inside what the collision says is there: the whole
/// of a cube, or one of the boxes of a block that fills part of its cell. A place on the top face of a box is not inside it.
pub fn is_inside(collision: &Collision, inside: [f64; 3]) -> bool {
    match collision {
        Collision::Empty => false,
        Collision::Full => true,
        Collision::Partial(boxes) => boxes.iter().any(|part| {
            (0..3).all(|axis| {
                inside[axis] >= f64::from(part.min[axis]) / 8.0
                    && inside[axis] < f64::from(part.max[axis]) / 8.0
            })
        }),
    }
}

/// What a ship can land in: the terrain of its level, the ground of the planets there (ships with terrain, whose ground is the generator's wherever it has not been
/// built on or dug out), and the other ships there. Terrain that cannot be read is a wall.
#[derive(Debug)]
pub struct Obstacles<'a> {
    world: &'a mut ServerWorld,
    level: LevelIndex,
    block_channel: ChannelIndex,
    state_channel: ChannelIndex,
    orientation_channel: Option<ChannelIndex>,
    /// The block that is air.
    air: u32,
    collisions: &'a BlockCollisions,
    /// What the terrain is at the voxels asked about, which a search asks about again and again.
    terrain: HashMap<VoxelCoordinate, Collision>,
    /// The planets of the level as they are now: which and where.
    planets: Vec<(GridId, GridPose)>,
    /// The other ships of the level as they are now: where each is, and its voxels.
    ships: Vec<(GridPose, Grid)>,
}

impl<'a> Obstacles<'a> {
    pub fn new(
        world: &'a mut ServerWorld,
        level: LevelIndex,
        block_channel: ChannelIndex,
        state_channel: ChannelIndex,
        orientation_channel: Option<ChannelIndex>,
        air: u32,
        collisions: &'a BlockCollisions,
    ) -> Self {
        let (planets, ships) = world
            .data()
            .grids()
            .iter()
            .filter(|(_, ship)| ship.level == level)
            .partition::<Vec<_>, _>(|(_, ship)| ship.terrain.is_some());
        let planets = planets
            .into_iter()
            .map(|(id, ship)| (id, ship.pose))
            .collect();
        let ships = ships
            .into_iter()
            .map(|(_, ship)| (ship.pose, ship.voxels.clone()))
            .collect();
        Self {
            world,
            level,
            block_channel,
            state_channel,
            orientation_channel,
            air,
            collisions,
            terrain: HashMap::new(),
            planets,
            ships,
        }
    }

    /// What the terrain is in a voxel.
    pub fn terrain_at(&mut self, voxel: VoxelCoordinate) -> Collision {
        if let Some(known) = self.terrain.get(&voxel) {
            return known.clone();
        }
        let found = match self.world.grid(GridId::root(self.level)) {
            Ok(mut terrain) => match (
                terrain.get(self.block_channel, voxel),
                terrain.get(self.state_channel, voxel),
                self.orientation_channel
                    .map_or(Ok(0), |channel| terrain.get(channel, voxel)),
            ) {
                (Ok(block), Ok(state), Ok(orientation)) => {
                    self.collisions.of(block, state, orientation)
                }
                _ => Collision::Full,
            },
            Err(_) => Collision::Full,
        };
        self.terrain.insert(voxel, found.clone());
        found
    }

    /// Whether a place of the world is inside the terrain.
    pub fn in_terrain(&mut self, place: WorldPosition) -> bool {
        let voxel = voxel_of(place);
        let offset = place.offset();
        let inside = [0, 1, 2].map(|axis| f64::from(offset[axis] - offset[axis].floor()));
        is_inside(&self.terrain_at(voxel), inside)
    }

    /// The voxel of the ground of a planet that a place of the world is inside, if it is (the planet, and the voxel in its own coordinates): the planet's blocks
    /// where it has any, else the generator's.
    pub fn in_a_planet(&self, place: WorldPosition) -> Option<(GridId, VoxelCoordinate)> {
        self.planets.iter().find_map(|&(grid, pose)| {
            let local = pose.world_to_local(place)?;
            let (brick, offset) = (local.brick(), local.offset());
            let here = [0, 1, 2].map(|axis| {
                let brick_start = [brick.x(), brick.y(), brick.z()][axis] * 32;
                brick_start as f64 + f64::from(offset[axis])
            });
            GeneratedHull {
                world: &*self.world,
                grid,
                block_channel: self.block_channel,
                state_channel: self.state_channel,
                orientation_channel: self.orientation_channel,
                air: self.air,
                collisions: self.collisions,
            }
            .solid_at(here)
            .then(|| (grid, voxel_of(local)))
        })
    }

    /// Whether a place of the world is inside a block of another ship.
    pub fn in_a_ship(&self, place: WorldPosition) -> bool {
        self.ships.iter().any(|(pose, voxels)| {
            let Some(local) = pose.world_to_local(place) else {
                return false;
            };
            let (brick, offset) = (local.brick(), local.offset());
            let here = [0, 1, 2].map(|axis| {
                let brick_start = [brick.x(), brick.y(), brick.z()][axis] * 32;
                brick_start as f64 + f64::from(offset[axis])
            });
            ShipHull {
                voxels,
                block_channel: self.block_channel,
                state_channel: self.state_channel,
                orientation_channel: self.orientation_channel,
                collisions: self.collisions,
            }
            .solid_at(here)
        })
    }

    /// Whether a place of the world is inside anything a ship cannot be in.
    pub fn solid_at(&mut self, place: WorldPosition) -> bool {
        self.in_terrain(place) || self.in_a_planet(place).is_some() || self.in_a_ship(place)
    }

    /// The solid voxels of the terrain (the level's, and the ground of its planets) that the given cells of the hull are in when the ship is at `pose`, each once,
    /// with the grid each is in and in that grid's own coordinates.
    pub fn terrain_in_the_way(
        &mut self,
        hull: &Hull,
        cells: &[usize],
        pose: &GridPose,
    ) -> BTreeSet<(GridId, VoxelCoordinate)> {
        let level = GridId::root(self.level);
        cells
            .iter()
            .filter_map(|&cell| hull.centre_at(cell, pose))
            .filter_map(|place| {
                if self.in_terrain(place) {
                    Some((level, voxel_of(place)))
                } else {
                    self.in_a_planet(place)
                }
            })
            .collect()
    }
}

/// Breaks the terrain at these voxels of a level and its planets (each with the grid it is in): every channel goes to its default there. Returns how many voxels it
/// changed.
pub fn break_terrain(
    world: &mut ServerWorld,
    level: LevelIndex,
    voxels: &BTreeSet<(GridId, VoxelCoordinate)>,
) -> usize {
    let Some(layout) = world
        .data()
        .level(level)
        .map(|level| level.overrides.layout().clone())
    else {
        return 0;
    };
    let mut changed = 0;
    for &(grid, voxel) in voxels {
        let Ok(mut terrain) = world.grid(grid) else {
            continue;
        };
        let mut any = false;
        for channel in layout.channel_indices() {
            if let Ok(true) = terrain.set(channel, voxel, layout.default_value(channel)) {
                any = true;
            }
        }
        changed += usize::from(any);
    }
    changed
}

/// Breaks the blocks of a ship at the given cells of its hull: every channel goes to its default there.
pub fn break_cells(ship: &mut MovingGrid, hull: &Hull, cells: &[usize]) {
    let layout = ship.voxels.layout().clone();
    for &cell in cells {
        for channel in layout.channel_indices() {
            ship.voxels
                .set(channel, hull.voxel(cell), layout.default_value(channel));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use block_models::{BlockDefinition, BlockTable};
    use voxel_grid::{ChannelDefinition, Grid, GridLayout};
    use voxel_space::{BrickCoordinate, LevelIndex, Orientation};
    use world_storage::PilotLogout;

    use super::*;

    const AIR: u32 = 0;
    const STONE: u32 = 1;

    fn collisions() -> BlockCollisions {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]);
        BlockCollisions::new(
            BlockTable::from_definitions(&[Some(&air), Some(&stone)]),
            None,
        )
    }

    /// A ship of stone at these voxels (in its own coordinates), with the channels of the block and the state.
    fn ship(voxels: &[[i64; 3]]) -> (MovingGrid, ChannelIndex, ChannelIndex) {
        let layout = Arc::new(
            GridLayout::new(vec![
                ChannelDefinition::number("test:block".parse().unwrap(), AIR),
                ChannelDefinition::number("test:state".parse().unwrap(), 0),
            ])
            .unwrap(),
        );
        let mut grid = Grid::new(Arc::clone(&layout));
        let mut channels = layout.channel_indices();
        let (block, state) = (channels.next().unwrap(), channels.next().unwrap());
        for &[x, y, z] in voxels {
            grid.set(block, VoxelCoordinate::new(x, y, z), STONE);
        }
        let ship = MovingGrid {
            level: LevelIndex::MAIN,
            pose: GridPose::identity(),
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
            mass: 0.0,
            centre_of_mass: [0.0; 3],
            held: false,
            frozen: false,
            on_pilot_logout: PilotLogout::Stay,
            voxels: grid,
            terrain: None,
            rail: None,
            sections: Vec::new(),
        };
        (ship, block, state)
    }

    fn hull(voxels: &[[i64; 3]]) -> Hull {
        let (ship, block, state) = ship(voxels);
        Hull::of(&ship, &collisions(), block, state, None)
    }

    fn pose_at(x: f32, y: f32, z: f32) -> GridPose {
        GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
            Orientation::IDENTITY,
        )
    }

    /// The terrain is these voxels, and nothing else.
    fn terrain(solid: &HashSet<VoxelCoordinate>) -> impl FnMut(WorldPosition) -> bool + '_ {
        |place| solid.contains(&voxel_of(place))
    }

    fn voxels_where(test: impl Fn([i64; 3]) -> bool) -> HashSet<VoxelCoordinate> {
        let mut found = HashSet::new();
        for x in -40..40 {
            for y in -40..40 {
                for z in -40..40 {
                    if test([x, y, z]) {
                        found.insert(VoxelCoordinate::new(x, y, z));
                    }
                }
            }
        }
        found
    }

    #[test]
    fn the_hull_is_the_cells_that_are_not_empty_in_order_of_height_then_z_then_x() {
        let hull = hull(&[
            [2, 1, 0],
            [0, 0, 1],
            [1, 0, 0],
            [0, 1, 0],
            [0, 0, 0],
            [40, 40, 40],
        ]);
        let voxels: Vec<[i64; 3]> = (0..hull.len())
            .map(|cell| {
                let voxel = hull.voxel(cell);
                [voxel.x(), voxel.y(), voxel.z()]
            })
            .collect();
        assert_eq!(
            voxels,
            [
                [0, 0, 0],
                [1, 0, 0],
                [0, 0, 1],
                [0, 1, 0],
                [2, 1, 0],
                [40, 40, 40]
            ]
        );
        assert!(!hull.is_empty());
        // A ship with nothing in it has an empty hull, and fits anywhere.
        let nothing = self::hull(&[]);
        assert!(nothing.is_empty() && nothing.is_free(&pose_at(5.0, 5.0, 5.0), &mut |_| true));
    }

    #[test]
    fn a_cell_is_blocked_when_its_centre_is_in_something_solid_and_not_when_it_only_rests_on_it() {
        // A floor of everything below y = 0, and a ship of 2 by 2 by 2.
        let floor = voxels_where(|[_, y, _]| y < 0);
        let cube = hull(&[
            [0, 0, 0],
            [1, 0, 0],
            [0, 1, 0],
            [1, 1, 0],
            [0, 0, 1],
            [1, 0, 1],
            [0, 1, 1],
            [1, 1, 1],
        ]);
        // On the floor: its lowest cells have their centres half a voxel above it.
        assert!(cube.is_free(&pose_at(3.0, 0.0, 3.0), &mut terrain(&floor)));
        assert_eq!(
            cube.blocked_cells(&pose_at(3.0, 0.0, 3.0), &mut terrain(&floor)),
            Vec::<usize>::new()
        );
        // A voxel down, the four lowest cells are in it, and they are the first four of the hull.
        assert!(!cube.is_free(&pose_at(3.0, -1.0, 3.0), &mut terrain(&floor)));
        assert_eq!(
            cube.blocked_cells(&pose_at(3.0, -1.0, 3.0), &mut terrain(&floor)),
            [0, 1, 2, 3]
        );
        // Between voxels: a ship half a voxel into the floor has the centres of its lowest cells just above it.
        assert!(cube.is_free(&pose_at(3.0, -0.4, 3.0), &mut terrain(&floor)));
        assert!(!cube.is_free(&pose_at(3.0, -0.6, 3.0), &mut terrain(&floor)));
    }

    #[test]
    fn a_turned_ship_is_blocked_where_its_cells_are_in_the_world() {
        // A row of three along the ship's x, turned a quarter about the vertical axis: its x is the world's -z.
        let row = hull(&[[0, 0, 0], [1, 0, 0], [2, 0, 0]]);
        let quarter = GridPose::new(
            WorldPosition::new(BrickCoordinate::ORIGIN, [10.0, 5.0, 10.0]),
            Orientation::from_yaw_pitch(std::f32::consts::FRAC_PI_2, 0.0),
        );
        // The cells' centres are at ship x = 0.5, 1.5, 2.5: world z = 10 - those, in the voxels 9, 8, 7 (x stays 10, y stays 5).
        let only_the_last = voxels_where(|voxel| voxel == [10, 5, 7]);
        assert_eq!(
            row.blocked_cells(&quarter, &mut terrain(&only_the_last)),
            [2]
        );
        let only_the_first = voxels_where(|voxel| voxel == [10, 5, 9]);
        assert_eq!(
            row.blocked_cells(&quarter, &mut terrain(&only_the_first)),
            [0]
        );
        // The same terrain does not touch it unturned.
        let unturned = pose_at(10.0, 5.0, 10.0);
        assert!(row.is_free(&unturned, &mut terrain(&only_the_last)));
        assert!(row.is_free(&unturned, &mut terrain(&only_the_first)));
    }

    #[test]
    fn a_place_that_is_not_in_the_world_is_a_wall() {
        // A pose so far out that the world cannot hold a cell of the ship: it is blocked, not free.
        let far = GridPose::new(
            WorldPosition::new(
                voxel_space::BrickCoordinate::new(voxel_space::BRICK_COORDINATE_MAXIMUM, 0, 0),
                [31.9, 0.0, 0.0],
            ),
            Orientation::IDENTITY,
        );
        let cube = hull(&[[0, 0, 0], [20, 0, 0], [40, 0, 0]]);
        let nothing = HashSet::new();
        let blocked = cube.blocked_cells(&far, &mut terrain(&nothing));
        assert!(!blocked.is_empty());
        assert!(!cube.is_free(&far, &mut terrain(&nothing)));
    }

    #[test]
    fn the_moves_are_nearest_first_then_higher_then_lower_x_then_lower_z() {
        let one = offsets_within(1);
        assert_eq!(one.len(), 7);
        assert_eq!(
            one,
            [
                [0, 0, 0],
                // At a voxel, the higher first, then the lower x, then the lower z.
                [0, 1, 0],
                [-1, 0, 0],
                [0, 0, -1],
                [0, 0, 1],
                [1, 0, 0],
                [0, -1, 0],
            ]
        );
        // Every move within the radius, and no other.
        let two = offsets_within(2);
        assert!(two.iter().all(|[x, y, z]| x * x + y * y + z * z <= 4));
        assert_eq!(two.len(), 33);
        // By squared distance, never backwards.
        let squares: Vec<i32> = two.iter().map(|[x, y, z]| x * x + y * y + z * z).collect();
        assert!(squares.windows(2).all(|pair| pair[0] <= pair[1]));
        // The reach of a move of 32 is the ball of that radius.
        assert_eq!(offsets_within(32).len(), 137_065);
    }

    #[test]
    fn the_nearest_free_pose_is_the_nearest_and_upward_before_sideways() {
        // A ship that is one cell, in the one solid voxel there is: it can be moved a voxel in six ways, and goes up.
        let cell = hull(&[[0, 0, 0]]);
        let alone = voxels_where(|voxel| voxel == [5, 5, 5]);
        let start = pose_at(5.0, 5.0, 5.0);
        let moved = cell
            .nearest_free_pose(&start, MOVE_RADIUS, &mut terrain(&alone))
            .unwrap();
        assert_eq!(moved.position, start.position.translated([0.0, 1.0, 0.0]));
        assert_eq!(moved.orientation, start.orientation);
        // Free where it is: it is not moved.
        let there = cell
            .nearest_free_pose(&pose_at(9.0, 9.0, 9.0), MOVE_RADIUS, &mut terrain(&alone))
            .unwrap();
        assert_eq!(there.position, pose_at(9.0, 9.0, 9.0).position);
    }

    #[test]
    fn the_nearest_free_pose_goes_around_what_cannot_be_gone_through_and_stops_at_the_radius() {
        // A wall: the ship is in the middle of a solid block of 11 on each side, and the nearest way out is up by 6.
        let cell = hull(&[[0, 0, 0]]);
        let block = voxels_where(|[x, y, z]| {
            (0..11).contains(&x) && (0..11).contains(&y) && (0..11).contains(&z)
        });
        let inside = pose_at(5.0, 5.0, 5.0);
        let out = cell
            .nearest_free_pose(&inside, MOVE_RADIUS, &mut terrain(&block))
            .unwrap();
        assert_eq!(out.position, inside.position.translated([0.0, 6.0, 0.0]));
        // With a radius of 5 there is none; with 6 there is.
        assert_eq!(
            cell.nearest_free_pose(&inside, 5, &mut terrain(&block)),
            None
        );
        assert!(
            cell.nearest_free_pose(&inside, 6, &mut terrain(&block))
                .is_some()
        );
        // Solid everywhere within the radius: none.
        let everything = voxels_where(|_| true);
        assert_eq!(
            cell.nearest_free_pose(&inside, 4, &mut terrain(&everything)),
            None
        );
        // And the same answer every time.
        assert_eq!(
            cell.nearest_free_pose(&inside, MOVE_RADIUS, &mut terrain(&block)),
            Some(out)
        );
    }

    #[test]
    fn a_ship_that_is_a_deck_finds_the_way_up_out_of_a_hill_without_looking_at_every_cell_of_it_for_every_place()
     {
        // A deck of 12 by 12 in a hill that is solid below y = 3: it is moved up by 4 to sit on it, and the search asks about few cells.
        let deck = hull(
            &(0..12)
                .flat_map(|x| (0..12).map(move |z| [x, 0, z]))
                .collect::<Vec<_>>(),
        );
        let hill = voxels_where(|[_, y, _]| y < 3);
        let mut asked = 0usize;
        let mut count = |place: WorldPosition| {
            asked += 1;
            hill.contains(&voxel_of(place))
        };
        let start = pose_at(5.0, -1.0, 5.0);
        let found = deck
            .nearest_free_pose(&start, MOVE_RADIUS, &mut count)
            .unwrap();
        assert_eq!(found.position, start.position.translated([0.0, 4.0, 0.0]));
        // Every place that is blocked is given up at its first cell (the lowest are first): the cells looked at are one for each place tried
        // and the 144 of the one that fits, not 144 for each place.
        assert!(asked < 144 + 300, "{asked} cells were looked at");
    }
}
