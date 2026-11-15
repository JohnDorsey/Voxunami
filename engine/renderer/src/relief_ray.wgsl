// What the reliefs that look into a picture share (`relief_prisms.wgsl`, `relief_parallax_occlusion.wgsl`; `relief_flat.wgsl` has the same names as constants and nothing to run): the
// answers `picture_prepare` leaves for the functions that read the picture, and the eye's ray into the face, found the same way for both.
//
// `picture_prepare` runs first in `fragment_main`, before anything reads a picture. It finds where the ray meets the relief, and the functions that read the picture then read it at
// `in.uv + parallax_shift`. The derivatives that choose the mip and solve the frame are those of `in.uv` (the shift may jump, and its derivative is no use). The relief is below
// the face, so the face's own polygon is the silhouette and nothing is drawn in front of it: the shader writes no depth, and the shadow maps are the flat face's. A ray that leaves
// the face's block reads the picture again from its other side (the repeat of the sampler), which is right where the neighbour has the same picture.
//
// Where a picture pixel is smaller than a pixel of the screen (`RELIEF_FADE_FROM` to `RELIEF_FADE_TO` picture pixels to the screen pixel) the relief would be noise, and is faded out
// of the picture and left out, which is also what bounds the cost: only the faces near the camera look into anything.

// How much of the relief is prisms at this pixel: 0 where there are none (the picture's tilts are what bumps the face then, as they are with `Relief::Flat`), up to 1. Only the
// prisms set it: the smooth relief keeps the tilts of the art.
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

// The picture pixels to a screen pixel the relief is whole below and gone above.
const RELIEF_FADE_FROM: f32 = 0.5;
const RELIEF_FADE_TO: f32 = 1.0;

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

// The eye's ray into the relief of a face's picture. Where `found` is false there is nothing to look past (no picture, no relief, too small on the screen, too far, a face seen
// nearly edge on) and the picture is read where the face is.
struct ReliefRay {
    found: bool,
    // Picture pixels to a voxel.
    pixels: f32,
    // How the ray moves in the picture's coordinates (voxels, to the right and down the rows) for each voxel of depth.
    travel: vec2<f32>,
    // How deep the relief is here, in voxels: `RELIEF_DEPTH`, less where the picture fades into the face's flat colour or the screen's pixels.
    depth: f32,
    // How much of the relief is to be seen at this pixel, 0 to 1 (the fade of the screen's pixels).
    visible: f32,
    // The face's right and top directions in the world, along the face and of length 1.
    right: vec3<f32>,
    up: vec3<f32>,
}

// (This takes derivatives, so it is called where the control flow is uniform: first in `fragment_main`, through `picture_prepare`.)
fn relief_ray(in: VertexOutput) -> ReliefRay {
    var ray = ReliefRay(false, 1.0, vec2<f32>(0.0), 0.0, 0.0, vec3<f32>(0.0), vec3<f32>(0.0));
    // (These are taken before anything branches, as in `picture_albedo`.)
    let uv_across = dpdx(in.uv);
    let uv_down = dpdy(in.uv);
    let place_across = dpdx(in.relative);
    let place_down = dpdy(in.relative);
    // (Bit 18 of the picture's word is the renderer's: the picture's normal companion has a pixel below the top. Nothing else is looked at of a picture without it.)
    if ((in.picture & (1u << 18u)) == 0u || frame.pictures.y <= 0.0) {
        return ray;
    }
    let share = picture_share(in.relative);
    if (share <= 0.0) {
        return ray;
    }
    let pixels = picture_pixels_per_voxel(in.picture);
    // Picture pixels to a screen pixel: past `RELIEF_FADE_TO` there is nothing to see.
    let footprint = max(length(uv_across), length(uv_down)) * pixels;
    let visible = 1.0 - smoothstep(RELIEF_FADE_FROM, RELIEF_FADE_TO, footprint);
    if (visible <= 0.0) {
        return ray;
    }
    // A place where the four pixels round it are all at the top (a picture with no normal companion is all top) has nothing to look past.
    let around = gather_heights(in.picture, in.uv);
    if (min(min(around.x, around.y), min(around.z, around.w)) > 0.999) {
        return ray;
    }
    let determinant = uv_across.x * uv_down.y - uv_across.y * uv_down.x;
    if (abs(determinant) < 0.05 * length(uv_across) * length(uv_down)) {
        return ray;
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
        return ray;
    }
    parallax_right = right;
    parallax_up = up;
    parallax_framed = true;
    let sideways = vec2<f32>(dot(eye, right), -dot(eye, up));
    ray.found = true;
    ray.pixels = pixels;
    ray.travel = sideways / into;
    ray.depth = RELIEF_DEPTH * share * visible;
    ray.visible = visible;
    ray.right = right;
    ray.up = up;
    return ray;
}
