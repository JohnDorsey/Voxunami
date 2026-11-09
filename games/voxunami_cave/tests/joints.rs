//! The dev ball joint, the block (step 2 of `docs/decisions/joints-and-cars.md`): what it is, how it lies in a cell, how it is found in a ship, and that a joint is
//! never taken for a docking port nor a port for a joint. (What two of them do together is the coupling logic, a later step.)

mod common;

use block_models::{Placement, Shape, ShapeBox};
use common::docking::{PLAIN, facing, make, port_block, stones};
use common::joints::{ball_end_toward, joint_block};
use common::{Scene, ship_id};
use game_shell::GameContent;
use voxel_space::{
    BrickCoordinate, FaceDirection, GridPose, Orientation, VoxelCoordinate, WorldPosition,
};
use voxunami_cave::content::{
    CaveContent, dev_ball_joint_id, dev_command_seat_id, dev_docking_port_id, dev_rocket_engine_id,
};
use voxunami_cave::items::{creative_only_blocks, placeable_blocks};
use voxunami_cave::plugins::docking;

/// The colour of the ball end (a flat colour for now: the block has no pictures yet).
const BALL_END: [f32; 3] = [0.2, 0.6, 1.0];

/// The joint as it is made, in eighths of a voxel (0.75 by 0.5 by 0.5 metres): its flat end is flush with the bottom of the cell and its ball end, up, is a quarter of a metre
/// short of the top; it is centred across.
const AS_MADE: ShapeBox = ShapeBox {
    min: [2, 0, 2],
    max: [6, 6, 6],
};

fn pose(x: f64, y: f64, z: f64) -> GridPose {
    GridPose::new(
        WorldPosition::new(BrickCoordinate::ORIGIN, [x, y, z]),
        Orientation::IDENTITY,
    )
}

#[test]
fn the_dev_ball_joint_is_the_last_block_of_cave_placeable_and_creative_only() {
    let content = CaveContent::new();
    let package = content.package_data();
    let id = dev_ball_joint_id();
    // (Last: the numbers of the blocks that were there before it do not move.)
    assert_eq!(
        package.blocks.last().map(|(identifier, _)| identifier),
        Some(&id)
    );
    assert!(placeable_blocks(&package).contains(&id));
    assert_eq!(
        creative_only_blocks(&package),
        [
            dev_docking_port_id(),
            dev_command_seat_id(),
            dev_rocket_engine_id(),
            id
        ]
    );
}

#[test]
fn a_dev_ball_joint_is_made_with_its_ball_end_up_one_state_and_is_placed_like_a_log() {
    let content = CaveContent::new();
    let table = content.block_table();
    let block = joint_block();
    assert_eq!(table.state_count(block), 1);
    assert_eq!(
        table.joint_face(block, 0, 0),
        Some(FaceDirection::PositiveY)
    );
    assert_eq!(table.axis_face(block, 0, 0), Some(FaceDirection::PositiveY));
    // It has no front and is not a docking port.
    assert_eq!(table.front_face(block, 0, 0), None);
    assert_eq!(table.docking_face(block, 0, 0), None);
    // Placed with its ball end away from the face that was clicked, whichever way the player looks (it is the same all round).
    assert_eq!(
        table.placement(block, 0),
        Placement::AlongPlacedFace(FaceDirection::PositiveY)
    );
    assert!(
        table
            .shape(table.model_of(block, 0, 0))
            .same_cells(&Shape::from_boxes(&[AS_MADE]))
    );
    // The dev docking port is not a joint, nor are the other blocks of flight.
    for other in [
        port_block(),
        content.blocks.stone,
        CaveContent::new().registries()[0]
            .1
            .raw_id_of(&dev_command_seat_id())
            .unwrap(),
    ] {
        for orientation in 0..24 {
            assert_eq!(table.joint_face(other, 0, orientation), None);
        }
    }
}

#[test]
fn the_ball_end_has_a_colour_of_its_own_wherever_the_joint_is_turned_and_nothing_else_has() {
    let content = CaveContent::new();
    let table = content.block_table();
    let block = joint_block();
    let palette = table.palette();
    for face in FaceDirection::ALL {
        let orientation = ball_end_toward(face).orientation;
        assert_eq!(table.joint_face(block, 0, orientation), Some(face));
        let end = table.material_of_face(block, 0, orientation, face);
        assert_eq!(palette[end as usize], BALL_END, "{face:?}");
        for other in FaceDirection::ALL
            .into_iter()
            .filter(|other| *other != face)
        {
            let material = table.material_of_face(block, 0, orientation, other);
            assert_eq!(palette[material as usize], table.color(block), "{face:?}");
        }
    }
}

