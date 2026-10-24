//! What a planet hides from a viewer who is outside it: the cubes that are wholly behind its horizon need not be made or drawn.
//!
//! The ground of a planet has a ball in it that is ground all through ([`terrain::SolidBall`], its core). A viewer outside the ball sees nothing that is behind it:
//! the lines from the viewer that touch the ball make a cone, and everything inside the cone and beyond the places they touch is hidden. This is conservative, because
//! the real ground is higher than the core, so the real horizon is nearer the viewer than the core's: a cube that is hidden here is hidden. (It is not exact: it hides
//! cubes that the ground hides and the core does not, no sooner than the core does, which near a planet's surface is some kilometres further than the horizon is.)

use region_octree::BrickCube;
use terrain::SolidBall;
use voxel_space::{BRICK_EDGE_LENGTH, WorldPosition};

/// How far, in voxels, a viewer may be from the place the view was worked out for, and the cull still be right: a viewer that is worked out again when it leaves its
/// brick has not gone farther than a brick's diagonal.
pub const VIEWER_MARGIN: f64 = BRICK_EDGE_LENGTH as f64 * 1.733;

/// Where a place is in voxels, in the coordinates of the grid it is in.
pub fn voxels_of(position: WorldPosition) -> [f64; 3] {
    let brick = position.brick();
    let offset = position.offset();
    let edge = BRICK_EDGE_LENGTH as f64;
    [
        brick.x() as f64 * edge + f64::from(offset[0]),
        brick.y() as f64 * edge + f64::from(offset[1]),
        brick.z() as f64 * edge + f64::from(offset[2]),
    ]
}

fn difference(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[0] - second[0],
        first[1] - second[1],
        first[2] - second[2],
    ]
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

