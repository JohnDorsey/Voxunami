//! Shapes as a pack writes them: a name, or boxes in eighths of a voxel.

use block_models::{SUBDIVISIONS, Shape, ShapeBox};
use serde_json::Value;
use voxel_space::FaceDirection;

use crate::walk::{Path, Walk, kind_of, nearest, quoted};

/// The sides a pack can name, as written after `half:` and `stair:`.
pub const FACE_NAMES: [(&str, FaceDirection); 6] = [
    ("negative_x", FaceDirection::NegativeX),
    ("positive_x", FaceDirection::PositiveX),
    ("negative_y", FaceDirection::NegativeY),
    ("positive_y", FaceDirection::PositiveY),
    ("negative_z", FaceDirection::NegativeZ),
    ("positive_z", FaceDirection::PositiveZ),
];

/// The shapes that have names: the text before a colon, or the whole text if there is none.
pub const SHAPE_NAMES: [&str; 4] = ["full", "empty", "half", "stair"];

/// Reads a shape: `"full"`, `"empty"`, `"half:negative_y"`, `"stair:positive_x"`, or
/// `{"boxes": [[min_x, min_y, min_z, max_x, max_y, max_z], ...]}`.
pub(crate) fn read_shape(walk: &mut Walk, value: &Value, path: &Path) -> Option<Shape> {
    match value {
        Value::String(text) => read_named(walk, text, path),
        Value::Object(_) => read_boxes(walk, value, path),
        other => {
            walk.fail(
                path,
                format!(
                    "expected a shape (a name or boxes), found {}",
                    kind_of(other)
                ),
            );
            None
        }
    }
}

fn read_named(walk: &mut Walk, text: &str, path: &Path) -> Option<Shape> {
    let (name, face) = match text.split_once(':') {
        Some((name, face)) => (name, Some(face)),
        None => (text, None),
    };
    let face_direction = |walk: &mut Walk, face: Option<&str>| -> Option<FaceDirection> {
        let Some(face) = face else {
            walk.fail(
                path,
                format!("\"{name}\" needs a side, as in \"{name}:negative_y\""),
            );
            return None;
        };
        let names: Vec<&str> = FACE_NAMES.iter().map(|(name, _)| *name).collect();
        let found = FACE_NAMES
            .iter()
            .find(|(candidate, _)| *candidate == face)
            .map(|(_, direction)| *direction);
        if found.is_none() {
            let hint = nearest(face, &names)
                .map(|near| format!(" (did you mean \"{near}\"?)"))
                .unwrap_or_default();
            walk.fail(
                path,
                format!(
                    "\"{face}\" is not a side{hint}; the sides are {}",
                    quoted(&names)
                ),
            );
        }
        found
    };
    match (name, face) {
        ("full", None) => Some(Shape::full()),
        ("empty", None) => Some(Shape::empty()),
        ("full" | "empty", Some(_)) => {
            walk.fail(path, format!("\"{name}\" takes no side"));
            None
        }
        ("half", face) => face_direction(walk, face).map(Shape::half),
        ("stair", face) => face_direction(walk, face).map(Shape::stair),
        _ => {
            let hint = nearest(name, &SHAPE_NAMES)
                .map(|near| format!(" (did you mean \"{near}\"?)"))
                .unwrap_or_default();
            walk.fail(
                path,
                format!(
                    "\"{name}\" is not a shape{hint}; the shapes are {}, or {{\"boxes\": [...]}}",
                    quoted(&SHAPE_NAMES)
                ),
            );
            None
        }
    }
}

fn read_boxes(walk: &mut Walk, value: &Value, path: &Path) -> Option<Shape> {
    let object = walk.object(value, path, &["boxes"])?;
    let list = walk.require(object, "boxes", path)?;
    let boxes_path = path.key("boxes");
    let list = walk.array(list, &boxes_path)?;
    if list.is_empty() {
        walk.fail(
            &boxes_path,
            "a shape of boxes has at least one box (use \"empty\" for nothing)",
        );
        return None;
    }
    let mut boxes = Vec::with_capacity(list.len());
    let mut good = true;
    for (index, entry) in list.iter().enumerate() {
        match read_box(walk, entry, &boxes_path.index(index)) {
            Some(shape_box) => boxes.push(shape_box),
            None => good = false,
        }
    }
    good.then(|| Shape::from_boxes(&boxes))
}

/// Six whole numbers from 0 to 8: where the box starts and ends on each axis, below on every axis.
fn read_box(walk: &mut Walk, value: &Value, path: &Path) -> Option<ShapeBox> {
    let list = walk.array(value, path)?;
    if list.len() != 6 {
        walk.fail(
            path,
            format!(
                "a box is 6 numbers (min x, y, z then max x, y, z), found {}",
                list.len()
            ),
        );
        return None;
    }
    let mut corners = [0u8; 6];
    let mut good = true;
    for (index, (slot, entry)) in corners.iter_mut().zip(list).enumerate() {
        match walk.integer(entry, &path.index(index), 0, SUBDIVISIONS as i64) {
            Some(number) => *slot = number as u8,
            None => good = false,
        }
    }
    if !good {
        return None;
    }
    let shape_box = ShapeBox {
        min: [corners[0], corners[1], corners[2]],
        max: [corners[3], corners[4], corners[5]],
    };
    for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
        if shape_box.min[axis] >= shape_box.max[axis] {
            walk.fail(
                path,
                format!(
                    "the box is empty on {name}: it starts at {} and ends at {}",
                    shape_box.min[axis], shape_box.max[axis]
                ),
            );
            good = false;
        }
    }
    good.then_some(shape_box)
}
