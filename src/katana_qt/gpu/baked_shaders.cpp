#include "baked_shaders.hpp"

#include <cstddef>
#include <utility>

namespace katana::qt::gpu {

namespace {

// Each stage's .qsb, embedded by the compiler (#embed; the directory is given
// by --embed-dir in gpu/CMakeLists.txt, and the dependency file names each
// blob, so a rebaked shader rebuilds this file). #embed is C++26; the pragma
// keeps a C++23 build (KATANA_CXX_STANDARD) from failing -Werror on it, as in
// plotting/frame.cpp.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wc++26-extensions"
constexpr unsigned char kTrianglesVertex[] = {
#embed "triangles.vert.qsb"
};
constexpr unsigned char kTrianglesFragment[] = {
#embed "triangles.frag.qsb"
};
constexpr unsigned char kLineEndsVertex[] = {
#embed "line_ends.vert.qsb"
};
constexpr unsigned char kLinesGeometry[] = {
#embed "lines.geom.qsb"
};
constexpr unsigned char kPointCentresVertex[] = {
#embed "point_centres.vert.qsb"
};
constexpr unsigned char kCloudCentresVertex[] = {
#embed "cloud_centres.vert.qsb"
};
constexpr unsigned char kSpritesGeometry[] = {
#embed "sprites.geom.qsb"
};
constexpr unsigned char kLinesInstancedVertex[] = {
#embed "lines_instanced.vert.qsb"
};
constexpr unsigned char kPointsInstancedVertex[] = {
#embed "points_instanced.vert.qsb"
};
constexpr unsigned char kCloudInstancedVertex[] = {
#embed "cloud_instanced.vert.qsb"
};
constexpr unsigned char kLinesFragment[] = {
#embed "lines.frag.qsb"
};
constexpr unsigned char kPointsFragment[] = {
#embed "points.frag.qsb"
};
constexpr unsigned char kCloudFragment[] = {
#embed "cloud.frag.qsb"
};
#pragma GCC diagnostic pop

template <std::size_t N> [[nodiscard]] QByteArray blob(const unsigned char (&bytes)[N])
{
    // Borrowed, not copied: the arrays live for the whole program.
    return QByteArray::fromRawData(reinterpret_cast<const char*>(bytes),
                                   static_cast<qsizetype>(N));
}

// The same table shader_library.cpp's hlslSource() answers from: which stage
// each program and expansion draws with.
[[nodiscard]] SerializedShaderLibrary::BlobTable bakedTable()
{
    using Blobs = SerializedShaderLibrary::Blobs;
    SerializedShaderLibrary::BlobTable table;
    const auto set = [&table](Program program, Expansion expansion, Blobs blobs) {
        table[static_cast<std::size_t>(program)][static_cast<std::size_t>(expansion)] =
            std::move(blobs);
    };
    const Blobs triangles{blob(kTrianglesVertex), {}, blob(kTrianglesFragment)};
    set(Program::Triangles, Expansion::GeometryShader, triangles);
    set(Program::Triangles, Expansion::Instanced, triangles);
    set(Program::Lines, Expansion::GeometryShader,
        {blob(kLineEndsVertex), blob(kLinesGeometry), blob(kLinesFragment)});
    set(Program::Lines, Expansion::Instanced, {blob(kLinesInstancedVertex), {}, blob(kLinesFragment)});
    set(Program::Points, Expansion::GeometryShader,
        {blob(kPointCentresVertex), blob(kSpritesGeometry), blob(kPointsFragment)});
    set(Program::Points, Expansion::Instanced,
        {blob(kPointsInstancedVertex), {}, blob(kPointsFragment)});
    set(Program::CloudPoints, Expansion::GeometryShader,
        {blob(kCloudCentresVertex), blob(kSpritesGeometry), blob(kCloudFragment)});
    set(Program::CloudPoints, Expansion::Instanced,
        {blob(kCloudInstancedVertex), {}, blob(kCloudFragment)});
    return table;
}

} // namespace

const ShaderLibrary& bakedShaders()
{
    static const SerializedShaderLibrary library(bakedTable());
    return library;
}

const ShaderLibrary& defaultShaders() { return bakedShaders(); }

} // namespace katana::qt::gpu
