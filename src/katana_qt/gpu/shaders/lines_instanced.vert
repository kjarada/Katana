#version 440
#extension GL_GOOGLE_include_directive : require
// kLinesInstancedVertex (shader_library.cpp): one instance of six vertices
// per line, one corner per invocation.
#include "frame.glsl"
#include "quad.glsl"

layout(location = 0) in vec3 a;
layout(location = 1) in vec4 colorA;
layout(location = 2) in float widthA;
layout(location = 3) in vec3 b;
layout(location = 4) in vec4 colorB;
layout(location = 5) in float widthB; // the same as widthA: both ends carry it

layout(location = 0) out vec4 vColor;
layout(location = 1) noperspective out vec2 vLocal;
layout(location = 2) flat out vec2 vShape;

// The geometry shader's strip (A-, A+, B-, B+) as two triangles, split along
// the same diagonal, so both ways rasterise the same two triangles.
const vec2 kCorners[6] = vec2[6](vec2(0.0, -1.0), vec2(0.0, 1.0), vec2(1.0, -1.0),
                                 vec2(1.0, -1.0), vec2(0.0, 1.0), vec2(1.0, 1.0));

void main()
{
    vec4 ca = mvp * vec4(exaggerate(a), 1.0);
    vec4 cb = mvp * vec4(exaggerate(b), 1.0);
    LineSetup s = setupLine(ca, cb, colorA.zyxw, colorB.zyxw, widthA);
    QuadVertex q = s.visible < 0.5 ? hiddenCorner() : lineCorner(s, kCorners[gl_VertexIndex]);
    gl_Position = q.pos;
    vColor = q.color;
    vLocal = q.local;
    vShape = q.shape;
}
