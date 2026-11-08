//! The `face_colors` of a block: the flat colour of faces that differ from the block's own, written by group
//! (`top`, `side`, `bottom`) or by one exact face (`positive_x`), the exact face beating its group.

use serde_json::Value;
use voxel_space::FaceDirection;

use crate::shape::FACE_NAMES;
use crate::walk::{Path, Walk};

/// The groups of faces, as the block is made: the top is +y, the bottom is -y, and the side is the other four.
pub const FACE_GROUPS: [&str; 3] = ["top", "side", "bottom"];

/// The colour of each face as the block is made, indexed by [`FaceDirection::index`].
pub(crate) type FaceColors = [Option<[f32; 3]>; 6];

fn faces_of_group(group: &str) -> Vec<FaceDirection> {
    match group {
        "top" => vec![FaceDirection::PositiveY],
        "bottom" => vec![FaceDirection::NegativeY],
        _ => FaceDirection::ALL
            .into_iter()
            .filter(|face| !matches!(face, FaceDirection::PositiveY | FaceDirection::NegativeY))
            .collect(),
    }
}

/// Every key `face_colors` takes: the groups, then the exact faces.
pub(crate) fn face_color_keys() -> Vec<&'static str> {
    FACE_GROUPS
        .into_iter()
        .chain(FACE_NAMES.iter().map(|(name, _)| *name))
        .collect()
}

/// Reads the object of face colours. Groups are applied first, whatever the order they are written in, and exact faces over them.
pub(crate) fn read_face_colors(walk: &mut Walk, value: &Value, path: &Path) -> Option<FaceColors> {
    let object = walk.object(value, path, &face_color_keys())?;
    if object.is_empty() {
        walk.fail(path, "give at least one face, or leave \"face_colors\" out");
        return None;
    }
    let mut colors: FaceColors = [None; 6];
    let mut good = true;
    for group in FACE_GROUPS {
        let Some(entry) = object.get(group) else {
            continue;
        };
        match walk.color(entry, &path.key(group)) {
            Some(color) => {
                for face in faces_of_group(group) {
                    colors[usize::from(face.index())] = Some(color);
                }
            }
            None => good = false,
        }
    }
    for (name, face) in FACE_NAMES {
        let Some(entry) = object.get(name) else {
            continue;
        };
        match walk.color(entry, &path.key(name)) {
            Some(color) => colors[usize::from(face.index())] = Some(color),
            None => good = false,
        }
    }
    good.then_some(colors)
}
