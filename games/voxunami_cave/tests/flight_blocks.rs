//! The two blocks of flight that exist for building and testing ships: the dev rocket engine, a three-tiered pyramid in gunmetal gray with a red face at the exhaust, placed
//! like a log; and the dev command seat, a stair in gunmetal gray whose bottom and back are brown. Both are for creative only.

use block_models::{BlockTable, Placement, Shape, ShapeBox};
use game_shell::GameContent;
use voxel_space::{FaceDirection, LatticeRotation};
use voxunami_cave::content::{CaveContent, dev_command_seat_id, dev_rocket_engine_id};
use voxunami_cave::items::{creative_only_blocks, placeable_blocks};

const RED: [f32; 3] = [200.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0];
const BROWN: [f32; 3] = [110.0 / 255.0, 70.0 / 255.0, 35.0 / 255.0];

fn raw_id(content: &CaveContent, id: &namespaced_id::NamespacedId) -> u32 {
    content.registries()[0].1.raw_id_of(id).unwrap()
}

/// The number of the turn that carries the face `from` to the face `to` (the first, if there are several).
fn orientation_carrying(from: FaceDirection, to: FaceDirection) -> u32 {
    LatticeRotation::all()
        .into_iter()
        .find(|rotation| rotation.apply_face(from) == to)
        .unwrap()
        .index()
        .into()
}

/// Dark, a little blue, and not red or brown.
fn assert_gunmetal(table: &BlockTable, block: u32) {
    let [red, green, blue] = table.color(block);
    assert!(
        red < 0.3 && green < 0.3 && blue < 0.3 && blue >= red,
        "{red} {green} {blue}"
    );
}

#[test]
fn both_blocks_can_be_placed_but_only_in_creative_and_come_after_the_docking_port() {
    let content = CaveContent::new();
    let package = content.package_data();
    let placeable = placeable_blocks(&package);
    let seat = dev_command_seat_id();
    let engine = dev_rocket_engine_id();
    // (The hotbar is in the order of the blocks: the seventh is the docking port, then these.)
    assert_eq!(placeable[7], seat);
    assert_eq!(placeable[8], engine);
    let creative = creative_only_blocks(&package);
    assert!(creative.contains(&seat) && creative.contains(&engine));
}

#[test]
fn the_engine_is_a_pyramid_of_three_tiers_with_a_red_face_at_its_wide_exhaust_end() {
    let content = CaveContent::new();
    let table = content.block_table();
    let block = raw_id(&content, &dev_rocket_engine_id());
    assert_eq!(table.state_count(block), 1);
    assert_gunmetal(&table, block);
    // As it is made the exhaust is up: the wide tier is at the top of the voxel, and the narrowest at the bottom (in eighths of a voxel).
    let made = table.shape(table.model_of(block, 0, 0));
    let tiers = Shape::from_boxes(&[
        ShapeBox {
            min: [0, 5, 0],
            max: [8, 8, 8],
        },
        ShapeBox {
            min: [1, 3, 1],
            max: [7, 5, 7],
        },
        ShapeBox {
            min: [2, 0, 2],
            max: [6, 3, 6],
        },
    ]);
    assert!(made.same_cells(&tiers));
    assert!(made.covers_face(FaceDirection::PositiveY));
    assert!(!made.covers_face(FaceDirection::NegativeY));
    assert_eq!(made.filled_cells(), 64 * 3 + 36 * 2 + 16 * 3);
    // The red face is the exhaust end, wherever the engine is turned: the one face, no other.
    let palette = table.palette();
    for exhaust in FaceDirection::ALL {
        let orientation = orientation_carrying(FaceDirection::PositiveY, exhaust);
        let red = table.material_of_face(block, 0, orientation, exhaust);
        assert_eq!(palette[red as usize], RED);
        assert_eq!(table.axis_face(block, 0, orientation), Some(exhaust));
        for other in FaceDirection::ALL {
            let wanted = if other == exhaust { red } else { block };
            assert_eq!(
                table.material_of_face(block, 0, orientation, other),
                wanted,
                "exhaust {exhaust:?}, looking at {other:?}"
            );
        }
        // The shape turns with it: the wide tier is on the exhaust side.
        assert!(
            table
                .shape(table.model_of(block, 0, orientation))
                .covers_face(exhaust)
        );
    }
}

