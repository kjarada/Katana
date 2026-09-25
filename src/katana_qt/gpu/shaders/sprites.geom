#version 440
#extension GL_GOOGLE_include_directive : require
// kSpritesGeometry (shader_library.cpp): one point in, a four-vertex strip
// out. Draw-list points and cloud points alike, by size in logical pixels.
layout(points) in;
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
    float halfSize = max(endSize[0], 1.0) * params.z * 0.5;
    vec4 c = pulledTowardsEye(endClip[0], halfSize + 0.5);
    if (spriteHidden(c, halfSize)) {
        return;
    }
    emitCorner(spriteCorner(c, endColor[0], halfSize, vec2(-1.0, -1.0)));
    emitCorner(spriteCorner(c, endColor[0], halfSize, vec2(-1.0, 1.0)));
    emitCorner(spriteCorner(c, endColor[0], halfSize, vec2(1.0, -1.0)));
    emitCorner(spriteCorner(c, endColor[0], halfSize, vec2(1.0, 1.0)));
    EndPrimitive();
}
