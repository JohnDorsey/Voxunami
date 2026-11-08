//! Cave's hotbar and inventory panel: panels of the screen that its client plugin builds from what the server has said.

mod common;

use common::{Player, PlayerExt, Rig, above_ground, ground_height};
use game_shell::client_plugin::MouseButton;
use game_shell::screen::{ScreenText, Source};
use game_shell::{GameContent, KeyCode};
use ui::{Anchor, Color, Direction, Node, Panel, Ui};
use voxel_space::VoxelCoordinate;

fn tick(rig: &mut Rig, player: &mut Player, count: usize) {
    for _ in 0..count {
        rig.step();
        player.advance().unwrap();
        let _ = player.update_view();
    }
}

fn hovering(rig: &mut Rig) -> (Player, VoxelCoordinate) {
    let mut player = rig.join([16.5, 110.0, 16.5], 0.0, 0.0);
    rig.settle(&mut [&mut player]);
    player.camera = above_ground(&player, 16, 16, 2.0);
    player.teleport_to_camera();
    rig.settle(&mut [&mut player]);
    let ground = ground_height(&player, 16, 16);
    (player, VoxelCoordinate::new(16, ground, 16))
}

/// The panels of the client plugins now on the screen, with where each came from.
fn overlays(player: &mut Player) -> Vec<(usize, Ui)> {
    let screen = player.screen(&ScreenText::default());
    screen
        .sources
        .iter()
        .zip(screen.uis)
        .filter_map(|(source, ui)| match source {
            Source::Overlay(index) => Some((*index, ui)),
            _ => None,
        })
        .collect()
}

/// The swatches of a row panel: colour, count and whether chosen.
fn swatches(panel: &Panel) -> Vec<(Color, Option<u32>, bool)> {
    panel
        .children
        .iter()
        .filter_map(|node| match node {
            Node::Swatch {
                color,
                count,
                selected,
            } => Some((*color, *count, *selected)),
            _ => None,
        })
        .collect()
}

/// The colour a block of the base blocks is drawn in on the screen.
fn byte_color(color: [f32; 3]) -> Color {
    Color(color.map(|component| (component * 255.0).round() as u8))
}

#[test]
fn the_hotbar_is_a_swatch_for_each_block_that_can_be_placed_in_its_colour_with_the_chosen_one_marked()
 {
    let mut rig = Rig::new();
    let (mut player, _) = hovering(&mut rig);
    tick(&mut rig, &mut player, 2);
    let panels = overlays(&mut player);
    let [(0, hotbar)] = &panels[..] else {
        panic!("only the hotbar is up: {panels:?}");
    };
    assert_eq!(hotbar.anchor, Anchor::Bottom);
    assert!(!hotbar.modal);
    assert_eq!(hotbar.root.direction, Direction::Row);
    let shown = swatches(&hotbar.root);
    // Stone, dirt, grass, lamp, wood, fire, and the dev docking port, command seat and rocket engine: in the colours the blocks are drawn in (the table of the game says
    // which: for a block with pictures it is their average), the first chosen, no counts in creative.
    let content = voxunami_cave::content::CaveContent::new();
    let table = content.block_table();
    let ids = content.blocks;
    let package = content.package_data();
    let position_of = |wanted: namespaced_id::NamespacedId| {
        package
            .blocks
            .iter()
            .position(|(identifier, _)| *identifier == wanted)
            .expect("the package has the dev block") as u32
    };
    let port = position_of(voxunami_cave::content::dev_docking_port_id());
    let seat = position_of(voxunami_cave::content::dev_command_seat_id());
    let engine = position_of(voxunami_cave::content::dev_rocket_engine_id());
    let expected_colors = [
        table.color(ids.stone),
        table.color(ids.dirt),
        table.color(ids.grass),
        table.color(ids.lamp),
        table.color(ids.wood),
        table.color(ids.fire),
        table.color(port),
        table.color(seat),
        table.color(engine),
    ];
    // (The two that have no pictures are what their files say.)
    assert_eq!(expected_colors[3], [1.0, 0.92, 0.65]);
    assert_eq!(expected_colors[5], [0.95, 0.5, 0.1]);
    // (The seat and the engine are gunmetal gray, and their brown and red faces are not their colour.)
    assert_eq!(expected_colors[7], [0.17, 0.21, 0.22]);
    assert_eq!(expected_colors[8], [0.17, 0.21, 0.22]);
    assert_eq!(shown.len(), 9);
    for (index, ((color, count, selected), expected)) in
        shown.iter().zip(expected_colors).enumerate()
    {
        assert_eq!(*color, byte_color(expected), "swatch {index}");
        assert_eq!(*count, None, "creative has no counts");
        assert_eq!(*selected, index == 0);
    }

    // A number key chooses another.
    player.key_pressed(KeyCode::Digit4);
    let panels = overlays(&mut player);
    let chosen: Vec<bool> = swatches(&panels[0].1.root)
        .iter()
        .map(|(_, _, selected)| *selected)
        .collect();
    assert_eq!(
        chosen,
        [false, false, false, true, false, false, false, false, false]
    );
    // The ninth key chooses the ninth block, the dev rocket engine; the right square bracket goes on round to the first, and the left one back to the last.
    let chosen_index = |player: &mut Player| -> usize {
        let panels = overlays(player);
        let chosen: Vec<bool> = swatches(&panels[0].1.root)
            .iter()
            .map(|(_, _, selected)| *selected)
            .collect();
        assert_eq!(
            chosen.iter().filter(|selected| **selected).count(),
            1,
            "one is always chosen"
        );
        chosen.iter().position(|selected| *selected).unwrap()
    };
    player.key_pressed(KeyCode::Digit9);
    assert_eq!(chosen_index(&mut player), 8);
    player.key_pressed(KeyCode::BracketRight);
    assert_eq!(chosen_index(&mut player), 0);
    player.key_pressed(KeyCode::BracketLeft);
    assert_eq!(chosen_index(&mut player), 8);
    player.key_pressed(KeyCode::BracketLeft);
    assert_eq!(chosen_index(&mut player), 7);
}

