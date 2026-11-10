// The fragment shader's hook for the sun's exact short ray (docs/specs/contact-trace-spec.md), used while the renderer holds voxels that block the sun.
//
// A shadow map decides by comparing depths one texel wide, and the bias that keeps a thin floor from shading itself leaves a lit strip of a texel or two at the foot of every wall
// (and a leak of the sun into a closed cave). So a face is first asked a question the lattice can answer exactly: starting in the cell in front of the face, does the sun's ray enter
// a voxel that blocks the sun whole within the next few cells? If it does the place is in shadow; if it does not, the shadow map speaks for what is farther.
//
// A voxel may block the sun in part (a slab, a stair, a post; the walk through the eighths of such a voxel is in `shapes_on.wgsl`, which is used only while some brick has such
// voxels, and `shapes_off.wgsl` stands in for it otherwise: it is a good deal of code, and a fragment shader that has it costs a quarter more on an integrated GPU even where there is
// none to walk). A brick that has such voxels has a partial slot too, 16 bits for each voxel, the number of its shape plus one (0:
// none), and the shapes are 512 bits each, one for each eighth of a voxel, in a list. The walk goes through the eighths a ray crosses in such a voxel as it goes through voxels.
//
// The voxels are in windows, one for each grid with bricks in view (the terrain of the level, a planet, a ship): a table of 64 by 64 by 64 entries, one for each brick, indexed by
// the brick's coordinates modulo 64, and a pool of slots of one bit for each voxel of a brick (see `occupancy_volume.rs`, the same numbers). A face is traced in the window of its own
// grid, in the grid's own axes, so a planet that turns is walked in its own lattice.

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

// The most cells the walk searches by its own rule, and the most it is asked to search.
const MOST_CELLS: u32 = 24u;
const MOST_ASKED_CELLS: u32 = 64u;
// The face must look at the sun by at least this (the sine of the angle between the sun and the face's plane): a face that looks away from it is lit, if at all, by the map.
const LEAST_SINE: f32 = 1.0e-3;
// How many texels the slope bias of the shadow map's casters adds to the strip, per unit of the sine (`CASTER_SLOPE_BIAS` in `shadow.rs`).
const CASTER_SLOPE_BIAS: f32 = 1.5;

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

// The cell the ray enters next, across the nearest boundary of the one it is in: on a tie the lowest axis (as the reference in `world_view::sun_blocked`).
fn advance(cell: ptr<function, vec3<i32>>, boundary: ptr<function, vec3<f32>>, direction: vec3<i32>, next: vec3<f32>) {
    if ((*boundary).x <= (*boundary).y && (*boundary).x <= (*boundary).z) {
        (*cell).x = (*cell).x + direction.x;
        (*boundary).x = (*boundary).x + next.x;
    } else if ((*boundary).y <= (*boundary).z) {
        (*cell).y = (*cell).y + direction.y;
        (*boundary).y = (*boundary).y + next.y;
    } else {
        (*cell).z = (*cell).z + direction.z;
        (*boundary).z = (*boundary).z + next.z;
    }
}

// How many cells of the sun's ray to search from a face `distance` from the camera whose plane the sun is `sine` over: enough to cover the strip the map leaves lit at the foot of
// a wall. The strip across the floor is a texel of the cascade the place is looked up in times (the cosine of the sun's height over the face plus the slope bias times its sine,
// and a half for the filter), and a ray that climbs `sine / cosine` cells for each cell it goes along crosses it in about that many steps more; a few more are slack. The
// matrix of suns and cascades in the tests (`contact_trace.rs`) is what holds this to account. 0 if the place is beyond the cascades: there is no map there to correct.
fn contact_cells(distance: f32, sine: f32) -> u32 {
    let count = u32(frame.shadow.x);
    var texel = 0.0;
    var found = false;
    for (var index = 0u; index < count; index = index + 1u) {
        let reach = frame.cascades[index].limits.x;
        if (distance < reach) {
            texel = frame.cascades[index].limits.y;
            // (Over the last of its reach the map is mixed with the next cascade's, whose texels are wider.)
            if (distance >= BLEND_FROM * reach && index + 1u < count) {
                texel = max(texel, frame.cascades[index + 1u].limits.y);
            }
            found = true;
            break;
        }
    }
    if (!found) {
        return 0u;
    }
    let cosine = sqrt(max(1.0 - sine * sine, 0.0));
    let strip = texel * (cosine + CASTER_SLOPE_BIAS * sine + 0.5);
    let climb = sine / max(cosine, 0.05);
    // (A ray along a diagonal crosses as many cells along as the strip is wide times 1.41 at most.)
    let steps = strip * (1.5 + climb);
    // (A strip of any width is crossed in a step at least, and two more are slack: never fewer than three.)
    return min(u32(min(ceil(steps), 64.0)) + 2u, MOST_CELLS);
}