#[test]
fn the_engine_is_placed_like_a_log_with_the_exhaust_away_from_the_surface_that_was_clicked() {
    let content = CaveContent::new();
    let table = content.block_table();
    let block = raw_id(&content, &dev_rocket_engine_id());
    assert_eq!(
        table.placement(block, 0),
        Placement::AlongPlacedFace(FaceDirection::PositiveY)
    );
    // Placed against a surface, the engine is put on the clicked face's side of it, and the exhaust end is that face's direction: away from the surface.
    let view = voxunami_cave::placement::PlacerView {
        eyes: [0.5, 5.5, 0.5],
        forward: [1.0, 0.0, 0.0],
    };
    for clicked in FaceDirection::ALL {
        let orientation = voxunami_cave::placement::orientation_of_placement(
            table.placement(block, 0),
            voxel_space::VoxelCoordinate::new(10, 10, 10),
            Some(clicked),
            &view,
        );
        assert_eq!(
            table.axis_face(block, 0, orientation),
            Some(clicked),
            "clicked {clicked:?}"
        );
        // And the red end is that face.
        assert_eq!(
            table.palette()[table.material_of_face(block, 0, orientation, clicked) as usize],
            RED,
            "clicked {clicked:?}"
        );
    }
}

#[test]
fn the_seat_is_a_stair_in_gunmetal_gray_whose_bottom_and_back_are_brown() {
    let content = CaveContent::new();
    let table = content.block_table();
    let block = raw_id(&content, &dev_command_seat_id());
    assert_eq!(table.state_count(block), 1);
    assert_gunmetal(&table, block);
    // As it is made the back is the east (the tall side of the stair) and the bottom is down; a sitter faces west.
    assert!(
        table
            .shape(table.model_of(block, 0, 0))
            .same_cells(&Shape::stair(FaceDirection::PositiveX))
    );
    let palette = table.palette();
    for (orientation, rotation) in LatticeRotation::all().into_iter().enumerate() {
        let back = rotation.apply_face(FaceDirection::PositiveX);
        let bottom = rotation.apply_face(FaceDirection::NegativeY);
        // The back and the bottom are one colour, so one entry of the palette.
        let brown = table.material_of_face(block, 0, orientation as u32, back);
        assert_eq!(palette[brown as usize], BROWN);
        for face in FaceDirection::ALL {
            let wanted = if face == back || face == bottom {
                brown
            } else {
                block
            };
            assert_eq!(
                table.material_of_face(block, 0, orientation as u32, face),
                wanted,
                "orientation {orientation}, face {face:?}"
            );
        }
    }
}

#[test]
fn the_seat_is_placed_with_its_back_to_the_player_so_that_whoever_sits_faces_the_way_the_player_looked()
 {
    let content = CaveContent::new();
    let table = content.block_table();
    let block = raw_id(&content, &dev_command_seat_id());
    assert_eq!(
        table.placement(block, 0),
        Placement::FrontToPlayer(FaceDirection::PositiveX)
    );
    // A player to the west of the place, looking east: the back (the front of the block) is toward them, the sitter faces east.
    let view = voxunami_cave::placement::PlacerView {
        eyes: [6.0, 10.5, 10.5],
        forward: [1.0, 0.0, 0.0],
    };
    let orientation = voxunami_cave::placement::orientation_of_placement(
        table.placement(block, 0),
        voxel_space::VoxelCoordinate::new(10, 10, 10),
        None,
        &view,
    );
    assert_eq!(
        table.front_face(block, 0, orientation),
        Some(FaceDirection::NegativeX)
    );
}
