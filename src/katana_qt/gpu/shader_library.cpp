#include "shader_library.hpp"

#include <cstddef>
#include <string>
#include <utility>

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

// ---- lines and points: the quad each becomes ----------------------------------------
//
// Shared by both expansions, so they draw the same pixels: the geometry shader
// calls these four times per line or point, the instanced vertex shader once
// per corner. `local` is in device pixels and linear in screen space
// (noperspective), so the fragment shader measures its distance from the
// segment or the centre exactly.

#define KATANA_GPU_QUAD_VERTEX                                                                \
    "struct QuadVertex\n"                                                                     \
    "{\n"                                                                                     \
    "    float4 pos : SV_Position;\n"                                                         \
    "    float4 color : COLOR0;\n"                                                            \
    "    noperspective float2 local : TEXCOORD0;\n"                                           \
    "    nointerpolation float2 shape : TEXCOORD1;\n"                                         \
    "};\n"

// Each corner keeps the clip-space w and depth of the end it belongs to, as
// the software path's quads do. A quad reaches half a pixel past the line's
// edges and ends: the antialiased coverage below falls to zero exactly there,
// so nothing further out could be drawn, and every fragment spent on an empty
// fringe is a fragment a dense TIN pays hundreds of times per pixel.
const char* const kQuadBuilders = R"(
QuadVertex hiddenCorner()
{
    QuadVertex o;
    o.pos = float4(0.0, 0.0, -1.0, 1.0); // outside the depth range: clipped, no area
    o.color = float4(0.0, 0.0, 0.0, 0.0);
    o.local = float2(0.0, 0.0);
    o.shape = float2(0.0, 0.0);
    return o;
}

struct LineSetup
{
    float4 ca;
    float4 cb;
    float4 colorA;
    float4 colorB;
    float2 dir;
    float len;
    float halfWidth;
    float visible;
};

LineSetup setupLine(float4 ca, float4 cb, float4 colorA, float4 colorB, float width)
{
    LineSetup s;
    // Reversed Z: a point is in front of the near plane when its depth z/w is
    // at most 1, i.e. w - z >= 0. Clip the segment there before dividing.
    float da = ca.w - ca.z;
    float db = cb.w - cb.z;
    s.visible = (da < 0.0 && db < 0.0) ? 0.0 : 1.0;
    if (s.visible > 0.5 && da < 0.0) {
        float t = da / (da - db);
        ca = lerp(ca, cb, t);
        colorA = lerp(colorA, colorB, t);
    } else if (s.visible > 0.5 && db < 0.0) {
        float t = db / (db - da);
        cb = lerp(cb, ca, t);
        colorB = lerp(colorB, colorA, t);
    }
    float2 halfSize = viewport.xy * 0.5;
    float2 sa = ca.xy / ca.w * halfSize;
    float2 sb = cb.xy / cb.w * halfSize;
    float2 d = sb - sa;
    s.len = length(d);
    s.dir = s.len > 1e-6 ? d / s.len : float2(1.0, 0.0);
    s.halfWidth = max(width, 1.0) * params.z * 0.5;
    // Both ends past the same edge of the view by more than the quad reaches:
    // nothing of the line can show, so it is dropped before the rasteriser.
    float2 reach = halfSize + s.halfWidth + 0.5;
    if (any(min(sa, sb) > reach) || any(max(sa, sb) < -reach)) {
        s.visible = 0.0;
    }
    s.ca = ca;
    s.cb = cb;
    s.colorA = colorA;
    s.colorB = colorB;
    return s;
}

// corner.x: 0 at end A, 1 at end B; corner.y: -1 or +1, the side. Square caps
// half a width past each end, as the software path draws them.
QuadVertex lineCorner(LineSetup s, float2 corner)
{
    float2 halfSize = viewport.xy * 0.5;
    float extent = s.halfWidth + 0.5;
    bool atA = corner.x < 0.5;
    float4 clip = atA ? s.ca : s.cb;
    float along = atA ? -extent : extent;
    float2 across = float2(-s.dir.y, s.dir.x);
    float2 offset = across * (corner.y * extent) + s.dir * along;
    clip.xy += offset / halfSize * clip.w;
    QuadVertex o;
    o.pos = clip;
    o.color = atA ? s.colorA : s.colorB;
    o.local = float2((atA ? 0.0 : s.len) + along, corner.y * extent);
    o.shape = float2(s.len, s.halfWidth);
    return o;
}

// A square of side 2 * halfSize device pixels centred on `c`; corner is
// (+-1, +-1).
QuadVertex spriteCorner(float4 c, float4 color, float halfSize, float2 corner)
{
    float extent = halfSize + 0.5;
    QuadVertex o;
    o.pos = c;
    o.pos.xy += corner * extent / (viewport.xy * 0.5) * c.w;
    o.color = color;
    o.local = corner * extent;
    o.shape = float2(halfSize, 0.0);
    return o;
}

