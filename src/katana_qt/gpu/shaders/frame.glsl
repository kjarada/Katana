// The frame's constants, read by every stage: the GLSL twin of
// KATANA_GPU_FRAME_CBUFFER in shader_library.cpp, field for field, and of
// FrameUniforms in gpu_renderer.cpp. vec4s only, so std140 puts each exactly
// where C++ does. mat4 is column-major in std140 as the HLSL cbuffer's
// float4x4 is by default, so `mvp * v` here is `mul(mvp, v)` there, on the
// same bytes (toFloatColumnMajor).
//
// These files are the Vulkan build's shaders (docs/gpu.md, "Shaders"): qsb
// compiles them to SPIR-V at build time. They must draw what the HLSL draws;
// a change to one belongs in the other.

layout(std140, binding = 0) uniform Frame
{
    mat4 mvp;         // origin-relative world -> clip, reversed Z
    vec4 viewport;    // xy: target size in device pixels, zw: 1 / size
    vec4 light;       // xyz: unit vector towards the light (world), w: ambient
    vec4 eyeRel;      // xyz: eye relative to the origin, w: 1 perspective
    vec4 forwardMode; // xyz: unit view direction, w: 1 = light per pixel
    vec4 params;      // x: exaggeration, y: its datum rel. origin, z: device
                      // pixels per logical pixel, w: cloud point size
    vec4 markPull;    // x: pull per pixel of reach (perspective: a fraction
                      // of the depth; orthographic: world units), y: clip z
                      // of the eye (perspective) or of the view direction
};

vec3 exaggerate(vec3 p)
{
    p.z = params.y + (p.z - params.y) * params.x;
    return p;
}
