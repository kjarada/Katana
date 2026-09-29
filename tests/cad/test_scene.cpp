// Document -> DrawList (PLAN.MD Phase 15, Rule 3).
//
// The assertions worth making about a scene builder are structural: that the
// renderer is given the geometry the model holds, that it is given it ONCE
// rather than per-triangle, that a tessellation honours its tolerance, and that
// building a scene cannot modify the document.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <format>
#include <functional>
#include <string>
#include <utility>

#include "katana/cad/scene.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/task_pool.hpp"
#include "katana/core/text.hpp"

using katana::cad::Document;
using katana::cad::SceneBuilder;
using katana::cad::SceneOptions;
using katana::cad::SceneSurface;
using katana::cad::SurfaceColoring;
using katana::cad::SurfaceStyle;
using katana::geometry::Point2;
using katana::render::DrawList;
using katana::terrain::TinSurface;

namespace {

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

TEST(CadScene, TheGridIsBoundedHoweverFineASpacingIsAskedFor)
{
    Document document;
    SceneBuilder builder;
    DrawList list;
    SceneOptions options;
    options.drawEntities = false;
    options.drawGrid = true;
    options.gridSpacing = 1.0e-6; // nonsense: 10^8 cells across a 100 m patch
    builder.build(document, {}, options, list);

    // The spacing is doubled until at most 200 cells span each side, so at
    // most 201 lines each way of at most 200 cells each: 2 * 201 * 200.
    EXPECT_LE(list.lines.size(), 2u * 201u * 200u);
    EXPECT_GT(list.lines.size(), 0u);
}

TEST(CadScene, AnAutomaticGridIsSizedToTheSceneAndCutIntoCells)
{
    // Nothing to draw, so the grid stands round the default 100 m patch
    // (-50..50 each way). Worked by hand from emitGrid's rule:
    //   span 100, 16 cells wanted -> 6.25 m -> the 1-2-5 step above it, 10 m;
    //   reach 15% of the span = 15 m, so x runs over -65..65:
    //   floor(-6.5) = -7 .. ceil(6.5) = 7, 14 cells and 15 lines each way;
    //   one DrawLine per cell: 15 lines x 14 cells x 2 directions = 420.
    // A full-length line per grid line would be 30; cut per cell it is 420,
    // which is what keeps a long diagonal from touching every tile.
    Document document;
    SceneBuilder builder;
    DrawList list;
    SceneOptions options;
    options.drawEntities = false;
    options.drawGrid = true;
    builder.build(document, {}, options, list);
    EXPECT_EQ(list.lines.size(), 420u);
    const auto box = list.bounds();
    EXPECT_NEAR(box.min.x, -70.0, 1e-9);
    EXPECT_NEAR(box.max.x, 70.0, 1e-9);

    // A scene 100 times larger gets a grid 100 times coarser and the same
    // number of cells, not a sea of 10 m lines.
    std::vector<katana::geometry::Point3> vertices = {
        {0.0, 0.0, 0.0}, {10000.0, 0.0, 10.0}, {10000.0, 10000.0, 10.0}, {0.0, 10000.0, 0.0}};
    auto big = TinSurface::create(std::move(vertices), {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(big.ok());
    SceneSurface item;
    item.surface = &*big;
    item.style = SurfaceStyle::Shaded;
    builder.build(document, {item}, options, list);
    // span 10 000 -> 625 -> 1000 m cells; reach 1500: -1500..11500 is
    // floor(-1.5) = -2 .. ceil(11.5) = 12, 14 cells, 15 lines: 420 again.
    EXPECT_EQ(list.lines.size(), 420u);
}

TEST(CadScene, TheGridDrawsItsAxesLastSoNoOtherGridLineBreaksThem)
{
    // The grid writes no depth (renderLayers), so where two of its lines
    // cross the later covers the earlier. Round the default 100 m patch the
    // origin is in range (-70..70, AnAutomaticGridIsSizedToTheSceneAndCutIntoCells),
    // so both axes are drawn, 2 px against every other line's 1: 14 cells
    // each, the last 2 x 14 = 28 of the 420 lines, and none before them.
    Document document;
    SceneBuilder builder;
    DrawList list;
    SceneOptions options;
    options.drawEntities = false;
    options.drawGrid = true;
    builder.build(document, {}, options, list);
    ASSERT_EQ(list.lines.size(), 420u);
    for (std::size_t k = 0; k < list.lines.size(); ++k) {
        EXPECT_EQ(list.lines[k].width, k < 392u ? 1.0f : 2.0f) << "line " << k;
    }
}

TEST(CadScene, TheGridStandsOnTheLowestSurface)
{
    // A surface from 0 to 10 lifted to 250..260: the grid goes under it at
    // 250, where the ground is, not at a z = 0 two hundred metres below.
    // Exactly in the plane of the surface's lowest part; drawn as a backdrop
    // it never shows through it (SceneFrame.AFlatSurfaceAtTheDatumCoversTheGridStandingUnderIt).
    std::vector<katana::geometry::Point3> vertices = {
        {0.0, 0.0, 250.0}, {100.0, 0.0, 260.0}, {100.0, 100.0, 260.0}, {0.0, 100.0, 250.0}};
    auto surface = TinSurface::create(std::move(vertices), {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(surface.ok());
    SceneSurface item;
    item.surface = &*surface;
    item.style = SurfaceStyle::Shaded;
    Document document;
    SceneBuilder builder;
    katana::cad::SceneLayers layers;
    SceneOptions options;
    builder.buildTerrain({item}, {}, options, layers);
    builder.buildEntities(document, {item}, options, layers);
    builder.buildGrid(options, layers);
    ASSERT_FALSE(layers.grid.lines.empty());
    const auto box = layers.grid.bounds();
    EXPECT_NEAR(box.min.z, 250.0, 1e-9);
    EXPECT_NEAR(box.max.z, 250.0, 1e-9);
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

TEST(CadScene, ByLayerEntitiesAreDrawnInTheirLayersColour)
{
    // The 3D view used to read the entity's own colour or a fixed default and
    // ignore the layer entirely. ByLayer is the DEFAULT for a new entity, so
    // everything drawn on a coloured layer came out the default grey while the
    // 2D view showed it correctly - the two views disagreed about the common
    // case, not a corner one.
    Document document;

    katana::entity::Layer kerb;
    kerb.name = "kerb";
    kerb.color = katana::entity::Color{255, 0, 0, 255};
    ASSERT_TRUE(document.execute(katana::commands::createLayer(kerb)).ok());
    ASSERT_TRUE(document.setCurrentLayer("kerb").ok());

    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 5.0),
                                                          document.currentAttributes()))
                    .ok());

    SceneOptions options = plainOptions();
    options.defaultColor = katana::render::rgba(1, 2, 3); // deliberately not red
    SceneBuilder builder;
    DrawList list;
    builder.build(document, {}, options, list);

    ASSERT_FALSE(list.colors.empty());
    EXPECT_EQ(list.colors.front(), katana::render::rgba(255, 0, 0, 255))
        << "a ByLayer entity must take its layer's colour, not the scene default";
    EXPECT_NE(list.colors.front(), options.defaultColor);
}

// There is deliberately no scene test for a named style overriding the layer,
// because no command creates a Style: the table can only be populated by
// loading a project that already contains one. resolveDisplay's own tests cover
// the chain. Recorded in PLAN.MD Phase 09 as an outstanding gap rather than
// left as a test that cannot be written.

namespace {

katana::entity::Linetype dashedLinetype()
{
    katana::entity::Linetype linetype;
    linetype.name = "dashed";
    linetype.pattern = {katana::entity::LinetypeElement{1.0},
                        katana::entity::LinetypeElement{-0.5}};
    return linetype;
}

// Puts a dashed linetype on a layer and returns a document using it.
void useDashedLayer(Document& document)
{
    ASSERT_TRUE(document.execute(katana::commands::createLinetype(dashedLinetype())).ok());
    katana::entity::Layer layer;
    layer.name = "fence";
    layer.linetype = "dashed";
    ASSERT_TRUE(document.execute(katana::commands::createLayer(layer)).ok());
    ASSERT_TRUE(document.setCurrentLayer("fence").ok());
}

} // namespace

TEST(CadScene, EveryCurveKindIsDashedIncludingAPlainLine)
{
    // The half-feature this catches: a Segment2 that adds its DrawLine directly
    // instead of going through the shared emitter stays SOLID in 3D while
    // polylines, arcs and circles dash. No compiler complains, and it is the
    // most confusing possible outcome - the same linetype works on some
    // entities and not others.
    SceneOptions options = plainOptions();
    SceneBuilder builder;

    struct Case {
        const char* what;
        katana::commands::CommandPtr (*make)(const katana::commands::EntityAttributes&);
    };

    const auto lineCount = [&](const std::function<void(Document&)>& draw) {
        Document document;
        useDashedLayer(document);
        draw(document);
        DrawList list;
        builder.build(document, {}, options, list);
        return list.lines.size();
    };

    // A 20 m run of a 1.5 m period is about 14 dashes; anything that stayed
    // solid would be exactly 1 line (or 4 for the closed shapes).
    const std::size_t line = lineCount([](Document& d) {
        ASSERT_TRUE(d.execute(katana::commands::createLine(Point2(0, 0), Point2(20, 0),
                                                           d.currentAttributes()))
                        .ok());
    });
    EXPECT_GT(line, 5u) << "a plain Segment2 must dash too, not stay solid";

    const std::size_t polyline = lineCount([](Document& d) {
        katana::geometry::Polyline2 p;
        p.vertices = {Point2(0, 0), Point2(20, 0), Point2(20, 20)};
        ASSERT_TRUE(d.execute(katana::commands::createPolyline(p, d.currentAttributes())).ok());
    });
    EXPECT_GT(polyline, 10u);

    const std::size_t circle = lineCount([](Document& d) {
        ASSERT_TRUE(
            d.execute(katana::commands::createCircle(Point2(0, 0), 10.0, d.currentAttributes()))
                .ok());
    });
    EXPECT_GT(circle, 10u);

    const std::size_t arc = lineCount([](Document& d) {
        ASSERT_TRUE(d.execute(katana::commands::createArc(
                                  katana::geometry::Arc2{Point2(0, 0), 10.0, 0.0, 1.5},
                                  d.currentAttributes()))
                        .ok());
    });
    EXPECT_GT(arc, 5u);
}

TEST(CadScene, TurningLinetypesOffDrawsEverythingSolid)
{
    Document document;
    useDashedLayer(document);
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0, 0), Point2(20, 0),
                                                          document.currentAttributes()))
                    .ok());

    SceneBuilder builder;
    DrawList dashed;
    SceneOptions options = plainOptions();
    builder.build(document, {}, options, dashed);

    options.drawLinetypes = false;
    DrawList solid;
    builder.build(document, {}, options, solid);

    EXPECT_GT(dashed.lines.size(), 5u);
    EXPECT_EQ(solid.lines.size(), 1u) << "off must mean one line, not a shorter pattern";
}

