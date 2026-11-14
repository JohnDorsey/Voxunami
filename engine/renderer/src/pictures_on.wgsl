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
            let texel = sample_picture(in.picture, in.uv, picture_dx, picture_dy);
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
    let texel = sample_specular(in.picture, in.uv, picture_dx, picture_dy);
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
    let texel = sample_normal(in.picture, in.uv, uv_across, uv_down);
    // The blue byte is the share of the ambient light that gets to the pixel, faded in with the picture like the tilt.
    let open = mix(1.0, texel.b, share);
    var tilt = (texel.xy - vec2<f32>(128.0 / 255.0)) * (255.0 / 127.0);
    // A pixel that does not tilt the face and is at the top of the relief (the stand-in of a picture with no normal companion, which is most of what is drawn: 128, 128 and a
    // height of 255; and a flat pixel at the top of one that has) leaves the face as it is: the frame below would only find the face's own normal again, and nothing is higher
    // than the top to shade it.
    let at_the_top = texel.a > 0.999;
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
    let determinant = uv_across.x * uv_down.y - uv_across.y * uv_down.x;
    if (abs(determinant) < 0.05 * length(uv_across) * length(uv_down)) {
        return PictureBump(vec3<f32>(0.0), open, 1.0);
    }
    let along_right = (place_across * uv_down.y - place_down * uv_across.y) / determinant;
    let along_down = (place_down * uv_across.x - place_across * uv_down.x) / determinant;
    // (Taken along the face, and of length 1.)
    let right = normalize(along_right - normal * dot(along_right, normal));
    let up = -normalize(along_down - normal * dot(along_down, normal));
    let tilted = normalize(right * tilt.x + up * tilt.y + normal * out_of_the_face);
    var sun = 1.0;
    if (!at_the_top && frame.sun_light.x > 0.0) {
        sun = 1.0 - share * (1.0 - relief_sun(in.picture, in.uv, uv_across, uv_down, texel.a, normal, right, up));
    }
    return PictureBump((tilted - normal) * share, open, sun);
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
    let start = RELIEF_DEPTH * (height - 1.0);
    let reach = min(-start / climb, RELIEF_REACH);
    let step_in_the_picture = vec2<f32>(dot(to_the_sun, right), -dot(to_the_sun, up));
    var blocked = 0.0;
    for (var index = 1; index <= RELIEF_STEPS; index = index + 1) {
        let along = reach * f32(index) / f32(RELIEF_STEPS);
        let ray = start + climb * along;
        let ground = RELIEF_DEPTH * (sample_normal(picture, coordinates + step_in_the_picture * along, ddx, ddy).a - 1.0);
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
