// The default shader pack: how the colour of a face is made of what the engine knows of it (`Surface`, in `voxel.wgsl`). A pack of the player's is a file like this one, and
// defines `pack_shade` too (`docs/specs/shader-pack-spec.md`).
//
// The sky's light lights every face the sun can reach, as much as its side allows, like another lamp; at night the light of the stars lights what the sun does not reach, where the
// sky is open, as the sky's light does by day; the sun lights the faces that look at it, by the cosine of the angle; and the lights combine like a screen blend: each is the share
// of full brightness, and what is missing from both is what is missing from the result.
fn pack_shade(s: Surface) -> vec3<f32> {
    let sky = frame.sun_light.y * s.sky_visible + frame.heavens.w * (1.0 - s.above_horizon) * s.uncovered;
    let shaded = s.side * (s.lamp + sky * (1.0 - s.lamp));
    let sun = frame.sun_light.x * max(dot(s.normal, frame.sun_direction.xyz), 0.0) * s.sun_visible;
    return s.albedo * (shaded + sun * (1.0 - shaded));
}
