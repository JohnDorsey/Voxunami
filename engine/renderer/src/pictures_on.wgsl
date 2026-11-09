// The fragment shader's hook for pictures when some block has them: the pictures of blocks, an array of layers for each of up to four sizes of picture (smallest
// first), and the sampler that reads them. A palette entry whose `w` holds a picture (see `FacePicture::word` in block_models) is drawn with it.
@group(0) @binding(4) var pictures_0: texture_2d_array<f32>;
@group(0) @binding(5) var pictures_1: texture_2d_array<f32>;
@group(0) @binding(6) var pictures_2: texture_2d_array<f32>;
@group(0) @binding(7) var pictures_3: texture_2d_array<f32>;
@group(0) @binding(8) var picture_sampler: sampler;

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

// How much of the picture there is at a place `relative` to the camera, 0 to 1: all of it up to the picture distance, fading out over the band that follows. Beyond it a
// face is whole and flat, which is also what the shadow of it is.
fn picture_share(relative: vec3<f32>) -> f32 {
    let largest_part = max(abs(relative.x), max(abs(relative.y), abs(relative.z)));
    let picture_distance = largest_part * length(relative / max(largest_part, 1.0e-30));
    return 1.0 - smoothstep(frame.pictures.x, frame.pictures.y, picture_distance);
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