TEST(CadScene, ADashedPathPastTheBudgetIsDrawnWholeAndSolid)
{
    // All or nothing. A truncating implementation would emit the budget's worth
    // of dashes and stop, silently drawing a 10 km boundary as a 1 km one.
    Document document;
    useDashedLayer(document);
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0, 0), Point2(100000.0, 0),
                                                          document.currentAttributes()))
                    .ok());

    SceneOptions options = plainOptions();
    options.maximumDashSpans = 100;
    SceneBuilder builder;
    DrawList list;
    builder.build(document, {}, options, list);

    ASSERT_EQ(list.lines.size(), 1u) << "the whole path, solid";
    const auto box = list.bounds();
    ASSERT_FALSE(box.empty());
    EXPECT_NEAR(box.max.x, 100000.0, 1e-6) << "the line must still reach its far end";
}

TEST(CadScene, AnUnknownLinetypeNameDrawsSolidRatherThanFailing)
{
    // Layer::linetype has always been a free string with no table behind it, so
    // every project in the field can name a pattern that does not exist.
    Document document;
    katana::entity::Layer layer;
    layer.name = "legacy";
    layer.linetype = "no-such-pattern";
    ASSERT_TRUE(document.execute(katana::commands::createLayer(layer)).ok());
    ASSERT_TRUE(document.setCurrentLayer("legacy").ok());
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0, 0), Point2(20, 0),
                                                          document.currentAttributes()))
                    .ok());

    SceneBuilder builder;
    DrawList list;
    builder.build(document, {}, plainOptions(), list);
    EXPECT_EQ(list.lines.size(), 1u) << "drawn solid, exactly as it drew before linetypes existed";
}

// ---- meshes (PLAN.MD 20.2, slice 4) ------------------------------------------------

namespace {

// A tetrahedron: four vertices, four faces, nothing shared with a surface.
katana::geometry::TriangleMesh tetrahedron()
{
    katana::geometry::TriangleMesh mesh;
    mesh.vertices = {katana::geometry::Point3(0, 0, 0), katana::geometry::Point3(10, 0, 0),
                     katana::geometry::Point3(0, 10, 0), katana::geometry::Point3(0, 0, 6)};
    mesh.faces = {{0, 2, 1}, {0, 1, 3}, {1, 2, 3}, {2, 0, 3}};
    return mesh;
}

} // namespace

TEST(SceneMeshes, AShadedMeshGivesTheRendererOneTrianglePerFace)
{
    const auto mesh = tetrahedron();
    katana::cad::SceneMesh item;
    item.name = "pit";
    item.mesh = &mesh;
    item.style = SurfaceStyle::Shaded;

    SceneOptions options;
    options.drawGrid = false;
    options.drawEntities = false;
    DrawList out;
    SceneBuilder builder;
    builder.appendMeshes({item}, options, out);
    EXPECT_EQ(out.triangles.size(), 4u);
    EXPECT_TRUE(out.lines.empty()) << "Shaded draws no edges";
}

TEST(SceneMeshes, EveryEdgeOfEveryFaceIsDrawnBecauseAMeshHasNoNeighbourTable)
{
    // A surface halves its edge count by drawing each shared edge once, which
    // it can because a TIN knows its neighbours. A mesh does not, so all
    // three edges of all four faces are drawn: twelve lines, not six.
    const auto mesh = tetrahedron();
    katana::cad::SceneMesh item;
    item.mesh = &mesh;
    item.style = SurfaceStyle::Wireframe;

    SceneOptions options;
    options.drawGrid = false;
    options.drawEntities = false;
    DrawList out;
    SceneBuilder builder;
    builder.appendMeshes({item}, options, out);
    EXPECT_EQ(out.lines.size(), 12u);
    EXPECT_TRUE(out.triangles.empty()) << "Wireframe fills nothing";
}

