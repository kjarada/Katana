// The 3D scene colouring a surface by its display style (cad/scene.hpp's
// SceneSurface::display, docs/surface_display.md "In 3D"): the colours
// expected are the ramp table's stops and hand-worked band cuts, drawn with
// the light off so a colour is the style's own.

#include <gtest/gtest.h>

#include <cmath>
#include <memory>
#include <vector>

#include "katana/cad/scene.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace {

using katana::cad::SceneBuilder;
using katana::cad::SceneLayers;
using katana::cad::SceneOptions;
using katana::cad::SceneSurface;
using katana::cad::SurfaceColoring;
using katana::cad::SurfaceDisplayStyle;
using katana::cad::SurfaceFaces;
using katana::cad::SurfaceStyle;
using katana::render::DrawList;
using katana::render::Rgba;
using katana::render::rgba;
using katana::terrain::TinSurface;

// The square 10 x 10 from (0, 0), two triangles, z from `height`.
template <typename Height> TinSurface square(Height height)
{
    std::vector<katana::geometry::Point3> vertices;
    for (const auto& [x, y] : {std::pair{0.0, 0.0}, std::pair{10.0, 0.0}, std::pair{10.0, 10.0},
                               std::pair{0.0, 10.0}}) {
        vertices.emplace_back(x, y, height(x, y));
    }
    auto tin = TinSurface::create(std::move(vertices), {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(tin.ok());
    return std::move(*tin);
}

TinSurface rolling(int cells)
{
    std::vector<katana::geometry::Point3> vertices;
    for (int j = 0; j <= cells; ++j) {
        for (int i = 0; i <= cells; ++i) {
            vertices.emplace_back(4.0 * i, 4.0 * j, 20.0 + 3.0 * std::sin(i * 0.7) + 2.0 * std::cos(j * 0.4));
        }
    }
    std::vector<katana::terrain::TinTriangle> triangles;
    const auto at = [cells](int i, int j) { return static_cast<std::uint32_t>(j * (cells + 1) + i); };
    for (int j = 0; j < cells; ++j) {
        for (int i = 0; i < cells; ++i) {
            triangles.push_back({at(i, j), at(i + 1, j), at(i + 1, j + 1)});
            triangles.push_back({at(i, j), at(i + 1, j + 1), at(i, j + 1)});
        }
    }
    auto tin = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(tin.ok());
    return std::move(*tin);
}

SceneOptions unlit()
{
    SceneOptions options;
    options.drawGrid = false;
    options.lightDirection = katana::render::Vec3(0.0, 0.0, 0.0); // the style's colours as they are
    return options;
}

SurfaceDisplayStyle styled(SurfaceFaces faces)
{
    SurfaceDisplayStyle style;
    style.name = "test";
    style.shading = SurfaceStyle::Shaded;
    style.faces3d = faces;
    return style;
}

SceneSurface itemOf(const TinSurface& tin, const SurfaceDisplayStyle& style)
{
    SceneSurface item;
    item.name = "ground";
    item.surface = &tin;
    katana::cad::applySurfaceDisplay(item, style);
    return item;
}

// Each triangle of `list`: its three corners' colour (asserted equal) and
// its corners' least and greatest x.
struct Face {
    Rgba colour;
    double lowX;
    double highX;
};
std::vector<Face> facesOf(const DrawList& list)
{
    std::vector<Face> faces;
    for (const auto& triangle : list.triangles) {
        const katana::render::VertexIndex corners[3] = {triangle.a, triangle.b, triangle.c};
        Face face{list.colors[corners[0]], 1e300, -1e300};
        for (const auto corner : corners) {
            EXPECT_EQ(list.colors[corner], face.colour);
            face.lowX = std::min(face.lowX, list.positions[corner].x);
            face.highX = std::max(face.highX, list.positions[corner].x);
        }
        faces.push_back(face);
    }
    return faces;
}

} // namespace

TEST(SceneSurfaceStyle, AStyleOnTheTerrainRampColoursASurfaceAsTheSceneAlwaysDid)
{
    const TinSurface tin = rolling(12);
    for (const SurfaceStyle shading : {SurfaceStyle::Shaded, SurfaceStyle::ShadedWithEdges}) {
        SceneSurface legacy;
        legacy.name = "ground";
        legacy.surface = &tin;
        legacy.style = shading;
        legacy.coloring = SurfaceColoring::Elevation;
        SurfaceDisplayStyle style = styled(SurfaceFaces::Elevation);
        style.shading = shading;
        style.ramp = "terrain";
        const SceneSurface item = itemOf(tin, style);
        SceneBuilder builder;
        SceneLayers before;
        SceneLayers after;
        builder.buildTerrain({legacy}, {}, SceneOptions{}, before);
        builder.buildTerrain({item}, {}, SceneOptions{}, after);
        EXPECT_EQ(before.terrain.positions.size(), after.terrain.positions.size());
        EXPECT_EQ(before.terrain.colors, after.terrain.colors);
        EXPECT_EQ(before.terrain.triangles.size(), after.terrain.triangles.size());
        EXPECT_EQ(before.edges.colors, after.edges.colors);
        EXPECT_EQ(before.rampLow, after.rampLow);
        EXPECT_EQ(before.rampHigh, after.rampHigh);
    }
}

TEST(SceneSurfaceStyle, ABandedStyleCutsEachTriangleAtItsBandEdgeAndColoursEachPartItsBand)
{
    // z = x from 0 to 10 in two bands, cut at 5. Triangle (0,0) (10,0)
    // (10,10) gives a triangle below 5 and a quadrilateral above (three
    // triangles drawn); (0,0) (10,10) (0,10) a quadrilateral below and a
    // triangle above (three more): six.
    const TinSurface tin = square([](double x, double) { return x; });
    SurfaceDisplayStyle style = styled(SurfaceFaces::Elevation);
    style.banded = true;
    style.bands = 2;
    SceneBuilder builder;
    DrawList list;
    builder.appendSurfaces({itemOf(tin, style)}, unlit(), list);
    const auto faces = facesOf(list);
    ASSERT_EQ(faces.size(), 6u);
    // Band 0 is the terrain ramp's start, band 1 its end.
    for (const Face& face : faces) {
        if (face.highX <= 5.0 + 1e-12) {
            EXPECT_EQ(face.colour, rgba(40, 70, 140));
        } else {
            EXPECT_GE(face.lowX, 5.0 - 1e-12);
            EXPECT_EQ(face.colour, rgba(245, 245, 245));
        }
    }
}

TEST(SceneSurfaceStyle, SlopeFacesTakeTheStylesRangeAndRamp)
{
    // z = x / 2: every face 50 %, the middle of 0 to 100, which on the grey
    // ramp is its middle stop (128, 128, 128).
    const TinSurface tin = square([](double x, double) { return x / 2.0; });
    SurfaceDisplayStyle style = styled(SurfaceFaces::Slope);
    style.ramp = "grey";
    style.rangeMin = 0.0;
    style.rangeMax = 100.0;
    SceneBuilder builder;
    DrawList list;
    builder.appendSurfaces({itemOf(tin, style)}, unlit(), list);
    const auto faces = facesOf(list);
    ASSERT_EQ(faces.size(), 2u);
    for (const Face& face : faces) {
        EXPECT_EQ(face.colour, rgba(128, 128, 128));
    }
}

TEST(SceneSurfaceStyle, AspectFacesAreTheColourOfTheSectorTheyLookTo)
{
    // z = 10 - y falls towards the north: aspect 0, sector N.
    const TinSurface tin = square([](double, double y) { return 10.0 - y; });
    SceneBuilder builder;
    DrawList list;
    builder.appendSurfaces({itemOf(tin, styled(SurfaceFaces::Aspect))}, unlit(), list);
    const auto north = katana::cad::aspectSectorColour(0);
    for (const Face& face : facesOf(list)) {
        EXPECT_EQ(face.colour, rgba(north.r, north.g, north.b, north.a));
    }
}

TEST(SceneSurfaceStyle, SmoothStopsColourEachVertexByItsHeight)
{
    const TinSurface tin = square([](double x, double) { return x; });
    SurfaceDisplayStyle style = styled(SurfaceFaces::Elevation);
    style.stops = {{0.0, {0, 0, 0, 255}}, {10.0, {200, 100, 50, 255}}};
    SceneBuilder builder;
    DrawList list;
    builder.appendSurfaces({itemOf(tin, style)}, unlit(), list);
    ASSERT_EQ(list.positions.size(), 4u);
    for (std::size_t i = 0; i < 4; ++i) {
        EXPECT_EQ(list.colors[i], list.positions[i].x == 0.0 ? rgba(0, 0, 0) : rgba(200, 100, 50));
    }
}

TEST(SceneSurfaceStyle, ASurfaceWithARangeOfItsOwnDoesNotStretchTheOneRamp)
{
    const TinSurface low = square([](double x, double) { return x; });                // 0 to 10
    const TinSurface high = square([](double x, double) { return 100.0 + 10.0 * x; }); // 100 to 200
    SceneSurface plain;
    plain.name = "low";
    plain.surface = &low;
    plain.style = SurfaceStyle::Shaded;
    SurfaceDisplayStyle own = styled(SurfaceFaces::Elevation);
    own.rangeMin = 100.0;
    own.rangeMax = 200.0;
    SceneBuilder builder;
    SceneLayers layers;
    builder.buildTerrain({plain, itemOf(high, own)}, {}, unlit(), layers);
    EXPECT_EQ(layers.rampLow, 0.0);
    EXPECT_EQ(layers.rampHigh, 10.0);
}

TEST(SceneSurfaceStyle, TheLegendIsTheFirstStyledSurfacesColours)
{
    const TinSurface tin = square([](double x, double) { return x; });
    SurfaceDisplayStyle style = styled(SurfaceFaces::Elevation);
    style.name = "Bands";
    style.banded = true;
    style.bands = 4;
    SceneBuilder builder;
    SceneLayers layers;
    builder.buildTerrain({itemOf(tin, style)}, {}, unlit(), layers);
    ASSERT_TRUE(layers.legend.has_value());
    EXPECT_EQ(layers.legend->title, "Elevation");
    EXPECT_EQ(layers.legend->entries.size(), 4u);
    EXPECT_EQ(layers.legendSurface, "ground");
    EXPECT_EQ(layers.legendStyle, "Bands");
    // A surface with no style has the one ramp's legend, not this one.
    SceneSurface plain;
    plain.surface = &tin;
    builder.buildTerrain({plain}, {}, unlit(), layers);
    EXPECT_FALSE(layers.legend.has_value());
    EXPECT_TRUE(layers.hasRamp());
}

TEST(SceneSurfaceStyle, ApplyingAStyleSetsTheShadingAndKeepsACopyOfTheStyle)
{
    const TinSurface tin = square([](double x, double) { return x; });
    SurfaceDisplayStyle style = styled(SurfaceFaces::Slope);
    style.shading = SurfaceStyle::Wireframe;
    style.flatColour = {1, 2, 3, 255};
    SceneSurface item = itemOf(tin, style);
    EXPECT_EQ(item.style, SurfaceStyle::Wireframe);
    ASSERT_TRUE(item.display.has_value());
    EXPECT_EQ(*item.display, style);
    EXPECT_EQ(item.coloring, SurfaceColoring::Slope);
    EXPECT_EQ(item.flatColor, rgba(1, 2, 3));
    // A copy: changing the style afterwards changes nothing drawn.
    style.ramp = "grey";
    EXPECT_EQ(item.display->ramp, "terrain");
}
