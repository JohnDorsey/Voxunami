// The part of the walk of the sun's ray (`contact_on.wgsl`) that tests it against the boxes of entities, when none is set: nothing here adds code to the walk (`entities_on.wgsl` is the
// real one; a fragment shader that has code is slower even where there is nothing for it to do).

fn entity_boxes_block(in: VertexOutput, sun: vec3<f32>, cells: u32) -> bool {
    return false;
}
