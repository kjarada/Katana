#pragma once

// A TIN surface as bytes, and back: how a project keeps its surfaces
// (docs/terrain.md, "Surfaces in the drawing"). The storage layer may not see
// the terrain layer (tools/check_layering.cmake), so it keeps each surface as
// an opaque record (storage::SurfaceRecord) whose bytes are written and read
// here.
//
// Exact. Every coordinate is its IEEE-754 bit pattern, so a surface reads back
// bit for bit - a negative zero stays negative, 0.1 stays the double nearest
// 0.1 - and the constrained edges (breaklines, a boundary) are kept with the
// triangles they belong to. The adjacency is not stored: TinSurface::create
// derives it, which is also what checks the bytes on the way in, so a damaged
// record is refused with a sentence and never becomes a surface that answers
// wrongly.
//
// Version 1, little-endian throughout:
//
//   "KTIN"                       4 bytes, the magic
//   version                      u32, 1
//   vertex count V, triangle T   u64 each
//   V vertices                   x, y, z as f64 bit patterns (24 bytes each)
//   T triangles                  three u32 vertex indices (12 bytes each)
//   T constraint masks           one byte each, bit k for edge k
//
// so a surface takes 24 + 24 V + 13 T bytes. A version is never changed once
// released; a later layout is a new version read beside this one, and a
// version newer than this build knows is refused with Unsupported rather than
// guessed at.

#include <cstdint>
#include <span>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::terrain {

// The version encodeTin writes.
inline constexpr std::uint32_t kTinCodecVersion = 1;

// The bytes encodeTin writes for a surface of `vertices` and `triangles`,
// without writing them: what a size check before a save needs.
[[nodiscard]] std::uint64_t encodedTinBytes(std::uint64_t vertices, std::uint64_t triangles);

// `surface` as version-1 bytes. Never fails: every TinSurface is encodable.
[[nodiscard]] std::string encodeTin(const TinSurface& surface);

// The surface `bytes` hold. ParseFailure, naming what is wrong, for bytes that
// are not the form - no magic, a header cut short, counts the bytes cannot
// hold, bytes left over; Unsupported for a version newer than this build
// writes; and what TinSurface::create refuses (an index out of range, a
// clockwise triangle, a non-finite coordinate ...) for a triangulation that
// is not valid. Nothing is allocated for a count before the bytes are known
// to hold it.
[[nodiscard]] katana::core::Result<TinSurface> decodeTin(std::span<const std::byte> bytes);
[[nodiscard]] katana::core::Result<TinSurface> decodeTin(std::string_view bytes);

} // namespace katana::terrain
