#include "shader_library.hpp"

#include <cstddef>

namespace katana::qt::gpu {

namespace {

// ---- HLSL --------------------------------------------------------------------------
//
// One constant buffer for every program, at register b0 (QRhi binding 0), laid
// out in float4s so its C++ twin (gpu_renderer.cpp, FrameUniforms) needs no
// packing rules. Vertex inputs are TEXCOORD<n> because that is the semantic
// QRhi's Direct3D 11 backend gives input attribute location n.

#define KATANA_GPU_FRAME_CBUFFER                                                              \
    "cbuffer Frame : register(b0)\n"                                                          \
    "{\n"                                                                                     \
    "    float4x4 mvp;        // origin-relative world -> clip, reversed Z\n"                 \
    "    float4 viewport;     // xy: target size in device pixels, zw: 1 / size\n"            \
    "    float4 light;        // xyz: unit vector towards the light (world), w: ambient\n"    \
    "    float4 eyeRel;       // xyz: eye relative to the origin, w: 1 perspective\n"         \
    "    float4 forwardMode;  // xyz: unit view direction, w: 1 = light per pixel\n"          \
    "    float4 params;       // x: exaggeration, y: its datum rel. origin, z: device\n"      \
    "                         // pixels per logical pixel, w: cloud point size\n"             \
    "};\n"                                                                                    \
    "float3 exaggerate(float3 p)\n"                                                           \
    "{\n"                                                                                     \
    "    p.z = params.y + (p.z - params.y) * params.x;\n"                                    \
    "    return p;\n"                                                                         \
    "}\n"

// Colours arrive as the draw list's 0xAARRGGBB, read as UNORM bytes in memory
// order - B, G, R, A - so .zyxw puts them back.

const char* const kTrianglesVertex = KATANA_GPU_FRAME_CBUFFER R"(
struct VSIn { float3 pos : TEXCOORD0; float4 color : TEXCOORD1; };
struct VSOut { float4 pos : SV_Position; float4 color : COLOR0; float3 rel : TEXCOORD0; };
VSOut main(VSIn i)
{
    VSOut o;
    float3 p = exaggerate(i.pos);
    o.pos = mul(mvp, float4(p, 1.0));
    o.color = i.color.zyxw;
    o.rel = p;
    return o;
}
)";

// Per-pixel lighting from the FACE normal, taken from the screen-space
// derivatives of the interpolated position: the draw list carries no normals,
// and a TIN's facets are what a surveyor wants to see. The normal is turned
// to face the eye before lighting, so a face lit from behind gets ambient
// only - unlike the baked abs(n.l) of the software path, which lights both
// sides of a face alike (map_view3d).
const char* const kTrianglesFragment = KATANA_GPU_FRAME_CBUFFER R"(
struct VSOut { float4 pos : SV_Position; float4 color : COLOR0; float3 rel : TEXCOORD0; };
float4 main(VSOut i) : SV_Target
{
    float4 c = i.color;
    if (forwardMode.w > 0.5) {
        float3 toEye = eyeRel.w > 0.5 ? eyeRel.xyz - i.rel : -forwardMode.xyz;
        float3 n = cross(ddx(i.rel), ddy(i.rel));
        float len = length(n);
        n = len > 0.0 ? n / len : normalize(toEye);
        if (dot(n, toEye) < 0.0) {
            n = -n;
        }
        float intensity = light.w + (1.0 - light.w) * saturate(dot(n, light.xyz));
        c.rgb *= intensity;
    }
    return c;
}
)";

