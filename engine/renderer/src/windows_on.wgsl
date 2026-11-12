// The part of the walk of the sun's ray (`contact_on.wgsl`) that walks the windows of the other grids, used while more than one window is traced: the ground meets the shade of a
// ship that stands on it, and a ship meets that of a cliff (`docs/specs/contact-trace-spec.md`, section 14).

// How many windows there are (the length of the array of `Windows`).
const WINDOW_COUNT: u32 = 4u;

// Whether the sun's ray from the place on this face meets a voxel that blocks the sun in the first `cells` cells of a window of any grid but the face's own. A window whose bricks
// are all farther from the place than the ray can go is not walked: that is the cheap test that keeps a ship's window from costing the ground's pixels a second walk where no
// ship is near.
fn other_windows_block(in: VertexOutput, sun: vec3<f32>, cells: u32) -> bool {
    // (A step along the ray crosses a cell and goes at most the diagonal of one; two more cells are slack.)
    let reach = 1.75 * f32(cells) + 2.0;
    for (var index = 0u; index < WINDOW_COUNT; index = index + 1u) {
        let window = contact_windows.windows[index];
        if (index + 1u == in.volume || window.origin.w == 0.0) {
            continue;
        }
        let place = window.to_local * (in.relative - window.origin.xyz);
        if (any(place < window.bounds_low.xyz - vec3<f32>(reach)) || any(place > window.bounds_high.xyz + vec3<f32>(reach))) {
            continue;
        }
        if (walk_window(window, in, sun, cells, false)) {
            return true;
        }
    }
    return false;
}
