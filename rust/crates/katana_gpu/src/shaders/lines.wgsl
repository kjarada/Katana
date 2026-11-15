// The lines: lines_instanced.vert and lines.frag (kLinesInstancedVertex and
// kLinesFragment in shader_library.cpp). Joined after frame.wgsl and
// quad.wgsl. One instance of six vertices per line, one corner per
// invocation; the line's two 20-byte ends are its 40-byte instance.

struct LineIn {
    @builtin(vertex_index) corner: u32,
    @location(0) a: vec3<f32>,
    @location(1) color_a: vec4<f32>,
    @location(2) width_a: f32,
    @location(3) b: vec3<f32>,
    @location(4) color_b: vec4<f32>,
    // The same as width_a: both ends carry it.
    @location(5) width_b: f32,
}

@vertex
fn vs_main(input: LineIn) -> QuadVertex {
    // The quad's strip (A-, A+, B-, B+) as two triangles, split along the
    // C++'s diagonal, so the pixels are those its two expansions drew.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let ca = frame.mvp * vec4<f32>(exaggerate(input.a), 1.0);
    let cb = frame.mvp * vec4<f32>(exaggerate(input.b), 1.0);
    let s = setup_line(ca, cb, input.color_a.zyxw, input.color_b.zyxw, input.width_a);
    if (s.visible < 0.5) {
        return hidden_corner();
    }
    return line_corner(s, corners[input.corner]);
}

// Coverage of the pixel by the line's rectangle, as the distance of the pixel
// centre inside the edge plus half a pixel, clamped to [0, 1]: exact for an
// edge crossing the pixel straight, and the usual analytic approximation
// otherwise. Multiplied into alpha and blended; 4x MSAA handles the
// silhouettes of the triangles underneath.
@fragment
fn fs_main(input: QuadVertex) -> @location(0) vec4<f32> {
    let beyond_ends = max(-input.local.x, input.local.x - input.shape.x);
    let coverage = clamp(input.shape.y + 0.5 - abs(input.local.y), 0.0, 1.0)
        * clamp(input.shape.y + 0.5 - beyond_ends, 0.0, 1.0);
    let alpha = input.color.a * coverage;
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(input.color.rgb, alpha);
}