TEST(SceneMeshes, APerFaceColourIsUsedAndAMissingOneFallsBackToTheMeshColour)
{
    const auto mesh = tetrahedron();
    katana::cad::SceneMesh item;
    item.mesh = &mesh;
    item.style = SurfaceStyle::Shaded;
    item.flatColor = katana::render::rgba(10, 20, 30);
    // Two of the four faces coloured: the other two take the mesh's colour,
    // so a partly coloured mesh is still wholly drawn.
    item.faceColors = {katana::render::rgba(200, 0, 0), katana::render::rgba(0, 200, 0)};

    SceneOptions options;
    options.drawGrid = false;
    options.drawEntities = false;
    options.lightDirection = katana::render::Vec3(0, 0, 0); // unlit: colours as given
    DrawList out;
    SceneBuilder builder;
    builder.appendMeshes({item}, options, out);
    ASSERT_EQ(out.triangles.size(), 4u);
    EXPECT_EQ(out.colors[out.triangles[0].a], katana::render::rgba(200, 0, 0));
    EXPECT_EQ(out.colors[out.triangles[1].a], katana::render::rgba(0, 200, 0));
    EXPECT_EQ(out.colors[out.triangles[2].a], katana::render::rgba(10, 20, 30));
    EXPECT_EQ(out.colors[out.triangles[3].a], katana::render::rgba(10, 20, 30));
}

TEST(SceneMeshes, AMeshIsFramedAndExaggeratedLikeASurface)
{
    const auto mesh = tetrahedron();
    katana::cad::SceneMesh item;
    item.mesh = &mesh;
    Document document;
    SceneOptions options;
    options.drawEntities = false;

    const auto plain = sceneBounds(document, {}, options, {item});
    EXPECT_EQ(plain.min, katana::render::Vec3(0, 0, 0));
    EXPECT_EQ(plain.max, katana::render::Vec3(10, 10, 6));

    // Exaggeration is applied as the scene is built, so the framing agrees
    // with what is drawn - the same rule surfaces follow.
    options.verticalExaggeration = 3.0;
    const auto tall = sceneBounds(document, {}, options, {item});
    EXPECT_EQ(tall.max.z, 18.0);

    katana::cad::SceneMesh hidden = item;
    hidden.visible = false;
    EXPECT_TRUE(sceneBounds(document, {}, options, {hidden}).empty())
        << "what is not drawn is not framed";
}

TEST(SceneMeshes, AFaceNamingAVertexThatDoesNotExistIsSkippedRatherThanRead)
{
    // The scene builder is the last line of defence: a mesh that never went
    // through validate() must not read past its vertices.
    auto mesh = tetrahedron();
    mesh.faces.push_back({0, 1, 99});
    katana::cad::SceneMesh item;
    item.mesh = &mesh;
    item.style = SurfaceStyle::Shaded;

    SceneOptions options;
    options.drawGrid = false;
    options.drawEntities = false;
    DrawList out;
    SceneBuilder builder;
    builder.appendMeshes({item}, options, out);
    EXPECT_EQ(out.triangles.size(), 4u) << "the four sound faces, and no fifth";
}

// ---- heights, draping, ramp, light, edges (docs/render.md, "The scene") ----------------

namespace {

using katana::cad::LineworkHeights;
using katana::cad::SceneLayers;

// A 3D string from (0, 50) to (100, 50) with its own heights 30 and 40.
katana::entity::Entity threeDString()
{
    katana::entity::Entity entity;
    katana::geometry::Polyline2 line;
    line.vertices = {Point2(0.0, 50.0), Point2(100.0, 50.0)};
    entity.geometry = line;
    katana::entity::setHeights(entity.properties, {30.0, 40.0});
    return entity;
}

// Terrain, drawing, selection and grid as the 3D view builds them.
SceneLayers layersOf(const Document& document, const std::vector<SceneSurface>& surfaces,
                     const SceneOptions& options)
{
    SceneBuilder builder;
    SceneLayers layers;
    builder.buildTerrain(surfaces, {}, options, layers);
    builder.buildEntities(document, surfaces, options, layers);
    builder.buildSelection(document, surfaces, options, layers);
    builder.buildGrid(options, layers);
    return layers;
}

// A flat square TIN at height z, 100 x 100 from (x0, y0).
TinSurface flatAt(double x0, double y0, double z)
{
    std::vector<katana::geometry::Point3> vertices = {
        {x0, y0, z}, {x0 + 100.0, y0, z}, {x0 + 100.0, y0 + 100.0, z}, {x0, y0 + 100.0, z}};
    auto surface = TinSurface::create(std::move(vertices), {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(surface.ok());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

} // namespace

TEST(SceneLinework, AThreeDStringIsDrawnAtItsOwnHeightsEvenOverASurface)
{
    // It was drawn at z = 0 whatever its heights, so a road string surveyed
    // at 30-40 m lay under the surface it was measured on.
    const TinSurface surface = ramp(); // 0..10 under the string
    Document document;
    ASSERT_TRUE(document.execute(katana::commands::createEntities({threeDString()})).ok());
    SceneSurface item;
    item.surface = &surface;
    const SceneLayers layers = layersOf(document, {item}, plainOptions());
    ASSERT_EQ(layers.entities.positions.size(), 2u);
    EXPECT_EQ(layers.entities.positions[0].z, 30.0);
    EXPECT_EQ(layers.entities.positions[1].z, 40.0);
}

TEST(SceneLinework, APlanLineIsDrapedOnTheSurfaceAndBentWhereItCrossesAnEdge)
{
    // ramp() is z = x / 10 over two triangles split along the diagonal from
    // (0, 0) to (100, 100). The line (10, 50)-(90, 50) starts in the upper
    // triangle at z 1, crosses the diagonal at (50, 50) where z is 5, and
    // ends in the lower one at z 9: three vertices, two lines. Without the
    // crossing a straight chord would cut through a ridge or float over a
    // valley of a real TIN.
    const TinSurface surface = ramp();
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(10.0, 50.0), Point2(90.0, 50.0)))
            .ok());
    SceneSurface item;
    item.surface = &surface;
    const SceneLayers layers = layersOf(document, {item}, plainOptions());
    ASSERT_EQ(layers.entities.positions.size(), 3u);
    ASSERT_EQ(layers.entities.lines.size(), 2u);
    EXPECT_NEAR(layers.entities.positions[0].z, 1.0, 1e-9);
    EXPECT_NEAR(layers.entities.positions[1].x, 50.0, 1e-9);
    EXPECT_NEAR(layers.entities.positions[1].z, 5.0, 1e-9);
    EXPECT_NEAR(layers.entities.positions[2].z, 9.0, 1e-9);
}

