// The fragment shader's hook for pictures when some block has them: the pictures of blocks, an array of layers for each of up to four sizes of picture (smallest
// first), and the sampler that reads them. A palette entry whose `w` holds a picture (see `FacePicture::word` in block_models) is drawn with it.
@group(0) @binding(4) var pictures_0: texture_2d_array<f32>;
@group(0) @binding(5) var pictures_1: texture_2d_array<f32>;
@group(0) @binding(6) var pictures_2: texture_2d_array<f32>;
@group(0) @binding(7) var pictures_3: texture_2d_array<f32>;
@group(0) @binding(8) var picture_sampler: sampler;
// The specular pictures that go with them (LabPBR `_s`), an array for each size with the same layers; data, not colours. Only the alpha is read so far: the emission. They are in
// a bind group of their own (the fourth), which only the pipelines made with this file carry: the layout of the pipelines that draw no pictures is as it was before they existed.
@group(3) @binding(0) var specular_0: texture_2d_array<f32>;
@group(3) @binding(1) var specular_1: texture_2d_array<f32>;
@group(3) @binding(2) var specular_2: texture_2d_array<f32>;
@group(3) @binding(3) var specular_3: texture_2d_array<f32>;
// And the normal pictures (LabPBR `_n`), the same: data, not colours. The red and green are the tilt of the face at the pixel; the rest is not read yet.
@group(3) @binding(4) var normals_0: texture_2d_array<f32>;
@group(3) @binding(5) var normals_1: texture_2d_array<f32>;
@group(3) @binding(6) var normals_2: texture_2d_array<f32>;
@group(3) @binding(7) var normals_3: texture_2d_array<f32>;

