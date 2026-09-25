#version 440
#extension GL_GOOGLE_include_directive : require
// kCloudCentresVertex (shader_library.cpp): cloud points all take params.w.
#include "frame.glsl"

layout(location = 0) in vec3 pos;
layout(location = 1) in vec4 color;

layout(location = 0) out vec4 endClip;
layout(location = 1) out vec4 endColor;
layout(location = 2) out float endSize;

void main()
{
    endClip = mvp * vec4(exaggerate(pos), 1.0);
    endColor = color.zyxw;
    endSize = params.w;
    gl_Position = endClip;
}
