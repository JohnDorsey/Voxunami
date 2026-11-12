// The part of the walk of the sun's ray (`contact_on.wgsl`) that goes through the eighths of a voxel that blocks the sun in part, when no brick holds such voxels: there are none
// to find, and nothing here uses a buffer or adds code to the walk (`shapes_on.wgsl` is the real one).

fn partial_number(entry: u32, cell: vec3<i32>) -> u32 {
    return 0u;
}

fn partial_blocks(shape: u32, position: vec3<f32>, toward_sun: vec3<f32>, start: vec3<i32>, test_first: bool) -> bool {
    return false;
}
