//! The places a player can go to with `/tp` (`docs/specs/places-spec.md`): a body (a planet, a moon, a star: any ship with terrain) by the name of its generator, and the
//! landmarks of the great rivers. A body is reached on the ground nearest to the player, at a latitude and longitude, or at coordinates of its own.

use crate::generators::great_rivers_generator_identifier;
use crate::generators::{GreatRivers, RiverKnot};
use namespaced_id::NamespacedId;
use server::ServerWorld;
use voxel_space::{BrickBox, GridId, LevelIndex, VoxelCoordinate, WorldPosition};
use world_storage::WorldData;

use crate::aboard::in_voxels;
use crate::landing::EYES_OVER_THE_GROUND;

/// A place a player can go to.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: NamespacedId,
    /// The level it is in.
    pub level: LevelIndex,
    pub kind: PlaceKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlaceKind {
    /// A ship with terrain: a planet, a moon, a star.
    Body(GridId),
    /// A knot of a great river, by its number along the river.
    RiverKnot { index: usize, knot: RiverKnot },
}

fn identifier(text: &str) -> NamespacedId {
    text.parse().expect("a constant identifier is valid")
}

/// The landmark where the great river is narrowest.
pub fn great_river_narrowest() -> NamespacedId {
    identifier("base_blocks:great_river_narrowest")
}

/// The landmark where the great river is widest.
pub fn great_river_widest() -> NamespacedId {
    identifier("base_blocks:great_river_widest")
}

/// Every place in the world: the bodies in the order they were made (a body is named as the generator that makes its ground is: the name of its file in a pack, such as
/// `moons:moon`), then the landmarks of the great rivers of the first level that has them.
pub fn places(world: &WorldData) -> Vec<Place> {
    let mut found = Vec::new();
    let generators = world.main().terrain.generators();
    for (grid, ship) in world.grids().iter() {
        let Some(terrain) = ship.terrain else {
            continue;
        };
        if let Some(entry) = generators.entry(terrain.generator) {
            found.push(Place {
                name: entry.identifier().clone(),
                level: ship.level,
                kind: PlaceKind::Body(grid),
            });
        }
    }
    let with_rivers = world.levels().find(|(_, _, level)| {
        level.preferred_default_generator() == &great_rivers_generator_identifier()
    });
    if let Some((level, _, _)) = with_rivers {
        let rivers = GreatRivers::new(world.world_seed);
        let knots: Vec<RiverKnot> = rivers.knots().copied().collect();
        let index_of = |wanted: RiverKnot| {
            knots
                .iter()
                .position(|knot| *knot == wanted)
                .expect("a knot of the river")
        };
        for (name, knot) in [
            (great_river_narrowest(), rivers.narrowest_knot()),
            (great_river_widest(), rivers.widest_knot()),
        ] {
            found.push(Place {
                name,
                level,
                kind: PlaceKind::RiverKnot {
                    index: index_of(knot),
                    knot,
                },
            });
        }
    }
    found
}

/// The place with this name, if the world has one.
pub fn find(world: &WorldData, name: &NamespacedId) -> Option<Place> {
    places(world).into_iter().find(|place| place.name == *name)
}

/// One coordinate as a player typed it: the whole voxel (exact for any number of digits of a whole number) and how far into it, as a fraction. `None` for what is not a
/// finite number.
pub fn parse_coordinate(text: &str) -> Option<(i128, f32)> {
    if let Ok(whole) = text.parse::<i128>() {
        return Some((whole, 0.0));
    }
    let value: f64 = text.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    let floor = value.floor();
    Some((floor as i128, (value - floor) as f32))
}

/// The position of three coordinates as `parse_coordinate` gives them, or `None` if it is outside the world.
pub fn position_of(coordinates: [(i128, f32); 3]) -> Option<WorldPosition> {
    let voxel =
        VoxelCoordinate::try_new(coordinates[0].0, coordinates[1].0, coordinates[2].0).ok()?;
    let (brick, local) = voxel.split();
    Some(WorldPosition::new(
        brick,
        [
            f32::from(local.x()) + coordinates[0].1,
            f32::from(local.y()) + coordinates[1].1,
            f32::from(local.z()) + coordinates[2].1,
        ],
    ))
}

/// Where on a body a player is to arrive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyArrival {
    /// On the ground nearest to where they are: where the line from the centre of the body to them meets it (the pole, if they are not in the body's level or are at
    /// its centre).
    Nearest,
    /// On the ground at a latitude and a longitude, in degrees.
    Direction { latitude: f64, longitude: f64 },
    /// At a place in the body's own axes, in voxels.
    Point([(i128, f32); 3]),
}

/// Where a player has arrived on a body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landed {
    /// Where their eyes are, in the world.
    pub position: WorldPosition,
    /// The latitude and longitude, in degrees, of the line they were put on (none for a place given in coordinates).
    pub direction: Option<(f64, f64)>,
}

