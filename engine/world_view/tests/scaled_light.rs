//! Lights kept in cells larger than a voxel are drawn as the brightest light that reaches each voxel.

use std::sync::Arc;

use voxel_grid::{Brick, ChannelDefinition, ChannelIndex, GridLayout};
use voxel_space::LocalVoxelIndex;
use world_view::{ScaledLight, ViewChannels, blend_brightness, combined_light};

struct Fixture {
    layout: Arc<GridLayout>,
    blocks: ChannelIndex,
    light: ChannelIndex,
    scales: [ChannelIndex; 3],
}

fn fixture() -> Fixture {
    let names = ["blocks", "light", "scale_a", "scale_b", "scale_c"];
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
        blocks: channel("blocks"),
        light: channel("light"),
        scales: [channel("scale_a"), channel("scale_b"), channel("scale_c")],
        layout,
    }
}

fn voxel(x: u8, y: u8, z: u8) -> LocalVoxelIndex {
    LocalVoxelIndex::new(x, y, z)
}

fn view(fixture: &Fixture, edge_logs: &[u8]) -> ViewChannels {
    let mut view = ViewChannels::new(fixture.blocks, None, Some(fixture.light));
    for (slot, (&edge_log, &channel)) in view
        .scaled_light
        .iter_mut()
        .zip(edge_logs.iter().zip(&fixture.scales))
    {
        *slot = Some(ScaledLight {
            channel,
            cell_edge_log2: edge_log,
            brightness_per_level: 17,
        });
    }
    view
}

#[test]
fn a_game_without_scaled_light_draws_its_light_channel_as_it_is() {
    let fixture = fixture();
    let brick = Brick::new(&fixture.layout);
    assert!(combined_light(&brick, &view(&fixture, &[])).is_none());
}

#[test]
fn a_level_at_the_anchor_of_a_cell_lights_every_voxel_of_the_cell_and_no_other() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    // A cell 4 voxels wide, whose first voxel is (4, 8, 0), at level 3.
    brick.set(fixture.scales[0], voxel(4, 8, 0), 3);
    let light = combined_light(&brick, &view(&fixture, &[2])).unwrap();
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
    brick.set(fixture.scales[0], voxel(5, 8, 0), 9);
    brick.set(fixture.scales[0], voxel(4, 9, 0), 9);
    let light = combined_light(&brick, &view(&fixture, &[2])).unwrap();
    assert!(LocalVoxelIndex::all().all(|local| light.get(local) == 0));
}

#[test]
fn each_voxel_is_as_bright_as_the_plain_light_and_every_scale_blended() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    // The scales have cells of 1, 2 and 8 voxels.
    brick.set(fixture.scales[0], voxel(10, 10, 10), 4); // 68, in one voxel
    brick.set(fixture.scales[1], voxel(10, 10, 10), 2); // 34, in the 2-cell from (10, 10, 10)
    brick.set(fixture.scales[2], voxel(8, 8, 8), 5); // 85, in the 8-cell from (8, 8, 8)
    brick.set(fixture.light, voxel(11, 10, 10), 200); // plain light
    brick.set(fixture.light, voxel(9, 9, 9), 20);
    let light = combined_light(&brick, &view(&fixture, &[0, 1, 3])).unwrap();
    // What is missing from full brightness is the product of what each light leaves missing:
    // 255 * (187/255) * (221/255) * (170/255) is about 108, so 147 of 255 (worked by hand, in whole
    // numbers, in this order).
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
    brick.set(fixture.scales[0], voxel(0, 0, 0), 15);
    brick.set(fixture.scales[1], voxel(0, 0, 0), 200); // not a level a game would store
    let light = combined_light(&brick, &view(&fixture, &[0, 0])).unwrap();
    assert_eq!(light.get(voxel(0, 0, 0)), 255);
}

#[test]
fn a_brick_whose_channels_are_each_the_same_everywhere_is_the_same_everywhere() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    let dark = combined_light(&brick, &view(&fixture, &[0, 1, 2])).unwrap();
    assert_eq!(dark.uniform_value(), Some(0));
    *brick.channel_mut(fixture.scales[1]) = voxel_grid::ChannelStorage::uniform(3);
    *brick.channel_mut(fixture.light) = voxel_grid::ChannelStorage::uniform(40);
    let lit = combined_light(&brick, &view(&fixture, &[0, 1, 2])).unwrap();
    assert_eq!(
        lit.uniform_value(),
        Some(83),
        "the light of 40 and 3 levels of 17 (51), blended"
    );
    // The same answer as working it out voxel by voxel, where it is the same, and brighter where
    // another scale adds to it.
    brick.set(fixture.scales[2], voxel(0, 0, 0), 1); // 17, in the cell of 4 at the origin
    let by_voxel = combined_light(&brick, &view(&fixture, &[0, 1, 2])).unwrap();
    assert_eq!(by_voxel.get(voxel(20, 20, 20)), 83);
    assert_eq!(by_voxel.get(voxel(3, 3, 3)), 94);
    assert_eq!(by_voxel.get(voxel(4, 0, 0)), 83);
}
