//! Lights kept in cells larger than a voxel are drawn blended, each with a weight of its own.

use std::sync::Arc;

use voxel_grid::{Brick, ChannelDefinition, ChannelIndex, ChannelStorage, GridLayout};
use voxel_space::BrickCoordinate;
use voxel_space::LocalVoxelIndex;
use voxunami_cave::scaled_light::{
    LevelFormat, MultiScaleView, ScaledLight, blend_brightness, combined_light,
};

/// Words as the game keeps them: six fields of five bits (the level is the greatest of them) and
/// above them two bits that say how many levels there are (0: 32, 1: 16, 2: 8, 3: 4).
const FORMAT: LevelFormat = LevelFormat {
    field_bits: 5,
    field_count: 6,
    code_shift: 30,
    code_bits: 2,
};

fn origin() -> BrickCoordinate {
    BrickCoordinate::new(0, 0, 0)
}

/// A word in which one face gives `level`, in a scheme with the number of levels `32 >> code`.
fn word(level: u32, code: u32) -> u32 {
    level | (code << 30)
}

struct Fixture {
    layout: Arc<GridLayout>,
    light: ChannelIndex,
    scales: [ChannelIndex; 3],
}

fn fixture() -> Fixture {
    let names = ["light", "scale_a", "scale_b", "scale_c"];
    let layout = Arc::new(
        GridLayout::new(
            names
                .iter()
                .map(|name| ChannelDefinition::number(format!("test:{name}").parse().unwrap(), 0))
                .collect(),
        )
        .unwrap(),
    );
    let channel = |name: &str| {
        layout
            .channel_index(&format!("test:{name}").parse().unwrap())
            .unwrap()
    };
    Fixture {
        light: channel("light"),
        scales: [channel("scale_a"), channel("scale_b"), channel("scale_c")],
        layout,
    }
}

fn voxel(x: u8, y: u8, z: u8) -> LocalVoxelIndex {
    LocalVoxelIndex::new(x, y, z)
}

/// The channels with scales of cells of the given edges (as powers of two), each as bright as `full`
/// at its greatest level.
fn view(fixture: &Fixture, scales: &[(u8, u16)]) -> MultiScaleView {
    MultiScaleView {
        light: Some(fixture.light),
        scales: scales
            .iter()
            .zip(&fixture.scales)
            .map(|(&(edge_log, full), &channel)| ScaledLight {
                channel,
                cell_width: 1 << edge_log,
                full_brightness: full,
                format: FORMAT,
            })
            .collect(),
    }
}

#[test]
fn a_word_says_its_level_and_how_many_levels_there_can_be() {
    assert_eq!(FORMAT.decode(0), (0, 31));
    assert_eq!(FORMAT.decode(word(9, 1)), (9, 15));
    assert_eq!(FORMAT.decode(word(31, 0)), (31, 31));
    assert_eq!(FORMAT.decode(word(3, 3)), (3, 3));
    // The level is the greatest of the fields.
    assert_eq!(FORMAT.decode(2 | (7 << 5) | (4 << 25) | (2 << 30)), (7, 7));
    let scale = ScaledLight {
        channel: fixture().scales[0],
        cell_width: 1,
        full_brightness: 255,
        format: FORMAT,
    };
    // Brightness is the share of the greatest level, in whatever scheme.
    assert_eq!(scale.brightness(word(15, 1)), 255);
    assert_eq!(scale.brightness(word(31, 0)), 255);
    assert_eq!(scale.brightness(word(3, 3)), 255);
    assert_eq!(scale.brightness(word(0, 2)), 0);
    assert_eq!(scale.brightness(word(7, 0)), 7 * 255 / 31);
    assert_eq!(scale.brightness(word(1, 3)), 85);
    // A weight over 255 reaches full brightness before the greatest level, and stays there.
    let heavy = ScaledLight {
        full_brightness: 382,
        ..scale
    };
    assert_eq!(heavy.brightness(word(10, 1)), 254);
    assert_eq!(heavy.brightness(word(11, 1)), 255);
    assert_eq!(heavy.brightness(word(15, 1)), 255);
    // And a weight under it is dimmer at the greatest level.
    let light = ScaledLight {
        full_brightness: 64,
        ..scale
    };
    assert_eq!(light.brightness(word(15, 1)), 64);
    assert_eq!(light.brightness(word(0, 1)), 0);
}

