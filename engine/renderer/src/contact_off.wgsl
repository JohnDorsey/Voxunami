// The fragment shader's hook for the sun's exact short ray when the renderer holds no voxels that block the sun: there is nothing to walk, the sun is not blocked, and nothing here
// uses a buffer. (This is a separate shader, and not a branch that is never taken, because a binding that the default fragment shader uses made two shadow tests fail on the
// integrated AMD GPU; see `docs/test-flakes.md` and `docs/specs/contact-trace-spec.md`, 4.4.)

fn contact_light(in: VertexOutput, distance: f32) -> f32 {
    return 1.0;
}

fn contact_debug(in: VertexOutput) -> vec4<f32> {
    return vec4<f32>(0.0);
}
