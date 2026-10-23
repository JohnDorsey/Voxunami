// Draws packed voxel quads. Each instance is one quad; the vertex shader expands it into two
// triangles, so there are no vertex or index buffers. Two kinds of quad share the fragment shader:
//
//  * whole-voxel quads (two u32s, see voxel_meshing::PackedQuad): a rectangle of whole voxel faces
//    on a voxel boundary;
//  * detailed quads (four u32s, see voxel_meshing::PackedDetailedQuad): a rectangle at any eighth of
//    a voxel, for blocks that do not fill their cell.
//
// Both carry the block (an index into the palette) and a brightness from 0 to 255.

struct Frame {
    view_projection: mat4x4<f32>,
    fog_color: vec4<f32>,
    // x: distance where fog starts, y: distance where terrain is fully fogged, z: ambient light
    // (0 to 1), the brightness of a face that no light reaches.
    fog_range: vec4<f32>,
    // Unit vector pointing towards the sun.
    sun_direction: vec4<f32>,
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
    @location(0) @interpolate(flat) color: vec3<f32>,
    @location(1) distance: f32,
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
    // full brightness, and what is missing from both is what is missing from the result.
    let lit = 1.0 - (1.0 - frame.fog_range.z) * (1.0 - f32(light) / 255.0);
    let shade = (0.4 + 0.6 * max(dot(normal, frame.sun_direction.xyz), 0.0)) * lit;

    var base = vec3<f32>(1.0, 0.0, 1.0);
    if (material >= AVATAR_BASE) {
        base = frame.avatars[material - AVATAR_BASE].rgb;
    } else if (material < arrayLength(&palette)) {
        base = palette[material].rgb;
    }

    var out: VertexOutput;
    out.position = frame.view_projection * vec4<f32>(relative, 1.0);
    out.color = base * shade;
    out.distance = length(relative);
    return out;
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
    // The voxel owns its face; a positive face is on the voxel's far side.
    let plane = origin + axis_unit(face / 2u) * select(0.0, 1.0, (face & 1u) == 1u);
    let local = corner_position(vertex_index, face, plane, width, height);
    return finish(placement.rotation * (local * placement.offset.w) + placement.offset.xyz, face, instance.material & 0xFFFFFFu, instance.material >> 24u);
}

@vertex
fn vertex_detailed(@builtin(vertex_index) vertex_index: u32, instance: DetailedInstance) -> VertexOutput {
    let origin = vec3<f32>(
        f32(instance.first & 0xFFFFu),
        f32(instance.first >> 16u),
        f32(instance.second & 0xFFFFu),
    ) / 8.0;
    let width = f32(instance.second >> 16u) / 8.0;
    let height = f32(instance.third & 0xFFFFu) / 8.0;
    let face = (instance.third >> 16u) & 7u;
    let local = corner_position(vertex_index, face, origin, width, height);
    return finish(placement.rotation * (local * placement.offset.w) + placement.offset.xyz, face, instance.material & 0xFFFFFFu, instance.third >> 24u);
}

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let fog = smoothstep(frame.fog_range.x, frame.fog_range.y, in.distance);
    return vec4<f32>(mix(in.color, frame.fog_color.rgb, fog), 1.0);
}
