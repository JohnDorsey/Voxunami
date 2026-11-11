// No shadows: the sun and the sky light every face that looks their way, whatever is in the way, and the horizon of the planet still hides them at night. The engine has
// worked out the terrain's shadow (`s.sun_visible`, `s.sky_visible`, `s.uncovered`); this pack does not look at it.
fn pack_shade(s: Surface) -> vec3<f32> {
    let sky = frame.sun_light.y * s.above_horizon + frame.heavens.w * (1.0 - s.above_horizon);
    let shaded = s.side * (s.lamp + sky * (1.0 - s.lamp));
    let sun = frame.sun_light.x * max(dot(s.normal, frame.sun_direction.xyz), 0.0) * s.above_horizon;
    return s.albedo * (shaded + sun * (1.0 - shaded));
}
