//! Which nodes of a grid are drawn for an orthographic view (`docs/specs/viewpoints-spec.md` 6.5): a map that looks over a level from a place that has no perspective.
//!
//! In an orthographic view a pixel is as many voxels across at every distance, so what is "fine enough" does not depend on the distance from the viewer, as it does for a
//! perspective view ([`crate::select_nodes`]). The cut is the cubes of **one level**, whose cells are about `cell_pixels` pixels across, over the box the picture sees: a
//! picture that is 1,920 pixels across at 3 pixels a cell is 20 columns of nodes however far above the ground it looks and however far it is zoomed out, which is the
//! cost of a map; and a zoom changes the level of the whole cut at once ([`orthographic_level`] goes up by one when a pixel is twice as many voxels across).
//!
//! Only IEEE addition, subtraction, multiplication, division, comparison and square roots are used, so the answer is the same on every machine.

use region_octree::BrickCube;
use voxel_space::{BRICK_EDGE_LENGTH, BrickBox, WorldPosition};

use crate::selection::{HALF_DIAGONAL, MAXIMUM_DETAIL_LEVEL, level_covering, span};

/// The box an orthographic camera sees, in the axes of the grid: from the eye, `depth` voxels along `forward`, and `half_width` and `half_height` voxels to each side of
/// that line along `right` and `up`. The three directions are unit vectors at right angles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrthographicView {
    pub eye: WorldPosition,
    pub right: [f64; 3],
    pub up: [f64; 3],
    pub forward: [f64; 3],
    pub half_width: f64,
    pub half_height: f64,
    pub depth: f64,
}

/// The detail level of the cut of an orthographic view whose pixel is `voxels_per_pixel` voxels across: the greatest level whose cell, `2^level` voxels, is at most
/// `cell_pixels` pixels. Never below 1 (a node of level 0 is a brick, and bricks are the fine ball's, not the map's), and never above the coarsest level there is.
pub fn orthographic_level(cell_pixels: f64, voxels_per_pixel: f64) -> u8 {
    let wanted = cell_pixels * voxels_per_pixel;
    let mut level = 1u8;
    while level < MAXIMUM_DETAIL_LEVEL && (1u128 << (level + 1)) as f64 <= wanted {
        level += 1;
    }
    level
}