TEST(SceneLinework, LineworkOffEverySurfaceGoesOnTheDatumOfTheLowestOne)
{
    // Two surfaces at 250 and 270; a line nowhere over either has nothing to
    // drape on and no heights, so it goes on the datum: the lower, 250 - not
    // on z = 0, 250 m under everything.
    const TinSurface low = flatAt(0.0, 0.0, 250.0);
    const TinSurface high = flatAt(200.0, 0.0, 270.0);
    Document document;
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(-500.0, -500.0),
                                                          Point2(-400.0, -450.0)))
                    .ok());
    SceneSurface a;
    a.surface = &low;
    SceneSurface b;
    b.surface = &high;
    const SceneLayers layers = layersOf(document, {a, b}, plainOptions());
    ASSERT_EQ(layers.entities.positions.size(), 2u);
    EXPECT_EQ(layers.entities.positions[0].z, 250.0);
    EXPECT_EQ(layers.entities.positions[1].z, 250.0);
    EXPECT_EQ(layers.datum, 250.0);
}

TEST(SceneLinework, WithNoSurfaceLineworkWithoutHeightsGoesAtTheLowestHeightDrawn)
{
    // No terrain: the datum is the lowest height any entity carries, 30
    // (threeDString), so a plan line sits with the survey rather than 30 m
    // below it at 0.
    Document document;
    ASSERT_TRUE(document.execute(katana::commands::createEntities({threeDString()})).ok());
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 0.0))).ok());
    const SceneLayers layers = layersOf(document, {}, plainOptions());
    ASSERT_EQ(layers.entities.positions.size(), 4u);
    EXPECT_EQ(layers.entities.positions[2].z, 30.0);
    EXPECT_EQ(layers.entities.positions[3].z, 30.0);
}

TEST(SceneLinework, TheModeDecidesBetweenHeightsDrapeAndDatum)
{
    // The 3D string (heights 30, 40) over ramp(): z = x / 10, so 0 under
    // its start at x 0 and 10 under its end at x 100.
    const TinSurface surface = ramp();
    Document document;
    ASSERT_TRUE(document.execute(katana::commands::createEntities({threeDString()})).ok());
    SceneSurface item;
    item.surface = &surface;
    SceneOptions options = plainOptions();

    options.linework = LineworkHeights::Drape; // heights ignored: on the surface
    SceneLayers layers = layersOf(document, {item}, options);
    ASSERT_GE(layers.entities.positions.size(), 2u);
    EXPECT_NEAR(layers.entities.positions.front().z, 0.0, 1e-9);
    EXPECT_NEAR(layers.entities.positions.back().z, 10.0, 1e-9);

    options.linework = LineworkHeights::Datum; // flat on the lowest surface, 0
    layers = layersOf(document, {item}, options);
    ASSERT_EQ(layers.entities.positions.size(), 2u);
    EXPECT_EQ(layers.entities.positions[0].z, 0.0);
    EXPECT_EQ(layers.entities.positions[1].z, 0.0);

    options.linework = LineworkHeights::Heights;
    layers = layersOf(document, {item}, options);
    ASSERT_EQ(layers.entities.positions.size(), 2u);
    EXPECT_EQ(layers.entities.positions[0].z, 30.0);
    EXPECT_EQ(layers.entities.positions[1].z, 40.0);
}

TEST(SceneLinework, SceneBoundsReachTheHeightsTheLineworkIsDrawnAt)
{
    // Framing uses sceneBounds; it must see the string at 30..40 above a
    // surface of 0..10, or zoom-extents cuts the string's top off.
    const TinSurface surface = ramp();
    Document document;
    ASSERT_TRUE(document.execute(katana::commands::createEntities({threeDString()})).ok());
    SceneSurface item;
    item.surface = &surface;
    const auto box = katana::cad::sceneBounds(document, {item}, plainOptions());
    EXPECT_EQ(box.min.z, 0.0);
    EXPECT_EQ(box.max.z, 40.0);
    // A 3D LINE - a Segment2 with two heights - is read by vertex as well.
    Document lines;
    katana::entity::Entity segment;
    segment.geometry = katana::geometry::Segment2{Point2(0.0, 0.0), Point2(10.0, 0.0)};
    katana::entity::setHeights(segment.properties, {12.0, 55.0});
    ASSERT_TRUE(lines.execute(katana::commands::createEntities({segment})).ok());
    const auto lineBox = katana::cad::sceneBounds(lines, {item}, plainOptions());
    EXPECT_EQ(lineBox.max.z, 55.0);
}

