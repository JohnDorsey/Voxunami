//! A player and a ship: whether the player stands on it, and whether they have left it.
//!
//! A player on the ground in a level who is standing on a block of a ship is *aboard* it: they are simulated in the ship's own
//! coordinates from then on, carried by it exactly. They stop being aboard when the ship has nothing under them to stand on or fall
//! onto, or they are further from it than a margin. The two conditions cannot both hold for one player in one place (one needs a block
//! under their feet and the other none), so a player at the edge of a deck is not passed back and forth, and neither takes more than the
//! one tick that the rules are looked at.

use grid_bodies::collision::SolidAt;
use voxel_space::{BRICK_EDGE_LENGTH, WorldPosition};
use world_storage::MovingGrid;

/// How far below the feet, in voxels, a block of a ship must be for a player on the ground to be standing on the ship. The controller
/// leaves a gap of a hundredth, and the boxes of a block come in eighths.
pub const STANDING_REACH: f64 = 0.1;

/// How far past all the bricks of a ship, in voxels, the middle of a player's body must be to have left it, whatever is under them.
pub const LEAVING_MARGIN: f64 = 1.0;

/// How far apart the places are that are looked at going down a column of a ship: the finest a block's shape goes.
const SAMPLE: f64 = 0.125;

/// A position as plain numbers, in voxels.
pub fn in_voxels(position: WorldPosition) -> [f64; 3] {
    let (brick, offset) = (position.brick(), position.offset());
    let edge = BRICK_EDGE_LENGTH as f64;
    [
        brick.x() as f64 * edge + f64::from(offset[0]),
        brick.y() as f64 * edge + f64::from(offset[1]),
        brick.z() as f64 * edge + f64::from(offset[2]),
    ]
}

/// The box that holds all the bricks of a ship, in its own coordinates, in voxels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    low: [f64; 3],
    high: [f64; 3],
}

impl Extent {
    /// The extent of a ship's bricks. `None` for a ship that has none.
    pub fn of(ship: &MovingGrid) -> Option<Self> {
        let (low, high) = ship.brick_extent()?;
        let edge = BRICK_EDGE_LENGTH as f64;
        Some(Self {
            low: [low.x(), low.y(), low.z()].map(|brick| brick as f64 * edge),
            high: [high.x(), high.y(), high.z()].map(|brick| (brick + 1) as f64 * edge),
        })
    }

    /// Whether a place is inside the box, or within `margin` of it.
    pub fn contains(&self, place: [f64; 3], margin: f64) -> bool {
        (0..3).all(|axis| {
            place[axis] >= self.low[axis] - margin && place[axis] <= self.high[axis] + margin
        })
    }

    /// How far a place is from the box: nothing inside it, else the distance to the nearest point of it.
    pub fn distance_to(&self, place: [f64; 3]) -> f64 {
        let squared: f64 = (0..3)
            .map(|axis| {
                let outside = (self.low[axis] - place[axis])
                    .max(place[axis] - self.high[axis])
                    .max(0.0);
                outside * outside
            })
            .sum();
        squared.sqrt()
    }

    /// The distances along `direction` from `from` at which a line enters the box and leaves it, the first not before `from`, or
    /// `None` if the line never goes through it from there.
    fn span(&self, from: [f64; 3], direction: [f64; 3]) -> Option<(f64, f64)> {
        let (mut enter, mut leave) = (f64::NEG_INFINITY, f64::INFINITY);
        for axis in 0..3 {
            if direction[axis].abs() < 1e-12 {
                if from[axis] < self.low[axis] || from[axis] > self.high[axis] {
                    return None;
                }
                continue;
            }
            let first = (self.low[axis] - from[axis]) / direction[axis];
            let second = (self.high[axis] - from[axis]) / direction[axis];
            enter = enter.max(first.min(second));
            leave = leave.min(first.max(second));
        }
        (enter <= leave && leave >= 0.0).then_some((enter.max(0.0), leave))
    }
}

/// Whether a block of the ship is under `from` (a place in the ship's own coordinates), going along `down` (a unit vector in them),
/// anywhere in the ship's extent: something to stand on, or to fall onto. A place above the ship is looked at from where the line enters
/// the extent, so a player who jumps from a deck is still over it.
pub fn is_supported(
    hull: &mut impl SolidAt,
    extent: &Extent,
    from: [f64; 3],
    down: [f64; 3],
) -> bool {
    let Some((enter, leave)) = extent.span(from, down) else {
        return false;
    };
    let mut distance = enter.max(STANDING_REACH);
    while distance <= leave {
        let place = [0, 1, 2].map(|axis| from[axis] + down[axis] * distance);
        if hull.solid_at(place) {
            return true;
        }
        distance += SAMPLE;
    }
    false
}