// A line is an instance of six vertices (two triangles), widened in screen
// space. Each corner keeps the clip-space w and depth of the endpoint it
// belongs to, as the software path's quads do. `local` is in device pixels and
// linear in screen space (noperspective), so the fragment shader can measure
// its distance from the segment exactly.
const char* const kLinesVertex = KATANA_GPU_FRAME_CBUFFER R"(
struct VSIn {
    float3 a : TEXCOORD0; float4 colorA : TEXCOORD1;
    float3 b : TEXCOORD2; float4 colorB : TEXCOORD3;
    float2 widthPad : TEXCOORD4;
    uint corner : SV_VertexID;
};
struct VSOut {
    float4 pos : SV_Position;
    float4 color : COLOR0;
    noperspective float2 local : TEXCOORD0;
    nointerpolation float2 shape : TEXCOORD1;
};
static const float2 kCorners[6] = {
    float2(0.0, -1.0), float2(0.0, 1.0), float2(1.0, 1.0),
    float2(0.0, -1.0), float2(1.0, 1.0), float2(1.0, -1.0)
};
VSOut main(VSIn i)
{
    VSOut o;
    float4 ca = mul(mvp, float4(exaggerate(i.a), 1.0));
    float4 cb = mul(mvp, float4(exaggerate(i.b), 1.0));
    float4 colorA = i.colorA.zyxw;
    float4 colorB = i.colorB.zyxw;
    // Reversed Z: a point is in front of the near plane when its depth z/w is
    // at most 1, i.e. w - z >= 0. Clip the segment there before dividing.
    float da = ca.w - ca.z;
    float db = cb.w - cb.z;
    o.shape = float2(0.0, 0.0);
    o.local = float2(0.0, 0.0);
    o.color = colorA;
    if (da < 0.0 && db < 0.0) {
        o.pos = float4(0.0, 0.0, -1.0, 1.0); // wholly behind: every corner the same, no area
        return o;
    }
    if (da < 0.0) {
        float t = da / (da - db);
        ca = lerp(ca, cb, t);
        colorA = lerp(colorA, colorB, t);
    } else if (db < 0.0) {
        float t = db / (db - da);
        cb = lerp(cb, ca, t);
        colorB = lerp(colorB, colorA, t);
    }
    float2 halfSize = viewport.xy * 0.5;
    float2 sa = ca.xy / ca.w * halfSize;
    float2 sb = cb.xy / cb.w * halfSize;
    float2 d = sb - sa;
    float len = length(d);
    float2 dir = len > 1e-6 ? d / len : float2(1.0, 0.0);
    float2 across = float2(-dir.y, dir.x);
    // Square caps half a width past each end, as the software path draws
    // them, and one pixel more all round for the antialiased edge to fade in.
    float halfWidth = max(i.widthPad.x, 1.0) * params.z * 0.5;
    float extent = halfWidth + 1.0;
    float2 corner = kCorners[i.corner];
    bool atA = corner.x < 0.5;
    float4 clip = atA ? ca : cb;
    float along = atA ? -extent : extent;
    float2 offset = across * (corner.y * extent) + dir * along;
    clip.xy += offset / halfSize * clip.w;
    o.pos = clip;
    o.color = atA ? colorA : colorB;
    o.local = float2((atA ? 0.0 : len) + along, corner.y * extent);
    o.shape = float2(len, halfWidth);
    return o;
}
)";

// Coverage of the pixel by the line's rectangle, as the distance of the pixel
// centre inside the edge plus half a pixel, clamped to [0, 1]: exact for an
// edge crossing the pixel straight, and the usual analytic approximation
// otherwise. Multiplied into alpha and blended; 4x MSAA handles the silhouettes
// of the triangles underneath.
const char* const kLinesFragment = R"(
struct VSOut {
    float4 pos : SV_Position;
    float4 color : COLOR0;
    noperspective float2 local : TEXCOORD0;
    nointerpolation float2 shape : TEXCOORD1;
};
float4 main(VSOut i) : SV_Target
{
    float beyondEnds = max(-i.local.x, i.local.x - i.shape.x);
    float coverage = saturate(i.shape.y + 0.5 - abs(i.local.y)) *
                     saturate(i.shape.y + 0.5 - beyondEnds);
    float alpha = i.color.a * coverage;
    if (alpha <= 0.0) {
        discard;
    }
    return float4(i.color.rgb, alpha);
}
)";

// The draw list's points: size x size squares centred on the point, as the
// software path draws them, with an antialiased edge.
const char* const kPointsVertex = KATANA_GPU_FRAME_CBUFFER R"(
struct VSIn {
    float3 pos : TEXCOORD0; float4 color : TEXCOORD1; float2 sizePad : TEXCOORD2;
    uint corner : SV_VertexID;
};
struct VSOut {
    float4 pos : SV_Position;
    float4 color : COLOR0;
    noperspective float2 local : TEXCOORD0;
    nointerpolation float2 shape : TEXCOORD1;
};
static const float2 kCorners[6] = {
    float2(-1.0, -1.0), float2(-1.0, 1.0), float2(1.0, 1.0),
    float2(-1.0, -1.0), float2(1.0, 1.0), float2(1.0, -1.0)
};
VSOut main(VSIn i)
{
    VSOut o;
    float4 c = mul(mvp, float4(exaggerate(i.pos), 1.0));
    o.color = i.color.zyxw;
    o.local = float2(0.0, 0.0);
    o.shape = float2(0.0, 0.0);
    if (c.w - c.z < 0.0) {
        o.pos = float4(0.0, 0.0, -1.0, 1.0); // behind the near plane
        return o;
    }
    float halfSize = max(i.sizePad.x, 1.0) * params.z * 0.5;
    float extent = halfSize + 1.0;
    float2 corner = kCorners[i.corner];
    c.xy += corner * extent / (viewport.xy * 0.5) * c.w;
    o.pos = c;
    o.local = corner * extent;
    o.shape = float2(halfSize, 0.0);
    return o;
}
)";

const char* const kPointsFragment = R"(
struct VSOut {
    float4 pos : SV_Position;
    float4 color : COLOR0;
    noperspective float2 local : TEXCOORD0;
    nointerpolation float2 shape : TEXCOORD1;
};
float4 main(VSOut i) : SV_Target
{
    float2 inside = i.shape.x + 0.5 - abs(i.local);
    float alpha = i.color.a * saturate(inside.x) * saturate(inside.y);
    if (alpha <= 0.0) {
        discard;
    }
    return float4(i.color.rgb, alpha);
}
)";

