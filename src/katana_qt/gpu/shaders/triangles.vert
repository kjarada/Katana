#version 440
#extension GL_GOOGLE_include_directive : require
// kTrianglesVertex (shader_library.cpp). Colours arrive as the draw list's
// 0xAARRGGBB read as UNORM bytes in memory order - B, G, R, A - so .zyxw puts
// them back.
#include "frame.glsl"

layout(location = 0) in vec3 pos;
layout(location = 1) in vec4 color;

layout(location = 0) out vec4 vColor;
layout(location = 1) out vec3 vRel;

void main()
{
    vec3 p = exaggerate(pos);
    gl_Position = mvp * vec4(p, 1.0);
    vColor = color.zyxw;
    vRel = p;
}
