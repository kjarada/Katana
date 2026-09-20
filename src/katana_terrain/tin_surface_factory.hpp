#pragma once

// Trusted construction of a TinSurface from arrays the module produced itself
// (triangulation output): skips the validation and the adjacency derivation of
// TinSurface::create(), which would cost a sort over all half-edges.

#include <cstdint>
#include <utility>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::terrain::detail {

struct TinSurfaceFactory {
    // Preconditions (not checked): indices in range, triangles counter-clockwise
    // by exact orientation, `neighbors` consistent with `triangles`,
    // `constrainedEdges` one mask per triangle and symmetric across shared edges.
    [[nodiscard]] static katana::core::Result<TinSurface>
    assemble(std::vector<Point3> vertices, std::vector<TinTriangle> triangles,
             std::vector<TinNeighbors> neighbors, std::vector<std::uint8_t> constrainedEdges)
    {
        TinSurface surface;
        surface.vertices_ = std::move(vertices);
        surface.triangles_ = std::move(triangles);
        surface.neighbors_ = std::move(neighbors);
        surface.constrainedEdges_ = std::move(constrainedEdges);
        if (const katana::core::Status status = surface.finalize(); !status) {
            return status.error();
        }
        return surface;
    }
};

} // namespace katana::terrain::detail
