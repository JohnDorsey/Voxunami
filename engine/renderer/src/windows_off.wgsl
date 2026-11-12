// The part of the walk of the sun's ray (`contact_on.wgsl`) that walks the windows of the other grids, when only one window is traced: a face has nothing but its own grid's voxels
// to meet, and nothing here adds code to the walk (`windows_on.wgsl` is the real one; it is a good deal of code, and a fragment shader that has it is slower even where there is
// nothing for it to do).

fn other_windows_block(in: VertexOutput, sun: vec3<f32>, cells: u32) -> bool {
    return false;
}
