// Draws packed voxel quads. Each instance is one quad (two u32s, see voxel_meshing::PackedQuad);
// the vertex shader expands it into two triangles, so there are no vertex or index buffers.

struct Frame {
    view_projection: mat4x4<f32>,
    fog_color: vec4<f32>,
    // x: distance where fog starts, y: distance where terrain is fully fogged.
    fog_range: vec4<f32>,
    // Unit vector pointing towards the sun.
    sun_direction: vec4<f32>,
    // Linear RGB per material value; values beyond the palette draw magenta.
    palette: array<vec4<f32>, 256>,
}

struct Placement {
    // Position of the brick's lowest corner relative to the camera, in voxels.
    offset: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> placement: Placement;

struct Instance {
    @location(0) geometry: u32,
    @location(1) material: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec3<f32>,
    @location(1) distance: f32,
}

fn axis_unit(axis: u32) -> vec3<f32> {
    return vec3<f32>(f32(axis == 0u), f32(axis == 1u), f32(axis == 2u));
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32, instance: Instance) -> VertexOutput {
    let origin = vec3<f32>(
        f32(instance.geometry & 31u),
        f32((instance.geometry >> 5u) & 31u),
        f32((instance.geometry >> 10u) & 31u),
    );
    let width = f32(((instance.geometry >> 15u) & 31u) + 1u);
    let height = f32(((instance.geometry >> 20u) & 31u) + 1u);
    let face = (instance.geometry >> 25u) & 7u;
    let axis = face / 2u;
    let positive = (face & 1u) == 1u;

    // Corners 0..3 run counter-clockwise around the quad seen from outside for positive faces;
    // negative faces list them the other way round so they also wind counter-clockwise.
    var order = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    if (!positive) {
        order = array<u32, 6>(0u, 2u, 1u, 0u, 3u, 2u);
    }
    let corner = order[vertex_index];
    var u_extent = 0.0;
    if (corner == 1u || corner == 2u) {
        u_extent = width;
    }
    var v_extent = 0.0;
    if (corner == 2u || corner == 3u) {
        v_extent = height;
    }

    // A quad on axis a spans axes (a + 1) % 3 and (a + 2) % 3. Built from unit vectors rather than
    // by writing to a dynamically indexed component, which some shader compilers reject.
    let normal_axis = axis_unit(axis);
    let u_axis = axis_unit((axis + 1u) % 3u);
    let v_axis = axis_unit((axis + 2u) % 3u);
    let local = origin + normal_axis * select(0.0, 1.0, positive) + u_axis * u_extent + v_axis * v_extent;
    let relative = local + placement.offset.xyz;

    let normal = normal_axis * select(-1.0, 1.0, positive);
    let shade = 0.4 + 0.6 * max(dot(normal, frame.sun_direction.xyz), 0.0);

    var base = vec3<f32>(1.0, 0.0, 1.0);
    if (instance.material < 256u) {
        base = frame.palette[instance.material].rgb;
    }

    var out: VertexOutput;
    out.position = frame.view_projection * vec4<f32>(relative, 1.0);
    out.color = base * shade;
    out.distance = length(relative);
    return out;
}

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let fog = smoothstep(frame.fog_range.x, frame.fog_range.y, in.distance);
    return vec4<f32>(mix(in.color, frame.fog_color.rgb, fog), 1.0);
}