// Cloud points: round, all the same screen size (params.w logical pixels).
const char* const kCloudVertex = KATANA_GPU_FRAME_CBUFFER R"(
struct VSIn { float3 pos : TEXCOORD0; float4 color : TEXCOORD1; uint corner : SV_VertexID; };
struct VSOut {
    float4 pos : SV_Position;
    float4 color : COLOR0;
    noperspective float2 local : TEXCOORD0;
    nointerpolation float2 shape : TEXCOORD1;
};
static const float2 kCorners[6] = {
    float2(-1.0, -1.0), float2(-1.0, 1.0), float2(1.0, 1.0),
    float2(-1.0, -1.0), float2(1.0, 1.0), float2(1.0, -1.0)
};
VSOut main(VSIn i)
{
    VSOut o;
    float4 c = mul(mvp, float4(exaggerate(i.pos), 1.0));
    o.color = i.color.zyxw;
    o.local = float2(0.0, 0.0);
    o.shape = float2(0.0, 0.0);
    if (c.w - c.z < 0.0) {
        o.pos = float4(0.0, 0.0, -1.0, 1.0);
        return o;
    }
    float radius = max(params.w, 1.0) * params.z * 0.5;
    float extent = radius + 1.0;
    float2 corner = kCorners[i.corner];
    c.xy += corner * extent / (viewport.xy * 0.5) * c.w;
    o.pos = c;
    o.local = corner * extent;
    o.shape = float2(radius, 0.0);
    return o;
}
)";

const char* const kCloudFragment = R"(
struct VSOut {
    float4 pos : SV_Position;
    float4 color : COLOR0;
    noperspective float2 local : TEXCOORD0;
    nointerpolation float2 shape : TEXCOORD1;
};
float4 main(VSOut i) : SV_Target
{
    float alpha = i.color.a * saturate(i.shape.x + 0.5 - length(i.local));
    if (alpha <= 0.0) {
        discard;
    }
    return float4(i.color.rgb, alpha);
}
)";

#undef KATANA_GPU_FRAME_CBUFFER

[[nodiscard]] QShader hlslShader(QShader::Stage stage, const char* source)
{
    QShader shader;
    shader.setStage(stage);
    // Shader model 5.0: what every Direct3D 11 feature level 11 device runs,
    // and what `noperspective` and SV_VertexID need.
    shader.setShader(QShaderKey(QShader::HlslShader, QShaderVersion(50)),
                     QShaderCode(QByteArray(source), QByteArrayLiteral("main")));
    return shader;
}

class RuntimeHlslLibrary final : public ShaderLibrary {
  public:
    [[nodiscard]] katana::core::Result<ShaderPair> program(Program program) const override
    {
        return ShaderPair{hlslShader(QShader::VertexStage, hlslSource(program, QShader::VertexStage)),
                          hlslShader(QShader::FragmentStage,
                                     hlslSource(program, QShader::FragmentStage))};
    }
    [[nodiscard]] std::string describe() const override
    {
        return "HLSL source compiled at run time (Direct3D 11)";
    }
};

} // namespace

const char* toString(Program program)
{
    switch (program) {
    case Program::Triangles:
        return "triangles";
    case Program::Lines:
        return "lines";
    case Program::Points:
        return "points";
    case Program::CloudPoints:
        return "cloud points";
    }
    return "unknown";
}

const char* hlslSource(Program program, QShader::Stage stage)
{
    const bool vertex = stage == QShader::VertexStage;
    switch (program) {
    case Program::Triangles:
        return vertex ? kTrianglesVertex : kTrianglesFragment;
    case Program::Lines:
        return vertex ? kLinesVertex : kLinesFragment;
    case Program::Points:
        return vertex ? kPointsVertex : kPointsFragment;
    case Program::CloudPoints:
        return vertex ? kCloudVertex : kCloudFragment;
    }
    return "";
}

const ShaderLibrary& runtimeHlslShaders()
{
    static const RuntimeHlslLibrary library;
    return library;
}

SerializedShaderLibrary::SerializedShaderLibrary(std::array<Blobs, kAllPrograms.size()> blobs)
    : blobs_(std::move(blobs))
{
}

katana::core::Result<ShaderPair> SerializedShaderLibrary::program(Program program) const
{
    const auto index = static_cast<std::size_t>(program);
    ShaderPair pair{QShader::fromSerialized(blobs_[index].vertex),
                    QShader::fromSerialized(blobs_[index].fragment)};
    if (!pair.vertex.isValid() || !pair.fragment.isValid()) {
        return katana::core::makeError(katana::core::ErrorCode::RenderingFailure,
                                       std::string("the precompiled shaders for ") +
                                           toString(program) + " do not deserialize");
    }
    return pair;
}

} // namespace katana::qt::gpu