#[test]
fn a_game_without_scaled_light_draws_its_light_channel_as_it_is() {
    let fixture = fixture();
    let brick = Brick::new(&fixture.layout);
    assert!(combined_light(&brick, origin(), &view(&fixture, &[])).is_none());
}

#[test]
fn a_level_at_the_anchor_of_a_cell_lights_every_voxel_of_the_cell_and_no_other() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    // A cell 4 voxels wide, whose first voxel is (4, 8, 0), at level 3 of 15, as bright as 255.
    brick.set(fixture.scales[0], voxel(4, 8, 0), word(3, 1));
    let light = combined_light(&brick, origin(), &view(&fixture, &[(2, 255)])).unwrap();
    let mut lit = 0;
    for local in LocalVoxelIndex::all() {
        let inside = (4..8).contains(&local.x()) && (8..12).contains(&local.y()) && local.z() < 4;
        assert_eq!(light.get(local), if inside { 51 } else { 0 }, "{local:?}");
        lit += usize::from(inside);
    }
    assert_eq!(lit, 64);
}

#[test]
fn a_level_anywhere_but_the_anchor_of_a_cell_is_not_light() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    brick.set(fixture.scales[0], voxel(5, 8, 0), word(9, 1));
    brick.set(fixture.scales[0], voxel(4, 9, 0), word(9, 1));
    let light = combined_light(&brick, origin(), &view(&fixture, &[(2, 255)])).unwrap();
    assert!(LocalVoxelIndex::all().all(|local| light.get(local) == 0));
}

#[test]
fn each_voxel_is_as_bright_as_the_plain_light_and_every_scale_blended() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    // Cells of 1, 2 and 8 voxels, of 15 levels, each as bright as 255 at its greatest.
    brick.set(fixture.scales[0], voxel(10, 10, 10), word(4, 1)); // 68, in one voxel
    brick.set(fixture.scales[1], voxel(10, 10, 10), word(2, 1)); // 34, in the 2-cell from (10, 10, 10)
    brick.set(fixture.scales[2], voxel(8, 8, 8), word(5, 1)); // 85, in the 8-cell from (8, 8, 8)
    brick.set(fixture.light, voxel(11, 10, 10), 200); // plain light
    brick.set(fixture.light, voxel(9, 9, 9), 20);
    let light = combined_light(
        &brick,
        origin(),
        &view(&fixture, &[(0, 255), (1, 255), (3, 255)]),
    )
    .unwrap();
    // What is missing from full brightness is the product of what each light leaves missing,
    // worked by hand in whole numbers, in this order.
    assert_eq!(
        light.get(voxel(10, 10, 10)),
        147,
        "68, 34 and 85 together, more than any"
    );
    assert_eq!(light.get(voxel(11, 10, 10)), 223, "200, 34 and 85");
    assert_eq!(
        light.get(voxel(15, 15, 15)),
        85,
        "only the big cell reaches here"
    );
    assert_eq!(
        light.get(voxel(9, 9, 9)),
        98,
        "the plain light of 20 and the big cell's 85"
    );
    assert_eq!(light.get(voxel(16, 8, 8)), 0, "outside the big cell");
}

#[test]
fn the_weights_make_fine_cells_count_for_more_than_coarse_ones() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    // The same level, 5 of 15, in a cell of 1 and a cell of 8 that hold different places.
    brick.set(fixture.scales[0], voxel(0, 0, 0), word(5, 1));
    brick.set(fixture.scales[1], voxel(8, 8, 8), word(5, 1));
    // Weights 382 and 64: the fine one is 5 * 382 / 15 = 127, the coarse one 5 * 64 / 15 = 21.
    let light = combined_light(&brick, origin(), &view(&fixture, &[(0, 382), (3, 64)])).unwrap();
    assert_eq!(light.get(voxel(0, 0, 0)), 127);
    assert_eq!(light.get(voxel(9, 9, 9)), 21);
}

