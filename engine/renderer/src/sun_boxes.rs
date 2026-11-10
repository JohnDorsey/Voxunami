//! The boxes of the entities as the sun's ray meets them (`docs/specs/contact-trace-spec.md`, section 16): an avatar or an item on the ground is no voxel of any grid, so the walk
//! through the lattice cannot find it, and the shadow map leaves the same lit strip at its foot as at a wall's. The few nearest entities are given to the shader as boxes, and a
//! face tests the sun's ray against them analytically (`entities_on.wgsl`; the reference is `world_view::sun_blocked_by_boxes`).

use bytemuck::{Pod, Zeroable};
use voxel_space::WorldPosition;

use crate::voxel_renderer::{Avatar, EntityBox};

/// How many boxes the shader is given: the nearest to the camera. A face tests the ray against all of them, so the number is a cost; the rest are left to the shadow map.
pub(crate) const MAXIMUM_SUN_BOXES: usize = 16;

/// One box as the shader has it (`SunBox` in `contact_on.wgsl`): the lowest and the highest corner, in voxels from the camera, along the axes of the world.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct SunBoxUniform {
    pub low: [f32; 4],
    pub high: [f32; 4],
}

/// The boxes of the nearest [`MAXIMUM_SUN_BOXES`] avatars and entity boxes, nearest first, relative to the camera.
///
/// An avatar is two voxels (the head, with the eyes in the middle of it, over the body), so its box is one voxel across and two high, from a voxel and a half under the eyes to a
/// half over them; an entity's box is as big as it is drawn (`EntityBox::key`: its sizes, in eighths of a voxel, made even and put from two to eight), about its position.
pub(crate) fn nearest_sun_boxes(
    avatars: &[Avatar],
    entity_boxes: &[EntityBox],
    camera: WorldPosition,
) -> Vec<SunBoxUniform> {
    let relative = |position: WorldPosition| position.relative_to(camera);
    let mut found: Vec<([f64; 3], [f64; 3])> =
        Vec::with_capacity(avatars.len() + entity_boxes.len());
    for avatar in avatars {
        let eyes = relative(avatar.position);
        found.push((
            [eyes[0] - 0.5, eyes[1] - 1.5, eyes[2] - 0.5],
            [eyes[0] + 0.5, eyes[1] + 0.5, eyes[2] + 0.5],
        ));
    }
    for entity_box in entity_boxes {
        let middle = relative(entity_box.position);
        let size = entity_box.key().1;
        found.push((
            std::array::from_fn(|axis| middle[axis] - f64::from(size[axis]) / 16.0),
            std::array::from_fn(|axis| middle[axis] + f64::from(size[axis]) / 16.0),
        ));
    }
    let distance = |(low, high): &([f64; 3], [f64; 3])| {
        (0..3)
            .map(|axis| {
                let middle = (low[axis] + high[axis]) / 2.0;
                middle * middle
            })
            .sum::<f64>()
    };
    found.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    found
        .into_iter()
        .take(MAXIMUM_SUN_BOXES)
        .map(|(low, high)| SunBoxUniform {
            low: [low[0] as f32, low[1] as f32, low[2] as f32, 0.0],
            high: [high[0] as f32, high[1] as f32, high[2] as f32, 0.0],
        })
        .collect()
}

#[cfg(test)]
mod tests;
