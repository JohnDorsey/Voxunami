//! What the cubes of an orthographic map are chosen as (`docs/specs/viewpoints-spec.md`, 9s): their level, whether any are bricks, and how large a box.
//!
//! A map is looked at from far, and it is made for frames before it is made for detail: its cells are **twice as large** as the game asks of the player's own view, it has
//! **bricks only very close in** (a voxel four pixels across or more) and in a **small ball** (a map sees them from above, and the ball the player's view meshes is mostly
//! cave faces under the ground, 3 to 7 million quads), and its box is **no larger than the picture needs** (the first map's had about eight times what was seen).

use world_view::OrthographicCut;

/// How many times as many pixels across the cells of a map are as the cells of the player's view ([`crate::definition::CoarseDetail::cell_pixels`]).
const MAP_COARSENESS: f64 = 2.0;

/// The most voxels a pixel may be across for a map to draw bricks.
const MOST_VOXELS_A_PIXEL_FOR_BRICKS: f64 = 0.25;

/// How many bricks round the middle of a map are drawn as bricks, when any are.
const MAP_BRICK_RADIUS: u32 = 6;

/// The least the sine of the way down is taken as: a map looked at along the ground sees without end, and is held to this (four times its height along the ground).
const LEAST_SINE: f64 = 0.25;

/// What the picture of a map is, as the cut needs it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MapPicture {
    /// How many voxels the picture is high.
    pub height: f64,
    /// How many pixels it is high.
    pub viewport_height: f64,
    /// How many times as wide as high it is.
    pub aspect: f64,
    /// Radians the map's camera looks down by.
    pub elevation: f64,
}

/// The cut for a map's picture, in a level whose nodes have cells of about `cell_pixels` pixels in the player's view and are drawn out to `maximum_distance` voxels.
pub(crate) fn orthographic_cut(
    picture: MapPicture,
    cell_pixels: f32,
    maximum_distance: f64,
) -> OrthographicCut {
    let voxels_per_pixel = picture.height / picture.viewport_height;
    let level =
        coarse_cells::orthographic_level(f64::from(cell_pixels) * MAP_COARSENESS, voxels_per_pixel);
    // How far the picture reaches over the ground from its middle: sideways half its width and along it half its height over the sine of the way it looks down; the box is
    // turned with the grid and not with the camera, so it reaches as far as the corner every way.
    let sine = libm::sin(picture.elevation.abs()).max(LEAST_SINE);
    let half_width = picture.height * 0.5 * picture.aspect;
    let half_along = picture.height * 0.5 / sine;
    let wanted = libm::sqrt(half_width * half_width + half_along * half_along);
    // Sizes are taken in steps (two cubes of the level, doubled as often as needed, and a quarter, half or three quarters more), so that a map that is turned, tilted or
    // zoomed a little keeps the cubes it has.
    let edge = f64::from(32u32 << level.min(26));
    let mut octave = edge * 2.0;
    while octave * 2.0 <= wanted {
        octave *= 2.0;
    }
    let extent = [1.0, 1.25, 1.5, 1.75, 2.0]
        .into_iter()
        .map(|part| octave * part)
        .find(|&extent| extent >= wanted)
        .unwrap_or(octave * 2.0);
    OrthographicCut {
        level,
        half_extent: extent.min(maximum_distance.max(edge)),
        brick_radius: (voxels_per_pixel <= MOST_VOXELS_A_PIXEL_FOR_BRICKS)
            .then_some(MAP_BRICK_RADIUS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAR: f64 = 1.0e9;

    fn picture(height: f64) -> MapPicture {
        MapPicture {
            height,
            viewport_height: 1080.0,
            aspect: 16.0 / 9.0,
            elevation: 0.5,
        }
    }

    #[test]
    fn a_map_has_a_small_ball_of_bricks_while_a_voxel_is_four_pixels_or_more_and_none_after() {
        for (height, radius) in [
            (10.0, Some(6)),
            (270.0, Some(6)),
            (271.0, None),
            (1_080.0, None),
            (20_000.0, None),
        ] {
            assert_eq!(
                orthographic_cut(picture(height), 3.0, FAR).brick_radius,
                radius,
                "{height}"
            );
        }
    }

    #[test]
    fn the_cells_of_a_map_are_between_one_and_two_times_the_levels_pixels_across() {
        // 3 pixels a cell in the player's view: 3 to 6 pixels in a map.
        let mut height = 2_000.0;
        while height < 3.0e6 {
            let cut = orthographic_cut(picture(height), 3.0, FAR);
            let pixels = f64::from(1u32 << cut.level) / (height / 1080.0);
            assert!(pixels > 3.0 && pixels <= 6.0, "{height}: {pixels}");
            height *= 1.37;
        }
    }

    #[test]
    fn the_box_holds_the_corners_of_the_picture_and_is_not_much_larger_than_they_need() {
        for height in [500.0, 1_440.0, 11_000.0, 20_256.0, 123_456.0] {
            for (aspect, elevation) in [(16.0 / 9.0, 0.5), (1.0, 1.5), (2.4, 0.7), (1.6, 0.0)] {
                let seen = MapPicture {
                    aspect,
                    elevation,
                    ..picture(height)
                };
                let cut = orthographic_cut(seen, 3.0, FAR);
                let sine = libm::sin(elevation).max(0.25);
                let wanted = libm::hypot(height * aspect / 2.0, height / 2.0 / sine);
                let edge = f64::from(32u32 << cut.level);
                assert!(cut.half_extent >= wanted, "{height} {aspect} {elevation}");
                assert!(
                    cut.half_extent <= (wanted * 1.25).max(edge * 2.0),
                    "{height} {aspect} {elevation}: {} for {wanted}",
                    cut.half_extent
                );
            }
        }
    }

    #[test]
    fn the_size_changes_in_steps_so_that_a_small_zoom_keeps_the_cut() {
        let mut sizes = std::collections::BTreeSet::new();
        let mut height = 10_000.0;
        // Two doublings in a hundred small zooms: a handful of cuts, not a hundred.
        for _ in 0..100 {
            let cut = orthographic_cut(picture(height), 3.0, FAR);
            sizes.insert((cut.level, cut.half_extent as u64));
            height *= libm::pow(4.0, 0.01);
        }
        assert!((6..=12).contains(&sizes.len()), "{sizes:?}");
    }

    #[test]
    fn the_box_reaches_no_farther_than_the_level_is_drawn() {
        // A picture a million voxels high would want a box of a million each way; the level is drawn out to half that.
        let cut = orthographic_cut(picture(1.0e6), 3.0, 500_000.0);
        assert_eq!(cut.half_extent, 500_000.0);
        // Never less than one cube of the level, which is the smallest box there is to draw.
        let cut = orthographic_cut(picture(1.0e6), 3.0, 1.0);
        assert_eq!(cut.half_extent, f64::from(32u32 << cut.level));
    }
}
