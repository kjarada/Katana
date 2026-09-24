// Document -> DrawList (PLAN.MD Phase 15, Rule 3).
//
// The assertions worth making about a scene builder are structural: that the
// renderer is given the geometry the model holds, that it is given it ONCE
// rather than per-triangle, that a tessellation honours its tolerance, and that
// building a scene cannot modify the document.

#include <gtest/gtest.h>

#include <cmath>
#include <functional>

#include "katana/cad/scene.hpp"
#include "katana/commands/entity_commands.hpp"

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

TEST(CadScene, TheGridStandsOnTheLowestSurface)
{
    // A surface from 0 to 10 lifted to 250..260: the grid goes under it at
    // 250, where the ground is, not at a z = 0 two hundred metres below.
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
