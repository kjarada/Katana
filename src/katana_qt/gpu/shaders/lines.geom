#version 440
#extension GL_GOOGLE_include_directive : require
// kLinesGeometry (shader_library.cpp): the two ends of a line in, a
// four-vertex strip out.
layout(lines) in;
layout(triangle_strip, max_vertices = 4) out;

#include "frame.glsl"
#include "quad.glsl"

layout(location = 0) in vec4 endClip[];
layout(location = 1) in vec4 endColor[];
layout(location = 2) in float endSize[];

layout(location = 0) out vec4 vColor;
layout(location = 1) noperspective out vec2 vLocal;
layout(location = 2) flat out vec2 vShape;

void emitCorner(QuadVertex q)
{
    gl_Position = q.pos;
    vColor = q.color;
    vLocal = q.local;
    vShape = q.shape;
    EmitVertex();
}

void main()
{
    LineSetup s = setupLine(endClip[0], endClip[1], endColor[0], endColor[1], endSize[0]);
    if (s.visible < 0.5) {
        return;
    }
    emitCorner(lineCorner(s, vec2(0.0, -1.0)));
    emitCorner(lineCorner(s, vec2(0.0, 1.0)));
    emitCorner(lineCorner(s, vec2(1.0, -1.0)));
    emitCorner(lineCorner(s, vec2(1.0, 1.0)));
    EndPrimitive();
}