TEST(SceneSurfaces, OneElevationRampColoursEveryVisibleSurfaceAlike)
{
    // Two surfaces, 0 to 10 high and 10 to 20 high: one ramp from 0 to 20,
    // so the height 10 both have is ONE colour, the ramp's middle, and each
    // surface is not its own blue-to-white. Light off, so the colours are
    // the ramp's own.
    std::vector<katana::geometry::Point3> lowVertices = {
        {0.0, 0.0, 0.0}, {100.0, 0.0, 10.0}, {100.0, 100.0, 10.0}, {0.0, 100.0, 0.0}};
    auto low = TinSurface::create(std::move(lowVertices), {{0, 1, 2}, {0, 2, 3}});
    std::vector<katana::geometry::Point3> highVertices = {
        {0.0, 200.0, 10.0}, {100.0, 200.0, 20.0}, {100.0, 300.0, 20.0}, {0.0, 300.0, 10.0}};
    auto high = TinSurface::create(std::move(highVertices), {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(low.ok());
    ASSERT_TRUE(high.ok());
    SceneSurface a;
    a.surface = &*low;
    a.style = SurfaceStyle::Shaded;
    SceneSurface b = a;
    b.surface = &*high;
    SceneOptions options = plainOptions();
    options.lightDirection = katana::render::Vec3(0.0, 0.0, 0.0); // off
    SceneBuilder builder;
    DrawList list;
    builder.appendSurfaces({a, b}, options, list);
    ASSERT_EQ(list.positions.size(), 8u);
    const auto colourAt = [&list](double y, double z) {
        for (std::size_t i = 0; i < list.positions.size(); ++i) {
            if (list.positions[i].y == y && list.positions[i].z == z) {
                return list.colors[i];
            }
        }
        ADD_FAILURE() << "no vertex at y " << y << " z " << z;
        return katana::render::Rgba{};
    };
    EXPECT_EQ(colourAt(0.0, 0.0), katana::cad::elevationRampColor(0.0));
    EXPECT_EQ(colourAt(0.0, 10.0), katana::cad::elevationRampColor(0.5));
    EXPECT_EQ(colourAt(200.0, 10.0), katana::cad::elevationRampColor(0.5));
    EXPECT_EQ(colourAt(200.0, 20.0), katana::cad::elevationRampColor(1.0));

    SceneLayers layers;
    builder.buildTerrain({a, b}, {}, options, layers);
    EXPECT_EQ(layers.rampLow, 0.0);
    EXPECT_EQ(layers.rampHigh, 20.0);
}

TEST(SceneMeshes, AWallTurnedAwayFromTheSunIsDarkerThanOneFacingIt)
{
    // One vertical wall on the line x = y, drawn twice: wound a, b, c its
    // normal (b - a) x (c - a) = (10, 10, 0) x (0, 0, 5) = (50, -50, 0)
    // faces south-east, away from the default sun (-0.5, 0.5, 0.707);
    // wound a, c, b it faces north-west, towards it. The old abs() lit both
    // alike. From SceneOptions' constants:
    //   hemisphere for a level normal (z = 0): 0.10 + (0.30 - 0.10) * 0.5 = 0.20
    //   facing:  n . sun = (0.5 + 0.5) / sqrt 2 = 0.7071, so 0.20 + 0.80 * 0.7071
    //   away:    n . sun < 0 adds nothing: 0.20
    katana::geometry::TriangleMesh mesh;
    mesh.vertices = {katana::geometry::Point3(0, 0, 0), katana::geometry::Point3(10, 10, 0),
                     katana::geometry::Point3(0, 0, 5)};
    mesh.faces = {{0, 1, 2}, {0, 2, 1}};
    katana::cad::SceneMesh item;
    item.mesh = &mesh;
    const katana::render::Rgba base = katana::render::rgba(200, 200, 200);
    item.flatColor = base;
    SceneBuilder builder;
    DrawList list;
    builder.appendMeshes({item}, plainOptions(), list);
    ASSERT_EQ(list.triangles.size(), 2u);
    EXPECT_EQ(list.colors[list.triangles[0].a], katana::render::shade(base, 0.20));
    EXPECT_EQ(list.colors[list.triangles[1].a],
              katana::render::shade(base, 0.20 + 0.80 * std::sqrt(0.5)));
}

TEST(SceneSurfaces, ADenseSurfaceIsDrawnWithoutEdgesByDefaultAndASparseOneWithThem)
{
    // Automatic: edges up to kDenseSurfaceTriangles, none above. 160 x 160
    // vertices make 2 * 159 * 159 = 50 562 triangles, past the 50 000.
    std::vector<katana::geometry::Point3> vertices;
    std::vector<katana::terrain::TinTriangle> triangles;
    constexpr std::uint32_t n = 160;
    for (std::uint32_t j = 0; j < n; ++j) {
        for (std::uint32_t i = 0; i < n; ++i) {
            vertices.emplace_back(static_cast<double>(i), static_cast<double>(j),
                                  0.01 * static_cast<double>((i * 7 + j * 3) % 11));
        }
    }
    for (std::uint32_t j = 0; j + 1 < n; ++j) {
        for (std::uint32_t i = 0; i + 1 < n; ++i) {
            const std::uint32_t v = j * n + i;
            triangles.push_back({v, v + 1, v + n + 1});
            triangles.push_back({v, v + n + 1, v + n});
        }
    }
    auto dense = TinSurface::create(std::move(vertices), std::move(triangles));
    ASSERT_TRUE(dense.ok());
    ASSERT_GT(dense->triangleCount(), katana::cad::kDenseSurfaceTriangles);
    const TinSurface sparse = ramp();
    const Document document;

    SceneSurface item; // style Automatic, the default
    item.surface = &*dense;
    SceneLayers layers = layersOf(document, {item}, plainOptions());
    EXPECT_TRUE(layers.edges.lines.empty());
    EXPECT_EQ(layers.terrain.triangles.size(), dense->triangleCount());

    item.surface = &sparse;
    layers = layersOf(document, {item}, plainOptions());
    EXPECT_EQ(layers.edges.lines.size(), 5u) << "3 + 3 - 1 shared: every edge once";
}

TEST(SceneSurfaces, EdgesFadeAwayAsTheTrianglesShrinkOnScreen)
{
    // ramp(): two triangles of 5000 m2 each, so the typical edge is the side
    // of an equilateral triangle of that area: sqrt(5000 * 4 / sqrt 3)
    // = 107.5 m. Seen at 2 px to that edge (under 4) the edges are gone;
    // at 50 px (over 12) they are at full strength, the ink colour.
    const TinSurface surface = ramp();
    SceneSurface item;
    item.surface = &surface;
    item.style = SurfaceStyle::ShadedWithEdges;
    const Document document;
    SceneLayers layers = layersOf(document, {item}, plainOptions());
    ASSERT_EQ(layers.edgeRuns.size(), 1u);
    EXPECT_NEAR(layers.edgeRuns[0].typicalEdge, std::sqrt(5000.0 * 4.0 / std::sqrt(3.0)), 1e-9);

    katana::render::Camera camera;
    camera.setViewportSize(500, 500);
    camera.setProjection(katana::render::Projection::Orthographic);
    const double edge = layers.edgeRuns[0].typicalEdge;
    camera.setOrthographicHeight(edge / 2.0 * 500.0); // 2 px per edge
    EXPECT_FALSE(SceneBuilder::fadeEdges(layers, camera));
    for (std::size_t i = 0; i < layers.edges.colors.size(); ++i) {
        EXPECT_EQ(layers.edges.colors[i], layers.edgeBase[i]);
    }
    camera.setOrthographicHeight(edge / 50.0 * 500.0); // 50 px per edge
    EXPECT_TRUE(SceneBuilder::fadeEdges(layers, camera));
    for (std::size_t i = 0; i < layers.edges.colors.size(); ++i) {
        EXPECT_EQ(layers.edges.colors[i], layers.edgeInk[i]);
    }
}

TEST(SceneLayersBuild, TheSelectionOverlayHoldsOnlyTheSelectedEntities)
{
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 0.0))).ok());
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 5.0), Point2(10.0, 5.0))).ok());
    document.selection().add(document.model().entities.ids().back());
    const SceneOptions options = plainOptions();
    const SceneLayers layers = layersOf(document, {}, options);
    EXPECT_EQ(layers.entities.lines.size(), 2u) << "the drawing, as drawn";
    ASSERT_EQ(layers.selection.lines.size(), 1u);
    EXPECT_EQ(layers.selection.colors.front(), options.selectionColor);
    EXPECT_EQ(layers.selection.positions.front().y, 5.0);
    EXPECT_GT(layers.selection.lines.front().depthBias, layers.entities.lines.front().depthBias)
        << "the overlay must win the tie with the entity's own first drawing";
}

TEST(SceneLayersBuild, TheSelectionOverlayIsTheSelectionColourOverADarkCasing)
{
    // selection_style.hpp: a 3 px core in #FF9F1C over a 5 px casing of
    // (16, 18, 22), in a list of its own. The casing's bias is between the
    // core's and the drawing's, so it is not lost behind the line it frames.
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 0.0))).ok());
    document.selection().add(document.model().entities.ids().back());
    const SceneOptions options = plainOptions();
    const SceneLayers layers = layersOf(document, {}, options);
    ASSERT_EQ(layers.selection.lines.size(), 1u);
    ASSERT_EQ(layers.selectionCasing.lines.size(), 1u);
    const katana::render::DrawLine& core = layers.selection.lines[0];
    const katana::render::DrawLine& casing = layers.selectionCasing.lines[0];
    EXPECT_EQ(layers.selection.colors[core.a], katana::render::rgba(0xFF, 0x9F, 0x1C));
    EXPECT_EQ(layers.selection.colors[core.b], katana::render::rgba(0xFF, 0x9F, 0x1C));
    EXPECT_EQ(layers.selectionCasing.colors[casing.a], katana::render::rgba(16, 18, 22));
    EXPECT_EQ(layers.selectionCasing.colors[casing.b], katana::render::rgba(16, 18, 22));
    EXPECT_EQ(core.width, 3.0f);
    EXPECT_EQ(casing.width, 5.0f);
    EXPECT_GT(core.depthBias, casing.depthBias);
    EXPECT_GT(casing.depthBias, layers.entities.lines.front().depthBias);
    // The same span.
    EXPECT_EQ(layers.selectionCasing.positions[casing.a], layers.selection.positions[core.a]);
    EXPECT_EQ(layers.selectionCasing.positions[casing.b], layers.selection.positions[core.b]);
}