// The colour of a picture at a place on a face: `coordinates` are in voxels (the picture repeats in each), `ddx` and `ddy` how much they change across a pixel, which
// say which smaller picture to read. The size of the picture is its number among the sizes in use (bits 1 and 2) and its place among the pictures of that size is
// bits 3 to 10. The colour is linear, as the picture is in an sRGB texture, and the fourth part is how much of the picture is there (0 where it is clear).
fn sample_picture(picture: u32, coordinates: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32> {
    let layer = i32((picture >> 3u) & 255u);
    let wrapped = fract(coordinates);
    var texel = vec4<f32>(1.0, 0.0, 1.0, 1.0);
    switch ((picture >> 1u) & 3u) {
        case 0u: {
            texel = textureSampleGrad(pictures_0, picture_sampler, wrapped, layer, ddx, ddy);
        }
        case 1u: {
            texel = textureSampleGrad(pictures_1, picture_sampler, wrapped, layer, ddx, ddy);
        }
        case 2u: {
            texel = textureSampleGrad(pictures_2, picture_sampler, wrapped, layer, ddx, ddy);
        }
        default: {
            texel = textureSampleGrad(pictures_3, picture_sampler, wrapped, layer, ddx, ddy);
        }
    }
    return texel;
}

// The specular picture of a picture at a place on a face, read as `sample_picture` reads the picture (the picture's word names the size and the layer). A picture with no specular
// companion has the pixel of nothing (all zero).
fn sample_specular(picture: u32, coordinates: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32> {
    let layer = i32((picture >> 3u) & 255u);
    let wrapped = fract(coordinates);
    var texel = vec4<f32>(0.0);
    switch ((picture >> 1u) & 3u) {
        case 0u: {
            texel = textureSampleGrad(specular_0, picture_sampler, wrapped, layer, ddx, ddy);
        }
        case 1u: {
            texel = textureSampleGrad(specular_1, picture_sampler, wrapped, layer, ddx, ddy);
        }
        case 2u: {
            texel = textureSampleGrad(specular_2, picture_sampler, wrapped, layer, ddx, ddy);
        }
        default: {
            texel = textureSampleGrad(specular_3, picture_sampler, wrapped, layer, ddx, ddy);
        }
    }
    return texel;
}

// The normal picture of a picture at a place on a face, read as `sample_picture` reads the picture. A picture with no normal companion has the pixel of a flat face (128, 128).
fn sample_normal(picture: u32, coordinates: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32> {
    let layer = i32((picture >> 3u) & 255u);
    let wrapped = fract(coordinates);
    var texel = vec4<f32>(128.0 / 255.0, 128.0 / 255.0, 1.0, 1.0);
    switch ((picture >> 1u) & 3u) {
        case 0u: {
            texel = textureSampleGrad(normals_0, picture_sampler, wrapped, layer, ddx, ddy);
        }
        case 1u: {
            texel = textureSampleGrad(normals_1, picture_sampler, wrapped, layer, ddx, ddy);
        }
        case 2u: {
            texel = textureSampleGrad(normals_2, picture_sampler, wrapped, layer, ddx, ddy);
        }
        default: {
            texel = textureSampleGrad(normals_3, picture_sampler, wrapped, layer, ddx, ddy);
        }
    }
    return texel;
}

// How much of the picture there is at a place `relative` to the camera, 0 to 1: all of it up to the picture distance, fading out over the band that follows. Beyond it a
// face is whole and flat, which is also what the shadow of it is.
fn picture_share(relative: vec3<f32>) -> f32 {
    let largest_part = max(abs(relative.x), max(abs(relative.y), abs(relative.z)));
    let picture_distance = largest_part * length(relative / max(largest_part, 1.0e-30));
    return 1.0 - smoothstep(frame.pictures.x, frame.pictures.y, picture_distance);
}

// How many pixels a picture has along a voxel's side: the side of the pictures of the size this word names.
fn picture_pixels_per_voxel(picture: u32) -> f32 {
    var side = 1u;
    switch ((picture >> 1u) & 3u) {
        case 0u: {
            side = textureDimensions(pictures_0).x;
        }
        case 1u: {
            side = textureDimensions(pictures_1).x;
        }
        case 2u: {
            side = textureDimensions(pictures_2).x;
        }
        default: {
            side = textureDimensions(pictures_3).x;
        }
    }
    return f32(side);
}

// Where the shadow of the face is looked up. With pixelated shadows off, where the pixel is. With them on, the middle of the picture's pixel the pixel of the screen is in, so
// that every screen pixel of a picture pixel is lit alike and the edge of a shadow steps along the picture's pixels (the share of the picture there is at the pixel scales the
// move, so that it fades out with the picture). The move is along the face: how the picture's coordinates and the place change across the screen say how far on the screen the
// middle is, and where that is. Where the face is nearly edge on (the coordinates hardly change in one direction) the place is left as it is.
fn pixelated_place(in: VertexOutput) -> vec3<f32> {
    // (These are taken before anything branches, as in `picture_albedo`.)
    let uv_across = dpdx(in.uv);
    let uv_down = dpdy(in.uv);
    let place_across = dpdx(in.relative);
    let place_down = dpdy(in.relative);
    if (in.picture == 0u || frame.pictures.z < 0.5 || frame.pictures.y <= 0.0) {
        return in.relative;
    }
    let share = picture_share(in.relative);
    if (share <= 0.0) {
        return in.relative;
    }
    let pixels = picture_pixels_per_voxel(in.picture);
    let middle = (floor(in.uv * pixels) + vec2<f32>(0.5)) / pixels;
    let determinant = uv_across.x * uv_down.y - uv_across.y * uv_down.x;
    if (abs(determinant) < 0.05 * length(uv_across) * length(uv_down)) {
        return in.relative;
    }
    let to_middle = middle - in.uv;
    let across = (to_middle.x * uv_down.y - to_middle.y * uv_down.x) / determinant;
    let down = (uv_across.x * to_middle.y - uv_across.y * to_middle.x) / determinant;
    return in.relative + (place_across * across + place_down * down) * share;
}

// The colour of the face at this pixel: a picture, fading into the face's own colour with the distance; and how much of the face is there, which is 1 unless the
// block cuts holes (bit 17 of the picture's word: the block's transparency is not the opaque one) and the picture is clear at this pixel (the fragment is then not
// drawn: a hole, in leaves and in the panes of glass). An opaque block draws its picture whole, whatever its alpha. Beyond the distance the face is a whole face of
// its flat colour, as the coarse levels of detail are.
fn picture_albedo(in: VertexOutput) -> vec4<f32> {
    // (How the coordinates of the picture change across a pixel is taken here, before anything branches: sampling with them is allowed anywhere, taking them is not.)
    let picture_dx = dpdx(in.uv);
    let picture_dy = dpdy(in.uv);
    var albedo = in.albedo;
    var coverage = 1.0;
    if (in.picture != 0u && frame.pictures.y > 0.0) {
        // The picture is all there is up to the distance, fades into the flat colour of the face over the band that follows, and is not read beyond it.
        let share = picture_share(in.relative);
        if (share > 0.0) {
            let texel = sample_picture(in.picture, in.uv + parallax_shift, picture_dx, picture_dy);
            albedo = mix(albedo, texel.rgb, share);
            if (((in.picture >> 17u) & 1u) == 1u) {
                coverage = mix(1.0, texel.a, share);
            }
        }
    }
    return vec4<f32>(albedo, coverage);
}

// What the picture's specular companion says of the pixel (LabPBR `_s`), faded out with the picture over the same band, so that beyond the picture distance a face is as flat as it
// is in the colour and the shadow: x is how smooth it is (the red byte, 0 to 1), y how much light it throws back straight on (the green byte, 0 to 1), z whether it is a metal
// (1 from a green byte of 230, which throws back its own colour: 0 otherwise), how much of it shines by itself, 0 to 1 (the alpha: 0 to 254 is none to all, and the loader
// has made 255, none, a 0), and how much of the sun's light it lets through (the blue byte: up to 64 is porosity, which is not read, and from 65 `(byte - 64) / 191`, 0 to 1).
fn picture_material(in: VertexOutput) -> PictureMaterial {
    // (How the coordinates of the picture change across a pixel is taken here, before anything branches, as in `picture_albedo`.)
    let picture_dx = dpdx(in.uv);
    let picture_dy = dpdy(in.uv);
    if (in.picture == 0u || frame.pictures.y <= 0.0) {
        return PictureMaterial(0.0, 0.0, 0.0, 0.0, 0.0);
    }
    let share = picture_share(in.relative);
    if (share <= 0.0) {
        return PictureMaterial(0.0, 0.0, 0.0, 0.0, 0.0);
    }
    let texel = sample_specular(in.picture, in.uv + parallax_shift, picture_dx, picture_dy);
    let metal = select(0.0, 1.0, texel.g * 255.0 >= 229.5);
    let emission = min(texel.a * (255.0 / 254.0), 1.0);
    let subsurface = clamp((texel.b * 255.0 - 64.0) / 191.0, 0.0, 1.0);
    return PictureMaterial(texel.r * share, texel.g * share, metal * share, emission * share, subsurface * share);
}

// How much the normal picture tilts the face's normal at this pixel: the tilted normal minus the face's own, in the world's axes, faded out with the picture over the same band
// (so that beyond the picture distance a face is as flat as it is in the colour and the shadow). It is added to the normal that the sun's light is the cosine of (and to no
// other: the shadow, which sides of a block the sun reaches, and the sky's light keep the face's own normal, so that a bump never lets the sun through a wall).
//
// The picture is LabPBR's: red and green are the normal's x and y (to the right in the picture and toward its top) as 128 plus or minus 127 for a tilt of 1 either way, and the
// part along the face is what makes the length 1. Which way is right and up in the world is found from how the picture's coordinates and the place change across the screen,
// so that it holds on every face and on a ship turned any way.
fn picture_bump(in: VertexOutput) -> PictureBump {
    // (These are taken before anything branches, as in `picture_albedo`.)
    let uv_across = dpdx(in.uv);
    let uv_down = dpdy(in.uv);
    let place_across = dpdx(in.relative);
    let place_down = dpdy(in.relative);
    if (in.picture == 0u || frame.pictures.y <= 0.0) {
        return PictureBump(vec3<f32>(0.0), 1.0, 1.0);
    }
    let share = picture_share(in.relative);
    if (share <= 0.0) {
        return PictureBump(vec3<f32>(0.0), 1.0, 1.0);
    }
    // (SPIKE: where the eye's ray meets the relief, which is where the picture is read when there is parallax: `picture_prepare`. A prism's side is a wall, and prisms are flat on top.)
    let uv = in.uv + parallax_shift;
    let prisms = parallax_mode() == 3u;
    let wall = dot(parallax_wall, parallax_wall) > 0.5;
    let texel = sample_normal(in.picture, uv, uv_across, uv_down);
    let height = select(texel.a, parallax_height, prisms);
    // The blue byte is the share of the ambient light that gets to the pixel, faded in with the picture like the tilt (a prism's wall is darker the deeper it is).
    let open = mix(1.0, select(texel.b, mix(1.0, 0.45, 1.0 - height), wall), share);
    var tilt = select((texel.xy - vec2<f32>(128.0 / 255.0)) * (255.0 / 127.0), vec2<f32>(0.0), prisms);
    // A pixel that does not tilt the face and is at the top of the relief (the stand-in of a picture with no normal companion, which is most of what is drawn: 128, 128 and a
    // height of 255; and a flat pixel at the top of one that has) leaves the face as it is: the frame below would only find the face's own normal again, and nothing is higher
    // than the top to shade it.
    let at_the_top = height > 0.999 && !wall;
    if (all(tilt == vec2<f32>(0.0)) && at_the_top) {
        return PictureBump(vec3<f32>(0.0), open, 1.0);
    }
    let steepness = dot(tilt, tilt);
    if (steepness > 1.0) {
        tilt = tilt * inverseSqrt(steepness);
    }
    let out_of_the_face = sqrt(max(1.0 - dot(tilt, tilt), 0.0));
    let normal = normalize(in.normal);
    // Where the picture's coordinates grow, along the face: the place changes by `place_across` and `place_down` while the coordinates change by `uv_across` and `uv_down`,
    // so the change of the place with u (to the right in the picture) and with v (down it: rows count from the top) is the solution of two equations. (Solved, and not the
    // usual cotangent frame, whose signs follow which way the screen's rows count and so turn the picture's directions both around on this one.) Where the face is nearly edge
    // on the equations have no good solution, and the face is left as it is.
    var right = parallax_right;
    var up = parallax_up;
    if (!parallax_framed) {
        let determinant = uv_across.x * uv_down.y - uv_across.y * uv_down.x;
        if (abs(determinant) < 0.05 * length(uv_across) * length(uv_down)) {
            return PictureBump(vec3<f32>(0.0), open, 1.0);
        }
        let along_right = (place_across * uv_down.y - place_down * uv_across.y) / determinant;
        let along_down = (place_down * uv_across.x - place_across * uv_down.x) / determinant;
        // (Taken along the face, and of length 1.)
        right = normalize(along_right - normal * dot(along_right, normal));
        up = -normalize(along_down - normal * dot(along_down, normal));
    }
    var tilted = normalize(right * tilt.x + up * tilt.y + normal * out_of_the_face);
    if (wall) {
        tilted = parallax_wall;
    }
    var sun = 1.0;
    if (!at_the_top && frame.sun_light.x > 0.0) {
        sun = 1.0 - share * (1.0 - relief_sun(in.picture, uv, uv_across, uv_down, height, normal, right, up));
    }
    return PictureBump((tilted - normal) * share, open, sun);
}

// ---- SPIKE: parallax (`docs/history/parallax-spike.md`). ----
// The eye's ray into the relief of a normal picture, found for a pixel before anything reads the picture, so that the picture is read where the ray meets the relief and not where
// it meets the flat face. The mode and the depth of the relief come in the one number `frame.pictures.w`: its whole part is the mode (0 none, 1 one shift by the height at the
// pixel, 2 a march through the heights as a smooth surface, 3 a march through them as square prisms, each pixel of the picture a column of its own height and the sides shaded),
// its fraction the depth in voxels (0 is the default). The coordinates the picture is read at are `in.uv + parallax_shift`; the derivatives that choose the mip and solve the frame
// are those of `in.uv` (the shift jumps from prism to prism, and its derivative is no use). Nothing here moves the face in depth, so no silhouette is made, nothing is hidden
// by the neighbour of a block, and the shadow maps are the flat face's.
var<private> parallax_shift: vec2<f32> = vec2<f32>(0.0);
// The side of a prism the eye's ray ended on, as a normal in the world's axes (0 where it ended on a top, or there is no prism); the height of the end of the ray (1 is the top);
// and the face's right and top directions, which `picture_bump` takes from here when they were found.
var<private> parallax_wall: vec3<f32> = vec3<f32>(0.0);
var<private> parallax_height: f32 = 1.0;
var<private> parallax_right: vec3<f32> = vec3<f32>(0.0);
var<private> parallax_up: vec3<f32> = vec3<f32>(0.0);
var<private> parallax_framed: bool = false;

const PARALLAX_STEPS: i32 = 12;
const PARALLAX_CELLS: i32 = 12;

fn parallax_mode() -> u32 {
    return u32(floor(frame.pictures.w));
}

fn relief_depth() -> f32 {
    let fraction = fract(frame.pictures.w);
    return select(RELIEF_DEPTH, fraction, fraction > 0.0);
}

// The height (0 to 1) of the pixel of a picture at an integer place (it repeats, so any place is one): a prism's height.
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

// The height of the picture at a place as a smooth surface: the four pixels round it mixed as a bilinear filter would (the sampler of the pictures is nearest when magnifying).
fn smooth_height(picture: u32, coordinates: vec2<f32>, pixels: f32) -> f32 {
    let heights = gather_heights(picture, coordinates);
    let within = fract(fract(coordinates) * pixels - vec2<f32>(0.5));
    return mix(mix(heights.w, heights.z, within.x), mix(heights.x, heights.y, within.x), within.y);
}

fn picture_prepare(in: VertexOutput) {
    // (These are taken before anything branches, as in `picture_albedo`.)
    let uv_across = dpdx(in.uv);
    let uv_down = dpdy(in.uv);
    let place_across = dpdx(in.relative);
    let place_down = dpdy(in.relative);
    let mode = parallax_mode();
    if (mode == 0u || in.picture == 0u || frame.pictures.y <= 0.0) {
        return;
    }
    let share = picture_share(in.relative);
    if (share <= 0.0) {
        return;
    }
    let pixels = picture_pixels_per_voxel(in.picture);
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
    let depth = relief_depth() * share;
    let sideways = vec2<f32>(dot(eye, right), -dot(eye, up));
    let travel = sideways / into;
    if (mode == 1u) {
        // Offset limiting: the picture is read where the ray would be at the depth the height at this pixel says, with no division by the cosine, so that the shift stays small.
        parallax_shift = sideways * (depth * (1.0 - smooth_height(in.picture, in.uv, pixels)));
        return;
    }
    if (mode == 2u) {
        // A march in equal steps down the ray, for the first where the ray is under the smooth surface, and the place between that step and the one before it where the two meet.
        var previous_gap = 1.0 - smooth_height(in.picture, in.uv, pixels);
        var hit = 1.0;
        for (var index = 1; index <= PARALLAX_STEPS; index = index + 1) {
            let fraction = f32(index) / f32(PARALLAX_STEPS);
            let gap = (1.0 - fraction) - smooth_height(in.picture, in.uv + travel * (depth * fraction), pixels);
            if (gap <= 0.0) {
                hit = fraction - (1.0 - previous_gap / max(previous_gap - gap, 1.0e-6)) / f32(PARALLAX_STEPS);
                break;
            }
            previous_gap = gap;
        }
        parallax_shift = travel * (depth * hit);
        return;
    }
    // Prisms: the ray walked through the grid of the picture's pixels, one column of each, from cell to cell as a line is drawn on a grid. `fraction` is how far down the relief the
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

// How deep the relief of a normal picture is, in voxels, from the top (a height of 255) to the lowest (0): the art of the dirt makes its tilts from a height field that is 1.6
// pixels of its 16 deep, so a tenth of a voxel; how far along the sun's ray a pixel looks for something higher than the ray (voxels, a few pixels of a picture of 16: the shade of
// a bump is short, and a longer look would find the next block's picture, which the repeat of this one stands for badly); and how soft the edge of a shade is (voxels of height
// between the ray being clear of the ground and being under it).
const RELIEF_DEPTH: f32 = 0.1;
const RELIEF_REACH: f32 = 0.25;
const RELIEF_SOFTNESS: f32 = 0.01;
const RELIEF_STEPS: i32 = 4;

// The share of the sun's light that gets past the relief of the picture to a pixel `height` high (0 to 1, 1 for the top), on a face whose normal is `normal` and in which the picture's
// right and top are the directions `right` and `up`: the ray from the pixel toward the sun climbs as it goes along the face, and if the picture's height at a place under the ray
// (looked at in a few steps, as far as the ray takes to climb past the top, and no farther than `RELIEF_REACH`) is above the ray, something stands over the pixel. A face turned
// from the sun has nothing of the sun to be shaded from. The coordinates of the picture are in voxels, as the place is, so a step of the ray along the face is the same step in them
// (to the right where the ray goes along `right`, and down the rows where it goes along `up`' opposite).
fn relief_sun(picture: u32, coordinates: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>, height: f32, normal: vec3<f32>, right: vec3<f32>, up: vec3<f32>) -> f32 {
    let to_the_sun = frame.sun_direction.xyz;
    let climb = dot(to_the_sun, normal);
    if (climb <= 0.02) {
        return 1.0;
    }
    let depth = relief_depth();
    let start = depth * (height - 1.0);
    let reach = min(-start / climb, RELIEF_REACH);
    let step_in_the_picture = vec2<f32>(dot(to_the_sun, right), -dot(to_the_sun, up));
    var blocked = 0.0;
    for (var index = 1; index <= RELIEF_STEPS; index = index + 1) {
        let along = reach * f32(index) / f32(RELIEF_STEPS);
        let ray = start + climb * along;
        let ground = depth * (sample_normal(picture, coordinates + step_in_the_picture * along, ddx, ddy).a - 1.0);
        blocked = max(blocked, smoothstep(0.0, RELIEF_SOFTNESS, ground - ray));
    }
    return 1.0 - blocked;
}

// ---- The shadow maps of blocks that cut holes: what leaves and glass hide of the sun is what their pictures are not clear of. ----
// The same quads as `shadow_vertex_main` and `shadow_vertex_detailed`, but each carries where it is on its picture, and the fragment stage does not keep a pixel that the
// picture is clear at, by the rule of `fragment_main` (which is why it takes the distance from the camera: a face beyond the picture distance is whole there too).

struct CutoutShadowVertex {
    @builtin(position) position: vec4<f32>,
    // Where the point is on the face's picture, in voxels (see `VertexOutput::uv`).
    @location(0) uv: vec2<f32>,
    // The picture on the face (bits of `FacePicture::word`), 0 for none.
    @location(1) @interpolate(flat) picture: u32,
    // Where the point is relative to the camera, in voxels.
    @location(2) relative: vec3<f32>,
}

fn cutout_shadow_vertex(corner: Corner, material: u32) -> CutoutShadowVertex {
    var picture = 0u;
    if (material < AVATAR_BASE && material < arrayLength(&palette)) {
        picture = bitcast<u32>(palette[material].w) & 0x7FFFFFu;
    }
    var out: CutoutShadowVertex;
    out.picture = picture;
    out.uv = vec2<f32>(0.0);
    if (picture != 0u) {
        out.uv = vec2<f32>(along(corner.local, (picture >> 11u) & 7u), -along(corner.local, (picture >> 14u) & 7u));
    }
    out.position = frame.view_projection * vec4<f32>(corner.relative, 1.0);
    out.relative = corner.relative;
    return out;
}

@vertex
fn shadow_cutout_vertex_main(@builtin(vertex_index) vertex_index: u32, instance: Instance) -> CutoutShadowVertex {
    return cutout_shadow_vertex(whole_corner(vertex_index, instance), instance.material & 0xFFFFFFu);
}

@vertex
fn shadow_cutout_vertex_detailed(@builtin(vertex_index) vertex_index: u32, instance: DetailedInstance) -> CutoutShadowVertex {
    return cutout_shadow_vertex(detailed_corner(vertex_index, instance), instance.material & 0xFFFFFFu);
}

@fragment
fn shadow_cutout_fragment(in: CutoutShadowVertex) {
    // (How the coordinates of the picture change across a pixel is taken before anything branches, as in `picture_albedo`.)
    let picture_dx = dpdx(in.uv);
    let picture_dy = dpdy(in.uv);
    // Only a block that cuts holes (bit 17 of the word) and only where its picture is drawn (a distance of 0 draws none).
    if (in.picture != 0u && ((in.picture >> 17u) & 1u) == 1u && frame.pictures.y > 0.0) {
        let share = picture_share(in.relative);
        if (share > 0.0) {
            let texel = sample_picture(in.picture, in.uv, picture_dx, picture_dy);
            if (mix(1.0, texel.a, share) < 0.5) {
                discard;
            }
        }
    }
}