#[test]
fn blending_lights_adds_them_up_and_never_goes_past_full_brightness() {
    assert_eq!(blend_brightness([]), 0);
    assert_eq!(blend_brightness([0, 0]), 0);
    assert_eq!(blend_brightness([128]), 128);
    // Two of half brightness make three quarters.
    assert!((i64::from(blend_brightness([128, 128])) - 192).abs() <= 1);
    // Darkness adds nothing, and full brightness is full whatever else there is.
    assert_eq!(blend_brightness([77, 0, 0]), 77);
    assert_eq!(blend_brightness([255, 90, 200]), 255);
    // More light is never less bright than any one of its parts.
    for first in (0..=255).step_by(5) {
        for second in (0..=255).step_by(5) {
            let both = blend_brightness([first, second]);
            assert!(both >= first.max(second), "{first} {second} {both}");
            assert!(both <= 255);
            // Two lights that are clearly there are brighter together (a very dim one beside a bright
            // one rounds away).
            if (40..=200).contains(&first) && (40..=200).contains(&second) {
                assert!(both > first.max(second), "{first} {second} {both}");
            }
        }
    }
    // Anything beyond full brightness counts as full.
    assert_eq!(blend_brightness([1000, 3]), 255);
}

#[test]
fn light_never_exceeds_the_brightest_a_voxel_can_be() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    brick.set(fixture.scales[0], voxel(0, 0, 0), word(15, 1));
    brick.set(fixture.scales[1], voxel(0, 0, 0), word(15, 1));
    let light = combined_light(&brick, origin(), &view(&fixture, &[(0, 382), (0, 382)])).unwrap();
    assert_eq!(light.get(voxel(0, 0, 0)), 255);
}

#[test]
fn a_brick_whose_channels_are_each_the_same_everywhere_is_the_same_everywhere() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    let scales = [(0, 255), (1, 255), (2, 255)];
    let dark = combined_light(&brick, origin(), &view(&fixture, &scales)).unwrap();
    assert_eq!(dark.uniform_value(), Some(0));
    *brick.channel_mut(fixture.scales[1]) = ChannelStorage::uniform(word(3, 1));
    *brick.channel_mut(fixture.light) = ChannelStorage::uniform(40);
    let lit = combined_light(&brick, origin(), &view(&fixture, &scales)).unwrap();
    assert_eq!(
        lit.uniform_value(),
        Some(83),
        "the light of 40 and 3 levels of 17 (51), blended"
    );
    // The same answer as working it out voxel by voxel, where it is the same, and brighter where
    // another scale adds to it.
    brick.set(fixture.scales[2], voxel(0, 0, 0), word(1, 1)); // 17, in the cell of 4 at the origin
    let by_voxel = combined_light(&brick, origin(), &view(&fixture, &scales)).unwrap();
    assert_eq!(by_voxel.get(voxel(20, 20, 20)), 83);
    assert_eq!(by_voxel.get(voxel(3, 3, 3)), 94);
    assert_eq!(by_voxel.get(voxel(4, 0, 0)), 83);
}

#[test]
fn the_words_scheme_not_the_view_says_how_many_levels_there_are() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    // The same brightness from the greatest level of each scheme.
    for (code, greatest) in [(0, 31), (1, 15), (2, 7), (3, 3)] {
        *brick.channel_mut(fixture.scales[0]) = ChannelStorage::uniform(word(greatest, code));
        let light = combined_light(&brick, origin(), &view(&fixture, &[(0, 200)])).unwrap();
        assert_eq!(light.uniform_value(), Some(200), "code {code}");
    }
}

/// The channels with one scale of cells of any width.
fn one_scale(fixture: &Fixture, width: u16) -> MultiScaleView {
    let mut view = view(fixture, &[]);
    view.scales.push(ScaledLight {
        channel: fixture.scales[0],
        cell_width: width,
        full_brightness: 255,
        format: FORMAT,
    });
    view
}

#[test]
fn cells_of_any_width_are_found_by_where_they_start_in_the_brick() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    // Cells of 5 in the first brick start at 0, 5, 10, ... 30 (the last one goes on into the next brick).
    // One at x = 10..14, y = 5..9, z = 0..4, with the word at its first voxel.
    brick.set(fixture.scales[0], voxel(10, 5, 0), word(3, 1));
    let light = combined_light(&brick, origin(), &one_scale(&fixture, 5)).unwrap();
    for local in LocalVoxelIndex::all() {
        let inside = (10..15).contains(&local.x()) && (5..10).contains(&local.y()) && local.z() < 5;
        assert_eq!(light.get(local), if inside { 51 } else { 0 }, "{local:?}");
    }
}

