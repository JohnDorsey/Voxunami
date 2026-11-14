// The relief of a picture drawn flat (`Relief::Flat`): the picture is read where the eye's ray meets the face, nothing is shifted and no wall is drawn. These are the names
// `pictures_on.wgsl` reads the relief's answers by, as constants, so that the shader with no prisms has nothing of them to run; `relief_prisms.wgsl` has them as variables.

const parallax_prisms: f32 = 0.0;
const parallax_shift: vec2<f32> = vec2<f32>(0.0);
const parallax_wall: vec3<f32> = vec3<f32>(0.0);
const parallax_height: f32 = 1.0;
const parallax_right: vec3<f32> = vec3<f32>(0.0);
const parallax_up: vec3<f32> = vec3<f32>(0.0);
const parallax_framed: bool = false;

fn picture_prepare(in: VertexOutput) {
}
