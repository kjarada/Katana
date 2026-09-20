// Document -> DrawList (PLAN.MD Phase 15, Rule 3).
//
// The assertions worth making about a scene builder are structural: that the
// renderer is given the geometry the model holds, that it is given it ONCE
// rather than per-triangle, that a tessellation honours its tolerance, and that
// building a scene cannot modify the document.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/cad/scene.hpp"
#include "katana/commands/entity_commands.hpp"

using katana::cad::chordArc;
using katana::cad::chordCircle;
using katana::cad::Document;
using katana::cad::SceneBuilder;
using katana::cad::SceneOptions;
using katana::cad::SceneSurface;
using katana::cad::SurfaceColoring;
using katana::cad::SurfaceStyle;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::render::DrawList;
using katana::terrain::TinSurface;

namespace {

constexpr double kTwoPi = 6.283185307179586476925;

// Two triangles over a 100 x 100 square, rising 10 to the east.
TinSurface ramp()
{
    std::vector<katana::geometry::Point3> vertices = {
        {0.0, 0.0, 0.0}, {100.0, 0.0, 10.0}, {100.0, 100.0, 10.0}, {0.0, 100.0, 0.0}};
    std::vector<katana::terrain::TinTriangle> triangles = {{0, 1, 2}, {0, 2, 3}};
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(surface.ok());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

SceneOptions plainOptions()
{
    SceneOptions options;
    options.drawGrid = false; // the grid would swamp the counts below
    return options;
}

} // namespace

TEST(CadScene, ChordingAnArcHonoursItsSagittaTolerance)
{
    const Arc2 arc{Point2(0.0, 0.0), 50.0, 0.0, kTwoPi * 0.25};

    for (double tolerance : {1.0, 0.1, 0.01, 0.001}) {
        const auto points = chordArc(arc, tolerance);
        ASSERT_GE(points.size(), 2u);
        EXPECT_EQ(points.front(), arc.startPoint());
        EXPECT_NEAR(points.back().distanceTo(arc.endPoint()), 0.0, 1e-9);

        // The true test: the midpoint of every chord must be within the
        // tolerance of the arc. That is the definition of the sagitta rule.
        for (std::size_t i = 1; i < points.size(); ++i) {
            const Point2 mid = (points[i - 1] + points[i]) * 0.5;
            const double deviation = std::abs(mid.distanceTo(arc.center) - arc.radius);
            EXPECT_LE(deviation, tolerance * 1.001)
                << "tolerance " << tolerance << " chord " << i;
        }
        // And it must not be wasteful: a tighter tolerance needs more chords.
        EXPECT_LE(points.size(), 8193u);
    }

    EXPECT_GT(chordArc(arc, 0.001).size(), chordArc(arc, 0.1).size())
        << "a tighter tolerance must produce more chords";
}

TEST(CadScene, ChordingACircleDoesNotRepeatTheClosingPoint)
{
    const Circle2 circle{Point2(10.0, 20.0), 5.0};
    const auto points = chordCircle(circle, 0.01);
    ASSERT_GE(points.size(), 3u);
    // The polyline is closed by the caller, so a duplicate first/last point
    // would draw a zero-length segment at the seam.
    EXPECT_GT(points.front().distanceTo(points.back()), 1e-6);
    for (const Point2& p : points) {
        EXPECT_NEAR(p.distanceTo(circle.center), circle.radius, 1e-9);
    }
}

TEST(CadScene, ChordingADegenerateArcDoesNotHang)
{
    // Zero radius, zero sweep and an absurd tolerance have all produced
    // infinite loops in tessellators before.
    EXPECT_TRUE(chordArc(Arc2{Point2(0, 0), 0.0, 0.0, 1.0}, 0.01).empty());
    EXPECT_LE(chordArc(Arc2{Point2(0, 0), 10.0, 0.0, 0.0}, 0.01).size(), 2u);
    EXPECT_GE(chordArc(Arc2{Point2(0, 0), 10.0, 0.0, kTwoPi}, 1e9).size(), 2u);
    EXPECT_LE(chordArc(Arc2{Point2(0, 0), 1e9, 0.0, kTwoPi}, 1e-12).size(), 8193u)
        << "a hairline tolerance on a huge radius must still be bounded";
}

TEST(CadScene, SurfaceVerticesAreSharedSoTheTransformRunsOncePerVertex)
{
    // The whole reason DrawList is indexed. An unshaded wireframe of a 4-vertex
    // surface must submit 4 vertices, not 3 per triangle.
    const TinSurface surface = ramp();
    Document document;
    SceneBuilder builder;
    DrawList list;

    SceneSurface item;
    item.name = "Existing";
    item.surface = &surface;
    item.style = SurfaceStyle::Wireframe;
    item.coloring = SurfaceColoring::Flat;

    SceneOptions options = plainOptions();
    options.drawEntities = false;
    builder.build(document, {item}, options, list);

    EXPECT_TRUE(list.triangles.empty()) << "wireframe draws no filled triangles";
    EXPECT_EQ(list.lines.size(), 5u) << "two triangles share one edge: 3 + 3 - 1 = 5";
}

TEST(CadScene, ShadedSurfacesProduceOneTrianglePerFacet)
{
    const TinSurface surface = ramp();
    Document document;
    SceneBuilder builder;
    DrawList list;

    SceneSurface item;
    item.surface = &surface;
    item.style = SurfaceStyle::Shaded;

    SceneOptions options = plainOptions();
    options.drawEntities = false;
    builder.build(document, {item}, options, list);

    EXPECT_EQ(list.triangles.size(), surface.triangleCount());
    EXPECT_TRUE(list.lines.empty());
    EXPECT_TRUE(list.allFinite());
}

TEST(CadScene, AHiddenOrInvisibleSurfaceContributesNothing)
{
    const TinSurface surface = ramp();
    Document document;
    SceneBuilder builder;
    DrawList list;
    SceneOptions options = plainOptions();
    options.drawEntities = false;

    SceneSurface item;
    item.surface = &surface;
    item.style = SurfaceStyle::Hidden;
    builder.build(document, {item}, options, list);
    EXPECT_TRUE(list.empty());

    item.style = SurfaceStyle::Shaded;
    item.visible = false;
    builder.build(document, {item}, options, list);
    EXPECT_TRUE(list.empty());

    item.surface = nullptr;
    item.visible = true;
    builder.build(document, {item}, options, list);
    EXPECT_TRUE(list.empty()) << "a null surface must be skipped, not dereferenced";
}

TEST(CadScene, VerticalExaggerationScalesAboutTheDatumAndNothingElse)
{
    const TinSurface surface = ramp(); // z from 0 to 10
    Document document;
    SceneBuilder builder;
    DrawList list;

    SceneOptions options = plainOptions();
    options.drawEntities = false;
    options.verticalExaggeration = 5.0;
    options.exaggerationDatum = 2.0;

    SceneSurface item;
    item.surface = &surface;
    item.style = SurfaceStyle::Wireframe;
    builder.build(document, {item}, options, list);

    const auto box = list.bounds();
    ASSERT_FALSE(box.empty());
    // z = datum + (z - datum) * 5: 0 -> -8, 10 -> 42.
    EXPECT_NEAR(box.min.z, -8.0, 1e-9);
    EXPECT_NEAR(box.max.z, 42.0, 1e-9);
    // Plan coordinates must be untouched: exaggerating height is not a licence
    // to move anything horizontally.
    EXPECT_NEAR(box.min.x, 0.0, 1e-9);
    EXPECT_NEAR(box.max.x, 100.0, 1e-9);

    // And sceneBounds must agree with what was actually built, or framing the
    // view would cut off the top of the exaggerated surface.
    const auto declared = katana::cad::sceneBounds(document, {item}, options);
    EXPECT_NEAR(declared.min.z, box.min.z, 1e-9);
    EXPECT_NEAR(declared.max.z, box.max.z, 1e-9);
}

TEST(CadScene, AnInvalidExaggerationFallsBackToOneRatherThanCollapsingTheScene)
{
    const TinSurface surface = ramp();
    Document document;
    SceneBuilder builder;
    DrawList list;
    SceneOptions options = plainOptions();
    options.drawEntities = false;

    SceneSurface item;
    item.surface = &surface;
    item.style = SurfaceStyle::Wireframe;

    for (double bad : {0.0, -3.0, std::nan(""), std::numeric_limits<double>::infinity()}) {
        options.verticalExaggeration = bad;
        builder.build(document, {item}, options, list);
        const auto box = list.bounds();
        ASSERT_FALSE(box.empty()) << "exaggeration " << bad;
        EXPECT_NEAR(box.min.z, 0.0, 1e-9) << "exaggeration " << bad;
        EXPECT_NEAR(box.max.z, 10.0, 1e-9) << "exaggeration " << bad;
    }
}

TEST(CadScene, DrawingEntitiesDoesNotModifyTheDocument)
{
    // Rule 3, asserted rather than assumed: the renderer path is read-only.
    Document document;
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 5.0),
                                                           document.currentAttributes()))
                    .ok());
    const auto before = document.model().entities.ids();
    const auto nextBefore = document.model().entities.nextId();

    SceneBuilder builder;
    DrawList list;
    builder.build(document, {}, plainOptions(), list);

    EXPECT_EQ(document.model().entities.ids(), before);
    EXPECT_EQ(document.model().entities.nextId(), nextBefore)
        << "building a scene must not consume an entity id";
    EXPECT_FALSE(list.lines.empty());
}

