use proptest::prelude::*;

use super::*;

fn unit_box() -> SunBox {
    SunBox::around([5.5, 0.5, 0.5], [0.5; 3])
}

#[test]
fn a_box_in_the_way_blocks_and_one_beside_or_behind_or_too_far_does_not() {
    let ahead = [unit_box()];
    // In the way: the box is x 5 to 6 on the ray along +x from the origin.
    assert!(sun_blocked_by_boxes(
        [0.0, 0.5, 0.5],
        [1.0, 0.0, 0.0],
        10.0,
        &ahead
    ));
    // The length of the direction does not matter.
    assert!(sun_blocked_by_boxes(
        [0.0, 0.5, 0.5],
        [30.0, 0.0, 0.0],
        10.0,
        &ahead
    ));
    // It is entered at x = 5, so 5 voxels reach it and 4.9 do not.
    assert!(sun_blocked_by_boxes(
        [0.0, 0.5, 0.5],
        [1.0, 0.0, 0.0],
        5.0,
        &ahead
    ));
    assert!(!sun_blocked_by_boxes(
        [0.0, 0.5, 0.5],
        [1.0, 0.0, 0.0],
        4.9,
        &ahead
    ));
    // Beside it: a ray a little over the top of the box, and one a little to the side.
    assert!(!sun_blocked_by_boxes(
        [0.0, 1.01, 0.5],
        [1.0, 0.0, 0.0],
        10.0,
        &ahead
    ));
    assert!(!sun_blocked_by_boxes(
        [0.0, 0.5, 1.01],
        [1.0, 0.0, 0.0],
        10.0,
        &ahead
    ));
    // Behind: the same box with the sun on the other side.
    assert!(!sun_blocked_by_boxes(
        [0.0, 0.5, 0.5],
        [-1.0, 0.0, 0.0],
        10.0,
        &ahead
    ));
    // Past it: the origin beyond the box, going on.
    assert!(!sun_blocked_by_boxes(
        [7.0, 0.5, 0.5],
        [1.0, 0.0, 0.0],
        10.0,
        &ahead
    ));
    // No boxes block nothing.
    assert!(!sun_blocked_by_boxes([0.0; 3], [1.0, 0.0, 0.0], 10.0, &[]));
}

#[test]
fn a_ray_that_starts_inside_a_box_or_touches_one_is_blocked() {
    let boxes = [unit_box()];
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, -2.0, 3.0]] {
        assert!(sun_blocked_by_boxes(
            [5.5, 0.5, 0.5],
            direction,
            0.0,
            &boxes
        ));
    }
    // Along a face of the box, in its plane: touching.
    assert!(sun_blocked_by_boxes(
        [0.0, 1.0, 0.5],
        [1.0, 0.0, 0.0],
        10.0,
        &boxes
    ));
    // Through an edge, and through a corner.
    assert!(sun_blocked_by_boxes(
        [4.0, 2.0, 0.5],
        [1.0, -1.0, 0.0],
        10.0,
        &boxes
    ));
    assert!(sun_blocked_by_boxes(
        [4.0, 2.0, 2.0],
        [1.0, -1.0, -1.0],
        10.0,
        &boxes
    ));
}

#[test]
fn a_ray_with_a_part_of_exactly_zero_is_between_the_planes_of_that_axis_or_never() {
    let boxes = [unit_box()];
    // Along x with y and z both zero, inside and outside the slab of each.
    for (y, z, blocked) in [
        (0.5, 0.5, true),
        (0.0, 0.0, true),
        (1.0, 1.0, true),
        (1.0001, 0.5, false),
        (0.5, -0.0001, false),
    ] {
        assert_eq!(
            sun_blocked_by_boxes([0.0, y, z], [1.0, 0.0, 0.0], 10.0, &boxes),
            blocked,
            "y {y}, z {z}"
        );
    }
    // Straight up from below the box: x and z are zero.
    assert!(sun_blocked_by_boxes(
        [5.5, -3.0, 0.5],
        [0.0, 1.0, 0.0],
        10.0,
        &boxes
    ));
    assert!(!sun_blocked_by_boxes(
        [6.5, -3.0, 0.5],
        [0.0, 1.0, 0.0],
        10.0,
        &boxes
    ));
}

#[test]
fn any_of_several_boxes_blocks() {
    let boxes = [
        SunBox::around([5.5, 10.5, 0.5], [0.5; 3]),
        SunBox::around([-4.0, 0.5, 0.5], [0.5; 3]),
        unit_box(),
    ];
    assert!(sun_blocked_by_boxes(
        [0.0, 0.5, 0.5],
        [1.0, 0.0, 0.0],
        10.0,
        &boxes
    ));
    assert!(sun_blocked_by_boxes(
        [0.0, 0.5, 0.5],
        [-1.0, 0.0, 0.0],
        10.0,
        &boxes
    ));
    assert!(!sun_blocked_by_boxes(
        [0.0, 0.5, 0.5],
        [0.0, 0.0, 1.0],
        10.0,
        &boxes
    ));
}

/// Whether a point lies in the box grown by `margin` on every side (shrunk, if it is negative).
fn inside(candidate: &SunBox, point: [f64; 3], margin: f64) -> bool {
    (0..3).all(|axis| {
        point[axis] >= candidate.low[axis] - margin && point[axis] <= candidate.high[axis] + margin
    })
}

/// Marches the ray in steps of `step` voxels and says whether a point of it is in one of the boxes grown by `margin`.
fn marched(
    origin: [f64; 3],
    direction: [f64; 3],
    longest: f64,
    boxes: &[SunBox],
    margin: f64,
) -> bool {
    let length = direction.iter().map(|part| part * part).sum::<f64>().sqrt();
    let step = 0.002;
    let count = (longest / step) as usize;
    (0..=count).any(|index| {
        let along = index as f64 * step / length;
        let point: [f64; 3] = std::array::from_fn(|axis| origin[axis] + direction[axis] * along);
        boxes
            .iter()
            .any(|candidate| inside(candidate, point, margin))
    })
}

fn any_box() -> impl Strategy<Value = SunBox> {
    (
        proptest::array::uniform3(-8.0f64..8.0),
        proptest::array::uniform3(0.06f64..1.0),
    )
        .prop_map(|(middle, half)| SunBox::around(middle, half))
}

proptest! {
    /// The slab method equals a ray marched in small steps, wherever the answer does not depend on the last thousandth of a voxel: a point of the march inside a box shrunk by
    /// that is a hit, and no point inside a box grown by it is a miss. Directions have parts of exactly zero some of the time.
    #[test]
    fn the_slab_method_finds_what_a_marched_ray_finds(
        origin in proptest::array::uniform3(-10.0f64..10.0),
        direction in proptest::array::uniform3(-1.0f64..1.0),
        zeroed in proptest::collection::vec(proptest::bool::ANY, 3),
        longest in 0.0f64..16.0,
        boxes in proptest::collection::vec(any_box(), 0..5),
    ) {
        let mut direction = direction;
        for axis in 0..3 {
            if zeroed[axis] {
                direction[axis] = 0.0;
            }
        }
        prop_assume!(direction.iter().any(|part| part.abs() > 1e-2));
        let by_slabs = sun_blocked_by_boxes(origin, direction, longest, &boxes);
        if marched(origin, direction, longest, &boxes, -1e-3) {
            prop_assert!(by_slabs, "a point of the ray is well inside a box and the slabs say clear");
        }
        if !marched(origin, direction, longest, &boxes, 1e-3) {
            prop_assert!(!by_slabs, "no point of the ray is near a box and the slabs say blocked");
        }
    }
}