/// The direction of a latitude and a longitude in a body's own axes: the north pole is +y, longitude 0 is along +x and longitude 90 along -z (east is counterclockwise seen from
/// the north pole).
pub fn direction_of(latitude: f64, longitude: f64) -> [f64; 3] {
    let (latitude, longitude) = (latitude.to_radians(), longitude.to_radians());
    let flat = libm::cos(latitude);
    [
        flat * libm::cos(longitude),
        libm::sin(latitude),
        0.0 - flat * libm::sin(longitude),
    ]
}

/// The latitude and longitude, in degrees, of a direction in a body's own axes (the longitude of a pole is 0, and from -180 exclusive to 180).
pub fn latitude_and_longitude(direction: [f64; 3]) -> (f64, f64) {
    let length =
        (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2])
            .sqrt();
    let latitude = libm::asin((direction[1] / length).clamp(-1.0, 1.0)).to_degrees();
    let mut longitude = libm::atan2(0.0 - direction[2], direction[0]).to_degrees();
    if longitude <= -180.0 {
        longitude += 360.0;
    }
    // (No minus zero.)
    (latitude + 0.0, longitude + 0.0)
}

/// Puts a player on a body (a ship with terrain): see [`BodyArrival`]. `player` is where their eyes are in the world, if they are in the body's level. An error says
/// why they cannot.
pub fn land_on_body(
    world: &ServerWorld,
    grid: GridId,
    arrival: BodyArrival,
    player: Option<WorldPosition>,
    block_channel: voxel_grid::ChannelIndex,
    air: u32,
) -> Result<Landed, String> {
    let ship = world
        .data()
        .grids()
        .get(grid)
        .ok_or("that body is not here any more")?;
    let terrain = ship.terrain.ok_or("that is not a body with ground")?;
    let direction = match arrival {
        BodyArrival::Point(coordinates) => {
            let local = position_of(coordinates).ok_or("that is outside the world")?;
            let position = ship
                .pose
                .local_to_world(local)
                .ok_or("that is outside the world")?;
            return Ok(Landed {
                position,
                direction: None,
            });
        }
        BodyArrival::Direction {
            latitude,
            longitude,
        } => direction_of(latitude, longitude),
        BodyArrival::Nearest => {
            let from_the_centre = player
                .and_then(|eyes| ship.pose.world_to_local(eyes))
                .map(in_voxels)
                .filter(|vector| vector.iter().any(|component| component.abs() > 1.0e-9));
            match from_the_centre {
                Some(vector) => {
                    let length =
                        (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2])
                            .sqrt();
                    vector.map(|component| component / length)
                }
                None => [0.0, 1.0, 0.0],
            }
        }
    };
    let ground = ground_along(world, grid, direction, terrain.extent, block_channel, air)?
        .ok_or("there is nothing to stand on there")?;
    // The middle of the voxel, and the eyes over its top: the voxel's extent along the direction is half the sum of the parts of the direction.
    let up_the_voxel = 0.5 * (direction[0].abs() + direction[1].abs() + direction[2].abs());
    let eyes = [0, 1, 2].map(|axis| {
        ground[axis] as f64
            + 0.5
            + direction[axis] * (up_the_voxel + f64::from(EYES_OVER_THE_GROUND))
    });
    // A check of the answer against the ground it was found in (the two are made by different code, and a player put in the ground is pushed out by the ticks after, where
    // nothing would say so): the voxels at the eyes and just under them are air.
    for metres in [0.0, 1.0] {
        let at = [0, 1, 2].map(|axis| (eyes[axis] - direction[axis] * metres).floor() as i128);
        let voxel = VoxelCoordinate::try_new(at[0], at[1], at[2])
            .map_err(|_| "that is outside the world")?;
        match world.moving_voxel(grid, block_channel, voxel) {
            Ok(block) if block == air => {}
            Ok(_) => return Err("the place found to stand on is not in the air".to_owned()),
            Err(error) => return Err(format!("the ground cannot be read: {error:?}")),
        }
    }
    let local = WorldPosition::try_from_voxels(eyes).ok_or("that is outside the world")?;
    let position = ship
        .pose
        .local_to_world(local)
        .ok_or("that is outside the world")?;
    Ok(Landed {
        position,
        direction: Some(latitude_and_longitude(direction)),
    })
}

