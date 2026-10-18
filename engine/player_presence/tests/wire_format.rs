//! The bytes of the presence messages, pinned: converting them to declared messages must not move one.

use namespaced_id::NamespacedId;
use player_presence::{PlayerPose, PlayerState, encode_level_change, encode_players, encode_pose};
use protocol::ClientId;
use voxel_space::{BrickCoordinate, LevelIndex, WorldPosition};

fn pose(x: i64, offset: [f32; 3], yaw: f32, pitch: f32) -> PlayerPose {
    PlayerPose {
        position: WorldPosition::new(BrickCoordinate::new(x, -2, 7), offset),
        yaw,
        pitch,
    }
}

#[test]
fn the_presence_messages_have_their_pinned_bytes() {
    let first = pose(3, [1.5, 31.99, 0.0], 0.25, -1.0);
    let second = pose(-9, [0.0, 0.5, 16.0], -3.0, 1.5);
    let level: NamespacedId = "test_content:second".parse().unwrap();
    let players = [
        (
            ClientId::new(1),
            PlayerState {
                pose: first,
                level: LevelIndex::MAIN,
            },
        ),
        (
            ClientId::new(7),
            PlayerState {
                pose: second,
                level: LevelIndex::new(2),
            },
        ),
    ];
    let pins: [(&str, Vec<u8>, &str); 4] = [
        (
            "a pose",
            encode_pose(&first),
            "06030e0000c03f85ebff41000000000000803e000080bf",
        ),
        (
            "a level change",
            encode_level_change(&level),
            "13746573745f636f6e74656e743a7365636f6e64",
        ),
        (
            "two players",
            encode_players(&players),
            "02010000000006030e0000c03f85ebff41000000000000803e000080bf070000000211030e000000000000003f00008041000040c00000c03f",
        ),
        ("no players", encode_players(&[]), "00"),
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
