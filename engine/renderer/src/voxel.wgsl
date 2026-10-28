// Draws packed voxel quads. Each instance is one quad; the vertex shader expands it into two
// triangles, so there are no vertex or index buffers. Two kinds of quad share the fragment shader:
//
//  * whole-voxel quads (two u32s, see voxel_meshing::PackedQuad): a rectangle of whole voxel faces
//    on a voxel boundary;
//  * detailed quads (four u32s, see voxel_meshing::PackedDetailedQuad): a rectangle at any eighth of
//    a voxel, for blocks that do not fill their cell.
//
// Both carry the block (an index into the palette) and a brightness from 0 to 255.

// One cascade of the shadow map: a picture of the terrain from the sun, of a square across the light round the camera.
struct Cascade {
    // From a place relative to the camera to the map: x and y across it (-1 to 1), z the depth (0 to 1, the sun's side deeper).
    matrix: mat4x4<f32>,
    // x: how far from the camera a place may be to be looked up in this cascade, y: the width of a texel in voxels, z: the distance in voxels from depth 0 to depth 1.
    limits: vec4<f32>,
}

struct Frame {
    view_projection: mat4x4<f32>,
    fog_color: vec4<f32>,
    // x: distance where fog starts, y: distance where terrain is fully fogged, z: ambient light
    // (0 to 1), the brightness of a face that no light reaches.
    fog_range: vec4<f32>,
    // Unit vector pointing towards the sun.
    sun_direction: vec4<f32>,
    // Unit vector the sides of faces are shaded by: the sun's, unless the atmosphere says otherwise.
    side_direction: vec4<f32>,
    // x: how bright the sun lights a face that looks straight at it (0 to 1), y: how bright the sky lights every face the sun can reach (0 to 1), z: 1 if the level's
    // ground is flat (the sun is hidden whenever it is below the horizon, y being up), w: how many of the balls below are used.
    sun_light: vec4<f32>,
    // The balls (planets, moons) that can hide the sun: the middle relative to the camera, in voxels (xyz), and the radius (w).
    sun_balls: array<vec4<f32>, 8>,
    // x: how many cascades of shadow map there are (0: the terrain casts no shadows), y: the share of the sky's light that reaches a place the terrain puts in shadow (0 to 1).
    shadow: vec4<f32>,
    // The cascades of the shadow map, finest first.
    cascades: array<Cascade, 11>,
    // Linear RGB of each avatar colour.
    avatars: array<vec4<f32>, 16>,
}

// The material value of the first avatar colour: the last 16 values a quad can carry (see AVATAR_MATERIAL_BASE in the renderer).
const AVATAR_BASE: u32 = 0xFFFFF0u;

