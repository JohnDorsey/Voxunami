use proptest::prelude::*;
use voxel_space::FaceDirection;

use super::*;
use crate::definition::TileRef;
use crate::shape::ShapeBox;

fn hit(shape: &Shape, origin: [f64; 3], direction: [f64; 3]) -> Option<SurfaceHit> {
    shape.first_surface(origin, direction, 100.0)
}

#[test]
fn a_ray_into_a_whole_voxel_meets_the_side_it_enters_by() {
    let found = hit(&Shape::full(), [-1.0, 0.25, 0.75], [1.0, 0.0, 0.0]).unwrap();
    assert_eq!(found.face, FaceDirection::NegativeX);
    assert_eq!(found.distance, 1.0);
    assert_eq!(found.point, [0.0, 0.25, 0.75]);
    // From the other side, going the other way.
    let found = hit(&Shape::full(), [3.0, 0.25, 0.75], [-2.0, 0.0, 0.0]).unwrap();
    assert_eq!(found.face, FaceDirection::PositiveX);
    assert_eq!(found.distance, 2.0);
    assert_eq!(found.point, [1.0, 0.25, 0.75]);
    // Down onto the top, up into the bottom, along z both ways.
    for (origin, direction, face, distance) in [
        (
            [0.5, 2.0, 0.5],
            [0.0, -1.0, 0.0],
            FaceDirection::PositiveY,
            1.0,
        ),
        (
            [0.5, -2.0, 0.5],
            [0.0, 1.0, 0.0],
            FaceDirection::NegativeY,
            2.0,
        ),
        (
            [0.5, 0.5, 2.0],
            [0.0, 0.0, -1.0],
            FaceDirection::PositiveZ,
            1.0,
        ),
        (
            [0.5, 0.5, -2.0],
            [0.0, 0.0, 1.0],
            FaceDirection::NegativeZ,
            2.0,
        ),
    ] {
        let found = hit(&Shape::full(), origin, direction).unwrap();
        assert_eq!(found.face, face);
        assert_eq!(found.distance, distance);
    }
}

#[test]
fn a_ray_over_a_slab_passes_and_one_onto_it_meets_the_top_inside_the_voxel() {
    let slab = Shape::half(FaceDirection::NegativeY);
    assert_eq!(hit(&slab, [-1.0, 0.75, 0.5], [1.0, 0.0, 0.0]), None);
    let found = hit(&slab, [0.25, 3.0, 0.5], [0.0, -1.0, 0.0]).unwrap();
    assert_eq!(found.face, FaceDirection::PositiveY);
    assert_eq!(found.distance, 2.5);
    assert_eq!(found.point, [0.25, 0.5, 0.5]);
    // A slanted ray that is still above the slab at x = 0 comes down onto its top (y = 0.5 at x = 0.25); one that is below the top there enters its side.
    let found = hit(&slab, [-1.0, 0.75, 0.5], [1.0, -0.2, 0.0]).unwrap();
    assert_eq!(found.face, FaceDirection::PositiveY);
    assert!((found.point[0] - 0.25).abs() < 1e-12, "{:?}", found.point);
    let found = hit(&slab, [-1.0, 0.25, 0.5], [1.0, -0.05, 0.0]).unwrap();
    assert_eq!(found.face, FaceDirection::NegativeX);
    assert_eq!(found.point[0], 0.0);
}

