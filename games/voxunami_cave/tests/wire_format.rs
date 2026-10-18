//! The bytes of Cave's messages, pinned: converting them to declared messages must not move one.

use namespaced_id::NamespacedId;
use physics::MoveInput;
use player_presence::PlayerPose;
use voxel_space::{BrickCoordinate, VoxelCoordinate, WorldPosition};
use voxunami_cave::messages::{
    MoveRequest, SetBlock, encode_light_scheme, encode_move_request, encode_set_block, encode_spawn,
};
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
        at: VoxelCoordinate::new(-40, 200, 5),
        block,
        state: 3,
    };
    let pins: [(&str, Vec<u8>, &str); 5] = [
        (
            "a spawn",
            encode_spawn(&pose),
            "06030e0000c03f85ebff41000000000000803e000080bf",
        ),
        (
            "a movement request",
            encode_move_request(&request),
            "01ff0001000020c00000403f",
        ),
        (
            "a block to set",
            encode_set_block(&set_block),
            "4f90030a10626173655f626c6f636b733a6c616d7003",
        ),
        ("no light scheme", encode_light_scheme(None), "00"),
        (
            "the 16 level scheme",
            encode_light_scheme(Some(LightScheme::LEVELS_16)),
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
