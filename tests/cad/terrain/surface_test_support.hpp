#pragma once

// What the tests of the drawing's surfaces share (docs/terrain.md, "Surfaces
// in the drawing"): small surfaces whose every height is known by
// construction, and a scratch project directory of a test's own.

#include <gtest/gtest.h>

#include <filesystem>
#include <memory>
#include <string>
#include <system_error>
#include <utility>

#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad::testing {

// The square [x0, x0 + size] x [y0, y0 + size] as two counter-clockwise
// triangles on the plane z = z0 + dz * x / size: its heights run from z0 on
// the west edge to z0 + dz on the east.
inline std::shared_ptr<const katana::terrain::TinSurface>
tiltedSquare(double z0, double dz = 0.0, double size = 10.0, double x0 = 0.0, double y0 = 0.0)
{
    using katana::terrain::Point3;
    auto made = katana::terrain::TinSurface::create(
        {Point3(x0, y0, z0), Point3(x0 + size, y0, z0 + dz), Point3(x0 + size, y0 + size, z0 + dz),
         Point3(x0, y0 + size, z0)},
        {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(made.ok()) << (made.ok() ? "" : made.error().describe());
    return std::make_shared<const katana::terrain::TinSurface>(
        made.ok() ? std::move(made).value() : katana::terrain::TinSurface{});
}

// `store` with one more surface; the test fails if the store refuses it.
inline katana::terrain::SurfaceStore
with(katana::terrain::SurfaceStore store, std::string name,
     std::shared_ptr<const katana::terrain::TinSurface> surface, std::string style = {},
     bool visible = true, std::string source = "test")
{
    katana::terrain::NamedSurface named;
    named.name = std::move(name);
    named.surface = std::move(surface);
    named.source = std::move(source);
    named.style = std::move(style);
    named.visible = visible;
    const auto added = store.add(std::move(named));
    EXPECT_TRUE(added.ok()) << (added.ok() ? "" : added.error().describe());
    return store;
}

// A directory of the test's own, removed by name: ctest runs cases at the
// same time, and a shared one would be removed from under another.
struct ScratchDirectory {
    std::filesystem::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(std::filesystem::temp_directory_path() / ("katana-cad-surfaces-" + name))
    {
        std::error_code ignored;
        std::filesystem::remove_all(path, ignored);
        std::filesystem::create_directories(path, ignored);
    }
    ~ScratchDirectory()
    {
        std::error_code ignored;
        std::filesystem::remove_all(path, ignored);
    }
    ScratchDirectory(const ScratchDirectory&) = delete;
    ScratchDirectory& operator=(const ScratchDirectory&) = delete;
};

} // namespace katana::cad::testing