#[test]
fn a_stair_has_a_face_inside_the_voxel_on_the_low_side_and_one_on_the_boundary_on_the_high_side() {
    let stair = Shape::stair(FaceDirection::PositiveX);
    // From the low side at the height of the upper step, the ray crosses the empty part and meets the step's riser in the middle of the voxel.
    let found = hit(&stair, [-1.0, 0.75, 0.5], [1.0, 0.0, 0.0]).unwrap();
    assert_eq!(found.face, FaceDirection::NegativeX);
    assert_eq!(found.distance, 1.5);
    assert_eq!(found.point, [0.5, 0.75, 0.5]);
    // From the high side, the step is whole to the boundary.
    let found = hit(&stair, [2.0, 0.75, 0.5], [-1.0, 0.0, 0.0]).unwrap();
    assert_eq!(found.face, FaceDirection::PositiveX);
    assert_eq!(found.point, [1.0, 0.75, 0.5]);
    // Down onto the tread of the low part and onto the top of the step.
    let found = hit(&stair, [0.25, 2.0, 0.5], [0.0, -1.0, 0.0]).unwrap();
    assert_eq!(
        (found.face, found.point[1]),
        (FaceDirection::PositiveY, 0.5)
    );
    let found = hit(&stair, [0.75, 2.0, 0.5], [0.0, -1.0, 0.0]).unwrap();
    assert_eq!(
        (found.face, found.point[1]),
        (FaceDirection::PositiveY, 1.0)
    );
}

#[test]
fn nothing_is_met_in_an_empty_shape_or_beyond_the_range_or_in_a_direction_that_is_none() {
    assert_eq!(
        hit(&Shape::empty(), [-1.0, 0.5, 0.5], [1.0, 0.0, 0.0]),
        None
    );
    let full = Shape::full();
    assert_eq!(
        full.first_surface([-1.0, 0.5, 0.5], [1.0, 0.0, 0.0], 0.99),
        None
    );
    assert!(
        full.first_surface([-1.0, 0.5, 0.5], [1.0, 0.0, 0.0], 1.0)
            .is_some()
    );
    assert_eq!(hit(&full, [-1.0, 0.5, 0.5], [0.0, 0.0, 0.0]), None);
    assert_eq!(hit(&full, [-1.0, 0.5, 0.5], [f64::NAN, 0.0, 0.0]), None);
    assert_eq!(hit(&full, [f64::NAN, 0.5, 0.5], [1.0, 0.0, 0.0]), None);
    // A ray that goes by, and one that goes away.
    assert_eq!(hit(&full, [-1.0, 1.5, 0.5], [1.0, 0.0, 0.0]), None);
    assert_eq!(hit(&full, [-1.0, 0.5, 0.5], [-1.0, 0.0, 0.0]), None);
    // One parallel to a side, outside it, and one along a side exactly (a grazing ray is no miss that matters, but it must not panic).
    assert_eq!(hit(&full, [0.5, 1.5, -1.0], [0.0, 0.0, 1.0]), None);
    let _ = hit(&full, [0.5, 1.0, -1.0], [0.0, 0.0, 1.0]);
}

#[test]
fn a_ray_that_begins_inside_ignores_the_sub_cell_it_is_in() {
    let stair = Shape::stair(FaceDirection::PositiveX);
    // In the empty upper part, going toward the riser: the riser is met.
    let found = hit(&stair, [0.1, 0.75, 0.5], [1.0, 0.0, 0.0]).unwrap();
    assert_eq!(found.face, FaceDirection::NegativeX);
    assert_eq!(found.point[0], 0.5);
    // Going up from there leaves the voxel and meets nothing.
    assert_eq!(hit(&stair, [0.1, 0.75, 0.5], [0.0, 1.0, 0.0]), None);
    // Inside a filled sub-cell and going out, the first thing entered is the next sub-cell, and a ray never meets "its own" cell.
    let found = hit(&Shape::full(), [0.51, 0.5, 0.5], [1.0, 0.0, 0.0]).unwrap();
    assert_eq!(found.face, FaceDirection::NegativeX);
    assert_eq!(found.point[0], 0.625);
}

#[test]
fn the_part_of_the_point_along_the_face_is_exactly_on_the_plane() {
    // 0.1 is not a multiple of an eighth and a slanted ray makes the arithmetic inexact; the plane must still be put at exactly 0.5.
    let slab = Shape::half(FaceDirection::NegativeY);
    let found = hit(&slab, [0.1, 2.3, 0.7], [0.3, -1.7, 0.2]).unwrap();
    assert_eq!(found.face, FaceDirection::PositiveY);
    assert_eq!(found.point[1], 0.5);
}

