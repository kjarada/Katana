#version 440
// kPointsFragment (shader_library.cpp): size x size squares with an
// antialiased edge.
layout(location = 0) in vec4 vColor;
layout(location = 1) noperspective in vec2 vLocal;
layout(location = 2) flat in vec2 vShape;

layout(location = 0) out vec4 fragColor;

void main()
{
    vec2 inside = vShape.x + 0.5 - abs(vLocal);
    float alpha = vColor.a * clamp(inside.x, 0.0, 1.0) * clamp(inside.y, 0.0, 1.0);
    if (alpha <= 0.0) {
        discard;
    }
    fragColor = vec4(vColor.rgb, alpha);
}