// Whether the sun's ray from the place on this face meets a voxel that blocks the sun in the first `cells` cells, in the voxels of this window. The ray starts in the cell in front
// of the face, so nothing is eased toward or away from anything: a wall beside the place is in the way or it is not, which is a fact about the lattice and not about rounding. Cells
// whose brick nothing is known of end the walk, clear.
//
// `own` is for the window of the grid the face is of: the face is in a plane of this lattice, and the cell in front of it, which is never tested, is where the walk starts. For any
// other window (a ship's, when the face is the ground's) the face is in no plane of the lattice: the walk starts in the cell the place is in, a sixteenth of a voxel out of the
// face, and tests it: a place inside a voxel of another grid is in the shade of it.
fn walk_window(window: Window, in: VertexOutput, sun: vec3<f32>, cells: u32, own: bool) -> bool {
    let toward_sun = window.to_local * sun;
    let normal = window.to_local * in.normal;
    let place = window.to_local * (in.relative - window.origin.xyz);
    // The face's axis in the lattice of the grid (its normal is along one, up to rounding), and which way it looks along it.
    let magnitude = abs(normal);
    var axis = 0u;
    if (magnitude.y > magnitude.x && magnitude.y >= magnitude.z) {
        axis = 1u;
    } else if (magnitude.z > magnitude.x && magnitude.z > magnitude.y) {
        axis = 2u;
    }
    let on_axis = axis_unit(axis);
    let looks = select(-1.0, 1.0, dot(normal, on_axis) > 0.0);
    // Where the ray starts (a face of this grid: at the place, on the face; another's: a sixteenth of a voxel out of the face along its normal), and the cell it starts in: for a face
    // of this grid the cell the place is in, moved by a sixteenth of a voxel out of the face. A face is in a plane that is a multiple of an eighth of a voxel (the finest a quad
    // goes), and such a plane is never within a sixteenth of a boundary of cells but on the face's own, and there this is the cell in front of it: the cell does not depend on a
    // rounding error in the place, however far from the origin the grid is turned.
    let origin = select(place + normal * 0.0625, place, own);
    var cell = vec3<i32>(floor(select(origin, place + looks * 0.0625 * on_axis, own)));
    let positive = toward_sun > vec3<f32>(0.0);
    let direction = select(vec3<i32>(-1), vec3<i32>(1), positive);
    let huge = 1.0e30;
    // An axis the ray does not move along has no boundary ahead of it (not the one behind it, which a zero turned into a huge number would put first).
    let moves = abs(toward_sun) > vec3<f32>(1.0e-20);
    let inverse = select(vec3<f32>(huge), 1.0 / toward_sun, moves);
    // How far along the ray the next boundary of a cell is, on each axis; and how much farther the one after it.
    var boundary = select(
        vec3<f32>(huge),
        (vec3<f32>(cell) + select(vec3<f32>(0.0), vec3<f32>(1.0), positive) - origin) * inverse,
        moves,
    );
    let next = abs(inverse);
    // The brick the ray is in, and what the table says of it: asked again only when the ray crosses into another (most steps stay in the brick).
    var brick = cell >> vec3<u32>(5u);
    var entry = brick_entry(window, brick);
    // (Whether the brick has voxels that block the sun in part: asked once for each brick the ray is in, and not for each voxel.)
    var has_partials = (entry >> PARTIAL_ENTRY_SHIFT) != 0u;
    // The cell the ray starts in, if it is another grid's walk: it is tested, whole or in part. (A brick the grid has nothing of is vacuum to a face of another grid: a ship has
    // bricks only where it has something, and the ground beside it is in bricks it has not; the walk goes on through them. For a face of the grid's own the bricks that are not
    // known end it, clear: they are the edge of what is loaded.)
    if (!own && cell_state(entry, cell) == CELL_BLOCKS) {
        return true;
    }
    // The voxel the ray starts in, if it blocks the sun in part (the top of a slab is in the middle of its voxel): the eighths of it that the ray crosses before it leaves, from
    // the one a sixteenth of a voxel out of the face, which for a face of this grid is not tested, as the start cell is not, and for another grid's is the one the point is in, and is.
    let start_number = select(0u, partial_number(entry, cell), has_partials);
    if (start_number != 0u) {
        let local = origin - vec3<f32>(cell);
        let start_sub = clamp(vec3<i32>(floor(local * 8.0 + select(vec3<f32>(0.0), looks * 0.5 * on_axis, own))), vec3<i32>(0), vec3<i32>(7));
        if (partial_blocks(start_number - 1u, local, toward_sun, start_sub, !own)) {
            return true;
        }
    }
    for (var step = 0u; step < cells; step = step + 1u) {
        let previous = cell;
        // Where along the ray the next voxel is entered.
        let entered_at = min(boundary.x, min(boundary.y, boundary.z));
        advance(&cell, &boundary, direction, next);
        let entered = cell >> vec3<u32>(5u);
        if (any(entered != brick)) {
            brick = entered;
            entry = brick_entry(window, brick);
            has_partials = (entry >> PARTIAL_ENTRY_SHIFT) != 0u;
        }
        let state = cell_state(entry, cell);
        if (state == CELL_BLOCKS) {
            return true;
        }
        if (state == CELL_UNKNOWN) {
            if (own) {
                return false;
            }
            continue;
        }
        if (has_partials) {
            let number = partial_number(entry, cell);
            if (number != 0u) {
                // The ray at the boundary it came across, in voxels from the corner of the voxel it enters: the part along that boundary's axis is exactly its near side.
                let crossed = cell != previous;
                let at = clamp(
                    select(origin + toward_sun * entered_at - vec3<f32>(cell), select(vec3<f32>(1.0), vec3<f32>(0.0), positive), crossed),
                    vec3<f32>(0.0),
                    vec3<f32>(1.0),
                );
                let first = select(
                    clamp(vec3<i32>(floor(at * 8.0)), vec3<i32>(0), vec3<i32>(7)),
                    select(vec3<i32>(7), vec3<i32>(0), positive),
                    crossed,
                );
                if (partial_blocks(number - 1u, at, toward_sun, first, true)) {
                    return true;
                }
            }
        }
    }
    return false;
}

