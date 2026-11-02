//! The bytes of the presence messages, pinned: converting them to declared messages must not move one.

use namespaced_id::NamespacedId;
use player_presence::{
    LevelChange, PlayerClient, PlayerKeyComponent, PlayerLook, PlayerPlaced, PlayerPose, PoseReport,
};
use protocol::{ClientId, Message, PlayerKey};
use voxel_space::{BrickCoordinate, WorldPosition};

fn pose(x: i128, offset: [f64; 3], yaw: f64, pitch: f64) -> PlayerPose {
    PlayerPose {
        position: WorldPosition::new(BrickCoordinate::new(x, -2, 7), offset),
        yaw,
        pitch,
    }
}

#[test]
fn the_presence_messages_have_their_pinned_bytes() {
    let first = pose(3, [1.5, 31.99, 0.0], 0.25, -1.0);
    let level: NamespacedId = "test_content:second".parse().unwrap();
    let pins: [(&str, Vec<u8>, &str); 6] = [
        (
            "a pose",
            PoseReport(first).encode(),
            // (Protocol 9: the brick (3, -2, 7) is three length-prefixed integers, 01 06, 01 03, 01 0e. Protocol 11: the offset and the two angles are `f64`, 8 bytes each:
            // 1.5 is 000000000000f83f, 31.99 is 3d0ad7a370fd3f40, 0.25 is 000000000000d03f and -1 is 000000000000f0bf; they were the `f32` 0000c03f, 85ebff41, 0000803e and 000080bf.)
            "01060103010e000000000000f83f3d0ad7a370fd3f400000000000000000000000000000d03f000000000000f0bf",
        ),
        (
            "a level change",
            LevelChange(level).encode(),
            "13746573745f636f6e74656e743a7365636f6e64",
        ),
        (
            "where a returning player is",
            PlayerPlaced(first.position).encode(),
            "01060103010e000000000000f83f3d0ad7a370fd3f400000000000000000",
        ),
        (
            "a player's connection",
            PlayerClient(ClientId::new(7)).encode(),
            "07000000",
        ),
        (
            "a player's look",
            PlayerLook {
                yaw: 0.25,
                pitch: -1.0,
            }
            .encode(),
            "000000000000d03f000000000000f0bf",
        ),
        (
            "a player's key",
            PlayerKeyComponent(PlayerKey::new([0xA5; 32])).encode(),
            &"a5".repeat(32),
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
