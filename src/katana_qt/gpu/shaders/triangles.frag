#version 440
#extension GL_GOOGLE_include_directive : require
// kTrianglesFragment (shader_library.cpp): per-pixel lighting from the FACE
// normal, from the screen-space derivatives of the position. dFdy runs down
// the screen under Vulkan as ddy does under Direct3D; either way the normal is
// turned to face the eye before it is used, so its sign cannot matter.
#include "frame.glsl"

layout(location = 0) in vec4 vColor;
layout(location = 1) in vec3 vRel;

layout(location = 0) out vec4 fragColor;

void main()
{
    vec4 c = vColor;
    if (forwardMode.w > 0.5) {
        vec3 toEye = eyeRel.w > 0.5 ? eyeRel.xyz - vRel : -forwardMode.xyz;
        vec3 n = cross(dFdx(vRel), dFdy(vRel));
        float len = length(n);
        n = len > 0.0 ? n / len : normalize(toEye);
        if (dot(n, toEye) < 0.0) {
            n = -n;
        }
        float intensity = light.w + (1.0 - light.w) * clamp(dot(n, light.xyz), 0.0, 1.0);
        c.rgb *= intensity;
    }
    fragColor = c;
}