struct Placement {
    // Position of the brick's lowest corner relative to the camera, in voxels (xyz), and how many voxels a unit of the mesh is (w: 1 for a brick, 2^n for
    // a coarse node of detail level n, whose mesh is in cells).
    offset: vec4<f32>,
    // How the grid the brick is in is turned (the identity for terrain): a position in the brick, turned, plus the offset, is where
    // it is relative to the camera.
    rotation: mat3x3<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
// Linear RGB per game material value; values beyond the palette draw magenta.
@group(0) @binding(1) var<storage, read> palette: array<vec4<f32>>;
// The cascades of the shadow map, a layer each, and the sampler that compares a depth with what a layer holds.
@group(0) @binding(2) var shadow_maps: texture_depth_2d_array;
@group(0) @binding(3) var shadow_sampler: sampler_comparison;
@group(1) @binding(0) var<uniform> placement: Placement;

struct Instance {
    @location(0) geometry: u32,
    // Bits 0-23: the block. Bits 24-31: the light.
    @location(1) material: u32,
}

struct DetailedInstance {
    // Origin x (bits 0-15) and y (16-31), in eighths of a voxel.
    @location(0) first: u32,
    // Origin z and width.
    @location(1) second: u32,
    // Height (0-15), face (16-23), light (24-31).
    @location(2) third: u32,
    @location(3) material: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    // The colour of the block.
    @location(0) @interpolate(flat) albedo: vec3<f32>,
    @location(1) distance: f32,
    // The face's normal, turned to the world's.
    @location(2) @interpolate(flat) normal: vec3<f32>,
    // Where the point is relative to the camera, in voxels.
    @location(3) relative: vec3<f32>,
    // x: the light of the lamps and the ambient light (0 to 1), y: how bright the side of the face makes it (0.4 to 1).
    @location(4) @interpolate(flat) lamp_and_side: vec2<f32>,
}

// How wide, in radians, the soft edge of a shadow is: the sun's disc, and the twilight of the air for a place at the ground of a ball. The same numbers as `sun.rs`.
const SUN_DISC_SOFTNESS: f32 = 0.005;
const TWILIGHT_SOFTNESS: f32 = 0.1;
// The most `asin` is asked about: its answer at exactly 1 is not a number on some GPUs (the integrated AMD one, in DX12). A place on or in the ball is as near the surface as this.
const MOST_OF_A_BALL: f32 = 0.999999;

// How far over the surface of a ball, in its radii, a face is still turned all the way to the way out of the ball, and how far it is no longer turned (see `Sunlight` in
// `sun.rs`).
const TURNED_UP_TO: f32 = 0.01;
const TURNED_NO_MORE_FROM: f32 = 0.03;

// How the shadow of the terrain is looked up. The same numbers as `shadow.rs`.
const BLEND_FROM: f32 = 0.85;
const DEPTH_BIAS_TEXELS: f32 = 1.0;
const FACING_FULL_FROM: f32 = 0.15;

struct Sunlight {
    // How much of the sun reaches a place: 1 for all of it, 0 for none.
    visible: f32,
    // The normal the cosine of the light is taken of.
    normal: vec3<f32>,
}

// What the sun does at a place (relative to the camera) for a face that looks along `normal`. The same arithmetic as `sunlight` in `sun.rs`.
fn sunlight(position: vec3<f32>, normal: vec3<f32>) -> Sunlight {
    let sun = frame.sun_direction.xyz;
    var visible = 1.0;
    var nearest_over = 1.0e30;
    var way_out = vec3<f32>(0.0);
    if (frame.sun_light.z > 0.5) {
        let softness = SUN_DISC_SOFTNESS + TWILIGHT_SOFTNESS;
        visible = smoothstep(-softness, softness, sun.y);
    }
    let count = u32(frame.sun_light.w);
    for (var index = 0u; index < count; index = index + 1u) {
        let ball = frame.sun_balls[index];
        let to_ball = ball.xyz - position;
        // The length from the vector scaled by its largest part: the square of a part of 10^20 or more does not fit an `f32`.
        let largest = max(abs(to_ball.x), max(abs(to_ball.y), abs(to_ball.z)));
        if (largest == 0.0) {
            return Sunlight(0.0, normal);
        }
        let scaled = to_ball / largest;
        let scaled_length = length(scaled);
        let true_distance = largest * scaled_length;
        let over = (true_distance - ball.w) / ball.w;
        if (over < nearest_over) {
            nearest_over = over;
            way_out = -(scaled / scaled_length);
        }
        // A place inside the ball is taken to be on its surface, in its direction: a face of a far node of a planet may be a little below the ground, inside the core, and
        // it is lit by the terminator of the planet like the ground over it.
        let ball_distance = max(true_distance, ball.w);
        let share = ball.w / ball_distance;
        let reach = asin(min(share, MOST_OF_A_BALL));
        let separation = acos(clamp(dot(scaled / scaled_length, sun), -1.0, 1.0));
        let share_squared = share * share;
        let softness = SUN_DISC_SOFTNESS + TWILIGHT_SOFTNESS * share_squared * share_squared * share_squared * share_squared;
        // A ball that takes up less of the sky than the soft edge is wide hides only part of the sun's disc, by the share of the area.
        let coverage = min(reach / softness, 1.0);
        visible = visible * (1.0 - coverage * coverage * (1.0 - smoothstep(reach - softness, reach + softness, separation)));
    }
    var lit_normal = normal;
    if (nearest_over < 1.0e29) {
        let turn = 1.0 - smoothstep(TURNED_UP_TO, TURNED_NO_MORE_FROM, nearest_over);
        let turned = normal + way_out * turn;
        let turned_length = length(turned);
        if (turned_length > 1.0e-3) {
            lit_normal = turned / turned_length;
        }
    }
    return Sunlight(visible, lit_normal);
}

fn axis_unit(axis: u32) -> vec3<f32> {
    return vec3<f32>(f32(axis == 0u), f32(axis == 1u), f32(axis == 2u));
}

// Which corner of the quad (0 to 3, counter-clockwise seen from outside for positive faces) each of
// the six vertices of its two triangles is; negative faces list them the other way round so they
// also wind counter-clockwise.
fn corner_of(vertex_index: u32, positive: bool) -> u32 {
    var order = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    if (!positive) {
        order = array<u32, 6>(0u, 2u, 1u, 0u, 3u, 2u);
    }
    return order[vertex_index];
}

// The position of a corner of a quad whose lowest corner is `origin` (in voxels) on the plane of
// its face, `width` along the first axis it spans and `height` along the second. A quad on axis a
// spans axes (a + 1) % 3 and (a + 2) % 3. Built from unit vectors rather than by writing to a
// dynamically indexed component, which some shader compilers reject.
fn corner_position(vertex_index: u32, face: u32, origin: vec3<f32>, width: f32, height: f32) -> vec3<f32> {
    let axis = face / 2u;
    let corner = corner_of(vertex_index, (face & 1u) == 1u);
    var u_extent = 0.0;
    if (corner == 1u || corner == 2u) {
        u_extent = width;
    }
    var v_extent = 0.0;
    if (corner == 2u || corner == 3u) {
        v_extent = height;
    }
    return origin + axis_unit((axis + 1u) % 3u) * u_extent + axis_unit((axis + 2u) % 3u) * v_extent;
}

fn finish(relative: vec3<f32>, face: u32, material: u32, light: u32) -> VertexOutput {
    // The face's normal in the grid's own axes, turned to the world's, where the sun is.
    let normal = placement.rotation * (axis_unit(face / 2u) * select(-1.0, 1.0, (face & 1u) == 1u));
    // The light of the face and the ambient light combine like a screen blend: each is the share of
    // full brightness, and what is missing from both is what is missing from the result. (The sun's light and
    // the sky's are added to it for each pixel, in `fragment_main`.)
    let lit = 1.0 - (1.0 - frame.fog_range.z) * (1.0 - f32(light) / 255.0);
    let side = 0.4 + 0.6 * max(dot(normal, frame.side_direction.xyz), 0.0);

    var base = vec3<f32>(1.0, 0.0, 1.0);
    if (material >= AVATAR_BASE) {
        base = frame.avatars[material - AVATAR_BASE].rgb;
    } else if (material < arrayLength(&palette)) {
        base = palette[material].rgb;
    }

    var out: VertexOutput;
    out.position = frame.view_projection * vec4<f32>(relative, 1.0);
    out.albedo = base;
    out.normal = normal;
    out.relative = relative;
    out.lamp_and_side = vec2<f32>(lit, side);
    // The distance, which for what is 10^20 voxels away or more is a square that does not fit an `f32` (and `length` may overflow, as the standard allows): the vector is
    // scaled by its largest part first.
    let largest = max(abs(relative.x), max(abs(relative.y), abs(relative.z)));
    out.distance = select(0.0, largest * length(relative / largest), largest > 0.0);
    return out;
}

struct Corner {
    // Where the corner is relative to the camera, in voxels.
    relative: vec3<f32>,
    face: u32,
}

fn whole_corner(vertex_index: u32, instance: Instance) -> Corner {
    let origin = vec3<f32>(
        f32(instance.geometry & 31u),
        f32((instance.geometry >> 5u) & 31u),
        f32((instance.geometry >> 10u) & 31u),
    );
    let width = f32(((instance.geometry >> 15u) & 31u) + 1u);
    let height = f32(((instance.geometry >> 20u) & 31u) + 1u);
    let face = (instance.geometry >> 25u) & 7u;
    // The voxel owns its face; a positive face is on the voxel's far side.
    let plane = origin + axis_unit(face / 2u) * select(0.0, 1.0, (face & 1u) == 1u);
    let local = corner_position(vertex_index, face, plane, width, height);
    return Corner(placement.rotation * (local * placement.offset.w) + placement.offset.xyz, face);
}

fn detailed_corner(vertex_index: u32, instance: DetailedInstance) -> Corner {
    let origin = vec3<f32>(
        f32(instance.first & 0xFFFFu),
        f32(instance.first >> 16u),
        f32(instance.second & 0xFFFFu),
    ) / 8.0;
    let width = f32(instance.second >> 16u) / 8.0;
    let height = f32(instance.third & 0xFFFFu) / 8.0;
    let face = (instance.third >> 16u) & 7u;
    let local = corner_position(vertex_index, face, origin, width, height);
    return Corner(placement.rotation * (local * placement.offset.w) + placement.offset.xyz, face);
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32, instance: Instance) -> VertexOutput {
    let corner = whole_corner(vertex_index, instance);
    return finish(corner.relative, corner.face, instance.material & 0xFFFFFFu, instance.material >> 24u);
}

@vertex
fn vertex_detailed(@builtin(vertex_index) vertex_index: u32, instance: DetailedInstance) -> VertexOutput {
    let corner = detailed_corner(vertex_index, instance);
    return finish(corner.relative, corner.face, instance.material & 0xFFFFFFu, instance.third >> 24u);
}

// The same quads for the shadow map: where they are in the cascade (the frame's matrix is the cascade's) and nothing else.
@vertex
fn shadow_vertex_main(@builtin(vertex_index) vertex_index: u32, instance: Instance) -> @builtin(position) vec4<f32> {
    return frame.view_projection * vec4<f32>(whole_corner(vertex_index, instance).relative, 1.0);
}

@vertex
fn shadow_vertex_detailed(@builtin(vertex_index) vertex_index: u32, instance: DetailedInstance) -> @builtin(position) vec4<f32> {
    return frame.view_projection * vec4<f32>(detailed_corner(vertex_index, instance).relative, 1.0);
}

// The share of the sun that the terrain lets through to a place, in one cascade of the map: 1 if nothing is nearer the sun than the place in its texel, 0 if something is
// (between them for a texel that is part one, part the other). The same arithmetic as `Cascade::lookup` in `shadow.rs`.
fn shadow_in_cascade(index: u32, position: vec3<f32>) -> f32 {
    let cascade = frame.cascades[index];
    let texel = cascade.limits.y;
    let clip = cascade.matrix * vec4<f32>(position, 1.0);
    let reference = clip.z + DEPTH_BIAS_TEXELS * texel / cascade.limits.z;
    let coordinates = vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    return textureSampleCompareLevel(shadow_maps, shadow_sampler, coordinates, index, reference);
}

// The share of the sun the terrain lets through to a place `distance` from the camera. The finest cascade that holds the place is looked in, mixed with the next over the last of its reach (and a place beyond the last cascade is not in shadow). The
// same as `cascade_for` in `shadow.rs`.
fn terrain_light(position: vec3<f32>, distance: f32) -> f32 {
    let count = u32(frame.shadow.x);
    var chosen = count;
    var blend = 0.0;
    for (var index = 0u; index < count; index = index + 1u) {
        // (A cascade that is not used has a reach of 0.)
        let reach = frame.cascades[index].limits.x;
        if (distance < reach) {
            chosen = index;
            blend = smoothstep(BLEND_FROM * reach, reach, distance);
            break;
        }
    }
    if (chosen == count) {
        return 1.0;
    }
    var light = shadow_in_cascade(chosen, position);
    if (blend > 0.0) {
        var next = 1.0;
        if (chosen + 1u < count && frame.cascades[chosen + 1u].limits.x > 0.0) {
            next = shadow_in_cascade(chosen + 1u, position);
        }
        light = mix(light, next, blend);
    }
    return light;
}

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let lamp = in.lamp_and_side.x;
    let side = in.lamp_and_side.y;
    var shaded = side * lamp;
    var sun = 0.0;
    if (frame.sun_light.x > 0.0 || frame.sun_light.y > 0.0) {
        // The sky lights every face the sun can reach, as much as its side allows, like another lamp; the sun lights the faces that look at it, by the cosine of the
        // angle; and the sun's light and what there was combine like a screen blend too.
        let light = sunlight(in.relative, in.normal);
        var sun_visible = light.visible;
        var sky_visible = light.visible;
        if (frame.shadow.x > 0.5) {
            // The terrain's shadow: on the sun's light as it is; on the sky's light only in part (a place in a shadow has the sky's light still, and the more the less of the sky
            // is hidden, which this does not know), and a face turned from the sun is in the shade of its own body, which fades in as the sun crosses its plane.
            let cosine = dot(light.normal, frame.sun_direction.xyz);
            var terrain = 0.0;
            if (cosine > 0.0) {
                // (The distance of this pixel, not the one the vertices interpolate: that is wrong across the big quads of coarse nodes, where it chooses a cascade too coarse.)
                let largest = max(abs(in.relative.x), max(abs(in.relative.y), abs(in.relative.z)));
                let distance = largest * length(in.relative / max(largest, 1.0e-30));
                terrain = terrain_light(in.relative, distance);
            }
            sun_visible = sun_visible * terrain;
            sky_visible = sky_visible * mix(frame.shadow.y, 1.0, terrain * smoothstep(0.0, FACING_FULL_FROM, cosine));
        }
        let sky = frame.sun_light.y * sky_visible;
        shaded = side * (lamp + sky * (1.0 - lamp));
        sun = frame.sun_light.x * max(dot(light.normal, frame.sun_direction.xyz), 0.0) * sun_visible;
    }
    let color = in.albedo * (shaded + sun * (1.0 - shaded));
    let fog = smoothstep(frame.fog_range.x, frame.fog_range.y, in.distance);
    return vec4<f32>(mix(color, frame.fog_color.rgb, fog), 1.0);
}
