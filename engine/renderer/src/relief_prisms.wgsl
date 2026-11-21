// The relief of a picture drawn as prisms (`Relief::Prisms`; `docs/specs/block-textures-spec.md`, "Prisms" under "As built", `docs/history/parallax-spike.md`): each pixel of the picture is a square
// column as deep below the face as the height in the alpha of its normal picture says (`RELIEF_DEPTH` at the lowest, a height of 255 being the face itself), the eye's ray is
// walked through the columns, and what it meets is drawn: the top of a column in the picture's colours and flat, or the side of one, lit as the sun says and darker the deeper it is.
//
// The ray, the answers `picture_prepare` leaves and the fade with the screen's pixels are in `relief_ray.wgsl`.

// The most columns a ray is walked through: at a grazing eye (a face seen from 20 degrees over it) a ray through a relief a tenth of a voxel deep crosses about 6 pixels of 16.
const PARALLAX_CELLS: i32 = 12;

// The height (0 to 1) of the pixel of a picture at an integer place (it repeats, so any place is one): a column's height.
fn column_height(picture: u32, cell: vec2<i32>, pixels: f32) -> f32 {
    return sample_normal(picture, (vec2<f32>(cell) + vec2<f32>(0.5)) / pixels, vec2<f32>(0.0), vec2<f32>(0.0)).a;
}

fn picture_prepare(in: VertexOutput) {
    let ray = relief_ray(in);
    if (!ray.found) {
        return;
    }
    let right = ray.right;
    let up = ray.up;
    let pixels = ray.pixels;
    let travel = ray.travel;
    let depth = ray.depth;
    parallax_prisms = ray.visible;
    // The ray walked through the grid of the picture's pixels, one column of each, from cell to cell as a line is drawn on a grid. `fraction` is how far down the relief the
    // ray is, 0 at the top and 1 at the lowest; a column's top is at `1 - height`, and the ray is in the column once it is past that.
    let start = fract(in.uv) * pixels;
    let moving = travel * (depth * pixels);
    let going_right = moving.x > 0.0;
    let going_down = moving.y > 0.0;
    var cell = vec2<i32>(floor(start));
    let never = 1.0e30;
    var next = vec2<f32>(never);
    var spacing = vec2<f32>(never);
    if (abs(moving.x) > 1.0e-6) {
        next.x = (select(f32(cell.x), f32(cell.x + 1), going_right) - start.x) / moving.x;
        spacing.x = 1.0 / abs(moving.x);
    }
    if (abs(moving.y) > 1.0e-6) {
        next.y = (select(f32(cell.y), f32(cell.y + 1), going_down) - start.y) / moving.y;
        spacing.y = 1.0 / abs(moving.y);
    }
    var entered = 0.0;
    var hit = 1.0;
    var wall = vec3<f32>(0.0);
    var crossed_across = false;
    for (var index = 0; index < PARALLAX_CELLS; index = index + 1) {
        let top = 1.0 - column_height(in.picture, cell, pixels);
        let leaves = min(min(next.x, next.y), 1.0);
        if (top <= entered) {
            // Already under the top on entering: the side of the column that the ray came in by (or, in the first cell, the top at the face itself).
            hit = entered;
            if (index > 0) {
                if (crossed_across) {
                    wall = right * select(1.0, -1.0, going_right);
                } else {
                    wall = up * select(-1.0, 1.0, going_down);
                }
            }
            break;
        }
        if (top <= leaves) {
            hit = top;
            break;
        }
        if (leaves >= 1.0) {
            hit = 1.0;
            break;
        }
        if (next.x < next.y) {
            cell.x = cell.x + select(-1, 1, going_right);
            entered = next.x;
            next.x = next.x + spacing.x;
            crossed_across = true;
        } else {
            cell.y = cell.y + select(-1, 1, going_down);
            entered = next.y;
            next.y = next.y + spacing.y;
            crossed_across = false;
        }
        hit = entered;
    }
    // (A wall is on the boundary of two cells: a hair farther down the ray is inside the column.)
    let nudge = select(0.0, 0.002, dot(wall, wall) > 0.5);
    parallax_shift = travel * (depth * (hit + nudge));
    parallax_wall = wall;
    parallax_height = 1.0 - hit;
}