// Behind the near plane (reversed Z: depth z/w above 1), or so far past an
// edge of the view that no corner can reach back into it.
bool spriteHidden(float4 c, float halfSize)
{
    if (c.w - c.z < 0.0) {
        return true;
    }
    float2 halfView = viewport.xy * 0.5;
    float2 s = abs(c.xy / c.w * halfView);
    return any(s > halfView + halfSize + 0.5);
}
)";

// ---- Expansion::GeometryShader --------------------------------------------------------
//
// The vertex shader transforms each end once; the geometry shader sees the
// two ends of a line (or one point) and appends a four-vertex strip.

const char* const kLineEndsVertex = R"(
struct VSIn { float3 pos : TEXCOORD0; float4 color : TEXCOORD1; float width : TEXCOORD2; };
struct End { float4 clip : TEXCOORD0; float4 color : TEXCOORD1; float size : TEXCOORD2; };
End main(VSIn i)
{
    End o;
    o.clip = mul(mvp, float4(exaggerate(i.pos), 1.0));
    o.color = i.color.zyxw;
    o.size = i.width;
    return o;
}
)";

const char* const kLinesGeometry = R"(
struct End { float4 clip : TEXCOORD0; float4 color : TEXCOORD1; float size : TEXCOORD2; };
[maxvertexcount(4)]
void main(line End v[2], inout TriangleStream<QuadVertex> strip)
{
    LineSetup s = setupLine(v[0].clip, v[1].clip, v[0].color, v[1].color, v[0].size);
    if (s.visible < 0.5) {
        return;
    }
    strip.Append(lineCorner(s, float2(0.0, -1.0)));
    strip.Append(lineCorner(s, float2(0.0, 1.0)));
    strip.Append(lineCorner(s, float2(1.0, -1.0)));
    strip.Append(lineCorner(s, float2(1.0, 1.0)));
}
)";

// The draw list's points carry their size; cloud points all take params.w.
// Both go on to the same geometry shader as a size in logical pixels.
const char* const kPointCentresVertex = R"(
struct VSIn { float3 pos : TEXCOORD0; float4 color : TEXCOORD1; float size : TEXCOORD2; };
struct End { float4 clip : TEXCOORD0; float4 color : TEXCOORD1; float size : TEXCOORD2; };
End main(VSIn i)
{
    End o;
    o.clip = mul(mvp, float4(exaggerate(i.pos), 1.0));
    o.color = i.color.zyxw;
    o.size = i.size;
    return o;
}
)";

const char* const kCloudCentresVertex = R"(
struct VSIn { float3 pos : TEXCOORD0; float4 color : TEXCOORD1; };
struct End { float4 clip : TEXCOORD0; float4 color : TEXCOORD1; float size : TEXCOORD2; };
End main(VSIn i)
{
    End o;
    o.clip = mul(mvp, float4(exaggerate(i.pos), 1.0));
    o.color = i.color.zyxw;
    o.size = params.w;
    return o;
}
)";

const char* const kSpritesGeometry = R"(
struct End { float4 clip : TEXCOORD0; float4 color : TEXCOORD1; float size : TEXCOORD2; };
[maxvertexcount(4)]
void main(point End v[1], inout TriangleStream<QuadVertex> strip)
{
    float halfSize = max(v[0].size, 1.0) * params.z * 0.5;
    if (spriteHidden(v[0].clip, halfSize)) {
        return;
    }
    strip.Append(spriteCorner(v[0].clip, v[0].color, halfSize, float2(-1.0, -1.0)));
    strip.Append(spriteCorner(v[0].clip, v[0].color, halfSize, float2(-1.0, 1.0)));
    strip.Append(spriteCorner(v[0].clip, v[0].color, halfSize, float2(1.0, -1.0)));
    strip.Append(spriteCorner(v[0].clip, v[0].color, halfSize, float2(1.0, 1.0)));
}
)";

// ---- Expansion::Instanced ---------------------------------------------------------------
//
// One instance of six vertices (two triangles) per line or point; the vertex
// shader runs once per corner and builds that corner alone.

