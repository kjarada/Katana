// The draw list's points: points_instanced.vert and points.frag
// (kPointsInstancedVertex and kPointsFragment in shader_library.cpp). Joined
// after frame.wgsl and quad.wgsl. Each point carries its size; one instance
// of six vertices per point.

struct PointIn {
    @builtin(vertex_index) corner: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) size: f32,
}

@vertex
fn vs_main(input: PointIn) -> QuadVertex {
    // As for lines: the strip's two triangles, split the same way.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let half_size = max(input.size, 1.0) * frame.params.z * 0.5;
    let c = pulled_towards_eye(frame.mvp * vec4<f32>(exaggerate(input.position), 1.0), half_size + 0.5);
    if (sprite_hidden(c, half_size)) {
        return hidden_corner();
    }
    return sprite_corner(c, input.color.zyxw, half_size, corners[input.corner]);
}

// size x size squares centred on the point, as the software path draws them,
// with an antialiased edge.
@fragment
fn fs_main(input: QuadVertex) -> @location(0) vec4<f32> {
    let inside = input.shape.x + 0.5 - abs(input.local);
    let alpha = input.color.a * clamp(inside.x, 0.0, 1.0) * clamp(inside.y, 0.0, 1.0);
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(input.color.rgb, alpha);
}
