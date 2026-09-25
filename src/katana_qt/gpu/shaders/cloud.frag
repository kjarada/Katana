#version 440
// kCloudFragment (shader_library.cpp): round sprites.
layout(location = 0) in vec4 vColor;
layout(location = 1) noperspective in vec2 vLocal;
layout(location = 2) flat in vec2 vShape;

layout(location = 0) out vec4 fragColor;

void main()
{
    float alpha = vColor.a * clamp(vShape.x + 0.5 - length(vLocal), 0.0, 1.0);
    if (alpha <= 0.0) {
        discard;
    }
    fragColor = vec4(vColor.rgb, alpha);
}