TEST(CadScene, HiddenEntitiesAreNotDrawn)
{
    Document document;
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 5.0),
                                                           document.currentAttributes()))
                    .ok());
    SceneBuilder builder;
    DrawList visible;
    builder.build(document, {}, plainOptions(), visible);
    ASSERT_EQ(visible.lines.size(), 1u);

    // Hide it through a command, as the application would.
    const auto id = document.model().entities.ids().back();
    ASSERT_TRUE(document.execute(katana::commands::setEntityVisible({id}, false)).ok());

    DrawList after;
    builder.build(document, {}, plainOptions(), after);
    EXPECT_TRUE(after.lines.empty());
}

TEST(CadScene, RebuildingIntoTheSameListDoesNotAccumulate)
{
    // The list is reused every frame; a build that appended instead of
    // replacing would grow without bound and look like a memory leak.
    Document document;
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 5.0),
                                                           document.currentAttributes()))
                    .ok());
    SceneBuilder builder;
    DrawList list;
    builder.build(document, {}, plainOptions(), list);
    const std::size_t first = list.positions.size();
    for (int i = 0; i < 5; ++i) {
        builder.build(document, {}, plainOptions(), list);
    }
    EXPECT_EQ(list.positions.size(), first);
}

TEST(CadScene, SelectedEntitiesAreDrawnInTheSelectionColour)
{
    Document document;
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 5.0),
                                                           document.currentAttributes()))
                    .ok());
    const auto id = document.model().entities.ids().back();

    SceneOptions options = plainOptions();
    SceneBuilder builder;
    DrawList list;
    builder.build(document, {}, options, list);
    ASSERT_FALSE(list.colors.empty());
    EXPECT_NE(list.colors.front(), options.selectionColor);

    document.selection().add(id);
    builder.build(document, {}, options, list);
    ASSERT_FALSE(list.colors.empty());
    EXPECT_EQ(list.colors.front(), options.selectionColor);
}