#[test]
fn in_survival_the_hotbar_counts_what_the_player_has_and_the_inventory_panel_lists_it_on_e() {
    let mut rig = Rig::new();
    let (mut player, _) = hovering(&mut rig);
    player.send_command("/gamemode survival");
    tick(&mut rig, &mut player, 2);
    // Nothing held: every swatch says 0.
    let panels = overlays(&mut player);
    assert!(
        swatches(&panels[0].1.root)
            .iter()
            .all(|(_, count, _)| *count == Some(0))
    );
    assert_eq!(panels.len(), 1, "the inventory is not open");

    // Open it: nothing in it yet.
    player.key_pressed(KeyCode::KeyE);
    let panels = overlays(&mut player);
    assert_eq!(panels.len(), 2);
    let (1, inventory) = &panels[1] else {
        panic!("the inventory comes after the hotbar");
    };
    assert_eq!(inventory.anchor, Anchor::Right);
    assert_eq!(inventory.root.title.as_deref(), Some("Inventory"));
    assert!(
        matches!(&inventory.root.children[..], [Node::Label { text, .. }] if text == "nothing")
    );

    // Break the ground: its item comes into the inventory, and shows in both places.
    player.mouse_pressed(MouseButton::Primary);
    tick(&mut rig, &mut player, 20);
    let panels = overlays(&mut player);
    let held_in_hotbar: Vec<Option<u32>> = swatches(&panels[0].1.root)
        .iter()
        .map(|(_, count, _)| *count)
        .collect();
    assert_eq!(
        held_in_hotbar.iter().flatten().sum::<u32>(),
        1,
        "one block was picked up: {held_in_hotbar:?}"
    );
    let rows: Vec<&Node> = panels[1].1.root.children.iter().collect();
    assert_eq!(rows.len(), 1, "one kind of item is held");
    let Node::Panel(row) = rows[0] else {
        panic!("a row for the item");
    };
    assert_eq!(swatches(row)[0].1, Some(1));
    assert!(
        matches!(&row.children[1], Node::Label { text, .. } if !text.is_empty() && !text.contains('_'))
    );

    // E again puts it away.
    player.key_pressed(KeyCode::KeyE);
    assert_eq!(overlays(&mut player).len(), 1);
}
