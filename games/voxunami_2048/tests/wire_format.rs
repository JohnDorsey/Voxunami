//! The bytes of the messages of 2048, pinned: converting them to declared messages must not move one.

use voxel_space::{FaceDirection, VoxelCoordinate};
use voxunami_2048::messages::{Progress, PushRequest, encode_progress, encode_push};
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
        ("a push", encode_push(&push), "0550d00f0002"),
        ("some progress", encode_progress(&progress), "c0c407638020"),
        (
            "no progress",
            encode_progress(&Progress::default()),
            "000000",
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
