// The voxels that block the sun, as the fragment shader reads them: the windows (one for each grid with bricks in view), their tables of bricks and the pool of bits. Two things read
// them: the walk of the sun's ray (`contact_on.wgsl`, `docs/specs/contact-trace-spec.md`) and the shade of the corners of a face (`volume_shade_level`, below). The shader with the voxels
// and no walk (the faces are shaded and there are no shadows) is this and `contact_none.wgsl`.
//
// The voxels are in windows, one for each grid with bricks in view (the terrain of the level, a planet, a ship): a table of 64 by 64 by 64 entries, one for each brick, indexed by
// the brick's coordinates modulo 64, and a pool of slots of one bit for each voxel of a brick (see `occupancy_volume.rs`, the same numbers). A face is looked at in the window of its own
// grid, in the grid's own axes, so a planet that turns is read in its own lattice.

const WINDOW_BRICKS: u32 = 64u;
// What an entry of a table says.
const ENTRY_UNKNOWN: u32 = 0u;
const ENTRY_EMPTY: u32 = 1u;
const ENTRY_FULL: u32 = 2u;
const FIRST_SLOT_ENTRY: u32 = 3u;
// An entry's lower half says what it says of the bits, its upper half is the number of the brick's partial slot plus one (0: it has none).
const BITS_ENTRY_MASK: u32 = 0xFFFFu;
const PARTIAL_ENTRY_SHIFT: u32 = 16u;

// The words of a slot: 32 voxels each, x along the bits, then y, then z along the words.
const SLOT_WORDS: u32 = 1024u;

// What the walk finds in a cell.
const CELL_CLEAR: u32 = 0u;
const CELL_BLOCKS: u32 = 1u;
const CELL_UNKNOWN: u32 = 2u;

struct Window {
    // Where the window's origin (the lowest corner of its first brick) is relative to the camera, in voxels (xyz); w: 1 if the window is traced, 0 if it is not (a window not in
    // use, or one that holds nothing that blocks the sun, or whose grid has no pose).
    origin: vec4<f32>,
    // From a vector in the world to the same in the grid's own axes: the turn of the grid, undone.
    to_local: mat3x3<f32>,
    // x, y, z: the brick coordinates of the first brick modulo the width of a window; w: where the window's table begins in `tables`.
    brick: vec4<u32>,
    // The lowest and the highest corner of the box that holds the window's bricks that block the sun, in voxels from the window's origin and in the grid's own axes (xyz).
    bounds_low: vec4<f32>,
    bounds_high: vec4<f32>,
}

// The box of an entity, in voxels from the camera, along the axes of the world (`entities_on.wgsl`).
struct SunBox {
    low: vec4<f32>,
    high: vec4<f32>,
}

struct Windows {
    windows: array<Window, 4>,
    // x: how many cells to search whatever the rule says (0: by the rule), y: 1 to draw the walk's answer and nothing else.
    debug: vec4<u32>,
    // x: how many of the boxes are set.
    entities: vec4<u32>,
    // The boxes of the nearest entities, nearest first.
    boxes: array<SunBox, 16>,
}

@group(2) @binding(0) var<uniform> contact_windows: Windows;
// The tables of the windows, one after the other.
@group(2) @binding(1) var<storage, read> tables: array<u32>;
@group(2) @binding(2) var<storage, read> pool: array<u32>;

// What the table says of the brick at `brick` (bricks from the window's origin, in the grid's own axes): one of the `ENTRY_` numbers, or a slot of the pool.
fn brick_entry(window: Window, brick: vec3<i32>) -> u32 {
    // (Beyond half a window from the origin a place of the table may be another brick's.)
    if (any(brick < vec3<i32>(-32)) || any(brick >= vec3<i32>(32))) {
        return ENTRY_UNKNOWN;
    }
    let place = (window.brick.xyz + vec3<u32>(brick)) & vec3<u32>(WINDOW_BRICKS - 1u);
    return tables[window.brick.w + (place.z * WINDOW_BRICKS + place.y) * WINDOW_BRICKS + place.x];
}

