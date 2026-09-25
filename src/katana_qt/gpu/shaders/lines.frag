#version 440
// kLinesFragment (shader_library.cpp): coverage of the pixel by the line's
// rectangle, multiplied into alpha and blended.
layout(location = 0) in vec4 vColor;
layout(location = 1) noperspective in vec2 vLocal;
layout(location = 2) flat in vec2 vShape;

layout(location = 0) out vec4 fragColor;

void main()
{
    float beyondEnds = max(-vLocal.x, vLocal.x - vShape.x);
    float coverage = clamp(vShape.y + 0.5 - abs(vLocal.y), 0.0, 1.0) *
                     clamp(vShape.y + 0.5 - beyondEnds, 0.0, 1.0);
    float alpha = vColor.a * coverage;
    if (alpha <= 0.0) {
        discard;
    }
    fragColor = vec4(vColor.rgb, alpha);
}