const char* const kLinesInstancedVertex = R"(
struct VSIn {
    float3 a : TEXCOORD0; float4 colorA : TEXCOORD1; float widthA : TEXCOORD2;
    float3 b : TEXCOORD3; float4 colorB : TEXCOORD4; float widthB : TEXCOORD5;
    uint corner : SV_VertexID;
};
// The geometry shader's strip (A-, A+, B-, B+) as two triangles, split along
// the same diagonal, so both ways rasterise the same two triangles.
static const float2 kCorners[6] = {
    float2(0.0, -1.0), float2(0.0, 1.0), float2(1.0, -1.0),
    float2(1.0, -1.0), float2(0.0, 1.0), float2(1.0, 1.0)
};
QuadVertex main(VSIn i)
{
    float4 ca = mul(mvp, float4(exaggerate(i.a), 1.0));
    float4 cb = mul(mvp, float4(exaggerate(i.b), 1.0));
    LineSetup s = setupLine(ca, cb, i.colorA.zyxw, i.colorB.zyxw, i.widthA);
    if (s.visible < 0.5) {
        return hiddenCorner();
    }
    return lineCorner(s, kCorners[i.corner]);
}
)";

// As for lines: the strip's two triangles, split the same way.
const char* const kSpriteCornersTable = R"(
static const float2 kCorners[6] = {
    float2(-1.0, -1.0), float2(-1.0, 1.0), float2(1.0, -1.0),
    float2(1.0, -1.0), float2(-1.0, 1.0), float2(1.0, 1.0)
};
)";

const char* const kPointsInstancedVertex = R"(
struct VSIn { float3 pos : TEXCOORD0; float4 color : TEXCOORD1; float size : TEXCOORD2;
              uint corner : SV_VertexID; };
QuadVertex main(VSIn i)
{
    float4 c = mul(mvp, float4(exaggerate(i.pos), 1.0));
    float halfSize = max(i.size, 1.0) * params.z * 0.5;
    if (spriteHidden(c, halfSize)) {
        return hiddenCorner();
    }
    return spriteCorner(c, i.color.zyxw, halfSize, kCorners[i.corner]);
}
)";

const char* const kCloudInstancedVertex = R"(
struct VSIn { float3 pos : TEXCOORD0; float4 color : TEXCOORD1; uint corner : SV_VertexID; };
QuadVertex main(VSIn i)
{
    float4 c = mul(mvp, float4(exaggerate(i.pos), 1.0));
    float halfSize = max(params.w, 1.0) * params.z * 0.5;
    if (spriteHidden(c, halfSize)) {
        return hiddenCorner();
    }
    return spriteCorner(c, i.color.zyxw, halfSize, kCorners[i.corner]);
}
)";

// ---- fragments, shared by both expansions ---------------------------------------------

// Coverage of the pixel by the line's rectangle, as the distance of the pixel
// centre inside the edge plus half a pixel, clamped to [0, 1]: exact for an
// edge crossing the pixel straight, and the usual analytic approximation
// otherwise. Multiplied into alpha and blended; 4x MSAA handles the silhouettes
// of the triangles underneath.
const char* const kLinesFragment = R"(
float4 main(QuadVertex i) : SV_Target
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
const char* const kPointsFragment = R"(
float4 main(QuadVertex i) : SV_Target
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
const char* const kCloudFragment = R"(
float4 main(QuadVertex i) : SV_Target
{
    float alpha = i.color.a * saturate(i.shape.x + 0.5 - length(i.local));
    if (alpha <= 0.0) {
        discard;
    }
    return float4(i.color.rgb, alpha);
}
)";

// Whole sources, assembled once: the preamble each stage needs, then its body.
struct Sources {
    std::string trianglesVertex;
    std::string trianglesFragment;
    std::string lineEndsVertex;
    std::string linesGeometry;
    std::string pointCentresVertex;
    std::string cloudCentresVertex;
    std::string spritesGeometry;
    std::string linesInstancedVertex;
    std::string pointsInstancedVertex;
    std::string cloudInstancedVertex;
    std::string linesFragment;
    std::string pointsFragment;
    std::string cloudFragment;
};

const Sources& sources()
{
    static const Sources assembled = [] {
        const std::string frame = KATANA_GPU_FRAME_CBUFFER;
        const std::string quad = KATANA_GPU_QUAD_VERTEX;
        const std::string builders = frame + quad + kQuadBuilders;
        Sources s;
        s.trianglesVertex = kTrianglesVertex;
        s.trianglesFragment = kTrianglesFragment;
        s.lineEndsVertex = frame + kLineEndsVertex;
        s.linesGeometry = builders + kLinesGeometry;
        s.pointCentresVertex = frame + kPointCentresVertex;
        s.cloudCentresVertex = frame + kCloudCentresVertex;
        s.spritesGeometry = builders + kSpritesGeometry;
        s.linesInstancedVertex = builders + kLinesInstancedVertex;
        s.pointsInstancedVertex = builders + kSpriteCornersTable + kPointsInstancedVertex;
        s.cloudInstancedVertex = builders + kSpriteCornersTable + kCloudInstancedVertex;
        s.linesFragment = quad + kLinesFragment;
        s.pointsFragment = quad + kPointsFragment;
        s.cloudFragment = quad + kCloudFragment;
        return s;
    }();
    return assembled;
}