/// The cubes of `level` that touch the box of the view and that `may_hold` says may hold something (a cube it says no to is left out with everything inside it), in the order
/// of a depth-first walk with the low half first. A cube is kept if the sphere round it touches the box, so a few cubes at the edges are kept that only just miss it.
pub fn select_orthographic(
    view: &OrthographicView,
    level: u8,
    may_hold: &dyn Fn(BrickCube) -> bool,
) -> Vec<BrickCube> {
    let level = level.min(MAXIMUM_DETAIL_LEVEL);
    // (As many bricks as the farthest corner of the box is far, and one more.)
    let farthest = (view.depth * view.depth
        + view.half_width * view.half_width
        + view.half_height * view.half_height)
        .sqrt();
    let reach = ((farthest / BRICK_EDGE_LENGTH as f64).ceil().max(0.0) as u128).saturating_add(1);
    let extent = BrickBox::around_by(view.eye.brick(), reach);
    let widest = {
        let (low, high) = (extent.min(), extent.max());
        [high.x() - low.x(), high.y() - low.y(), high.z() - low.z()]
            .into_iter()
            .max()
            .expect("three axes")
            + 1
    };
    let root_level = level_covering(widest as u128).max(level);
    let mut chosen = Vec::new();
    for root in BrickCube::covering(extent, root_level) {
        descend(root, view, level, may_hold, &mut chosen);
    }
    chosen
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Whether the sphere round the cube touches the box of the view.
fn touches_the_box(cube: BrickCube, view: &OrthographicView) -> bool {
    let spans = span(cube, view.eye);
    let middle = [
        (spans[0].0 + spans[0].1) * 0.5,
        (spans[1].0 + spans[1].1) * 0.5,
        (spans[2].0 + spans[2].1) * 0.5,
    ];
    let radius = cube.edge_length() as f64 * BRICK_EDGE_LENGTH as f64 * HALF_DIAGONAL;
    let (across, upward, ahead) = (
        dot(middle, view.right),
        dot(middle, view.up),
        dot(middle, view.forward),
    );
    across.abs() <= view.half_width + radius
        && upward.abs() <= view.half_height + radius
        && ahead >= -radius
        && ahead <= view.depth + radius
}

fn descend(
    cube: BrickCube,
    view: &OrthographicView,
    level: u8,
    may_hold: &dyn Fn(BrickCube) -> bool,
    chosen: &mut Vec<BrickCube>,
) {
    if !touches_the_box(cube, view) || !may_hold(cube) {
        return;
    }
    if cube.level() <= level {
        chosen.push(cube);
        return;
    }
    let half = 1i128 << (cube.level() - 1);
    let origin = cube.origin();
    for octant in 0..8i128 {
        let (dx, dy, dz) = (octant & 1, (octant >> 1) & 1, octant >> 2);
        let child = BrickCube::new(
            voxel_space::BrickCoordinate::new(
                origin.x() + dx * half,
                origin.y() + dy * half,
                origin.z() + dz * half,
            ),
            cube.level() - 1,
        )
        .expect("half of a cube is a cube");
        descend(child, view, level, may_hold, chosen);
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::BrickCoordinate;

    use super::*;

    const DOWN: [f64; 3] = [0.0, -1.0, 0.0];

    /// A view straight down from `height` voxels over the middle of the brick at the origin, `across` voxels wide and `along` voxels long (the top of the picture is toward
    /// -z), with a depth of `depth`.
    fn looking_down(height: f64, across: f64, along: f64, depth: f64) -> OrthographicView {
        OrthographicView {
            eye: WorldPosition::new(BrickCoordinate::new(0, 0, 0), [16.0, height, 16.0]),
            right: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, -1.0],
            forward: DOWN,
            half_width: across / 2.0,
            half_height: along / 2.0,
            depth,
        }
    }

    /// Ground that fills every cube that reaches below y = 0 and no cube that is wholly above it: the cubes a map sees (everything under the surface is buried, so only the
    /// cubes that hold the surface are kept).
    fn ground_at_zero(cube: BrickCube) -> bool {
        let edge = cube.edge_length() as i128 * 32;
        let low = cube.origin().origin().y();
        low <= 0 && 0 < low + edge
    }

    #[test]
    fn the_level_is_the_greatest_whose_cell_is_at_most_so_many_pixels_and_goes_up_by_one_for_a_pixel_twice_as_wide()
     {
        // 3 pixels a cell, a pixel of 1 voxel: cells of 2 voxels (level 1; 4 is more than 3).
        assert_eq!(orthographic_level(3.0, 1.0), 1);
        assert_eq!(orthographic_level(2.0, 5.0), 3); // 10 voxels: a cell of 8
        assert_eq!(orthographic_level(4.0, 4.0), 4); // exactly 16
        assert_eq!(orthographic_level(4.0, 4.0 - 1.0e-9), 3);
        // Never below 1, however close it looks.
        assert_eq!(orthographic_level(3.0, 0.001), 1);
        assert_eq!(orthographic_level(3.0, 0.0), 1);
        // A pixel twice as wide is one level more, all the way up, and the coarsest level is the end.
        let mut previous = orthographic_level(3.0, 1.0);
        for step in 1..60 {
            let level = orthographic_level(3.0, 2.0_f64.powi(step));
            assert_eq!(level, previous + 1, "{step}");
            previous = level;
        }
        assert_eq!(orthographic_level(3.0, 1.0e300), MAXIMUM_DETAIL_LEVEL);
    }

    #[test]
    fn every_cube_is_of_the_level_asked_and_they_tile_the_box_without_overlap_and_leave_out_what_is_far_from_it()
     {
        let view = looking_down(300.0, 400.0, 200.0, 1_000.0);
        let level = 3;
        let cubes = select_orthographic(&view, level, &|_| true);
        assert!(cubes.iter().all(|cube| cube.level() == level));
        let edge = 32i128 << level;
        // No two overlap (all are of one level, so they are the same cube or apart).
        let mut origins: Vec<_> = cubes
            .iter()
            .map(|cube| {
                let origin = cube.origin().origin();
                (origin.x(), origin.y(), origin.z())
            })
            .collect();
        let count = origins.len();
        origins.sort_unstable();
        origins.dedup();
        assert_eq!(origins.len(), count);
        // Every point of the box is in a cube (a grid of points of the box, as the eye's own axes have them).
        let at = |across: f64, along: f64, ahead: f64| -> (i128, i128, i128) {
            // The eye is at (16, 300, 16); right is +x, the picture's up is -z, forward is -y.
            let x = 16.0 + across;
            let y = 300.0 - ahead;
            let z = 16.0 - along;
            (x.floor() as i128, y.floor() as i128, z.floor() as i128)
        };
        for across in [-199.0, -100.0, 0.0, 100.0, 199.0] {
            for along in [-99.0, 0.0, 99.0] {
                for ahead in [1.0, 300.0, 600.0, 999.0] {
                    let (x, y, z) = at(across, along, ahead);
                    let holds = origins.contains(&(
                        x.div_euclid(edge) * edge,
                        y.div_euclid(edge) * edge,
                        z.div_euclid(edge) * edge,
                    ));
                    assert!(holds, "{across} {along} {ahead}");
                }
            }
        }
        // Nothing is kept that is far beyond the box: its middle is within the box and a cube's radius, to each side.
        for origin in &origins {
            let radius = edge as f64 * HALF_DIAGONAL;
            let middle = [
                origin.0 as f64 + edge as f64 / 2.0 - 16.0,
                origin.1 as f64 + edge as f64 / 2.0 - 300.0,
                origin.2 as f64 + edge as f64 / 2.0 - 16.0,
            ];
            assert!(middle[0].abs() <= 200.0 + radius, "{origin:?}");
            assert!(middle[2].abs() <= 100.0 + radius, "{origin:?}");
            assert!(
                -middle[1] >= -radius && -middle[1] <= 1_000.0 + radius,
                "{origin:?}"
            );
        }
    }

    #[test]
    fn the_number_of_cubes_over_ground_is_the_same_from_any_height_and_is_a_few_hundred_for_a_screen()
     {
        // 1,920 by 1,080 pixels, a pixel 2 voxels across: the picture is 3,840 by 2,160 voxels, and the cells are 4 voxels (level 2), 2 pixels across.
        let voxels_per_pixel = 2.0;
        let level = orthographic_level(3.0, voxels_per_pixel);
        assert_eq!(level, 2);
        let at_height = |height: f64| {
            let view = looking_down(
                height,
                1_920.0 * voxels_per_pixel,
                1_080.0 * voxels_per_pixel,
                height + 1_000.0,
            );
            select_orthographic(&view, level, &ground_at_zero).len()
        };
        let near = at_height(500.0);
        let far = at_height(100_000.0);
        assert_eq!(near, far, "the height of the eye does not matter");
        // A cube of level 2 is 128 voxels: 30 by 17 columns, each with the cube that holds the surface (and some at the edge that only just touch the box).
        assert!((30 * 17..=33 * 20).contains(&near), "{near}");
        // A pixel 16 times as wide has a cut of 4 levels more and the same number of columns across.
        let wide = looking_down(500.0, 1_920.0 * 32.0, 1_080.0 * 32.0, 1_500.0);
        let wide_level = orthographic_level(3.0, 32.0);
        assert_eq!(wide_level, level + 4);
        let wide_count = select_orthographic(&wide, wide_level, &ground_at_zero).len();
        assert!(wide_count <= near + near / 8, "{wide_count} against {near}");
    }

    #[test]
    fn a_cube_that_may_not_hold_anything_is_left_out_with_all_that_is_in_it() {
        let view = looking_down(300.0, 400.0, 400.0, 1_000.0);
        let everything = select_orthographic(&view, 2, &|_| true);
        let none = select_orthographic(&view, 2, &|_| false);
        assert!(none.is_empty());
        // Refusing the cubes of one half of the world leaves exactly the others.
        let right_only =
            select_orthographic(&view, 2, &|cube| cube.origin().x() >= 0 || cube.level() > 6);
        let expected = everything
            .iter()
            .filter(|cube| cube.origin().x() >= 0)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(right_only, expected);
        assert!(!expected.is_empty() && expected.len() < everything.len());
    }

    #[test]
    fn a_view_that_looks_aslant_keeps_the_cubes_it_sees_and_not_the_ones_behind_the_eye() {
        // Looking toward -z and down at 45 degrees, from 300 voxels up and 300 voxels back: the middle of the picture is at the ground under the origin.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let view = OrthographicView {
            eye: WorldPosition::new(BrickCoordinate::new(0, 0, 0), [16.0, 300.0, 316.0]),
            right: [1.0, 0.0, 0.0],
            up: [0.0, s, -s],
            forward: [0.0, -s, -s],
            half_width: 150.0,
            half_height: 150.0,
            depth: 800.0,
        };
        let cubes = select_orthographic(&view, 3, &ground_at_zero);
        assert!(!cubes.is_empty());
        let here = |x: i128, z: i128| {
            cubes.iter().any(|cube| {
                let edge = 256;
                let origin = cube.origin().origin();
                (origin.x()..origin.x() + edge).contains(&x)
                    && (origin.z()..origin.z() + edge).contains(&z)
            })
        };
        assert!(here(16, 16), "under the middle of the picture");
        assert!(
            !here(16, -900),
            "ahead of it along the ground, past the top of the picture"
        );
        assert!(
            here(16, -100),
            "ahead of it along the ground, in the picture"
        );
        assert!(!here(16, 1_500), "behind the eye");
        assert!(!here(1_000, 16), "far to the side");
        // The same view with the eye turned round sees the other side.
        let turned = OrthographicView {
            right: [-1.0, 0.0, 0.0],
            up: [0.0, s, s],
            forward: [0.0, -s, s],
            ..view
        };
        let others = select_orthographic(&turned, 3, &ground_at_zero);
        assert_ne!(cubes, others);
    }

    #[test]
    fn the_cubes_are_the_same_every_time_and_a_view_a_very_long_way_from_the_origin_has_as_many() {
        let view = looking_down(300.0, 400.0, 400.0, 1_000.0);
        assert_eq!(
            select_orthographic(&view, 2, &ground_at_zero),
            select_orthographic(&view, 2, &ground_at_zero)
        );
        let near = select_orthographic(&view, 2, &ground_at_zero).len();
        let far = OrthographicView {
            eye: WorldPosition::new(
                BrickCoordinate::new(10_i128.pow(30), 0, -10_i128.pow(30)),
                [16.0, 300.0, 16.0],
            ),
            ..view
        };
        assert_eq!(select_orthographic(&far, 2, &ground_at_zero).len(), near);
    }
}
