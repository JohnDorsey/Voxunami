// The fragment shader's hook for pictures when no block has any: the face is its own colour, and nothing here uses a texture or a sampler.
// (This is a separate shader, and not a branch that is never taken, because a shader that merely uses the pictures' bindings made the first pictures of a fresh renderer
// on the integrated AMD GPU go wrong in tests that never draw a picture; see `docs/test-flakes.md`.)

fn picture_albedo(in: VertexOutput) -> vec3<f32> {
    return in.albedo;
}
