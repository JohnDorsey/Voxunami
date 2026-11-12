// The part of the walk of the sun's ray (`contact_on.wgsl`) that tests it against the boxes of entities, used while some avatar or box is set: an entity is no voxel of any grid, so
// the walk through the lattice cannot find it, and the shadow map leaves the same lit strip at its foot as at a wall's (`docs/specs/contact-trace-spec.md`, section 16).
//
// The nearest few entities are in the frame's uniform as boxes along the axes of the world, in voxels from the camera. The test is the slab method, in the same frame as the face's
// place: a ray that starts inside a box, or touches one, is blocked.

// How many boxes there are (the length of the array of `Windows`).
const SUN_BOX_COUNT: u32 = 16u;

// Whether the sun's ray from the place on this face meets the box of an entity within the length the first `cells` cells of the walk can cover (1.75 voxels each: a step along the
// ray crosses a cell and goes at most the diagonal of one, less slack). The ray starts on the face itself and not a little out of it, as it does for a face of the grid's own: an
// entity stands on the floor, and a ray that began a sixteenth of a voxel up would pass over the tip of the shadow of a flat one.
fn entity_boxes_block(in: VertexOutput, sun: vec3<f32>, cells: u32) -> bool {
    let count = min(contact_windows.entities.x, SUN_BOX_COUNT);
    let origin = in.relative;
    let longest = 1.75 * f32(cells);
    let huge = 1.0e30;
    // An axis the ray does not move along has no planes ahead of it: it stays between the two or never is.
    let moves = abs(sun) > vec3<f32>(1.0e-20);
    let inverse = select(vec3<f32>(0.0), 1.0 / sun, moves);
    for (var index = 0u; index < count; index = index + 1u) {
        let low = contact_windows.boxes[index].low.xyz - origin;
        let high = contact_windows.boxes[index].high.xyz - origin;
        if (any((!moves) & ((low > vec3<f32>(0.0)) | (high < vec3<f32>(0.0))))) {
            continue;
        }
        let first = low * inverse;
        let second = high * inverse;
        let entered = select(vec3<f32>(-huge), min(first, second), moves);
        let left = select(vec3<f32>(huge), max(first, second), moves);
        let near = max(entered.x, max(entered.y, entered.z));
        let far = min(left.x, min(left.y, left.z));
        if (near <= far && far >= 0.0 && near <= longest) {
            return true;
        }
    }
    return false;
}
