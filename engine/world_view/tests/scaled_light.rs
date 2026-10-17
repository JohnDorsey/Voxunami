//! Lights kept in cells larger than a voxel are drawn as the brightest light that reaches each voxel.

use std::sync::Arc;

use voxel_grid::{Brick, ChannelDefinition, ChannelIndex, GridLayout};
use voxel_space::LocalVoxelIndex;
use world_view::{ScaledLight, ViewChannels, combined_light};

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
fn each_voxel_is_as_bright_as_the_brightest_of_the_light_channel_and_every_scale() {
    let fixture = fixture();
    let mut brick = Brick::new(&fixture.layout);
    // The scales have cells of 1, 2 and 8 voxels.
    brick.set(fixture.scales[0], voxel(10, 10, 10), 4); // 68, in one voxel
    brick.set(fixture.scales[1], voxel(10, 10, 10), 2); // 34, in the 2-cell from (10, 10, 10)
    brick.set(fixture.scales[2], voxel(8, 8, 8), 5); // 85, in the 8-cell from (8, 8, 8)
    brick.set(fixture.light, voxel(11, 10, 10), 200); // plain light
    brick.set(fixture.light, voxel(9, 9, 9), 20);
    let light = combined_light(&brick, &view(&fixture, &[0, 1, 3])).unwrap();
    assert_eq!(
        light.get(voxel(10, 10, 10)),
        85,
        "the brightest of 68, 34 and 85"
    );
    assert_eq!(
        light.get(voxel(11, 10, 10)),
        200,
        "plain light brighter than the cell's 85"
    );
    assert_eq!(
        light.get(voxel(15, 15, 15)),
        85,
        "only the big cell reaches here"
    );
    assert_eq!(
        light.get(voxel(9, 9, 9)),
        85,
        "the plain light of 20 is dimmer"
    );
    assert_eq!(light.get(voxel(16, 8, 8)), 0, "outside the big cell");
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
        Some(51),
        "3 levels of 17 beat the light of 40"
    );
    // The same answer as working it out voxel by voxel.
    brick.set(fixture.scales[2], voxel(0, 0, 0), 1); // not the same everywhere now: 17, dimmer
    let by_voxel = combined_light(&brick, &view(&fixture, &[0, 1, 2])).unwrap();
    assert!(LocalVoxelIndex::all().all(|local| by_voxel.get(local) == 51));
}
