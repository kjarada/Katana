#version 440
#extension GL_GOOGLE_include_directive : require
// kPointsInstancedVertex (shader_library.cpp).
#include "frame.glsl"
#include "quad.glsl"

layout(location = 0) in vec3 pos;
layout(location = 1) in vec4 color;
layout(location = 2) in float size;

layout(location = 0) out vec4 vColor;
layout(location = 1) noperspective out vec2 vLocal;
layout(location = 2) flat out vec2 vShape;

// As for lines: the strip's two triangles, split the same way.
const vec2 kCorners[6] = vec2[6](vec2(-1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, -1.0),
                                 vec2(1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, 1.0));

void main()
{
    float halfSize = max(size, 1.0) * params.z * 0.5;
    vec4 c = pulledTowardsEye(mvp * vec4(exaggerate(pos), 1.0), halfSize + 0.5);
    QuadVertex q = spriteHidden(c, halfSize)
                       ? hiddenCorner()
                       : spriteCorner(c, color.zyxw, halfSize, kCorners[gl_VertexIndex]);
    gl_Position = q.pos;
    vColor = q.color;
    vLocal = q.local;
    vShape = q.shape;
}