/// The first voxel that is not air on the line from outside the ground of a body (where it leaves the box of its extent) to its centre, along `direction` (a unit vector,
/// in the body's own axes): the top of the ground where that line meets it. Bricks that are air all through are crossed in one step.
fn ground_along(
    world: &ServerWorld,
    grid: GridId,
    direction: [f64; 3],
    extent: BrickBox,
    block_channel: voxel_grid::ChannelIndex,
    air: u32,
) -> Result<Option<[i128; 3]>, String> {
    const EDGE: f64 = 32.0;
    // Where the line leaves the box of the extent.
    let reach = |axis: usize| {
        let (low, high) = match axis {
            0 => (extent.min().x(), extent.max().x()),
            1 => (extent.min().y(), extent.max().y()),
            _ => (extent.min().z(), extent.max().z()),
        };
        ((low.abs()).max(high.abs() + 1) as f64) * EDGE
    };
    let mut distance = (0..3)
        .filter(|&axis| direction[axis].abs() > 1.0e-12)
        .map(|axis| reach(axis) / direction[axis].abs())
        .fold(f64::INFINITY, f64::min);
    if !distance.is_finite() {
        return Ok(None);
    }
    let mut last_brick = None;
    let mut steps = 0u64;
    while distance >= 0.0 {
        steps += 1;
        if steps > 100_000_000 {
            return Err("the ground there is too far to find".to_owned());
        }
        let at = direction.map(|component| component * distance);
        let voxel = VoxelCoordinate::try_new(
            at[0].floor() as i128,
            at[1].floor() as i128,
            at[2].floor() as i128,
        );
        let Ok(voxel) = voxel else {
            distance -= 1.0;
            continue;
        };
        let (brick, _) = voxel.split();
        let all_air = match last_brick {
            Some((known, all_air)) if known == brick => all_air,
            _ => {
                let all_air = matches!(
                    world.moving_uniform_value(grid, block_channel, brick),
                    Ok(Some(block)) if block == air
                );
                last_brick = Some((brick, all_air));
                all_air
            }
        };
        if all_air {
            // To the face of the brick the line leaves through going in, and a little over.
            let origin = brick.origin();
            let corner = [origin.x() as f64, origin.y() as f64, origin.z() as f64];
            let to_the_face = (0..3)
                .filter(|&axis| direction[axis].abs() > 1.0e-12)
                .map(|axis| {
                    // Going in, a coordinate falls where the direction rises.
                    let face = if direction[axis] > 0.0 {
                        corner[axis]
                    } else {
                        corner[axis] + EDGE
                    };
                    (at[axis] - face) / direction[axis]
                })
                .fold(f64::INFINITY, f64::min);
            distance -= to_the_face.max(0.0) + 0.01;
            continue;
        }
        match world.moving_voxel(grid, block_channel, voxel) {
            Ok(block) if block != air => {
                return Ok(Some([voxel.x(), voxel.y(), voxel.z()]));
            }
            Ok(_) => {}
            Err(error) => return Err(format!("the ground cannot be read: {error:?}")),
        }
        distance -= 0.5;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_number_is_exact_at_any_size_and_a_fraction_is_how_far_into_the_voxel() {
        assert_eq!(parse_coordinate("0"), Some((0, 0.0)));
        assert_eq!(parse_coordinate("-3"), Some((-3, 0.0)));
        assert_eq!(parse_coordinate("20.5"), Some((20, 0.5)));
        assert_eq!(parse_coordinate("-3.25"), Some((-4, 0.75)));
        assert_eq!(
            parse_coordinate("4730365236290400000000000"),
            Some((4_730_365_236_290_400_000_000_000, 0.0))
        );
        assert_eq!(parse_coordinate("1e3"), Some((1000, 0.0)));
        for not_a_number in ["", "a", "nan", "inf", "-inf", "1,5", "--1"] {
            assert_eq!(parse_coordinate(not_a_number), None, "{not_a_number:?}");
        }
    }

    #[test]
    fn the_directions_of_latitudes_and_longitudes_are_the_axes_of_a_body() {
        let close =
            |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1.0e-12);
        assert!(close(direction_of(90.0, 0.0), [0.0, 1.0, 0.0]));
        assert!(close(direction_of(-90.0, 123.0), [0.0, -1.0, 0.0]));
        assert!(close(direction_of(0.0, 0.0), [1.0, 0.0, 0.0]));
        assert!(close(direction_of(0.0, 90.0), [0.0, 0.0, -1.0]));
        assert!(close(direction_of(0.0, -90.0), [0.0, 0.0, 1.0]));
        assert!(close(direction_of(0.0, 180.0), [-1.0, 0.0, 0.0]));
    }

    #[test]
    fn a_direction_gives_back_its_latitude_and_longitude_with_no_minus_zero_and_no_minus_180() {
        for (latitude, longitude) in [
            (0.0, 0.0),
            (45.0, 90.0),
            (-30.0, -120.0),
            (10.0, 179.0),
            (0.0, 180.0),
        ] {
            let (found_latitude, found_longitude) =
                latitude_and_longitude(direction_of(latitude, longitude));
            assert!((found_latitude - latitude).abs() < 1.0e-9);
            assert!(
                (found_longitude - longitude).abs() < 1.0e-9,
                "{longitude}: {found_longitude}"
            );
        }
        // Straight along -x is 180, not -180, and a pole's longitude is 0.
        assert_eq!(latitude_and_longitude([-1.0, 0.0, 0.0]), (0.0, 180.0));
        let (latitude, longitude) = latitude_and_longitude([0.0, 1.0, 0.0]);
        assert_eq!(
            (latitude, format!("{longitude:?}")),
            (90.0, "0.0".to_owned())
        );
    }
}
