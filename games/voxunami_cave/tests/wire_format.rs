//! The bytes of Cave's messages, pinned: converting them to declared messages must not move one.

use namespaced_id::NamespacedId;
use physics::MoveInput;
use player_presence::PlayerPose;
use protocol::Message;
use voxel_space::{BrickCoordinate, FaceDirection, GridId, VoxelCoordinate, WorldPosition};
use voxunami_cave::messages::{FlightReadoutIs, LightSchemeChanged, MoveRequest, SetBlock, Spawn};
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
        roll: 0,
    };
    let rolling = MoveRequest { roll: 1, ..request };
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
    let pins: [(&str, Vec<u8>, &str); 8] = [
        (
            "a spawn",
            Spawn(pose).encode(),
            // (Protocol 9: a coordinate is a length and the bytes of its zigzag value: the brick (3, -2, 7) is 01 06, 01 03, 01 0e. Protocol 11: the offset and the angles are `f64`, as in
            // the presence messages, which the same numbers pin in `player_presence`'s `wire_format.rs`.)
            "01060103010e000000000000f83f3d0ad7a370fd3f400000000000000000000000000000d03f000000000000f0bf",
        ),
        (
            "a movement request",
            request.encode(),
            // (Protocol 11: the two angles are `f64`: -2.5 was the `f32` 000020c0 and is now 00000000000004c0, and 0.75 was 0000403f and is now 000000000000e83f.)
            "01ff000100000000000004c0000000000000e83f",
        ),
        (
            "a movement request that rolls to the right",
            rolling.encode(),
            // (The same, with the bit for E in the flags: 01 jump, 08 roll right; Q is 04.)
            "01ff000900000000000004c0000000000000e83f",
        ),
        (
            "a readout of two lines",
            FlightReadoutIs(vec!["ab".to_owned(), String::new()]).encode(),
            // (Two lines: the count, then each as its length and its bytes: "ab" is 02 61 62, and nothing is 00.)
            "0202616200",
        ),
        (
            "a block to set",
            set_block.encode(),
            // (The voxel (-40, 200, 5) is 01 4f, 02 90 01, 01 0a.)
            "00014f029001010a10626173655f626c6f636b733a6c616d700306",
        ),
        (
            "a block to set in a ship",
            set_block_in_a_ship.encode(),
            // (The voxel (1, 2, 3) is 01 02, 01 04, 01 06.)
            "878080801001020104010603613a620000",
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