TEST(CadScene, TheGridIsBoundedHoweverFarTheViewIsZoomedOut)
{
    Document document;
    SceneBuilder builder;
    DrawList list;
    SceneOptions options;
    options.drawEntities = false;
    options.drawGrid = true;
    options.gridLines = 100'000; // nonsense from a zoomed-out view
    builder.build(document, {}, options, list);

    // Clamped to 400 each way: 2 * 400 + 1 lines in each direction.
    EXPECT_LE(list.lines.size(), 2u * (2u * 400u + 1u));
    EXPECT_GT(list.lines.size(), 0u);

    options.gridSpacing = 0.0;
    builder.build(document, {}, options, list);
    EXPECT_TRUE(list.lines.empty()) << "a zero spacing must draw nothing, not loop forever";
}

TEST(CadScene, SceneBoundsCoverBothSurfacesAndDrawingGeometry)
{
    const TinSurface surface = ramp();
    Document document;
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(-50.0, -50.0),
                                                           Point2(-10.0, -10.0),
                                                           document.currentAttributes()))
                    .ok());

    SceneSurface item;
    item.surface = &surface;
    SceneOptions options = plainOptions();

    const auto box = katana::cad::sceneBounds(document, {item}, options);
    ASSERT_FALSE(box.empty());
    EXPECT_NEAR(box.min.x, -50.0, 1e-9) << "the drawing extends further west than the surface";
    EXPECT_NEAR(box.max.x, 100.0, 1e-9) << "the surface extends further east than the drawing";
    EXPECT_NEAR(box.max.z, 10.0, 1e-9);
}