fn picture(across: FaceDirection, up: FaceDirection) -> FacePicture {
    FacePicture {
        tile: TileRef::new(0, 0).unwrap(),
        across,
        up,
        cut_out: false,
    }
}

#[test]
fn a_point_on_a_face_is_a_place_on_its_picture_counted_from_the_left_and_the_top() {
    // A side of the block as it is made (+z): across is +x, up is +y, so the left is x = 0 and the top is y = 1.
    let side = picture(FaceDirection::PositiveX, FaceDirection::PositiveY);
    assert_eq!(
        side.place_of([0.25, 0.75, 1.0]),
        PicturePlace {
            across: 0.25,
            down: 0.25
        }
    );
    // The top as it is made: across +x, up -z, so the top of the picture is at z = 0 and the bottom at z = 1.
    let top = picture(FaceDirection::PositiveX, FaceDirection::NegativeZ);
    assert_eq!(
        top.place_of([0.25, 1.0, 0.75]),
        PicturePlace {
            across: 0.25,
            down: 0.75
        }
    );
    // The side that looks the other way (-z): across is -x, so the left is at x = 1.
    let back = picture(FaceDirection::NegativeX, FaceDirection::PositiveY);
    assert_eq!(
        back.place_of([0.25, 0.75, 0.0]),
        PicturePlace {
            across: 0.75,
            down: 0.25
        }
    );
    // A picture that lies along z and y.
    let along = picture(FaceDirection::NegativeZ, FaceDirection::PositiveY);
    assert_eq!(
        along.place_of([1.0, 0.5, 0.125]),
        PicturePlace {
            across: 0.875,
            down: 0.5
        }
    );
}

#[test]
fn a_place_is_the_pixel_that_holds_it_and_the_far_edge_belongs_to_the_last_pixel() {
    let place = |across, down| PicturePlace { across, down };
    assert_eq!(place(0.0, 0.0).pixel(16), [0, 0]);
    assert_eq!(place(0.0625, 0.99).pixel(16), [1, 15]);
    assert_eq!(place(0.0624, 0.5).pixel(16), [0, 8]);
    assert_eq!(place(1.0, 1.0).pixel(16), [15, 15]);
    assert_eq!(place(0.5, 0.25).pixel(32), [16, 8]);
}

proptest! {
    /// Each of the six faces in each of its four turns: the pixel of a point is inside the picture, and moving the point along `across` moves the pixel's column the
    /// way the picture's left-to-right goes, and moving it along `up` moves the row upward.
    #[test]
    fn however_a_picture_sits_on_a_face_the_pixel_follows_across_and_up(
        face_index in 0u8..6,
        turn in 0usize..4,
        up_positive in any::<bool>(),
        along in 0.01f64..0.99,
        beside in 0.01f64..0.99,
    ) {
        let face = FaceDirection::from_index(face_index).unwrap();
        // The four directions in the face's plane, in turn; `across` is one of them and `up` is along the other axis of the plane, either way.
        let in_plane: Vec<FaceDirection> = FaceDirection::ALL
            .into_iter()
            .filter(|direction| direction.axis() != face.axis())
            .collect();
        let across = in_plane[turn];
        let other_axis = (0..3).find(|axis| *axis != face.axis() && *axis != across.axis()).unwrap();
        let up = FaceDirection::from_index((other_axis * 2) as u8 + u8::from(up_positive)).unwrap();
        let picture = picture(across, up);
        let point_at = |first: f64, second: f64| {
            let mut point = [0.0; 3];
            point[face.axis()] = if face.is_positive() { 1.0 } else { 0.0 };
            point[across.axis()] = first;
            point[up.axis()] = second;
            point
        };
        let here = picture.place_of(point_at(along, beside));
        let further_across = picture.place_of(point_at((along + 0.005).min(1.0), beside));
        let further_up = picture.place_of(point_at(along, (beside + 0.005).min(1.0)));
        // `across` is the way the picture's left-to-right goes: a point farther along it is farther right (when across is positive) or farther left.
        prop_assert_eq!(further_across.across > here.across, across.is_positive());
        prop_assert_eq!(further_across.down, here.down);
        // `up`: a point farther along it is higher, so its row is smaller (when up is positive).
        prop_assert_eq!(further_up.down < here.down, up.is_positive());
        prop_assert_eq!(further_up.across, here.across);
        for value in [here.across, here.down] {
            prop_assert!((0.0..=1.0).contains(&value));
        }
    }
}

