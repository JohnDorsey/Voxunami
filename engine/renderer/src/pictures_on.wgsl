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
        let largest_part = max(abs(in.relative.x), max(abs(in.relative.y), abs(in.relative.z)));
        let picture_distance = largest_part * length(in.relative / max(largest_part, 1.0e-30));
        // The picture is all there is up to the distance, fades into the flat colour of the face over the band that follows, and is not read beyond it.
        let share = 1.0 - smoothstep(frame.pictures.x, frame.pictures.y, picture_distance);
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
