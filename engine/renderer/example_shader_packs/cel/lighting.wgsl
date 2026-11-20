// Cel shading: the sun's light in steps (`SETTING_STEPS` of them: the setting `steps` of this pack's `shaderpack.json`, three unless the player says another number). Everything else is the default's (see `default_lighting()` in the renderer: this file is that one with the step added).
fn pack_shade(s: Surface) -> vec3<f32> {
    let sky = frame.sun_light.y * s.sky_visible + frame.heavens.w * (1.0 - s.above_horizon) * s.uncovered;
    let shaded = s.side * (s.lamp + sky * (1.0 - s.lamp));
    // How much the sun lights the face, as the default has it, and then rounded to the nearest of 0, a step, two steps and so on up to 1.
    let cosine = max(dot(s.normal, frame.sun_direction.xyz), 0.0) * s.sun_visible;
    let steps = f32(SETTING_STEPS);
    let stepped = floor(cosine * steps + 0.5) / steps;
    let sun = frame.sun_light.x * stepped;
    return s.albedo * (shaded + sun * (1.0 - shaded));
}
