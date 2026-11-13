// The default shader pack: how the colour of a face is made of what the engine knows of it (`Surface`, in `voxel.wgsl`). A pack of the player's is a file like this one, and
// defines `pack_shade` too (`docs/specs/shader-pack-spec.md`).
//
// The sky's light lights every face the sun can reach, as much as its side allows, like another lamp; at night the light of the stars lights what the sun does not reach, where the
// sky is open, as the sky's light does by day; the sun lights the faces that look at it, by the cosine of the angle (of the normal the picture's bumps tilt: `shading_normal`);
// the picture's ambient occlusion cuts the lamps' light, the ambient light and the sky's (not the sun's: `ambient_occlusion`);
// and the sun's glint (`sun_glint`) is added on top, where the picture says a pixel is smooth and throws light back;
// and the lights combine like a screen blend: each is the share
// of full brightness, and what is missing from both is what is missing from the result.
// The sun's glint on a smooth pixel: the share of the sun's light that the pixel throws straight to the eye, where the half-way vector between the sun and the eye meets the
// shading normal. A microfacet rule of the usual kind: the width of the patch `alpha` is the square of the roughness (one minus the smoothness, and no narrower than 0.03, which
// is a mirror as far as a screen can tell), the patch is `alpha^2 / (pi ((n.h)^2 (alpha^2 - 1) + 1)^2)`, what is thrown back is that times the Fresnel term (Schlick's, from the
// reflectance, or from the pixel's own colour for a metal) over four times the square of the sun's angle to the half-way vector, times the cosine of the sun's angle to the
// normal. The sun alone glints (the lamps and the sky have no direction to be thrown from); the face's own normal must look at the sun, so that a bump does not throw light
// from the far side of a wall; and the shadow and the horizon cut it as they cut the sun.
fn sun_glint(s: Surface) -> vec3<f32> {
    if (s.reflectance <= 0.0 && s.metal <= 0.0) {
        return vec3<f32>(0.0);
    }
    let to_the_sun = frame.sun_direction.xyz;
    if (dot(s.normal, to_the_sun) <= 0.0 || s.distance <= 0.0) {
        return vec3<f32>(0.0);
    }
    let to_the_eye = -s.relative / s.distance;
    let halfway = normalize(to_the_sun + to_the_eye);
    let along_normal = max(dot(s.shading_normal, halfway), 0.0);
    let sun_to_halfway = max(dot(to_the_sun, halfway), 0.0);
    let roughness = 1.0 - s.smoothness;
    let width = max(roughness * roughness, 0.03);
    let width_squared = width * width;
    let spread = along_normal * along_normal * (width_squared - 1.0) + 1.0;
    let patch_strength = width_squared / (3.14159265 * spread * spread);
    let straight_on = mix(vec3<f32>(s.reflectance), s.albedo, s.metal);
    let fresnel = straight_on + (vec3<f32>(1.0) - straight_on) * pow(1.0 - sun_to_halfway, 5.0);
    let visibility = 0.25 / max(sun_to_halfway * sun_to_halfway, 0.01);
    let cosine = max(dot(s.shading_normal, to_the_sun), 0.0);
    return fresnel * (patch_strength * visibility * cosine * frame.sun_light.x * s.sun_visible);
}

fn pack_shade(s: Surface) -> vec3<f32> {
    let sky = frame.sun_light.y * s.sky_visible + frame.heavens.w * (1.0 - s.above_horizon) * s.uncovered;
    let shaded = s.ambient_occlusion * s.side * (s.lamp + sky * (1.0 - s.lamp));
    let sun = frame.sun_light.x * max(dot(s.shading_normal, frame.sun_direction.xyz), 0.0) * s.sun_visible;
    let lit = s.albedo * (shaded + sun * (1.0 - shaded)) + sun_glint(s);
    // What shines by itself shows its colour whatever the light: the pixel is its colour by the share it shines, and lit as it was by the rest.
    return mix(lit, s.albedo, s.emission);
}
