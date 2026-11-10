use voxel_space::{BrickCoordinate, WorldPosition};

use super::*;

fn at(x: f64, y: f64, z: f64) -> WorldPosition {
    WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z])
}

fn item(position: WorldPosition, size: [u8; 3]) -> EntityBox {
    EntityBox {
        position,
        size,
        material: 1,
    }
}

#[test]
fn an_avatar_is_a_box_a_voxel_wide_and_two_high_with_the_eyes_in_its_upper_voxel() {
    let avatar = Avatar {
        position: at(10.0, 20.0, 30.0),
        color: 3,
    };
    let boxes = nearest_sun_boxes(&[avatar], &[], at(0.0, 0.0, 0.0));
    assert_eq!(
        boxes,
        [SunBoxUniform {
            low: [9.5, 18.5, 29.5, 0.0],
            high: [10.5, 20.5, 30.5, 0.0],
        }]
    );
}

#[test]
fn an_entity_box_is_as_big_as_it_is_drawn_made_even_and_put_from_two_to_eight_eighths() {
    // (The sizes the mesh is made in: `EntityBox::key`.)
    let boxes = nearest_sun_boxes(
        &[],
        &[
            item(at(1.0, 2.0, 3.0), [8, 4, 2]),
            item(at(11.0, 12.0, 13.0), [3, 0, 9]),
        ],
        at(0.0, 0.0, 0.0),
    );
    assert_eq!(
        boxes,
        [
            SunBoxUniform {
                low: [0.5, 1.75, 2.875, 0.0],
                high: [1.5, 2.25, 3.125, 0.0],
            },
            SunBoxUniform {
                low: [10.75, 11.875, 12.5, 0.0],
                high: [11.25, 12.125, 13.5, 0.0],
            },
        ]
    );
}

#[test]
fn boxes_are_relative_to_the_camera_however_many_bricks_away_it_is() {
    let camera = WorldPosition::new(BrickCoordinate::new(5, -2, 7), [1.0, 2.0, 3.0]);
    let avatar = Avatar {
        position: WorldPosition::new(BrickCoordinate::new(3, -2, 8), [0.5, 4.0, 3.0]),
        color: 0,
    };
    // 2 bricks back along x, none along y, and one forward along z.
    let boxes = nearest_sun_boxes(&[avatar], &[], camera);
    assert_eq!(boxes.len(), 1);
    let middle_x = -64.0 + 0.5 - 1.0;
    assert_eq!(boxes[0].low, [middle_x - 0.5, 2.0 - 1.5, 32.0 - 0.5, 0.0]);
    assert_eq!(boxes[0].high, [middle_x + 0.5, 2.0 + 0.5, 32.0 + 0.5, 0.0]);
}

#[test]
fn only_the_nearest_boxes_are_kept_nearest_first_whatever_they_are() {
    // Twenty boxes at distances 1 to 20 along x, told of in a scrambled order, some avatars and some items.
    let order: Vec<u32> = (0..20).map(|index| (index * 7) % 20 + 1).collect();
    let (mut avatars, mut items) = (Vec::new(), Vec::new());
    for &distance in &order {
        let position = at(f64::from(distance) * 3.0, 0.0, 0.0);
        if distance % 2 == 0 {
            avatars.push(Avatar { position, color: 0 });
        } else {
            items.push(item(position, [8, 8, 8]));
        }
    }
    let boxes = nearest_sun_boxes(&avatars, &items, at(0.0, 0.0, 0.0));
    assert_eq!(boxes.len(), MAXIMUM_SUN_BOXES);
    let middles: Vec<f32> = boxes.iter().map(|b| (b.low[0] + b.high[0]) / 2.0).collect();
    let expected: Vec<f32> = (1..=16).map(|distance| distance as f32 * 3.0).collect();
    assert_eq!(middles, expected);
}

#[test]
fn no_entities_make_no_boxes() {
    assert!(nearest_sun_boxes(&[], &[], at(0.0, 0.0, 0.0)).is_empty());
}