// 0 if the sun's ray from the place on this face meets a voxel that blocks the sun in the first `forced_cells` cells (or the number the rule gives, if that is 0) of the window of the
// face's own grid, or of another grid's (`other_windows_block`), or the box of an entity (`entity_boxes_block`), 1 if not, which says nothing of what is farther: that is the shadow map's business.
fn contact_walk(in: VertexOutput, distance: f32, forced_cells: u32) -> f32 {
    if (in.volume == 0u) {
        return 1.0;
    }
    let sun = frame.sun_direction.xyz;
    let sine = dot(in.normal, sun);
    if (sine <= LEAST_SINE) {
        return 1.0;
    }
    var cells = min(forced_cells, MOST_ASKED_CELLS);
    if (cells == 0u) {
        cells = contact_cells(distance, sine);
        if (cells == 0u) {
            return 1.0;
        }
    }
    if (walk_window(contact_windows.windows[in.volume - 1u], in, sun, cells, true)) {
        return 0.0;
    }
    if (entity_boxes_block(in, sun, cells)) {
        return 0.0;
    }
    if (other_windows_block(in, sun, cells)) {
        return 0.0;
    }
    return 1.0;
}

// How much of the sun the voxels let through to the place of this face, `distance` from the camera, in the first few cells of the ray: 0 or 1.
fn contact_light(in: VertexOutput, distance: f32) -> f32 {
    return contact_walk(in, distance, 0u);
}

// The walk's answer alone, when it is asked for: white where the sun is not blocked, black where it is. (Alpha 0: nothing is drawn instead of the lit face.)
fn contact_debug(in: VertexOutput) -> vec4<f32> {
    if (contact_windows.debug.y == 0u) {
        return vec4<f32>(0.0);
    }
    // (The distance as `fragment_main` takes it.)
    let largest = max(abs(in.relative.x), max(abs(in.relative.y), abs(in.relative.z)));
    let distance = largest * length(in.relative / max(largest, 1.0e-30));
    let answer = contact_walk(in, distance, contact_windows.debug.x);
    return vec4<f32>(answer, answer, answer, 1.0);
}
