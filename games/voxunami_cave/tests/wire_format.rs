//! The bytes of Cave's messages, pinned: converting them to declared messages must not move one.

use namespaced_id::NamespacedId;
use physics::MoveInput;
use player_presence::PlayerPose;
use protocol::Message;
use voxel_space::{BrickCoordinate, FaceDirection, GridId, VoxelCoordinate, WorldPosition};
use voxunami_cave::messages::{LightSchemeChanged, MoveRequest, SetBlock, Spawn};
use voxunami_cave::multiscale_light::LightScheme;

#[test]
fn cave_messages_have_their_pinned_bytes() {
    let pose = PlayerPose {
        position: WorldPosition::new(BrickCoordinate::new(3, -2, 7), [1.5, 31.99, 0.0]),
        yaw: 0.25,
        pitch: -1.0,
    };
    let request = MoveRequest {
        input: MoveInput {
            forward: 1,
            strafe: -1,
            vertical: 0,
            jump: true,
            sprint: false,
            yaw: -2.5,
        },
        pitch: 0.75,
    };
    let block: NamespacedId = "base_blocks:lamp".parse().unwrap();
    let set_block = SetBlock {
        grid: GridId::MAIN,
        at: VoxelCoordinate::new(-40, 200, 5),
        block,
        state: 3,
        against: Some(FaceDirection::PositiveZ),
    };
    let set_block_in_a_ship = SetBlock {
        grid: GridId::new(GridId::FIRST_ALLOCATED + 7),
        at: VoxelCoordinate::new(1, 2, 3),
        block: "a:b".parse().unwrap(),
        state: 0,
        against: None,
    };
    let pins: [(&str, Vec<u8>, &str); 6] = [
        (
            "a spawn",
            Spawn(pose).encode(),
            "06030e0000c03f85ebff41000000000000803e000080bf",
        ),
        (
            "a movement request",
            request.encode(),
            "01ff0001000020c00000403f",
        ),
        (
            "a block to set",
            set_block.encode(),
            "004f90030a10626173655f626c6f636b733a6c616d700306",
        ),
        (
            "a block to set in a ship",
            set_block_in_a_ship.encode(),
            "878080801002040603613a620000",
        ),
        ("no light scheme", LightSchemeChanged(None).encode(), "00"),
        (
            "the 16 level scheme",
            LightSchemeChanged(Some(LightScheme::LEVELS_16)).encode(),
            "040101020408",
        ),
    ];
    let wrong: Vec<String> = pins
        .iter()
        .filter_map(|(name, bytes, pinned)| {
            let actual: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            (actual != *pinned).then(|| format!("{name}: {actual}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}
