// Flat rectangles in pixels of the screen, from the top left, one instance each.

struct Screen {
    size: vec2<f32>,
    padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> screen: Screen;

struct Instance {
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
}

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vertex_main(@builtin(vertex_index) index: u32, instance: Instance) -> Vertex {
    // Two triangles: the corners of a unit square.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    let pixel = instance.rect.xy + corners[index] * instance.rect.zw;
    var out: Vertex;
    out.position = vec4<f32>(
        pixel.x / screen.size.x * 2.0 - 1.0,
        1.0 - pixel.y / screen.size.y * 2.0,
        0.0,
        1.0,
    );
    out.color = instance.color;
    return out;
}

@fragment
fn fragment_main(vertex: Vertex) -> @location(0) vec4<f32> {
    return vertex.color;
}