TEST(SceneLayersBuild, ASelectedPointIsDrawnLargerThanAnUnselectedOne)
{
    // An unselected point is SceneOptions::pointSize, 5 px; a selected one a
    // 9 px core over an 11 px casing (selection_style.hpp).
    Document document;
    ASSERT_TRUE(document.execute(katana::commands::createPoint(Point2(3.0, 4.0))).ok());
    document.selection().add(document.model().entities.ids().back());
    const SceneOptions options = plainOptions();
    const SceneLayers layers = layersOf(document, {}, options);
    ASSERT_EQ(layers.entities.points.size(), 1u);
    EXPECT_EQ(layers.entities.points[0].size, 5.0f);
    ASSERT_EQ(layers.selection.points.size(), 1u);
    EXPECT_EQ(layers.selection.points[0].size, 9.0f);
    EXPECT_EQ(layers.selection.colors[layers.selection.points[0].a],
              katana::render::rgba(0xFF, 0x9F, 0x1C));
    ASSERT_EQ(layers.selectionCasing.points.size(), 1u);
    EXPECT_EQ(layers.selectionCasing.points[0].size, 11.0f);
    EXPECT_EQ(layers.selectionCasing.colors[layers.selectionCasing.points[0].a],
              katana::render::rgba(16, 18, 22));
}

namespace {

// A line on "design" and one on "asbuilt", the design one selected; the view
// hides "design" of its own.
struct GhostScene {
    Document document;
    katana::cad::LayerOverrides view;
    SceneOptions options = plainOptions();

    GhostScene()
    {
        for (const char* name : {"design", "asbuilt"}) {
            katana::entity::Layer layer;
            layer.name = name;
            EXPECT_TRUE(document.execute(katana::commands::createLayer(layer)).ok());
        }
        katana::commands::EntityAttributes design;
        design.layer = "design";
        EXPECT_TRUE(document
                        .execute(katana::commands::createLine(Point2(0.0, 0.0),
                                                              Point2(10.0, 0.0), design))
                        .ok());
        katana::commands::EntityAttributes asBuilt;
        asBuilt.layer = "asbuilt";
        EXPECT_TRUE(document
                        .execute(katana::commands::createLine(Point2(0.0, 5.0),
                                                              Point2(10.0, 5.0), asBuilt))
                        .ok());
        document.selection().add(document.model().entities.ids().front());
        EXPECT_TRUE(view.hide("design"));
        options.layers = &view;
        options.selectionGhosts = true;
    }
};

} // namespace

TEST(SceneLayersBuild, ASelectedEntityOnALayerTheViewHidesIsOneFaintLineInTheOverlay)
{
    // selection_style.hpp: one 1 px line of (130, 88, 32) - the selection
    // colour at 45 % over the 3D view's ground - with no casing.
    const GhostScene scene;
    const SceneLayers layers = layersOf(scene.document, {}, scene.options);
    ASSERT_EQ(layers.entities.lines.size(), 1u) << "the drawing: the as-built line alone";
    EXPECT_EQ(layers.entities.positions[layers.entities.lines[0].a].y, 5.0);
    ASSERT_EQ(layers.selection.lines.size(), 1u);
    EXPECT_TRUE(layers.selectionCasing.lines.empty()) << "a ghost has no casing";
    const katana::render::DrawLine& ghost = layers.selection.lines[0];
    EXPECT_EQ(layers.selection.colors[ghost.a], katana::render::rgba(130, 88, 32));
    EXPECT_EQ(ghost.width, 1.0f);
    EXPECT_EQ(layers.selection.positions[ghost.a].y, 0.0) << "the design line";
    EXPECT_EQ(layers.selection.positions[ghost.b].y, 0.0);
}

TEST(SceneLayersBuild, NoGhostWhereTheDocumentHidesTheLayerOrTheViewTurnedGhostsOff)
{
    {
        GhostScene scene;
        scene.options.selectionGhosts = false;
        const SceneLayers layers = layersOf(scene.document, {}, scene.options);
        EXPECT_TRUE(layers.selection.lines.empty()) << "ghosts off";
    }
    {
        GhostScene scene;
        katana::entity::Layer design = *scene.document.model().layers.find("design");
        design.visible = false;
        ASSERT_TRUE(scene.document.execute(katana::commands::updateLayer(design)).ok());
        const SceneLayers layers = layersOf(scene.document, {}, scene.options);
        EXPECT_TRUE(layers.selection.lines.empty())
            << "a layer the drawing hides stays hidden in every view";
    }
    {
        // No view: the document rule alone, which draws it, selected.
        GhostScene scene;
        scene.options.layers = nullptr;
        const SceneLayers layers = layersOf(scene.document, {}, scene.options);
        ASSERT_EQ(layers.selection.lines.size(), 1u);
        EXPECT_EQ(layers.selection.colors[layers.selection.lines[0].a],
                  scene.options.selectionColor);
        EXPECT_EQ(layers.selectionCasing.lines.size(), 1u);
    }
}

// ---- one frame: the layers drawn as the 3D view draws them -----------------------