#[test]
fn turned_to_any_face_the_joint_lies_flush_with_the_opposite_face_and_ends_a_quarter_metre_short_of_the_face_it_looks_at_so_two_facing_ones_are_half_a_metre_apart()
 {
    let content = CaveContent::new();
    let table = content.block_table();
    let block = joint_block();
    for face in FaceDirection::ALL {
        let model = table.model_of(block, 0, ball_end_toward(face).orientation);
        let shape = table.shape(model);
        let axis = face.axis();
        // The box that it should be: six eighths along the axis it looks along, flush with the cell's face on the other side, four across, centred.
        let mut expected = AS_MADE;
        let (from, to) = axis_range(face);
        expected.min = [2; 3];
        expected.max = [6; 3];
        expected.min[axis] = from;
        expected.max[axis] = to;
        assert!(
            shape.same_cells(&Shape::from_boxes(&[expected])),
            "{face:?}: {:?}",
            shape.boxes()
        );
        // The ball end is `8 - tip` eighths from the cell's boundary it looks at, on both signs: so facing across one boundary, the two tips are
        // 2 + 2 = 4 eighths, half a metre, apart.
        let tip = if face.is_positive() {
            shape.boxes().iter().map(|part| part.max[axis]).max()
        } else {
            shape.boxes().iter().map(|part| part.min[axis]).min()
        }
        .unwrap();
        let short_of_the_boundary = if face.is_positive() { 8 - tip } else { tip };
        assert_eq!(short_of_the_boundary, 2, "{face:?}");
    }
}

/// Where the joint lies along the axis of the face its ball end looks at, in eighths: flush with the other side and six long.
fn axis_range(face: FaceDirection) -> (u8, u8) {
    if face.is_positive() { (0, 6) } else { (2, 8) }
}

#[test]
fn a_ship_has_the_ends_of_its_joints_and_the_ports_of_its_ports_and_a_joint_is_never_a_port() {
    let mut scene = Scene::new();
    scene.run(|world| {
        world.set_gravity_settings(voxel_space::GravitySettings {
            base: [0.0; 3],
            ..voxel_space::GravitySettings::default()
        });
    });
    // A row of stone with a joint at each end looking outward and a docking port in the middle of a second ship.
    let mut with_joints = stones(1, 4);
    with_joints.push((
        [0, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::NegativeX),
    ));
    with_joints.push((
        [4, 0, 0],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveX),
    ));
    with_joints.push((
        [2, 1, 0],
        joint_block(),
        ball_end_toward(FaceDirection::PositiveY),
    ));
    make(&mut scene, pose(20.0, 230.0, 20.0), true, with_joints);
    let mut with_a_port = stones(0, 2);
    with_a_port.push(([2, 0, 0], port_block(), facing(FaceDirection::PositiveX)));
    with_a_port.push(([1, 1, 0], stone_block(), PLAIN));
    make(&mut scene, pose(20.0, 270.0, 20.0), true, with_a_port);

    let docking = docking(&CaveContent::new());
    let grids = scene.rig.server.world().grids();
    let jointed = grids.get(ship_id(0)).unwrap();
    let ported = grids.get(ship_id(1)).unwrap();

    let ends: Vec<(VoxelCoordinate, FaceDirection)> = docking
        .joint_ends_of(jointed)
        .into_iter()
        .map(|end| (end.voxel, end.face))
        .collect();
    assert_eq!(
        ends,
        [
            (VoxelCoordinate::new(0, 0, 0), FaceDirection::NegativeX),
            (VoxelCoordinate::new(4, 0, 0), FaceDirection::PositiveX),
            (VoxelCoordinate::new(2, 1, 0), FaceDirection::PositiveY),
        ],
        "in the order the voxels of a brick are scanned (x fastest, then y, then z), as ports are"
    );
    assert!(docking.ports_of(jointed).is_empty());
    assert!(docking.docked_pairs(jointed).is_empty());

    assert!(docking.joint_ends_of(ported).is_empty());
    let ports = docking.ports_of(ported);
    assert_eq!(ports.len(), 1);
    assert_eq!(
        (ports[0].voxel, ports[0].face),
        (VoxelCoordinate::new(2, 0, 0), FaceDirection::PositiveX)
    );
}

fn stone_block() -> u32 {
    CaveContent::new().blocks.stone
}
