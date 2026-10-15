// A plus sign at the centre of the screen: white with a dark outline, so it shows on any background.

struct Screen {
    size: vec2<f32>,
    padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> screen: Screen;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle that covers the whole screen.
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fragment_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let offset = abs(position.xy - screen.size * 0.5);
    let arm = max(6.0, screen.size.y / 90.0);
    let half_width = 1.0;
    let in_bar = (offset.x <= half_width && offset.y <= arm)
        || (offset.y <= half_width && offset.x <= arm);
    if in_bar {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
    let in_outline = (offset.x <= half_width + 1.0 && offset.y <= arm + 1.0)
        || (offset.y <= half_width + 1.0 && offset.x <= arm + 1.0);
    if !in_outline {
        discard;
    }
    return vec4<f32>(0.0, 0.0, 0.0, 0.7);
}
