// The fragment shader's hook for pictures when no block has any: the face is its own colour, and nothing here uses a texture or a sampler.
// (This is a separate shader, and not a branch that is never taken, because a shader that merely uses the pictures' bindings made the first pictures of a fresh renderer
// on the integrated AMD GPU go wrong in tests that never draw a picture; see `docs/test-flakes.md`.)

fn picture_albedo(in: VertexOutput) -> vec4<f32> {
    return vec4<f32>(in.albedo, 1.0);
}

// What the specular picture says of the pixel (smoothness, reflectance, metal, emission, subsurface scattering): nothing (there are no pictures, so no specular pictures).
fn picture_material(in: VertexOutput) -> PictureMaterial {
    return PictureMaterial(0.0, 0.0, 0.0, 0.0, 0.0);
}

// How much the normal picture tilts the face's normal, and how much of the ambient light gets to the pixel: no tilt and all of it (there are no pictures, so no normal pictures).
fn picture_bump(in: VertexOutput) -> vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

// Where the shadow of the face is looked up: where the pixel is (there are no pictures to be wholly in the light or in the shade).
fn pixelated_place(in: VertexOutput) -> vec3<f32> {
    return in.relative;
}