/// Whether everything in `cube` is hidden from `viewer` (in voxels) by `core`, wherever the viewer is within `margin` voxels of that place: the cube is farther than
/// the places where the lines from the viewer touch the core, and the whole of it is inside the cone they make.
///
/// The cube is taken to be its bounding sphere, of centre `q` and radius `r` (made `margin` larger), and the core to be `margin` smaller. With `d` the distance from the
/// viewer to the core's centre and `w` the distance to `q`, the angle `alpha` the cone is half-wide has the sine `radius / d` and the angle the sphere takes up has the
/// sine `r / w`. The sphere is in the cone if its centre is within `alpha` less that angle of the cone's axis, which is a comparison of cosines (no inverse functions),
/// and it is beyond the touching places if its nearest point, `w - r`, is farther than `sqrt(d^2 - radius^2)`, the distance to them.
pub fn is_hidden_by(core: &SolidBall, viewer: [f64; 3], cube: BrickCube, margin: f64) -> bool {
    let edge = cube.edge_length() as f64 * BRICK_EDGE_LENGTH as f64;
    let origin = cube.origin();
    let centre = [
        origin.x() as f64 * BRICK_EDGE_LENGTH as f64 + edge / 2.0,
        origin.y() as f64 * BRICK_EDGE_LENGTH as f64 + edge / 2.0,
        origin.z() as f64 * BRICK_EDGE_LENGTH as f64 + edge / 2.0,
    ];
    // The half of the diagonal of the cube, which is the radius of the smallest sphere that holds it.
    let sphere = edge / 2.0 * 3.0f64.sqrt() + margin;
    let radius = core.radius - margin;

    let to_core = difference(core.centre, viewer);
    let to_cube = difference(centre, viewer);
    let core_distance = dot(to_core, to_core).sqrt();
    let cube_distance = dot(to_cube, to_cube).sqrt();
    // A viewer in the ball, or in the sphere, sees it all.
    if radius <= 0.0 || core_distance <= radius || cube_distance <= sphere {
        return false;
    }
    let sine_of_cone = radius / core_distance;
    let sine_of_sphere = sphere / cube_distance;
    if sine_of_sphere >= sine_of_cone {
        return false;
    }
    let cosine_of_cone = (1.0 - sine_of_cone * sine_of_cone).sqrt();
    let cosine_of_sphere = (1.0 - sine_of_sphere * sine_of_sphere).sqrt();
    // The cosine of the cone less the sphere: the sphere's centre has to be at a smaller angle than that, so with a greater cosine.
    let cosine_of_limit = cosine_of_cone * cosine_of_sphere + sine_of_cone * sine_of_sphere;
    let cosine_of_angle = dot(to_core, to_cube) / (core_distance * cube_distance);
    let touching_distance = (core_distance * core_distance - radius * radius).sqrt();
    cosine_of_angle > cosine_of_limit && cube_distance - sphere > touching_distance
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use voxel_space::BrickCoordinate;

    use super::*;

    fn core() -> SolidBall {
        SolidBall {
            centre: [0.0; 3],
            radius: 1000.0,
        }
    }

    fn cube_at(x: i64, y: i64, z: i64, level: u8) -> BrickCube {
        BrickCube::containing(BrickCoordinate::new(x, y, z), level)
    }

    /// A viewer on the surface of the core at the top (+y), 2 voxels up.
    const VIEWER: [f64; 3] = [0.0, 1002.0, 0.0];

    #[test]
    fn a_place_in_voxels_is_the_brick_and_the_offset() {
        let position = WorldPosition::new(BrickCoordinate::new(3, -2, 0), [1.5, 31.0, 0.25]);
        assert_eq!(voxels_of(position), [97.5, -33.0, 0.25]);
    }

    #[test]
    fn a_cube_on_the_far_side_of_the_core_is_hidden_and_one_on_this_side_or_beside_the_viewer_is_not()
     {
        // Far side: the core's centre is at the origin, so the cube whose centre is 1000 below the origin (bricks are 32 voxels).
        let behind = cube_at(0, -32, 0, 1);
        assert!(is_hidden_by(&core(), VIEWER, behind, 0.0));
        // A cube with the viewer in it, however big, hides nothing.
        assert!(!is_hidden_by(&core(), VIEWER, cube_at(0, 31, 0, 7), 0.0));
        // Beside the viewer, on the ground: not hidden (the horizon is a few dozen voxels off at this height, and these are near).
        for near in [
            cube_at(0, 31, 0, 0),
            cube_at(1, 31, 0, 0),
            cube_at(-2, 31, 3, 1),
            cube_at(0, 31, 6, 2),
        ] {
            assert!(!is_hidden_by(&core(), VIEWER, near, 0.0), "{near:?}");
        }
        // Over the viewer's head and far away: in the open sky, not behind anything.
        assert!(!is_hidden_by(&core(), VIEWER, cube_at(0, 400, 0, 3), 0.0));
        // The viewer inside the core sees all of it.
        assert!(!is_hidden_by(&core(), [0.0; 3], behind, 0.0));
        assert!(!is_hidden_by(
            &core(),
            [10.0, 20.0, 30.0],
            cube_at(0, -32, 0, 0),
            0.0
        ));
    }

    #[test]
    fn a_cube_inside_the_core_is_hidden_from_a_viewer_on_its_surface() {
        // Under the viewer's feet: ground, all of it, so nothing of it is seen, near or far.
        for cube in [
            cube_at(0, 20, 0, 0),
            cube_at(0, 24, 0, 1),
            cube_at(-1, 10, 1, 2),
            cube_at(3, 0, -2, 0),
        ] {
            assert!(is_hidden_by(&core(), VIEWER, cube, 0.0), "{cube:?}");
        }
        // But not the cube the viewer stands in, or the sky over it.
        assert!(!is_hidden_by(&core(), VIEWER, cube_at(0, 31, 0, 0), 0.0));
        assert!(!is_hidden_by(&core(), VIEWER, cube_at(0, 40, 0, 0), 0.0));
    }

    #[test]
    fn the_margin_hides_less_and_never_more() {
        // A cube just inside what is hidden with no margin, and not with a large one.
        let behind_sideways = cube_at(40, -32, 0, 1);
        let hidden_without = is_hidden_by(&core(), VIEWER, behind_sideways, 0.0);
        let hidden_with = is_hidden_by(&core(), VIEWER, behind_sideways, 200.0);
        assert!(hidden_without >= hidden_with);
        // The cube on the axis, far from the edge of the cone, is hidden with any margin that leaves the core something, and not when the margin eats the core.
        let behind = cube_at(0, -32, 0, 1);
        assert!(is_hidden_by(&core(), VIEWER, behind, 56.0));
        assert!(!is_hidden_by(&core(), VIEWER, behind, 1000.0));
        assert!(!is_hidden_by(&core(), VIEWER, behind, 2000.0));
    }

    /// The least distance from `point` to the segment from `from` to `to`.
    fn distance_to_segment(point: [f64; 3], from: [f64; 3], to: [f64; 3]) -> f64 {
        let along = difference(to, from);
        let length_squared = dot(along, along);
        let toward = difference(point, from);
        let share = if length_squared == 0.0 {
            0.0
        } else {
            (dot(toward, along) / length_squared).clamp(0.0, 1.0)
        };
        let nearest = [
            from[0] + along[0] * share,
            from[1] + along[1] * share,
            from[2] + along[2] * share,
        ];
        let gap = difference(point, nearest);
        dot(gap, gap).sqrt()
    }

    fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
        [
            first[1] * second[2] - first[2] * second[1],
            first[2] * second[0] - first[0] * second[2],
            first[0] * second[1] - first[1] * second[0],
        ]
    }

    fn normalized(vector: [f64; 3]) -> [f64; 3] {
        let length = dot(vector, vector).sqrt();
        vector.map(|component| component / length)
    }

    fn moved(from: [f64; 3], by: [f64; 3], distance: f64) -> [f64; 3] {
        [
            from[0] + by[0] * distance,
            from[1] + by[1] * distance,
            from[2] + by[2] * distance,
        ]
    }

    /// The places of a cube that a hidden cube must have behind the core: its eight corners, which are the farthest-out places of it however it is seen, its middle, and
    /// places inside it at these shares of its edge.
    fn places_in(cube: BrickCube, shares: &[(f64, f64, f64)]) -> Vec<[f64; 3]> {
        let edge = cube.edge_length() as f64 * BRICK_EDGE_LENGTH as f64;
        let low = cube.origin();
        let low = [
            low.x() as f64 * 32.0,
            low.y() as f64 * 32.0,
            low.z() as f64 * 32.0,
        ];
        let mut places = Vec::new();
        for corner in 0..8 {
            places.push([
                low[0] + f64::from(corner & 1) * edge,
                low[1] + f64::from(corner >> 1 & 1) * edge,
                low[2] + f64::from(corner >> 2 & 1) * edge,
            ]);
        }
        places.push([
            low[0] + edge / 2.0,
            low[1] + edge / 2.0,
            low[2] + edge / 2.0,
        ]);
        for &(x, y, z) in shares {
            places.push([low[0] + x * edge, low[1] + y * edge, low[2] + z * edge]);
        }
        places
    }

    /// Where the viewer may be, if it is no farther than `margin` from `viewer`: the places `margin` away along the axes, along the diagonals of the cube they make,
    /// and along the directions across the line to the cube (where the viewer's moving changes what it sees of the edge of the shadow the most).
    fn viewers_within(viewer: [f64; 3], toward: [f64; 3], margin: f64) -> Vec<[f64; 3]> {
        let mut directions = Vec::new();
        for x in [-1.0, 0.0, 1.0] {
            for y in [-1.0, 0.0, 1.0] {
                for z in [-1.0, 0.0, 1.0] {
                    if x != 0.0 || y != 0.0 || z != 0.0 {
                        directions.push(normalized([x, y, z]));
                    }
                }
            }
        }
        let sight = normalized(difference(toward, viewer));
        let least = (0..3)
            .min_by(|first, second| sight[*first].abs().total_cmp(&sight[*second].abs()))
            .unwrap();
        let mut axis = [0.0; 3];
        axis[least] = 1.0;
        let across = normalized(cross(sight, axis));
        let also_across = cross(sight, across);
        for direction in [across, also_across] {
            directions.push(direction);
            directions.push(direction.map(|component| -component));
        }
        directions
            .into_iter()
            .map(|direction| moved(viewer, direction, margin))
            .collect()
    }

    /// If `cube` is hidden from `viewer` (by itself, and with a margin from every viewer that far off), every place of it that there is to try is behind the core.
    fn check_a_hidden_cube(
        viewer: [f64; 3],
        cube: BrickCube,
        shares: &[(f64, f64, f64)],
    ) -> Result<(), TestCaseError> {
        let places = places_in(cube, shares);
        let behind = |seen_from: [f64; 3], place: [f64; 3]| {
            distance_to_segment(core().centre, seen_from, place) < core().radius + 1.0e-6
        };
        if is_hidden_by(&core(), viewer, cube, 0.0) {
            for &place in &places {
                prop_assert!(
                    behind(viewer, place),
                    "{place:?} is not behind the core seen from {viewer:?}"
                );
            }
        }
        let margin = 56.0;
        if is_hidden_by(&core(), viewer, cube, margin) {
            let middle = places[8];
            for seen_from in viewers_within(viewer, middle, margin) {
                for &place in &places {
                    prop_assert!(
                        behind(seen_from, place),
                        "{place:?} is not behind the core seen from {seen_from:?}, which is within {margin} of {viewer:?}, for which the cube was hidden"
                    );
                }
            }
        }
        Ok(())
    }

    /// A cube chosen to be where the decision is hard: in a direction from the viewer that is within a little of the half angle of the shadow of the core, at a distance
    /// that is a share of the distance to the places where the shadow begins, and of any of the sizes. `None` for a viewer that is in the core or at its surface.
    fn cube_near_the_edge_of_the_shadow(
        viewer: [f64; 3],
        edge_share: f64,
        around: (f64, f64),
        distance_share: f64,
        level: u8,
    ) -> Option<BrickCube> {
        let to_core = difference(core().centre, viewer);
        let core_distance = dot(to_core, to_core).sqrt();
        if core_distance <= core().radius + 1.0 {
            return None;
        }
        let axis = normalized(to_core);
        // A direction across the axis, and one across both, and a mix of them by `around`.
        let least = (0..3)
            .min_by(|first, second| axis[*first].abs().total_cmp(&axis[*second].abs()))
            .unwrap();
        let mut other = [0.0; 3];
        other[least] = 1.0;
        let across = normalized(cross(axis, other));
        let also_across = cross(axis, across);
        let mix = [
            across[0] * around.0 + also_across[0] * around.1,
            across[1] * around.0 + also_across[1] * around.1,
            across[2] * around.0 + also_across[2] * around.1,
        ];
        let sideways = normalized(mix);
        let sine = core().radius / core_distance * edge_share;
        if !(0.0..1.0).contains(&sine) {
            return None;
        }
        let cosine = (1.0 - sine * sine).sqrt();
        let touching = (core_distance * core_distance - core().radius * core().radius).sqrt();
        let distance = touching * distance_share;
        let point = [
            viewer[0] + distance * (axis[0] * cosine + sideways[0] * sine),
            viewer[1] + distance * (axis[1] * cosine + sideways[1] * sine),
            viewer[2] + distance * (axis[2] * cosine + sideways[2] * sine),
        ];
        let brick = |coordinate: f64| (coordinate / BRICK_EDGE_LENGTH as f64).floor() as i64;
        Some(BrickCube::containing(
            BrickCoordinate::new(brick(point[0]), brick(point[1]), brick(point[2])),
            level,
        ))
    }

    proptest! {
        /// The point of the cull: whatever it hides is hidden. For viewers at many distances and cubes of many sizes at many places, every place tried in a cube that is
        /// said to be hidden is behind the core: the line from the viewer to it passes through the core (or it is inside it). And the same for a viewer anywhere within
        /// the margin of where it was said to be, for a cube that is hidden with the margin.
        #[test]
        fn a_cube_that_is_hidden_has_every_place_of_it_behind_the_core(
            viewer in (-4000.0f64..4000.0, -4000.0f64..4000.0, -4000.0f64..4000.0),
            origin in (-100i64..100, -100i64..100, -100i64..100),
            level in 0u8..9,
            shares in proptest::collection::vec((0.0f64..1.0, 0.0f64..1.0, 0.0f64..1.0), 12),
        ) {
            let viewer = [viewer.0, viewer.1, viewer.2];
            let cube = BrickCube::containing(BrickCoordinate::new(origin.0, origin.1, origin.2), level);
            check_a_hidden_cube(viewer, cube, &shares)?;
        }

        /// The same, for the cubes where it is hard: just inside and just outside the shadow, and just before and just after the places where it begins, at every size.
        #[test]
        fn a_cube_that_is_hidden_near_the_edge_of_the_shadow_has_every_place_of_it_behind_the_core(
            viewer in (-4000.0f64..4000.0, -4000.0f64..4000.0, -4000.0f64..4000.0),
            edge_share in 0.5f64..1.3,
            around in (-1.0f64..1.0, -1.0f64..1.0),
            distance_share in 0.1f64..6.0,
            level in 0u8..9,
            shares in proptest::collection::vec((0.0f64..1.0, 0.0f64..1.0, 0.0f64..1.0), 12),
        ) {
            let viewer = [viewer.0, viewer.1, viewer.2];
            prop_assume!(around.0.abs() + around.1.abs() > 0.05);
            if let Some(cube) = cube_near_the_edge_of_the_shadow(viewer, edge_share, around, distance_share, level) {
                check_a_hidden_cube(viewer, cube, &shares)?;
            }
        }
    }

    #[test]
    fn a_dense_scan_of_the_cubes_where_the_shadow_begins_finds_none_hidden_that_is_not() {
        // The viewer is 3,000 from the core's centre, so the lines that touch the core are 19.5 degrees off the way to it, and touch it 2,828 from the viewer. The scan
        // is of the cubes in the place where the edge of the shadow meets the places it begins at, which random cubes hardly ever are: the thin place where the tests
        // of the angle and of the distance both nearly fail.
        let viewer = [0.0, 3000.0, 0.0];
        let (mut hidden, mut total) = (0, 0);
        for level in 0..4 {
            let step = 16 << level;
            let mut x = 640;
            while x < 1240 {
                let mut y = 40;
                while y < 640 {
                    let mut z = -160;
                    while z < 160 {
                        let brick = |coordinate: i64| coordinate.div_euclid(32);
                        let cube = BrickCube::containing(
                            BrickCoordinate::new(brick(x), brick(y), brick(z)),
                            level,
                        );
                        total += 1;
                        hidden += usize::from(is_hidden_by(&core(), viewer, cube, 0.0));
                        check_a_hidden_cube(viewer, cube, &[]).unwrap();
                        z += step.min(48);
                    }
                    y += step.min(48);
                }
                x += step.min(48);
            }
        }
        // (Some of them hidden, some not: the scan crosses the edge.)
        assert!(
            hidden > total / 20 && hidden < total * 19 / 20,
            "{hidden} of {total}"
        );
    }

    #[test]
    fn the_property_tests_above_are_not_vacuous_a_good_share_of_the_cubes_they_try_are_hidden() {
        // A deterministic sweep: viewers on a ring above the core and cubes on a lattice; some are hidden and some are not.
        let mut hidden = 0;
        let mut total = 0;
        for ring in 0..16 {
            // (A viewer 1,200 from the centre, in a direction across a half circle from +x to -x over +y.)
            let across = f64::from(ring) - 7.5;
            let direction = normalized([across, 8.0 - across.abs() * 0.5 + 1.0, 0.0]);
            let viewer = direction.map(|component| component * 1200.0);
            for x in -20..20 {
                for y in -20..20 {
                    total += 1;
                    hidden += usize::from(is_hidden_by(
                        &core(),
                        viewer,
                        cube_at(x * 4, y * 4, 0, 2),
                        0.0,
                    ));
                }
            }
        }
        assert!(
            hidden > total / 10 && hidden < total * 9 / 10,
            "{hidden} of {total}"
        );
        // And the cubes that are chosen near the edge of the shadow are hidden and not hidden about equally: the sweep is where the decision is hard.
        let (mut near_hidden, mut near_total) = (0, 0);
        for step in 0..400 {
            let viewer = [
                0.0,
                1100.0 + f64::from(step % 20) * 100.0,
                f64::from(step % 7) * 50.0,
            ];
            let edge_share = 0.8 + f64::from(step % 11) * 0.04;
            let around = (f64::from(step % 5) - 2.0, f64::from(step % 3) - 1.0 + 0.3);
            let distance_share = 0.3 + f64::from(step % 13) * 0.4;
            let level = (step % 8) as u8;
            if let Some(cube) =
                cube_near_the_edge_of_the_shadow(viewer, edge_share, around, distance_share, level)
            {
                near_total += 1;
                near_hidden += usize::from(is_hidden_by(&core(), viewer, cube, 0.0));
            }
        }
        assert!(near_total > 300, "{near_total}");
        assert!(
            near_hidden > near_total / 10 && near_hidden < near_total * 9 / 10,
            "{near_hidden} of {near_total}"
        );
    }
}
