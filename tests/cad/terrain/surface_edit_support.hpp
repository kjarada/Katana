#pragma once

// Surfaces made by hand for the surface edit verbs' and tools' tests
// (docs/surface_editing.md): every level, count and area they give is worked
// out on paper where a test asserts it.

#include <gtest/gtest.h>

#include <cstdint>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad::testing {

using katana::geometry::Point3;
using katana::terrain::TinSurface;
using katana::terrain::TinTriangle;

inline TinSurface makeSurface(std::vector<Point3> vertices, std::vector<TinTriangle> triangles,
                              std::vector<std::uint8_t> masks = {})
{
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles), std::move(masks));
    EXPECT_TRUE(surface.ok()) << (surface.ok() ? "" : surface.error().describe());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

// [0,10]^2 as two triangles split along the diagonal (0,0)-(10,10).
inline TinSurface squareSurface(double z0 = 0, double z1 = 0, double z2 = 0, double z3 = 0)
{
    return makeSurface({Point3(0, 0, z0), Point3(10, 0, z1), Point3(10, 10, z2), Point3(0, 10, z3)},
                       {{0, 1, 2}, {0, 2, 3}});
}

// [0,10]^2 with the apex (5,5,apex) joined to the four corners at 0.
inline TinSurface pyramidSurface(double apex = 10.0)
{
    return makeSurface({Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 0), Point3(0, 10, 0),
                        Point3(5, 5, apex)},
                       {{0, 1, 4}, {1, 2, 4}, {2, 3, 4}, {0, 4, 3}});
}

// `cells` x `cells` squares of side `cell` from (x0, y0), each split along
// its rising diagonal, the levels from `level(x, y)`.
template <typename Level>
TinSurface gridSurface(int cells, double cell, Level level, double x0 = 0.0, double y0 = 0.0)
{
    std::vector<Point3> vertices;
    for (int j = 0; j <= cells; ++j) {
        for (int i = 0; i <= cells; ++i) {
            const double x = x0 + i * cell;
            const double y = y0 + j * cell;
            vertices.emplace_back(x, y, level(x, y));
        }
    }
    const auto at = [cells](int i, int j) { return static_cast<std::uint32_t>(j * (cells + 1) + i); };
    std::vector<TinTriangle> triangles;
    for (int j = 0; j < cells; ++j) {
        for (int i = 0; i < cells; ++i) {
            triangles.push_back({at(i, j), at(i + 1, j), at(i + 1, j + 1)});
            triangles.push_back({at(i, j), at(i + 1, j + 1), at(i, j + 1)});
        }
    }
    return makeSurface(std::move(vertices), std::move(triangles));
}

inline TinSurface flatGridSurface(int cells, double cell, double z = 0.0, double x0 = 0.0,
                                  double y0 = 0.0)
{
    return gridSurface(cells, cell, [z](double, double) { return z; }, x0, y0);
}

// `surface` added to the document's surfaces as `name`, as one undo step.
inline void putSurface(Document& document, const std::string& name, TinSurface surface,
                       bool visible = true)
{
    katana::terrain::SurfaceStore store = document.surfaces();
    katana::terrain::NamedSurface named;
    named.name = name;
    named.surface = std::make_shared<const TinSurface>(std::move(surface));
    named.source = "test";
    named.visible = visible;
    ASSERT_TRUE(store.add(std::move(named)).ok());
    ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST_SURFACE").ok());
}

inline const TinSurface& surfaceOf(const Document& document, const std::string& name)
{
    static const TinSurface none;
    const auto* named = document.surfaces().find(name);
    EXPECT_NE(named, nullptr) << "no surface " << name;
    return named != nullptr && named->surface ? *named->surface : none;
}

} // namespace katana::cad::testing
