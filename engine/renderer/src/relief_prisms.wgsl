// The relief of a picture drawn as prisms (`Relief::Prisms`; `docs/specs/block-textures-spec.md`, "Prisms" under "As built", `docs/history/parallax-spike.md`): each pixel of the picture is a square
// column as deep below the face as the height in the alpha of its normal picture says (`RELIEF_DEPTH` at the lowest, a height of 255 being the face itself), the eye's ray is
// walked through the columns, and what it meets is drawn: the top of a column in the picture's colours and flat, or the side of one, lit as the sun says and darker the deeper it is.
//
// `picture_prepare` runs first in `fragment_main`, before anything reads a picture. It finds where the ray meets the relief, and the functions that read the picture then read it at
// `in.uv + parallax_shift`. The derivatives that choose the mip and solve the frame are those of `in.uv` (the shift jumps from column to column, and its derivative is no use).
// The relief is below the face, so the face's own polygon is the silhouette and nothing is drawn in front of it: the shader writes no depth, and the shadow maps are the flat face's.
// A ray that leaves the face's block reads the picture again from its other side (the repeat of the sampler), which is right where the neighbour has the same picture.
//
// Where a picture pixel is smaller than a pixel of the screen (`PRISM_FADE_FROM` to `PRISM_FADE_TO` picture pixels to the screen pixel) the columns would be noise, and are
// faded out of the picture and left out, which is also what bounds the cost: only the faces near the camera walk anything.

// How much of the relief is prisms at this pixel: 0 where it is not walked (the picture's tilts are what bumps the face then, as they are with `Relief::Flat`), up to 1.
var<private> parallax_prisms: f32 = 0.0;
// Where the picture is read, relative to where it is on the face.
var<private> parallax_shift: vec2<f32> = vec2<f32>(0.0);
// The side of a column the eye's ray ended on, as a normal in the world's axes (0 where it ended on a top, or there is no column); the height of the end of the ray (1 is the top);
// and the face's right and top directions, which `picture_bump` takes from here when they were found.
var<private> parallax_wall: vec3<f32> = vec3<f32>(0.0);
var<private> parallax_height: f32 = 1.0;
var<private> parallax_right: vec3<f32> = vec3<f32>(0.0);
var<private> parallax_up: vec3<f32> = vec3<f32>(0.0);
var<private> parallax_framed: bool = false;

// The most columns a ray is walked through: at a grazing eye (a face seen from 20 degrees over it) a ray through a relief a tenth of a voxel deep crosses about 6 pixels of 16.
const PARALLAX_CELLS: i32 = 12;
// The picture pixels to a screen pixel the relief is whole below and gone above.
const PRISM_FADE_FROM: f32 = 0.5;
const PRISM_FADE_TO: f32 = 1.0;

// The height (0 to 1) of the pixel of a picture at an integer place (it repeats, so any place is one): a column's height.
fn column_height(picture: u32, cell: vec2<i32>, pixels: f32) -> f32 {
    return sample_normal(picture, (vec2<f32>(cell) + vec2<f32>(0.5)) / pixels, vec2<f32>(0.0), vec2<f32>(0.0)).a;
}

// The heights of the four pixels round a place (the alpha of the normal picture, gathered in one go), in the order of `textureGather`: (left, below), (right, below), (right, above)
// and (left, above), where below is the next row.
fn gather_heights(picture: u32, coordinates: vec2<f32>) -> vec4<f32> {
    let layer = i32((picture >> 3u) & 255u);
    let wrapped = fract(coordinates);
    var heights = vec4<f32>(1.0);
    switch ((picture >> 1u) & 3u) {
        case 0u: {
            heights = textureGather(3, normals_0, picture_sampler, wrapped, layer);
        }
        case 1u: {
            heights = textureGather(3, normals_1, picture_sampler, wrapped, layer);
        }
        case 2u: {
            heights = textureGather(3, normals_2, picture_sampler, wrapped, layer);
        }
        default: {
            heights = textureGather(3, normals_3, picture_sampler, wrapped, layer);
        }
    }
    return heights;
}

fn picture_prepare(in: VertexOutput) {
    // (These are taken before anything branches, as in `picture_albedo`.)
    let uv_across = dpdx(in.uv);
    let uv_down = dpdy(in.uv);
    let place_across = dpdx(in.relative);
    let place_down = dpdy(in.relative);
    // (Bit 18 of the picture's word is the renderer's: the picture's normal companion has a pixel below the top. Nothing else is looked at of a picture without it.)
    if ((in.picture & (1u << 18u)) == 0u || frame.pictures.y <= 0.0) {
        return;
    }
    let share = picture_share(in.relative);
    if (share <= 0.0) {
        return;
    }
    let pixels = picture_pixels_per_voxel(in.picture);
    // Picture pixels to a screen pixel: past `PRISM_FADE_TO` there is nothing to see.
    let footprint = max(length(uv_across), length(uv_down)) * pixels;
    let visible = 1.0 - smoothstep(PRISM_FADE_FROM, PRISM_FADE_TO, footprint);
    if (visible <= 0.0) {
        return;
    }
    // A place where the four pixels round it are all at the top (a picture with no normal companion is all top) has nothing to look past.
    let around = gather_heights(in.picture, in.uv);
    if (min(min(around.x, around.y), min(around.z, around.w)) > 0.999) {
        return;
    }
    let determinant = uv_across.x * uv_down.y - uv_across.y * uv_down.x;
    if (abs(determinant) < 0.05 * length(uv_across) * length(uv_down)) {
        return;
    }
    let normal = normalize(in.normal);
    let along_right = (place_across * uv_down.y - place_down * uv_across.y) / determinant;
    let along_down = (place_down * uv_across.x - place_across * uv_down.x) / determinant;
    let right = normalize(along_right - normal * dot(along_right, normal));
    let up = -normalize(along_down - normal * dot(along_down, normal));
    // The eye's ray goes from the camera to the point; into the face it goes by `into` of its length. In the picture's coordinates (voxels, to the right and down the rows) it
    // moves `sideways` for each voxel of its length and `travel` for each voxel of depth. A face seen nearly edge on is left flat.
    let eye = normalize(in.relative);
    let into = -dot(eye, normal);
    if (into <= 0.05) {
        return;
    }
    parallax_right = right;
    parallax_up = up;
    parallax_framed = true;
    parallax_prisms = visible;
    let depth = RELIEF_DEPTH * share * visible;
    let sideways = vec2<f32>(dot(eye, right), -dot(eye, up));
    let travel = sideways / into;
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