#undef KATANA_GPU_FRAME_CBUFFER
#undef KATANA_GPU_QUAD_VERTEX

[[nodiscard]] QShader hlslShader(QShader::Stage stage, const char* source)
{
    QShader shader;
    shader.setStage(stage);
    // Shader model 5.0: what every Direct3D 11 feature level 11 device runs,
    // and what `noperspective`, SV_VertexID and the geometry stage need.
    shader.setShader(QShaderKey(QShader::HlslShader, QShaderVersion(50)),
                     QShaderCode(QByteArray(source), QByteArrayLiteral("main")));
    return shader;
}

class RuntimeHlslLibrary final : public ShaderLibrary {
  public:
    [[nodiscard]] katana::core::Result<ShaderStages> program(Program program,
                                                             Expansion expansion) const override
    {
        ShaderStages stages;
        stages.vertex = hlslShader(QShader::VertexStage,
                                   hlslSource(program, expansion, QShader::VertexStage));
        const char* geometry = hlslSource(program, expansion, QShader::GeometryStage);
        if (*geometry != '\0') {
            stages.geometry = hlslShader(QShader::GeometryStage, geometry);
        }
        stages.fragment = hlslShader(QShader::FragmentStage,
                                     hlslSource(program, expansion, QShader::FragmentStage));
        return stages;
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

const char* toString(Expansion expansion)
{
    switch (expansion) {
    case Expansion::GeometryShader:
        return "geometry shader";
    case Expansion::Instanced:
        return "instanced";
    }
    return "unknown";
}

const char* hlslSource(Program program, Expansion expansion, QShader::Stage stage)
{
    const Sources& s = sources();
    const bool geometryPath = expansion == Expansion::GeometryShader;
    if (stage == QShader::GeometryStage) {
        if (!geometryPath || program == Program::Triangles) {
            return "";
        }
        return program == Program::Lines ? s.linesGeometry.c_str() : s.spritesGeometry.c_str();
    }
    const bool vertex = stage == QShader::VertexStage;
    if (!vertex && stage != QShader::FragmentStage) {
        return "";
    }
    switch (program) {
    case Program::Triangles:
        return vertex ? s.trianglesVertex.c_str() : s.trianglesFragment.c_str();
    case Program::Lines:
        if (!vertex) {
            return s.linesFragment.c_str();
        }
        return geometryPath ? s.lineEndsVertex.c_str() : s.linesInstancedVertex.c_str();
    case Program::Points:
        if (!vertex) {
            return s.pointsFragment.c_str();
        }
        return geometryPath ? s.pointCentresVertex.c_str() : s.pointsInstancedVertex.c_str();
    case Program::CloudPoints:
        if (!vertex) {
            return s.cloudFragment.c_str();
        }
        return geometryPath ? s.cloudCentresVertex.c_str() : s.cloudInstancedVertex.c_str();
    }
    return "";
}

const ShaderLibrary& runtimeHlslShaders()
{
    static const RuntimeHlslLibrary library;
    return library;
}

SerializedShaderLibrary::SerializedShaderLibrary(BlobTable blobs) : blobs_(std::move(blobs)) {}

katana::core::Result<SerializedShaderLibrary::BlobTable>
SerializedShaderLibrary::serialize(const ShaderLibrary& library)
{
    BlobTable table;
    for (const Program program : kAllPrograms) {
        for (const Expansion expansion : kAllExpansions) {
            auto stages = library.program(program, expansion);
            if (!stages) {
                return stages.error();
            }
            Blobs& blobs =
                table[static_cast<std::size_t>(program)][static_cast<std::size_t>(expansion)];
            blobs.vertex = stages->vertex.serialized();
            if (stages->geometry.isValid()) {
                blobs.geometry = stages->geometry.serialized();
            }
            blobs.fragment = stages->fragment.serialized();
        }
    }
    return table;
}

katana::core::Result<ShaderStages> SerializedShaderLibrary::program(Program program,
                                                                    Expansion expansion) const
{
    const Blobs& blobs =
        blobs_[static_cast<std::size_t>(program)][static_cast<std::size_t>(expansion)];
    ShaderStages stages{QShader::fromSerialized(blobs.vertex), QShader{},
                        QShader::fromSerialized(blobs.fragment)};
    bool valid = stages.vertex.isValid() && stages.fragment.isValid();
    if (!blobs.geometry.isEmpty()) {
        stages.geometry = QShader::fromSerialized(blobs.geometry);
        valid = valid && stages.geometry.isValid();
    }
    if (!valid) {
        return katana::core::makeError(katana::core::ErrorCode::RenderingFailure,
                                       std::string("the precompiled shaders for ") +
                                           toString(program) + " (" + toString(expansion) +
                                           ") do not deserialize");
    }
    return stages;
}

} // namespace katana::qt::gpu
