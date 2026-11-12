// The part of the walk of the sun's ray (`contact_on.wgsl`) that goes through the eighths of a voxel that blocks the sun in part, used while some brick holds such voxels
// (`docs/specs/contact-trace-spec.md`, section 13).

// The words of a partial slot: two voxels to a word, in the order of the voxels' indexes.
const PARTIAL_SLOT_WORDS: u32 = 16384u;
// The most eighths the walk goes through in one voxel: seven along each axis at the very most.
const MOST_SUB_CELLS: u32 = 24u;
@group(2) @binding(3) var<storage, read> partial_pool: array<u32>;
// The cells of the shapes: 16 words each, two for each layer of z (the lower four rows of y, then the upper four), bit `(y mod 4) * 8 + x`.
@group(2) @binding(4) var<storage, read> shapes: array<u32>;

// The shape of the voxel at `cell` if it blocks the sun in part, given the entry of the brick it is in: the shape's number plus one, or 0 for a voxel that does not.
fn partial_number(entry: u32, cell: vec3<i32>) -> u32 {
    let slot = entry >> PARTIAL_ENTRY_SHIFT;
    if (slot == 0u) {
        return 0u;
    }
    let inside = vec3<u32>(cell & vec3<i32>(31));
    let index = (inside.z * 32u + inside.y) * 32u + inside.x;
    let word = partial_pool[(slot - 1u) * PARTIAL_SLOT_WORDS + index / 2u];
    return (word >> (16u * (index & 1u))) & 0xFFFFu;
}

// Whether the eighth `sub` (each part 0 to 7) of the voxel is filled in the shape.
fn shape_fills(shape: u32, sub: vec3<i32>) -> bool {
    let word = shapes[shape * 16u + u32(sub.z) * 2u + u32(sub.y >> 2u)];
    return ((word >> u32((sub.y & 3) * 8 + sub.x)) & 1u) == 1u;
}

// Whether the ray from `position` (in voxels from the voxel's lowest corner), which starts in the eighth `start` of a voxel of this `shape`, meets a filled eighth before it leaves
// the voxel. The eighth it starts in is tested only if `test_first`. On a tie the lowest axis goes first, as in the walk of voxels.
fn partial_blocks(shape: u32, position: vec3<f32>, toward_sun: vec3<f32>, start: vec3<i32>, test_first: bool) -> bool {
    var sub = start;
    if (test_first && shape_fills(shape, sub)) {
        return true;
    }
    let positive = toward_sun > vec3<f32>(0.0);
    let direction = select(vec3<i32>(-1), vec3<i32>(1), positive);
    let huge = 1.0e30;
    let moves = abs(toward_sun) > vec3<f32>(1.0e-20);
    let inverse = select(vec3<f32>(huge), 1.0 / toward_sun, moves);
    var boundary = select(
        vec3<f32>(huge),
        ((vec3<f32>(sub) + select(vec3<f32>(0.0), vec3<f32>(1.0), positive)) * 0.125 - position) * inverse,
        moves,
    );
    let next = abs(inverse) * 0.125;
    for (var step = 0u; step < MOST_SUB_CELLS; step = step + 1u) {
        advance(&sub, &boundary, direction, next);
        if (any(sub < vec3<i32>(0)) || any(sub > vec3<i32>(7))) {
            return false;
        }
        if (shape_fills(shape, sub)) {
            return true;
        }
    }
    return false;
}