#[test]
fn a_cell_that_began_in_the_brick_before_is_found_at_the_edge_of_this_one() {
    let fixture = fixture();
    // Cells of 5: the one that starts at x = 30 goes to 34, so in the next brick (x = 32 on) it is at
    // 32..34 and its word is at local x = 0. Likewise along the other axes, and the brick can
    // be anywhere, including before the origin.
    for brick_coordinate in [
        BrickCoordinate::new(1, 0, 0),
        BrickCoordinate::new(0, 1, 1),
        BrickCoordinate::new(-1, 2, -3),
    ] {
        let mut brick = Brick::new(&fixture.layout);
        brick.set(fixture.scales[0], voxel(0, 0, 0), word(2, 1));
        let light = combined_light(&brick, brick_coordinate, &one_scale(&fixture, 5)).unwrap();
        // The voxels of the cell that are in this brick: from the edge to where the cell ends, along
        // each axis where the cell was begun in the brick before.
        let origin = [
            brick_coordinate.x() * 32,
            brick_coordinate.y() * 32,
            brick_coordinate.z() * 32,
        ];
        let reach = |axis: usize| {
            // How far into this brick the cell that holds its first voxel goes.
            let start = origin[axis].div_euclid(5) * 5;
            (start + 5 - origin[axis]).clamp(0, 32) as u8
        };
        let (rx, ry, rz) = (reach(0), reach(1), reach(2));
        let mut lit = 0;
        for local in LocalVoxelIndex::all() {
            let inside = local.x() < rx && local.y() < ry && local.z() < rz;
            lit += usize::from(inside);
            if inside {
                assert_eq!(light.get(local), 34, "{brick_coordinate:?} {local:?}");
            }
        }
        // The voxels past the cell are in the next cell, which has no word here.
        assert_eq!(
            light.get(voxel(rx.min(31), 0, 0)).min(34),
            if rx < 32 { 0 } else { 34 }
        );
        assert_eq!(lit, usize::from(rx) * usize::from(ry) * usize::from(rz));
    }
}

#[test]
fn cells_wider_than_a_brick_start_at_the_edge_of_every_brick_they_are_in() {
    let fixture = fixture();
    // A cell of 40 starts at 0 and runs to 39: in the second brick (32 on) it is 32..39, with its word at
    // local 0.
    let mut brick = Brick::new(&fixture.layout);
    brick.set(fixture.scales[0], voxel(0, 0, 0), word(1, 1));
    let light = combined_light(
        &brick,
        BrickCoordinate::new(1, 0, 0),
        &one_scale(&fixture, 40),
    )
    .unwrap();
    assert_eq!(light.get(voxel(0, 0, 0)), 17);
    assert_eq!(
        light.get(voxel(7, 31, 31)),
        17,
        "y and z are in the same cell all through the brick"
    );
    assert_eq!(light.get(voxel(7, 0, 0)), 17);
    assert_eq!(light.get(voxel(8, 0, 0)), 0, "x = 40 starts the next cell");
}

#[test]
fn a_width_that_divides_the_brick_is_found_as_a_mask_would_find_it() {
    let fixture = fixture();
    for width in [1u16, 2, 4, 8, 16, 32] {
        let mut brick = Brick::new(&fixture.layout);
        let mut by_hand = Vec::new();
        for anchor in (0..32).step_by(usize::from(width)).step_by(1) {
            let word_here = word(1 + (anchor as u32 % 14), 1);
            brick.set(fixture.scales[0], voxel(anchor as u8, 0, 0), word_here);
            by_hand.push((anchor as u8, word_here));
        }
        let light = combined_light(&brick, origin(), &one_scale(&fixture, width)).unwrap();
        for local_x in 0..32u8 {
            let anchor = local_x & !(width as u8 - 1);
            let expected_word = by_hand
                .iter()
                .find(|&&(at, _)| at == anchor)
                .map_or(0, |&(_, w)| w);
            let (level, greatest) = FORMAT.decode(expected_word);
            assert_eq!(
                light.get(voxel(local_x, 0, 0)),
                level * 255 / greatest,
                "width {width} x {local_x}"
            );
        }
    }
}