// What the voxel at `cell` (voxels from the window's origin) says about the sun, given the entry of the brick it is in: it blocks it, it does not, or nothing is known of its brick.
fn cell_state(entry: u32, cell: vec3<i32>) -> u32 {
    let bits = entry & BITS_ENTRY_MASK;
    if (bits == ENTRY_EMPTY) {
        return CELL_CLEAR;
    }
    if (bits == ENTRY_FULL) {
        return CELL_BLOCKS;
    }
    if (bits == ENTRY_UNKNOWN) {
        return CELL_UNKNOWN;
    }
    let inside = vec3<u32>(cell & vec3<i32>(31));
    let word = pool[(bits - FIRST_SLOT_ENTRY) * SLOT_WORDS + inside.z * 32u + inside.y];
    return (word >> inside.x) & 1u;
}

// Whether the voxel at `cell` (voxels from the window's origin) blocks the sun whole (a cube that hides what is behind it: the cubes that shade the faces beside them), 0 or 1.
// `near_entry` is the table's entry of the brick `near_brick` (bricks from the window's origin), which most of the cells asked about in a row are in: only a cell of another brick
// looks the table up.
fn blocks_cell(window: Window, cell: vec3<i32>, near_brick: vec3<i32>, near_entry: u32) -> f32 {
    let brick = cell >> vec3<u32>(5u);
    var entry = near_entry;
    if (any(brick != near_brick)) {
        entry = brick_entry(window, brick);
    }
    return f32(cell_state(entry, cell) == CELL_BLOCKS);
}

// The level of shade (0 to 3) of the voxel face the pixel is on (ambient occlusion), worked out from the voxels that block the sun in the window of the face's grid. At each corner
// of the face's voxel the level is the number of cubes among the two cells beside the cell in front of the face and the one diagonal to it, or 3 when both beside it are cubes
// (nothing gets round a corner that is shut on both sides); the four are blended bilinearly across the voxel, so two faces beside each other agree about the corner they share and
// the shade has no seam where one meets the next, however the faces are cut into quads. A cell whose brick is not known counts as open. 0 where the face has no window.
// The reference is `corner_levels` in the tests (`tests/gpu_rendering/volume_shade.rs`), which the shader is held to by pictures.
fn volume_shade_level(in: VertexOutput) -> f32 {
    if (in.volume == 0u) {
        return 0.0;
    }
    let window = contact_windows.windows[in.volume - 1u];
    let normal = window.to_local * in.normal;
    let place = window.to_local * (in.relative - window.origin.xyz);
    let magnitude = abs(normal);
    var axis = 0u;
    if (magnitude.y > magnitude.x && magnitude.y >= magnitude.z) {
        axis = 1u;
    } else if (magnitude.z > magnitude.x && magnitude.z > magnitude.y) {
        axis = 2u;
    }
    let on_axis = axis_unit(axis);
    let looks = select(-1.0, 1.0, dot(normal, on_axis) > 0.0);
    let u_unit = axis_unit((axis + 1u) % 3u);
    let v_unit = axis_unit((axis + 2u) % 3u);
    let u_step = vec3<i32>(u_unit);
    let v_step = vec3<i32>(v_unit);
    // The cell in front of the face: along the axis half a voxel out of the plane (whatever rounding the plane has), across it the cell the place is in.
    let front = vec3<i32>(floor(place + looks * 0.5 * on_axis));
    let near_brick = front >> vec3<u32>(5u);
    let near_entry = brick_entry(window, near_brick);
    let low_low = blocks_cell(window, front - u_step - v_step, near_brick, near_entry);
    let mid_low = blocks_cell(window, front - v_step, near_brick, near_entry);
    let high_low = blocks_cell(window, front + u_step - v_step, near_brick, near_entry);
    let low_mid = blocks_cell(window, front - u_step, near_brick, near_entry);
    let high_mid = blocks_cell(window, front + u_step, near_brick, near_entry);
    let low_high = blocks_cell(window, front - u_step + v_step, near_brick, near_entry);
    let mid_high = blocks_cell(window, front + v_step, near_brick, near_entry);
    let high_high = blocks_cell(window, front + u_step + v_step, near_brick, near_entry);
    let level_00 = select(low_mid + mid_low + low_low, 3.0, low_mid + mid_low > 1.5);
    let level_10 = select(high_mid + mid_low + high_low, 3.0, high_mid + mid_low > 1.5);
    let level_01 = select(low_mid + mid_high + low_high, 3.0, low_mid + mid_high > 1.5);
    let level_11 = select(high_mid + mid_high + high_high, 3.0, high_mid + mid_high > 1.5);
    let across = vec2<f32>(fract(dot(place, u_unit)), fract(dot(place, v_unit)));
    return mix(mix(level_00, level_10, across.x), mix(level_01, level_11, across.x), across.y);
}
