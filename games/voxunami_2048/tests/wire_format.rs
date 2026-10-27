//! The bytes of the messages of 2048, pinned: converting them to declared messages must not move one.

use protocol::Message;
use voxel_space::{FaceDirection, VoxelCoordinate};
use voxunami_2048::messages::{Progress, PushRequest};
use voxunami_2048::tool::Tool;

#[test]
fn the_messages_of_2048_have_their_pinned_bytes() {
    let push = PushRequest {
        voxel: VoxelCoordinate::new(-3, 40, 1000),
        direction: FaceDirection::NegativeX,
        tool: Tool::Subtract,
    };
    let progress = Progress {
        score: 123_456,
        pushes: 99,
        largest_merge: 4096,
    };
    let pins: [(&str, Vec<u8>, &str); 3] = [
        // The voxel is three length-prefixed integers (protocol 9): -3 is 01 05, 40 is 01 50, 1000 is 02 d0 07 (each the length and the zigzag value); then the direction and the tool.
        ("a push", push.encode(), "0105015002d0070002"),
        ("some progress", progress.encode(), "c0c407638020"),
        ("no progress", Progress::default().encode(), "000000"),
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
