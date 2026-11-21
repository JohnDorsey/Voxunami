// The fragment shader's hook for the sun's exact short ray when the renderer holds the voxels that block the sun (`volume_table.wgsl`) for the shade of the corners of faces and there are no
// shadows to correct: there is no walk, and the sun is not blocked by it.

fn contact_light(in: VertexOutput, distance: f32) -> f32 {
    return 1.0;
}

fn contact_debug(in: VertexOutput) -> vec4<f32> {
    return vec4<f32>(0.0);
}
