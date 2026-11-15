// The point clouds: cloud_instanced.vert and cloud.frag
// (kCloudInstancedVertex and kCloudFragment in shader_library.cpp). Joined
// after frame.wgsl and quad.wgsl. Every cloud point takes params.w logical
// pixels; one instance of six vertices per point.

struct CloudIn {
    @builtin(vertex_index) corner: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(input: CloudIn) -> QuadVertex {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let half_size = max(frame.params.w, 1.0) * frame.params.z * 0.5;
    let c = pulled_towards_eye(frame.mvp * vec4<f32>(exaggerate(input.position), 1.0), half_size + 0.5);
    if (sprite_hidden(c, half_size)) {
        return hidden_corner();
    }
    return sprite_corner(c, input.color.zyxw, half_size, corners[input.corner]);
}

// Round, all the same screen size (params.w logical pixels).
@fragment
fn fs_main(input: QuadVertex) -> @location(0) vec4<f32> {
    let alpha = input.color.a * clamp(input.shape.x + 0.5 - length(input.local), 0.0, 1.0);
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(input.color.rgb, alpha);
}