namespace {

using katana::render::Camera;
using katana::render::Framebuffer;
using katana::render::Projection;
using katana::render::Rgba;
using katana::render::StandardView;

constexpr Rgba kBlack = katana::render::rgba(0, 0, 0);

// A TIN of n x n square cells over the square `size` wide from (x0, y0), at
// the heights `height` gives.
TinSurface gridTin(double x0, double y0, double size, int n,
                   const std::function<double(double, double)>& height)
{
    std::vector<katana::geometry::Point3> vertices;
    for (int j = 0; j <= n; ++j) {
        for (int i = 0; i <= n; ++i) {
            const double x = x0 + size * i / n;
            const double y = y0 + size * j / n;
            vertices.push_back({x, y, height(x, y)});
        }
    }
    std::vector<katana::terrain::TinTriangle> triangles;
    for (int j = 0; j < n; ++j) {
        for (int i = 0; i < n; ++i) {
            const auto v0 = static_cast<std::uint32_t>(j * (n + 1) + i);
            const auto v3 = v0 + static_cast<std::uint32_t>(n + 1);
            triangles.push_back({v0, v0 + 1, v3 + 1});
            triangles.push_back({v0, v3 + 1, v3});
        }
    }
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(surface.ok());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

// One frame of `layers` through a copy of `camera`, on one thread, over black.
Framebuffer frameOf(SceneLayers& layers, Camera camera)
{
    auto target = Framebuffer::create(camera.viewportWidth(), camera.viewportHeight());
    EXPECT_TRUE(target.ok());
    katana::core::TaskPool pool(0);
    katana::render::RenderOptions options;
    options.background = kBlack;
    options.pool = &pool;
    katana::render::Rasterizer rasterizer;
    EXPECT_TRUE(katana::cad::renderLayers(layers, camera, rasterizer, *target, options).ok());
    return std::move(*target);
}

std::size_t countPixels(const Framebuffer& frame, Rgba color)
{
    return static_cast<std::size_t>(
        std::count(frame.color().begin(), frame.color().end(), color));
}

} // namespace

TEST(SceneFrame, AFlatSurfaceAtTheDatumCoversTheGridStandingUnderIt)
{
    // A pad flat at 5 m, 200 m square in 13 x 13 cells. The datum is its
    // lowest point, 5, so the grid lies exactly in its plane. A filled
    // triangle is pushed back by its own depth slope, and the grid drawn
    // before it with depth won: 8,902 of the pad's 235,340 pixels at the iso
    // view were grid lines, the whole grid pattern across the pad. Drawn as
    // a backdrop the grid is under the pad everywhere, so every pixel of the
    // pad is what the pad draws with no grid at all.
    const TinSurface pad = gridTin(0.0, 0.0, 200.0, 13, [](double, double) { return 5.0; });
    SceneSurface item;
    item.surface = &pad;
    item.style = SurfaceStyle::Shaded;
    Document document;
    SceneOptions withGrid; // the grid the view draws by default
    SceneOptions noGrid = withGrid;
    noGrid.drawGrid = false;
    SceneLayers gridded = layersOf(document, {item}, withGrid);
    SceneLayers bare = layersOf(document, {item}, noGrid);
    ASSERT_FALSE(gridded.grid.lines.empty());
    ASSERT_NEAR(gridded.grid.bounds().min.z, 5.0, 1e-9)
        << "the grid is not in the pad's plane, so this proves nothing";

    const std::pair<Projection, StandardView> views[] = {
        {Projection::Perspective, StandardView::IsoSouthWest},
        {Projection::Orthographic, StandardView::IsoSouthWest},
        {Projection::Perspective, StandardView::Top},
        {Projection::Orthographic, StandardView::Top}};
    for (const auto& [projection, standard] : views) {
        SCOPED_TRACE(std::to_string(static_cast<int>(projection)) + " " +
                     std::to_string(static_cast<int>(standard)));
        Camera camera;
        camera.setViewportSize(800, 600);
        camera.setProjection(projection);
        camera.setStandardView(standard);
        ASSERT_TRUE(camera.frame(bare.bounds));
        const Framebuffer plain = frameOf(bare, camera);
        const Framebuffer seen = frameOf(gridded, camera);
        std::size_t padPixels = 0;
        std::size_t changed = 0;
        std::size_t gridBeside = 0;
        for (std::size_t k = 0; k < plain.color().size(); ++k) {
            if (plain.color()[k] != kBlack) {
                ++padPixels;
                changed += seen.color()[k] != plain.color()[k] ? 1u : 0u;
            } else if (seen.color()[k] != kBlack) {
                ++gridBeside;
            }
        }
        ASSERT_GT(padPixels, 50000u) << "the pad is not in view, so this proves nothing";
        EXPECT_EQ(changed, 0u) << "grid pixels drawn over the pad, of " << padPixels;
        EXPECT_GT(gridBeside, 1000u) << "the grid was not drawn at all";
    }
}

TEST(SceneFrame, DrawingLinesDrapedOnASurfaceCrossItsEdgesUnbroken)
{
    // A plane rising 5% east and 2% north, 400 m square in 40 x 40 cells:
    // 3,200 triangles, under kDenseSurfaceTriangles, so the default style
    // draws its edges, and 10 m cells are 20 px or more at the frame, so at
    // full strength. Across it 40 plan lines, draped. An edge (1 footprint)
    // and a draped line (1.5) each have one depth across their width; off
    // their centres those differ by up to a pixel of the plane's slope,
    // which at a low angle is more than the half footprint between them, so
    // the edge drawn first won every crossing: 2.4% of the line pixels lost
    // at the iso view, 9.4% at 0.25 rad, 18.7% at 0.12. Edges that write no
    // depth leave the lines exactly the pixels they draw on the bare plane.
    const TinSurface plane =
        gridTin(0.0, 0.0, 400.0, 40, [](double x, double y) { return 0.05 * x + 0.02 * y; });
    Document document;
    for (int k = 0; k < 30; ++k) {
        const double y = 7.0 + 12.0 * k;
        ASSERT_TRUE(
            document.execute(katana::commands::createLine(Point2(5.0, y), Point2(395.0, y + 30.0)))
                .ok());
    }
    for (int k = 0; k < 10; ++k) {
        const double x = 13.0 + 37.0 * k;
        ASSERT_TRUE(
            document.execute(katana::commands::createLine(Point2(x, 5.0), Point2(x + 20.0, 395.0)))
                .ok());
    }
    SceneSurface edged;
    edged.surface = &plane; // the default, Automatic: edges at this size
    SceneSurface shaded = edged;
    shaded.style = SurfaceStyle::Shaded;
    SceneLayers withEdges = layersOf(document, {edged}, plainOptions());
    SceneLayers bare = layersOf(document, {shaded}, plainOptions());
    ASSERT_FALSE(withEdges.edges.lines.empty()) << "no edges, so this proves nothing";
    ASSERT_TRUE(bare.edges.lines.empty());
    const Rgba ink = bare.entities.colors.front();

    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        for (double elevation : {0.61, 0.25, 0.12}) {
            SCOPED_TRACE(std::to_string(static_cast<int>(projection)) + " at " +
                         katana::core::formatExactReal(elevation));
            Camera camera;
            camera.setViewportSize(1200, 800);
            camera.setProjection(projection);
            camera.setStandardView(StandardView::IsoSouthWest);
            camera.setOrientation(camera.azimuth(), elevation);
            ASSERT_TRUE(camera.frame(bare.bounds));
            const Framebuffer plain = frameOf(bare, camera);
            const std::size_t alone = countPixels(plain, ink);
            ASSERT_GT(alone, 5000u) << "the lines are not in view, so this proves nothing";
            const Framebuffer seen = frameOf(withEdges, camera);
            EXPECT_EQ(countPixels(seen, ink), alone);
            std::size_t edgePixels = 0;
            for (std::size_t k = 0; k < plain.color().size(); ++k) {
                edgePixels += seen.color()[k] != plain.color()[k] ? 1u : 0u;
            }
            EXPECT_GT(edgePixels, 5000u) << "the edges were not drawn, so this proves nothing";
        }
    }
}

