// The frame's constants, read by every stage: the WGSL twin of frame.glsl
// (src/katana_qt/gpu/shaders) and of KATANA_GPU_FRAME_CBUFFER in
// shader_library.cpp, field for field, and of FrameUniforms in gpu_renderer.rs.
// vec4s only, so the uniform layout puts each exactly where the Rust writes
// it; mat4x4 is column-major, so `frame.mvp * v` here is `mvp * v` in the GLSL
// and `mul(mvp, v)` in the HLSL, on the same bytes (to_float_column_major).
//
// Every program module is this file, then quad.wgsl where the program widens
// lines or points, then the program's own file (shader_library.rs): WGSL has
// no include, so the pieces are joined as the GLSL's #include joined them.

struct Frame {
    // origin-relative world -> clip, reversed Z
    mvp: mat4x4<f32>,
    // xy: target size in device pixels, zw: 1 / size
    viewport: vec4<f32>,
    // xyz: unit vector towards the light (world), w: ambient
    light: vec4<f32>,
    // xyz: eye relative to the origin, w: 1 perspective
    eye_rel: vec4<f32>,
    // xyz: unit view direction, w: 1 = light per pixel
    forward_mode: vec4<f32>,
    // x: exaggeration, y: its datum rel. origin, z: device pixels per logical
    // pixel, w: cloud point size
    params: vec4<f32>,
    // x: pull per pixel of reach (perspective: a fraction of the depth;
    // orthographic: world units), y: clip z of the eye (perspective) or of the
    // view direction
    mark_pull: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;

fn exaggerate(position: vec3<f32>) -> vec3<f32> {
    var p = position;
    p.z = frame.params.y + (p.z - frame.params.y) * frame.params.x;
    return p;
}
