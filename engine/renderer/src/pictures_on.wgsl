// The fragment shader's hook for pictures when some block has them: the pictures of blocks, an array of layers for each of up to four sizes of picture (smallest
// first), and the sampler that reads them. A palette entry whose `w` holds a picture (see `FacePicture::word` in block_models) is drawn with it.
@group(0) @binding(4) var pictures_0: texture_2d_array<f32>;
@group(0) @binding(5) var pictures_1: texture_2d_array<f32>;
@group(0) @binding(6) var pictures_2: texture_2d_array<f32>;
@group(0) @binding(7) var pictures_3: texture_2d_array<f32>;
@group(0) @binding(8) var picture_sampler: sampler;

// The colour of a picture at a place on a face: `coordinates` are in voxels (the picture repeats in each), `ddx` and `ddy` how much they change across a pixel, which
// say which smaller picture to read. The size of the picture is its number among the sizes in use (bits 1 and 2) and its place among the pictures of that size is
// bits 3 to 10. The colour is linear, as the picture is in an sRGB texture.
fn sample_picture(picture: u32, coordinates: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec3<f32> {
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
    return texel.rgb;
}

// The colour of the face at this pixel: a picture, fading into the face's own colour with the distance.
fn picture_albedo(in: VertexOutput) -> vec3<f32> {
    // Where the pixel is on the face's picture, in voxels: the position in the brick (the placement undone) along the way the picture runs across the face and up it
    // (the image's own rows go downward). It is worked out here and not passed from the vertex shader, because a pair more to interpolate costs a tenth of a frame
    // on the integrated GPU. The picture's own bits say the ways, which are of no use for a face with none: that is not read.
    let in_brick = transpose(placement.rotation) * (in.relative - placement.offset.xyz) / placement.offset.w;
    let uv = vec2<f32>(along(in_brick, (in.picture >> 11u) & 7u), -along(in_brick, (in.picture >> 14u) & 7u));
    // (How the coordinates of the picture change across a pixel is taken here, before anything branches: sampling with them is allowed anywhere, taking them is not.)
    let picture_dx = dpdx(uv);
    let picture_dy = dpdy(uv);
    var albedo = in.albedo;
    if (in.picture != 0u && frame.pictures.y > 0.0) {
        let largest_part = max(abs(in.relative.x), max(abs(in.relative.y), abs(in.relative.z)));
        let picture_distance = largest_part * length(in.relative / max(largest_part, 1.0e-30));
        // The picture is all there is up to the distance, fades into the flat colour of the face over the band that follows, and is not read beyond it.
        let share = 1.0 - smoothstep(frame.pictures.x, frame.pictures.y, picture_distance);
        if (share > 0.0) {
            albedo = mix(albedo, sample_picture(in.picture, uv, picture_dx, picture_dy), share);
        }
    }
    return albedo;
}