fn arbitrary_shape() -> impl Strategy<Value = Shape> {
    let one_box =
        ([0u8..8, 0u8..8, 0u8..8], [1u8..=8, 1u8..=8, 1u8..=8]).prop_map(|(low, size)| {
            let max = std::array::from_fn(|axis| (low[axis] + size[axis]).min(8));
            ShapeBox { min: low, max }
        });
    proptest::collection::vec(one_box, 1..5).prop_map(|boxes| Shape::from_boxes(&boxes))
}

/// The first place a ray enters any filled sub-cell, found by trying every filled sub-cell, from outside the voxel: the oracle for the walk.
fn brute_force(
    shape: &Shape,
    origin: [f64; 3],
    direction: [f64; 3],
) -> Option<(FaceDirection, f64)> {
    let length = direction.iter().map(|part| part * part).sum::<f64>().sqrt();
    let direction = direction.map(|part| part / length);
    let mut best: Option<(FaceDirection, f64)> = None;
    for z in 0..SUBDIVISIONS {
        for y in 0..SUBDIVISIONS {
            for x in 0..SUBDIVISIONS {
                if !shape.fills([x, y, z]) {
                    continue;
                }
                let low = [x, y, z].map(|cell| cell as f64 / 8.0);
                let mut enter = f64::NEG_INFINITY;
                let mut leave = f64::INFINITY;
                let mut through = 0;
                for axis in 0..3 {
                    if direction[axis] == 0.0 {
                        if origin[axis] < low[axis] || origin[axis] > low[axis] + 0.125 {
                            enter = f64::INFINITY;
                        }
                        continue;
                    }
                    let first = (low[axis] - origin[axis]) / direction[axis];
                    let second = (low[axis] + 0.125 - origin[axis]) / direction[axis];
                    let (near, far) = if first < second {
                        (first, second)
                    } else {
                        (second, first)
                    };
                    if near > enter {
                        enter = near;
                        through = axis;
                    }
                    leave = leave.min(far);
                }
                if enter > leave || enter < 0.0 {
                    continue;
                }
                if best.is_none_or(|(_, at)| enter < at) {
                    best = Some((
                        FaceDirection::from_index(
                            (through * 2) as u8 + u8::from(direction[through] < 0.0),
                        )
                        .unwrap(),
                        enter,
                    ));
                }
            }
        }
    }
    best
}

proptest! {
    /// From outside the voxel, the walk finds what trying every filled sub-cell finds: the same side, at the same distance.
    #[test]
    fn the_walk_agrees_with_trying_every_filled_sub_cell(
        shape in arbitrary_shape(),
        origin in [-2.0f64..3.0, -2.0f64..3.0, -2.0f64..3.0],
        direction in [-1.0f64..1.0, -1.0f64..1.0, -1.0f64..1.0],
    ) {
        prop_assume!(direction.iter().any(|part| part.abs() > 1e-3));
        prop_assume!(origin.iter().any(|part| !(0.0..=1.0).contains(part)));
        let expected = brute_force(&shape, origin, direction);
        let found = shape.first_surface(origin, direction, 1000.0);
        prop_assert_eq!(found.map(|found| found.face), expected.map(|(face, _)| face));
        if let (Some(found), Some((_, distance))) = (found, expected) {
            prop_assert!((found.distance - distance).abs() < 1e-9, "{} against {}", found.distance, distance);
            // The point is on the ray, and inside or on the voxel.
            for axis in 0..3 {
                prop_assert!((-1e-9..=1.0 + 1e-9).contains(&found.point[axis]));
            }
        }
    }
}
