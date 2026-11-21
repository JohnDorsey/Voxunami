// The relief of a picture drawn as parallax occlusion (`Relief::ParallaxOcclusion`; `docs/history/parallax-spike.md`, where it was the lumpy one): the heights of the normal picture
// (its alpha, 255 being the face itself and 0 the lowest, `RELIEF_DEPTH` below it) are taken as a smooth surface, the four pixels round a place mixed as a bilinear filter would
// (the sampler of the pictures is nearest when magnifying, and the pixel grid is what prisms keep), and the eye's ray is marched down through it in equal steps. The picture is read
// where the ray first is under the surface, found between the last two steps by interpolation.
//
// There are no walls and no flat tops: what is seen is the picture at a shifted place, lit by the tilts of the normal picture there (`picture_bump`, with `parallax_prisms` 0), so the
// surface reads as warped and smoothly shaded. The ray, the answers `picture_prepare` leaves and the fade with the screen's pixels are in `relief_ray.wgsl`.
//
// The steps are equal and few, so a relief deeper than about 0.15 voxel at a low eye shows them as streaks; `RELIEF_DEPTH` is a setting for the eye to find the edge of.

const OCCLUSION_STEPS: i32 = 12;

// The height (0 to 1) of the picture at a place as a smooth surface: the four pixels round it mixed as a bilinear filter would.
fn smooth_height(picture: u32, coordinates: vec2<f32>, pixels: f32) -> f32 {
    let heights = gather_heights(picture, coordinates);
    let within = fract(fract(coordinates) * pixels - vec2<f32>(0.5));
    return mix(mix(heights.w, heights.z, within.x), mix(heights.x, heights.y, within.x), within.y);
}

fn picture_prepare(in: VertexOutput) {
    let ray = relief_ray(in);
    if (!ray.found) {
        return;
    }
    // `previous_gap` is how far the ray is above the surface at the step before (the ray starts at the top, at the face); the first step where it is not above is the hit, and the
    // place between that step and the one before it where the two meet is found as a line from one gap to the other.
    var previous_gap = 1.0 - smooth_height(in.picture, in.uv, ray.pixels);
    var hit = 1.0;
    for (var index = 1; index <= OCCLUSION_STEPS; index = index + 1) {
        let fraction = f32(index) / f32(OCCLUSION_STEPS);
        let gap = (1.0 - fraction) - smooth_height(in.picture, in.uv + ray.travel * (ray.depth * fraction), ray.pixels);
        if (gap <= 0.0) {
            hit = fraction - (1.0 - previous_gap / max(previous_gap - gap, 1.0e-6)) / f32(OCCLUSION_STEPS);
            break;
        }
        previous_gap = gap;
    }
    parallax_shift = ray.travel * (ray.depth * hit);
}