TEST(SceneFrame, OfTwoCoincidentSurfacesTheEdgesShownAreThoseOfTheSurfaceShown)
{
    // A design surface often repeats the existing one's triangles outside
    // the works. Coincident, the surface drawn first wins the tie and is the
    // one seen; its edges won too while edges wrote depth. Writing none, the
    // second surface's edges - drawn later, and in a real archive fainter,
    // its triangles being smaller on average - painted over the edges of
    // the surface seen. The edges are emitted surface by surface in reverse,
    // so painted in order the first surface's are on top. Here the two
    // differ only in their flat colour, so the second's edges have their own
    // colours and not one pixel of them may show.
    const TinSurface ground =
        gridTin(0.0, 0.0, 200.0, 10, [](double x, double y) { return 0.03 * x + 0.01 * y; });
    SceneSurface first;
    first.surface = &ground;
    first.style = SurfaceStyle::ShadedWithEdges;
    first.coloring = SurfaceColoring::Flat;
    first.flatColor = katana::render::rgba(90, 150, 90);
    SceneSurface second = first;
    second.flatColor = katana::render::rgba(150, 90, 150);
    Document document;
    SceneLayers layers = layersOf(document, {first, second}, plainOptions());
    ASSERT_EQ(layers.edgeRuns.size(), 2u);

    Camera camera;
    camera.setViewportSize(800, 600);
    camera.setStandardView(StandardView::IsoSouthWest);
    ASSERT_TRUE(camera.frame(layers.bounds));
    const Framebuffer seen = frameOf(layers, camera);
    // The colours each surface's edges were drawn in this frame (fadeEdges
    // sets them for the camera).
    const auto inkOf = [&layers](const SceneLayers::EdgeRun& run) {
        std::vector<Rgba> inks(layers.edges.colors.begin() + run.first,
                               layers.edges.colors.begin() + run.first + run.count);
        std::sort(inks.begin(), inks.end());
        inks.erase(std::unique(inks.begin(), inks.end()), inks.end());
        return inks;
    };
    const auto countOf = [&seen](const std::vector<Rgba>& inks) {
        return static_cast<std::size_t>(
            std::count_if(seen.color().begin(), seen.color().end(), [&inks](Rgba c) {
                return std::binary_search(inks.begin(), inks.end(), c);
            }));
    };
    const std::vector<Rgba> firstInk = inkOf(layers.edgeRuns[0]);
    const std::vector<Rgba> secondInk = inkOf(layers.edgeRuns[1]);
    for (Rgba c : secondInk) {
        ASSERT_FALSE(std::binary_search(firstInk.begin(), firstInk.end(), c))
            << "the two surfaces share an edge colour, so this proves nothing";
    }
    EXPECT_GT(countOf(firstInk), 2000u) << "the first surface's edges were not drawn";
    EXPECT_EQ(countOf(secondInk), 0u);
}

TEST(SceneFrame, ASelectedLineIsDrawnInTheSelectionColourBetweenTwoEdgesOfItsCasing)
{
    // A line along x through the middle of the default iso view, selected:
    // down the middle column the first and last pixels drawn are the
    // casing's (16, 18, 22), with the core's #FF9F1C between them and none
    // of the line's own white - the casing drawn first writes no depth, so
    // the core covers it, and the core's pull beats the line's (scene.hpp,
    // renderLayers). Not the top view: a flat drawing seen face-on has a
    // depth range thinner than one pixel's pull, every pull stops at the
    // near plane and the line's own drawing wins the tie (docs/render.md,
    // "Outstanding").
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(100.0, 0.0))).ok());
    document.selection().add(document.model().entities.ids().back());
    SceneLayers layers = layersOf(document, {}, plainOptions());
    Camera camera;
    camera.setViewportSize(200, 100);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(StandardView::IsoSouthWest);
    ASSERT_TRUE(camera.frame(layers.bounds));
    const Framebuffer seen = frameOf(layers, camera);
    const Rgba core = katana::render::rgba(0xFF, 0x9F, 0x1C);
    const Rgba casing = katana::render::rgba(16, 18, 22);
    std::vector<Rgba> column;
    for (int row = 0; row < 100; ++row) {
        const Rgba c = seen.colorAt(100, row);
        if (c != kBlack) {
            column.push_back(c);
        }
    }
    std::string drawn;
    for (const Rgba c : column) {
        drawn += std::format(" {:06x}", c & 0xFFFFFFu);
    }
    ASSERT_GE(column.size(), 5u) << "a 5 px casing is at least 5 rows:" << drawn;
    EXPECT_EQ(column.front(), casing) << drawn;
    EXPECT_EQ(column.back(), casing) << drawn;
    EXPECT_GE(std::count(column.begin(), column.end(), core), 2) << "the 3 px core:" << drawn;
    // Nothing but the two, and the core in one run between the casing's.
    const auto first = std::find(column.begin(), column.end(), core);
    const auto last = std::find(column.rbegin(), column.rend(), core).base();
    EXPECT_TRUE(std::all_of(first, last, [core](Rgba c) { return c == core; })) << drawn;
    EXPECT_TRUE(std::all_of(column.begin(), first, [casing](Rgba c) { return c == casing; }))
        << drawn;
    EXPECT_TRUE(std::all_of(last, column.end(), [casing](Rgba c) { return c == casing; }))
        << drawn;
}

TEST(SceneFrame, TheDepthRangeTakesInAGhostOutsideWhatTheViewDraws)
{
    // The view draws the as-built line at the origin and hides "design",
    // where the selected line lies a kilometre further along the view
    // direction. Framed on what is drawn, the depth range fitted to that
    // alone ends short of the ghost; with the overlay taken in it reaches it.
    Document document;
    for (const char* name : {"design", "asbuilt"}) {
        katana::entity::Layer layer;
        layer.name = name;
        ASSERT_TRUE(document.execute(katana::commands::createLayer(layer)).ok());
    }
    katana::commands::EntityAttributes asBuilt;
    asBuilt.layer = "asbuilt";
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(10.0, 0.0),
                                                          asBuilt))
                    .ok());
    katana::commands::EntityAttributes design;
    design.layer = "design";
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(1000.0, 1000.0),
                                                          Point2(1010.0, 1000.0), design))
                    .ok());
    document.selection().add(document.model().entities.ids().back());
    katana::cad::LayerOverrides view;
    ASSERT_TRUE(view.hide("design"));
    SceneOptions options = plainOptions();
    options.layers = &view;

    const auto depthsOfTheGhost = [](const Camera& camera) {
        const katana::math::Vec3 f = camera.forward();
        const katana::math::Vec3 e = camera.eye();
        return std::pair{(katana::math::Vec3(1000.0, 1000.0, 0.0) - e).dot(f),
                         (katana::math::Vec3(1010.0, 1000.0, 0.0) - e).dot(f)};
    };
    const auto fitted = [&document, &options](bool ghosts) {
        options.selectionGhosts = ghosts;
        SceneLayers layers = layersOf(document, {}, options);
        Camera camera;
        camera.setViewportSize(400, 300);
        camera.setStandardView(StandardView::IsoSouthWest);
        EXPECT_TRUE(camera.frame(layers.bounds));
        auto target = Framebuffer::create(400, 300);
        EXPECT_TRUE(target.ok());
        katana::core::TaskPool pool(0);
        katana::render::RenderOptions render;
        render.pool = &pool;
        katana::render::Rasterizer rasterizer;
        EXPECT_TRUE(katana::cad::renderLayers(layers, camera, rasterizer, *target, render).ok());
        return camera;
    };

    const Camera without = fitted(false);
    const auto [nearWithout, farWithout] = depthsOfTheGhost(without);
    ASSERT_GT(std::max(nearWithout, farWithout), without.farPlane())
        << "the ghost is inside the drawn box's range anyway, so this proves nothing";

    const Camera with = fitted(true);
    const auto [a, b] = depthsOfTheGhost(with);
    EXPECT_LE(std::max(a, b), with.farPlane());
    EXPECT_GE(std::min(a, b), with.nearPlane());
}