/// Whether a player whose feet are at `feet` in the world, with `down` the way the world pulls (a unit vector in the world's axes),
/// stands on a block of the ship: one is just below the feet.
pub fn stands_on(
    hull: &mut impl SolidAt,
    ship: &MovingGrid,
    feet: WorldPosition,
    down: [f32; 3],
) -> bool {
    let Some(local) = ship.pose.world_to_local(feet) else {
        return false;
    };
    let from = in_voxels(local);
    let down = ship.pose.direction_to_local(down).map(f64::from);
    let place = [0, 1, 2].map(|axis| from[axis] + down[axis] * STANDING_REACH);
    hull.solid_at(place)
}

/// Whether a player aboard a ship has left it: the middle of their body, `centre`, is further than [`LEAVING_MARGIN`] from the ship's
/// bricks, or they are in the air with nothing of the ship under their `feet` along `down` (all in the ship's own coordinates, `down` a
/// unit vector). A player on the ground is standing on the ship, so there is no need to look.
pub fn has_left(
    hull: &mut impl SolidAt,
    extent: &Extent,
    centre: [f64; 3],
    feet: [f64; 3],
    down: [f64; 3],
    on_the_ground: bool,
) -> bool {
    !extent.contains(centre, LEAVING_MARGIN)
        || (!on_the_ground && !is_supported(hull, extent, feet, down))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use block_models::BlockDefinition;
    use grid_bodies::collision::{BlockCollisions, ShipHull};
    use voxel_grid::{ChannelDefinition, ChannelIndex, Grid, GridLayout};
    use voxel_space::{BrickCoordinate, GridPose, LevelIndex, Orientation, VoxelCoordinate};
    use world_storage::PilotLogout;

    use super::*;
    use crate::block_properties::test_table;

    const STONE: u32 = 1;

    fn collisions() -> BlockCollisions {
        let air = BlockDefinition::air();
        let stone = BlockDefinition::cube([0.5; 3]);
        BlockCollisions::new(test_table(&[Some(&air), Some(&stone)]), None)
    }

    /// A ship with a floor of stone two voxels thick (y 0 and 1) from x and z 0 to 9, and a block standing at (2, 2, 2), in a pose.
    fn ship(pose: GridPose) -> (MovingGrid, ChannelIndex, ChannelIndex) {
        let layout = Arc::new(
            GridLayout::new(vec![
                ChannelDefinition::number("test:block".parse().unwrap(), 0),
                ChannelDefinition::number("test:state".parse().unwrap(), 0),
            ])
            .unwrap(),
        );
        let mut voxels = Grid::new(layout);
        let mut channels = voxels.layout().channel_indices();
        let (block, state) = (channels.next().unwrap(), channels.next().unwrap());
        for x in 0..10 {
            for z in 0..10 {
                for y in 0..2 {
                    voxels.set(block, VoxelCoordinate::new(x, y, z), STONE);
                }
            }
        }
        voxels.set(block, VoxelCoordinate::new(2, 2, 2), STONE);
        let ship = MovingGrid {
            level: LevelIndex::MAIN,
            pose,
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
        };
        (ship, block, state)
    }

    fn at(x: f32, y: f32, z: f32) -> WorldPosition {
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z])
    }

    #[test]
    fn the_extent_of_a_ship_is_the_box_of_its_bricks_and_a_margin_widens_it() {
        let (mut ship, block, _) = ship(GridPose::identity());
        let extent = Extent::of(&ship).unwrap();
        assert_eq!(extent.low, [0.0; 3]);
        assert_eq!(extent.high, [32.0; 3]);
        // A second brick, on the next x.
        ship.voxels
            .set(block, VoxelCoordinate::new(40, 5, 5), STONE);
        let extent = Extent::of(&ship).unwrap();
        assert_eq!((extent.low, extent.high), ([0.0; 3], [64.0, 32.0, 32.0]));
        assert!(extent.contains([63.9, 1.0, 1.0], 0.0));
        assert!(!extent.contains([64.1, 1.0, 1.0], 0.0));
        assert!(extent.contains([64.9, 1.0, 1.0], 1.0));
        assert!(!extent.contains([65.1, 1.0, 1.0], 1.0));
        assert!(extent.contains([-1.0, -1.0, -1.0], 1.0));
        assert!(!extent.contains([-1.1, 1.0, 1.0], 1.0));
        // A ship with nothing has no extent.
        ship.voxels = Grid::new(Arc::clone(ship.voxels.layout()));
        assert_eq!(Extent::of(&ship), None);
    }

    #[test]
    fn a_place_is_as_far_from_the_extent_as_the_nearest_point_of_it() {
        let extent = Extent {
            low: [0.0; 3],
            high: [32.0; 3],
        };
        assert_eq!(extent.distance_to([5.0, 5.0, 5.0]), 0.0);
        assert_eq!(extent.distance_to([32.0, 0.0, 32.0]), 0.0);
        assert_eq!(extent.distance_to([40.0, 10.0, 10.0]), 8.0);
        assert_eq!(extent.distance_to([10.0, -3.0, 10.0]), 3.0);
        // Past a corner, it is the distance to the corner: (3, 4) away is 5.
        assert_eq!(extent.distance_to([35.0, 36.0, 10.0]), 5.0);
    }

    #[test]
    fn a_line_goes_through_the_extent_from_where_it_enters_to_where_it_leaves_or_misses_it() {
        let extent = Extent {
            low: [0.0; 3],
            high: [32.0; 3],
        };
        // From inside, down: out through the bottom.
        assert_eq!(
            extent.span([5.0, 10.0, 5.0], [0.0, -1.0, 0.0]),
            Some((0.0, 10.0))
        );
        // From above, down: in through the top at 8, out at 40.
        assert_eq!(
            extent.span([5.0, 40.0, 5.0], [0.0, -1.0, 0.0]),
            Some((8.0, 40.0))
        );
        // From below, down: it never goes through from there.
        assert_eq!(extent.span([5.0, -3.0, 5.0], [0.0, -1.0, 0.0]), None);
        // Beside it, going along it: it never does.
        assert_eq!(extent.span([40.0, 10.0, 5.0], [0.0, -1.0, 0.0]), None);
        // Slantwise.
        let (enter, leave) = extent.span([-4.0, 4.0, 5.0], [1.0, 0.0, 0.0]).unwrap();
        assert_eq!((enter, leave), (4.0, 36.0));
    }

    #[test]
    fn something_is_under_a_place_over_the_deck_and_over_the_air_beside_it_and_above_the_deck_too()
    {
        let collisions = collisions();
        let (ship, block_channel, state_channel) = ship(GridPose::identity());
        let extent = Extent::of(&ship).unwrap();
        let mut hull = ShipHull {
            voxels: &ship.voxels,
            block_channel,
            state_channel,
            orientation_channel: None,
            collisions: &collisions,
        };
        let down = [0.0, -1.0, 0.0];
        // Over the floor: standing on it, a little above it, and well above the deck but inside the ship's box.
        assert!(is_supported(&mut hull, &extent, [5.0, 2.0, 5.0], down));
        assert!(is_supported(&mut hull, &extent, [5.0, 20.0, 5.0], down));
        // Above the box altogether, as a player is in a jump from the deck: the line is followed from where it enters the box.
        assert!(is_supported(&mut hull, &extent, [5.0, 40.0, 5.0], down));
        // Over the part of the box that has no floor: nothing, and so the player falls past the ship.
        assert!(!is_supported(&mut hull, &extent, [20.0, 3.0, 20.0], down));
        // Beside the box.
        assert!(!is_supported(&mut hull, &extent, [50.0, 3.0, 5.0], down));
        // Under the floor going down.
        assert!(!is_supported(&mut hull, &extent, [5.0, -1.0, 5.0], down));
        // Sideways along the floor, the column is a row: a player in a ship rolled a quarter turn is over the wall that is now their floor.
        assert!(is_supported(
            &mut hull,
            &extent,
            [20.0, 0.5, 5.0],
            [-1.0, 0.0, 0.0]
        ));
        assert!(!is_supported(
            &mut hull,
            &extent,
            [20.0, 5.0, 5.0],
            [-1.0, 0.0, 0.0]
        ));
    }

    #[test]
    fn a_player_stands_on_a_block_that_is_a_tenth_below_their_feet_and_not_one_that_is_more() {
        let collisions = collisions();
        let (ship, block_channel, state_channel) = ship(GridPose::identity());
        let mut hull = ShipHull {
            voxels: &ship.voxels,
            block_channel,
            state_channel,
            orientation_channel: None,
            collisions: &collisions,
        };
        let down = [0.0, -32.0, 0.0].map(|c: f32| c / 32.0);
        // The top of the floor is at 2. Feet on it, a hair above it (the gap the controller leaves), and not on it.
        assert!(stands_on(&mut hull, &ship, at(5.0, 2.0, 5.0), down));
        assert!(stands_on(&mut hull, &ship, at(5.0, 2.05, 5.0), down));
        assert!(!stands_on(&mut hull, &ship, at(5.0, 2.2, 5.0), down));
        // Over the edge of the deck.
        assert!(!stands_on(&mut hull, &ship, at(10.5, 2.0, 5.0), down));
        // On top of the block that stands on the deck.
        assert!(stands_on(&mut hull, &ship, at(2.5, 3.0, 2.5), down));
    }

    #[test]
    fn a_ship_turned_over_stands_a_player_on_the_face_of_its_floor_that_is_up_in_the_world() {
        // A half turn about x: ship (x, y, z) is world (10 + x, 20 - y, 10 - z). The floor, y 0 to 2 in the ship, is the world heights
        // 18 to 20, and what the ship calls the underside of the floor (y = 0) is the face that is up in the world.
        let collisions = collisions();
        let pose = GridPose::new(
            at(10.0, 20.0, 10.0),
            Orientation::from_components([1.0, 0.0, 0.0, 0.0]).unwrap(),
        );
        let (ship, block_channel, state_channel) = ship(pose);
        let mut hull = ShipHull {
            voxels: &ship.voxels,
            block_channel,
            state_channel,
            orientation_channel: None,
            collisions: &collisions,
        };
        let down = [0.0, -1.0, 0.0];
        // Feet on the face that is up in the world: the floor is under them, along the world's down.
        assert!(stands_on(&mut hull, &ship, at(15.0, 20.05, 5.0), down));
        // Under the floor (feet at 17.9, in air): the floor is above them, and what is below them is air.
        assert!(!stands_on(&mut hull, &ship, at(15.0, 17.9, 5.0), down));
        // Where the floor ends, along the ship's x: world x 20 and more.
        assert!(!stands_on(&mut hull, &ship, at(20.5, 20.05, 5.0), down));
        // The world's down is the ship's up, so "under" the player in the ship's own terms is above them, and a column from their feet
        // runs from the face the other way.
        let extent = Extent::of(&ship).unwrap();
        let local_down = ship
            .pose
            .direction_to_local([0.0, -1.0, 0.0])
            .map(f64::from);
        assert!(local_down[1] > 0.999);
        let feet = in_voxels(ship.pose.world_to_local(at(15.0, 20.05, 5.0)).unwrap());
        assert!(is_supported(&mut hull, &extent, feet, local_down));
    }

    #[test]
    fn a_player_has_left_when_far_enough_off_or_in_the_air_with_nothing_under_them() {
        let collisions = collisions();
        let (ship, block_channel, state_channel) = ship(GridPose::identity());
        let extent = Extent::of(&ship).unwrap();
        let mut hull = ShipHull {
            voxels: &ship.voxels,
            block_channel,
            state_channel,
            orientation_channel: None,
            collisions: &collisions,
        };
        let down = [0.0, -1.0, 0.0];
        let on = |hull: &mut ShipHull<'_>, centre: [f64; 3], feet: [f64; 3], ground| {
            has_left(hull, &extent, centre, feet, down, ground)
        };
        // Standing on the deck, and jumping from it: aboard.
        assert!(!on(&mut hull, [5.0, 2.9, 5.0], [5.0, 2.0, 5.0], true));
        assert!(!on(&mut hull, [5.0, 3.9, 5.0], [5.0, 3.0, 5.0], false));
        // A jump that takes them above the ship's box, over the deck: still aboard (the margin is a metre, and the column is followed
        // from where it enters the box).
        assert!(!on(&mut hull, [5.0, 33.0, 5.0], [5.0, 32.1, 5.0], false));
        // Past the margin above: left, deck under them or not.
        assert!(on(&mut hull, [5.0, 33.5, 5.0], [5.0, 32.6, 5.0], false));
        // Walked off the edge of the deck, in the air: left at once, though inside the box (there is a floor of air under them).
        assert!(on(&mut hull, [20.0, 2.9, 5.0], [20.0, 2.0, 5.0], false));
        // The same place, on the ground (standing on something the ship has): not left. (A player on the ground in a ship is on a block.)
        assert!(!on(&mut hull, [20.0, 2.9, 5.0], [20.0, 2.0, 5.0], true));
        // Far to the side, on the ground or in the air.
        assert!(on(&mut hull, [40.0, 2.9, 5.0], [40.0, 2.0, 5.0], true));
    }
}
