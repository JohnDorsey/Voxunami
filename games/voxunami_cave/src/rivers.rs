//! Where a player goes to see the great rivers: the eyes in view of a knot, for `/tp base_blocks:great_river_narrowest` and `..._widest` (`places.rs`, `docs/great-rivers-spec.md`).

use terrain::{DensityField, GreatRivers, RiverKnot};
use voxel_space::{LevelOfDetail, VoxelCoordinate};

/// The way a camera with these angles (radians) looks, in the axes of the level: yaw about +y (positive turns toward -x), then pitch up.
pub fn forward(yaw: f32, pitch: f32) -> [f64; 3] {
    let (yaw, pitch) = (f64::from(yaw), f64::from(pitch));
    [
        -libm::cos(pitch) * libm::sin(yaw),
        libm::sin(pitch),
        -libm::cos(pitch) * libm::cos(yaw),
    ]
}

/// How far from a knot a player is put to see it: three radii and two hundred metres.
pub fn viewing_distance(knot: &RiverKnot) -> f64 {
    3.0 * knot.radius() + 200.0
}

/// Where the eyes of a player go to see a knot of a river: the viewing distance back along the way they look, so that the knot is straight ahead, or, if that is
/// in the water (they looked along the river and the place is in it), the place at right angles to the river.
pub fn eyes_in_view_of(rivers: &GreatRivers, knot: &RiverKnot, forward: [f64; 3]) -> [i128; 3] {
    let distance = viewing_distance(knot);
    let ahead = [0, 1, 2].map(|axis| knot.position[axis] - (forward[axis] * distance) as i128);
    let in_water = VoxelCoordinate::try_new(ahead[0], ahead[1], ahead[2])
        .map(|place| rivers.field().sample(place, LevelOfDetail::FULL_DETAIL) > 0.0)
        .unwrap_or(true);
    if in_water { knot.viewpoint() } else { ahead }
}

/// A length in metres in words a player can take in: metres, kilometres, or light years.
pub fn length_in_words(metres: f64) -> String {
    const LIGHT_YEAR: f64 = 9.460_730_472_580_8e15;
    if metres < 10_000.0 {
        format!("{} m", metres.round())
    } else if metres < 0.1 * LIGHT_YEAR {
        format!("{:.3e} km", metres / 1000.0)
    } else {
        format!("{:.3e} light years", metres / LIGHT_YEAR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_looks_down_minus_z_and_yaw_turns_it_toward_minus_x_and_pitch_up() {
        let close = |found: [f64; 3], wanted: [f64; 3]| {
            found
                .iter()
                .zip(wanted)
                .all(|(found, wanted)| (found - wanted).abs() < 1.0e-6)
        };
        let quarter = std::f32::consts::FRAC_PI_2;
        assert!(close(forward(0.0, 0.0), [0.0, 0.0, -1.0]));
        assert!(close(forward(quarter, 0.0), [-1.0, 0.0, 0.0]));
        assert!(close(forward(0.0, quarter), [0.0, 1.0, 0.0]));
        assert!(close(forward(-quarter, 0.0), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn lengths_are_in_metres_kilometres_or_light_years() {
        assert_eq!(length_in_words(80.0), "80 m");
        assert_eq!(length_in_words(9_999.4), "9999 m");
        assert_eq!(length_in_words(12_000_000.0), "1.200e4 km");
        assert_eq!(
            length_in_words(9.460_730_472_580_8e24),
            "1.000e9 light years"
        );
    }

    #[test]
    fn a_player_is_put_back_along_their_view_from_a_knot_unless_that_is_in_the_water() {
        let rivers = GreatRivers::new(2024);
        let knot = rivers.narrowest_knot();
        // Looking along -z: back from the knot along +z unless that is in the river.
        let straight = forward(0.0, 0.0);
        let eyes = eyes_in_view_of(&rivers, &knot, straight);
        let from_knot = [0, 1, 2].map(|axis| (eyes[axis] - knot.position[axis]) as f64);
        let distance = viewing_distance(&knot);
        let in_water = |place: [i128; 3]| {
            rivers.field().sample(
                VoxelCoordinate::new(place[0], place[1], place[2]),
                LevelOfDetail::FULL_DETAIL,
            ) > 0.0
        };
        assert!(!in_water(eyes));
        if eyes == knot.viewpoint() {
            // (The river runs along z here, and the player would have been in it.)
            let ahead = [
                knot.position[0],
                knot.position[1],
                knot.position[2] + distance as i128,
            ];
            assert!(in_water(ahead));
        } else {
            assert!(from_knot[0].abs() < 1.0 && from_knot[1].abs() < 1.0);
            assert!((from_knot[2] - distance).abs() < 1.0);
        }
        // Looking along the river from the end of one: the way out of the water is at right angles.
        let [dx, dy, dz] = knot.direction.map(|component| component as f64);
        let norm = (dx * dx + dy * dy + dz * dz).sqrt();
        let along = [dx / norm, dy / norm, dz / norm];
        let eyes = eyes_in_view_of(&rivers, &knot, along);
        assert!(!in_water(eyes));
    }
}
