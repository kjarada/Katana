// A TIN surface as bytes and back (tin_codec.hpp).

#include "katana/terrain/tin_codec.hpp"

#include <bit>
#include <cstddef>
#include <cstdint>
#include <string>
#include <utility>
#include <vector>

namespace katana::terrain {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr std::string_view kMagic = "KTIN";
// The magic, the version and the two counts.
constexpr std::uint64_t kHeaderBytes = 4 + 4 + 8 + 8;
constexpr std::uint64_t kVertexBytes = 3 * 8;
// Three indices and the constraint mask.
constexpr std::uint64_t kTriangleBytes = 3 * 4 + 1;

void appendLittle(std::string& out, std::uint64_t value, std::size_t bytes)
{
    for (std::size_t i = 0; i < bytes; ++i) {
        out.push_back(static_cast<char>((value >> (8 * i)) & 0xFFU));
    }
}

// Reads what appendLittle wrote. Every read is bounds-checked by the caller,
// which has compared the whole size with the counts first.
class Reader {
  public:
    explicit Reader(std::span<const std::byte> bytes) : bytes_(bytes) {}

    [[nodiscard]] std::uint64_t little(std::size_t size)
    {
        std::uint64_t value = 0;
        for (std::size_t i = 0; i < size; ++i) {
            value |= std::uint64_t{std::to_integer<std::uint8_t>(bytes_[offset_ + i])} << (8 * i);
        }
        offset_ += size;
        return value;
    }
    [[nodiscard]] double real() { return std::bit_cast<double>(little(8)); }
    [[nodiscard]] std::uint8_t byte() { return static_cast<std::uint8_t>(little(1)); }

  private:
    std::span<const std::byte> bytes_;
    std::size_t offset_ = 0;
};

katana::core::Error damaged(std::string what)
{
    return makeError(ErrorCode::ParseFailure, "a stored surface is damaged: " + what);
}

} // namespace

std::uint64_t encodedTinBytes(std::uint64_t vertices, std::uint64_t triangles)
{
    return kHeaderBytes + kVertexBytes * vertices + kTriangleBytes * triangles;
}

std::string encodeTin(const TinSurface& surface)
{
    const std::vector<Point3>& vertices = surface.vertices();
    const std::vector<TinTriangle>& triangles = surface.triangles();
    const std::vector<std::uint8_t>& masks = surface.constrainedEdges();
    std::string out;
    out.reserve(static_cast<std::size_t>(encodedTinBytes(vertices.size(), triangles.size())));
    out += kMagic;
    appendLittle(out, kTinCodecVersion, 4);
    appendLittle(out, vertices.size(), 8);
    appendLittle(out, triangles.size(), 8);
    for (const Point3& vertex : vertices) {
        appendLittle(out, std::bit_cast<std::uint64_t>(vertex.x), 8);
        appendLittle(out, std::bit_cast<std::uint64_t>(vertex.y), 8);
        appendLittle(out, std::bit_cast<std::uint64_t>(vertex.z), 8);
    }
    for (const TinTriangle& triangle : triangles) {
        for (const std::uint32_t index : triangle) {
            appendLittle(out, index, 4);
        }
    }
    // TinSurface::create gives every triangle a mask, zero when none was
    // given; the empty surface (no triangles) has none to write.
    for (std::size_t t = 0; t < triangles.size(); ++t) {
        out.push_back(static_cast<char>(t < masks.size() ? masks[t] : std::uint8_t{0}));
    }
    return out;
}

Result<TinSurface> decodeTin(std::span<const std::byte> bytes)
{
    if (bytes.size() < kHeaderBytes) {
        return damaged("its header is cut short (" + std::to_string(bytes.size()) + " bytes)");
    }
    for (std::size_t i = 0; i < kMagic.size(); ++i) {
        if (std::to_integer<char>(bytes[i]) != kMagic[i]) {
            return damaged("it does not begin KTIN, so it is no surface Katana wrote");
        }
    }
    Reader reader(bytes.subspan(kMagic.size()));
    const std::uint64_t version = reader.little(4);
    if (version > kTinCodecVersion) {
        return makeError(ErrorCode::Unsupported,
                         "a stored surface was written by a newer version of Katana",
                         "version=" + std::to_string(version) +
                             " supported=" + std::to_string(kTinCodecVersion));
    }
    if (version == 0) {
        return damaged("its version is 0, which no Katana writes");
    }
    const std::uint64_t vertexCount = reader.little(8);
    const std::uint64_t triangleCount = reader.little(8);
    // Each count against what the bytes can hold BEFORE they are multiplied
    // or allocated for: a count the record chose must not choose the size of
    // an allocation, nor overflow the sum below.
    const std::uint64_t body = bytes.size() - kHeaderBytes;
    if (vertexCount > body / kVertexBytes || triangleCount > body / kTriangleBytes) {
        return damaged("it counts more vertices or triangles than its bytes hold");
    }
    const std::uint64_t expected = encodedTinBytes(vertexCount, triangleCount);
    if (expected != bytes.size()) {
        return damaged("it holds " + std::to_string(bytes.size()) + " bytes where its counts say " +
                       std::to_string(expected));
    }
    std::vector<Point3> vertices(static_cast<std::size_t>(vertexCount));
    for (Point3& vertex : vertices) {
        vertex.x = reader.real();
        vertex.y = reader.real();
        vertex.z = reader.real();
    }
    std::vector<TinTriangle> triangles(static_cast<std::size_t>(triangleCount));
    for (TinTriangle& triangle : triangles) {
        for (std::uint32_t& index : triangle) {
            index = static_cast<std::uint32_t>(reader.little(4));
        }
    }
    std::vector<std::uint8_t> masks(static_cast<std::size_t>(triangleCount));
    for (std::uint8_t& mask : masks) {
        mask = reader.byte();
    }
    // The adjacency is derived, and every rule a triangulation keeps checked,
    // by the one constructor of a surface from arrays.
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles), std::move(masks));
    if (!surface) {
        return makeError(surface.error().code,
                         "a stored surface is not a valid triangulation: " +
                             surface.error().message,
                         surface.error().context);
    }
    return surface;
}

Result<TinSurface> decodeTin(std::string_view bytes)
{
    return decodeTin(
        std::span<const std::byte>(reinterpret_cast<const std::byte*>(bytes.data()), bytes.size()));
}

} // namespace katana::terrain
