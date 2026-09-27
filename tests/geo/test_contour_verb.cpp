// The CONTOUR verb (src/katana_app/geo/contour_verbs.cpp, docs/terrain.md
// "Contours") and the one place its two engines meet
// (include/katana/interop/geo/contour_entities.hpp): contour lines of a
// surface traced exactly, of a raster by GDAL, drawn as polylines on
// <layer>/major and <layer>/minor at their levels, in one undo step, and kept
// inside the closed shapes a scope takes.
//
// The fixture is tests/geo/data/plane.asc: 40 x 30 cells of 1 m from (0,0),
// z = 100 + 0.05 x at the cell centres, so the level L lies at x = (L - 100)
// / 0.05: 100.5 at x = 10, 101.0 at 20, 101.5 at 30. Its heights run from
// 100.025 to 101.975 (the extreme centres, x = 0.5 and 39.5), so those three
// are the only half-metre levels in it.

#include <cmath>
#include <filesystem>
#include <optional>
#include <stop_token>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "contract_support.hpp"
#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/interop/geo/contour_entities.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "session.hpp"

namespace {

namespace geo = katana::app::geo;
namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::terrain::Contour;

const std::string kData = KATANA_GEO_TEST_DATA;
const std::string kPlane = kData + "/plane.asc";

// GDAL's AAIGrid driver reads plane.asc's decimals as Float32, so each height
// is within half a Float32 step of the value written: 2^(6-23) / 2 =
// 3.815e-6 near 100. A contour is placed by linear interpolation between two
// centres 1 m apart on a gradient of 0.05 per metre, so a height error of e
// moves it e / 0.05 along x: at most 3.815e-6 / 0.05 = 7.63e-5 m. (GDAL gave
// x = 9.99998 for 10.) The TIN engine on a surface made from those heights
// has the same bound, from the same heights.
constexpr double kFloat32PlaneTolerance = 7.62939453125e-6 / 2.0 / 0.05;

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-contour-verb-" + name))
    {
        std::error_code error;
        std::filesystem::remove_all(path_, error);
        std::filesystem::create_directories(path_, error);
    }
    ~TempDir()
    {
        std::error_code error;
        std::filesystem::remove_all(path_, error);
    }
    TempDir(const TempDir&) = delete;
    TempDir& operator=(const TempDir&) = delete;
    [[nodiscard]] const std::filesystem::path& path() const { return path_; }

  private:
    std::filesystem::path path_;
};

Entity levelled(double x, double y, double z)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{Point2(x, y)};
    katana::entity::setHeights(entity.properties, {z});
    return entity;
}

Entity polyline(std::vector<Point2> vertices, bool closed)
{
    Entity entity;
    Polyline2 line;
    line.vertices = std::move(vertices);
    line.closed = closed;
    entity.geometry = line;
    return entity;
}

class ContourVerb : public ::testing::Test {
  protected:
    TempDir scratch{::testing::UnitTest::GetInstance()->current_test_info()->name()};
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, scratch.path(), {}, {}};

    katana::core::Result<std::string> run(const std::string& line)
    {
        return geo::runNow(context, line);
    }

    std::string ran(const std::string& line)
    {
        auto reply = run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string();
    }

    static std::optional<geo::Record> record(const std::string& reply, const std::string& kind)
    {
        for (const geo::Record& each : geo::parseRecords(reply)) {
            if (each.kind == kind) {
                return each;
            }
        }
        return std::nullopt;
    }

    void draw(std::vector<Entity> entities)
    {
        ASSERT_TRUE(document.execute(katana::commands::createEntities(std::move(entities))).ok());
    }

    // The contours on `layer`, in id order.
    std::vector<const Entity*> on(const std::string& layer) const
    {
        std::vector<const Entity*> found;
        for (const katana::entity::EntityId id : document.model().entities.idsOnLayer(layer)) {
            found.push_back(document.model().entities.find(id));
        }
        return found;
    }

    static const Polyline2& lineOf(const Entity& entity)
    {
        return std::get<Polyline2>(entity.geometry);
    }

    static double levelOf(const Entity& entity)
    {
        const auto found = entity.properties.find(std::string(katana::entity::kElevationProperty));
        EXPECT_NE(found, entity.properties.end());
        return found == entity.properties.end() ? std::nan("") : std::get<double>(found->second);
    }

    // Every vertex of `entity` within `tolerance` of x = (level - 100) / 0.05.
    static void expectOnThePlane(const Entity& entity, double tolerance)
    {
        const double level = levelOf(entity);
        const double x = (level - 100.0) / 0.05;
        for (const Point2& vertex : lineOf(entity).vertices) {
            EXPECT_NEAR(vertex.x, x, tolerance) << "level " << level;
        }
    }

    // The plane z = 100 + 0.05 x made from four levelled corners, which the
    // TIN reproduces exactly (0.05 x 40 = 2 is exact in binary).
    void exactPlane()
    {
        draw({levelled(0, 0, 100), levelled(40, 0, 102), levelled(40, 30, 102),
              levelled(0, 30, 100)});
        ran("SURFACE FROM DRAWING NAME exact");
        const auto ids = document.model().entities.ids();
        ASSERT_TRUE(document.execute(katana::commands::deleteEntities(ids)).ok());
    }
};

// ---- the contract: what CONTOUR binds of GDAL's -----------------------------------------------

TEST(ContourContract, TheRasterEngineStillFindsTheArgumentsItBinds)
{
    using katana::geo_test::expectArgument;
    expectArgument({"raster", "contour"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "contour"}, "interval", gp::ArgType::Real, false);
    expectArgument({"raster", "contour"}, "offset", gp::ArgType::Real, false);
    expectArgument({"raster", "contour"}, "elevation-name", gp::ArgType::String, false);
    expectArgument({"raster", "contour"}, "3d", gp::ArgType::Boolean, false);
    expectArgument({"raster", "clip"}, "bbox", gp::ArgType::RealList, false);
    expectArgument({"raster", "neighbors"}, "kernel", gp::ArgType::StringList, true);
    expectArgument({"raster", "neighbors"}, "size", gp::ArgType::Integer, false);
}

// ---- the two engines ------------------------------------------------------------------------

TEST_F(ContourVerb, TheExecutorTakesContourAndHelpNamesIt)
{
    EXPECT_TRUE(geo::handles("CONTOUR SURFACE ground interval=1"));
    EXPECT_TRUE(geo::handles("contour raster 1 interval=1"));
    EXPECT_NE(geo::helpText().find("CONTOUR SURFACE <name>"), std::string::npos);
}

TEST_F(ContourVerb, TheRasterEngineGivesTheLevelsAtTenTwentyAndThirty)
{
    const std::string reply = ran("CONTOUR FILE \"" + kPlane + "\" interval=0.5");
    const auto contours = record(reply, "contours");
    ASSERT_TRUE(contours) << reply;
    EXPECT_EQ(contours->get("method").value_or(""), "grid");
    EXPECT_EQ(contours->get("cell").value_or(""), "1");
    EXPECT_EQ(contours->get("levels").value_or(""), "3");
    EXPECT_EQ(contours->get("count").value_or(""), "3");
    EXPECT_EQ(contours->get("smoothed").value_or(""), "no");
    // base 0, major every 5: k = 201, 202, 203 are none of them multiples.
    const auto minor = on("terrain/contours/minor");
    ASSERT_EQ(minor.size(), 3U);
    EXPECT_TRUE(on("terrain/contours/major").empty());
    EXPECT_DOUBLE_EQ(levelOf(*minor[0]), 100.5);
    EXPECT_DOUBLE_EQ(levelOf(*minor[1]), 101.0);
    EXPECT_DOUBLE_EQ(levelOf(*minor[2]), 101.5);
    for (const Entity* contour : minor) {
        expectOnThePlane(*contour, kFloat32PlaneTolerance);
        // GDAL carries a line to the raster's edge, y = 0 to 30.
        EXPECT_NEAR(lineOf(*contour).length(), 30.0, 1e-9);
        EXPECT_FALSE(lineOf(*contour).closed);
    }
}

TEST_F(ContourVerb, TheTinEngineIsExactOnAnExactPlane)
{
    exactPlane();
    // base 100.25: the levels 100.25, 100.75, 101.25 and 101.75 lie at x = 5,
    // 15, 25 and 35, none on a vertex. Interpolating exact heights leaves only
    // the rounding of a few operations near 100: far under 1e-9.
    const std::string reply = ran("CONTOUR SURFACE exact interval=0.5 base=100.25");
    const auto contours = record(reply, "contours");
    ASSERT_TRUE(contours) << reply;
    EXPECT_EQ(contours->get("method").value_or(""), "tin");
    EXPECT_EQ(contours->get("cell").value_or("x"), "");
    const auto minor = on("terrain/contours/minor");
    const auto major = on("terrain/contours/major");
    ASSERT_EQ(minor.size() + major.size(), 4U) << reply;
    for (const auto* list : {&minor, &major}) {
        for (const Entity* contour : *list) {
            expectOnThePlane(*contour, 1e-9);
            // The surface spans y = 0 to 30, so does each contour.
            EXPECT_NEAR(lineOf(*contour).length(), 30.0, 1e-9);
        }
    }
}

TEST_F(ContourVerb, TheTinEngineOnTheRastersSurfaceHasTheFloat32Bound)
{
    ran("SURFACE FROM FILE \"" + kPlane + "\" NAME plane");
    ran("CONTOUR SURFACE plane interval=0.5");
    const auto minor = on("terrain/contours/minor");
    ASSERT_EQ(minor.size(), 3U);
    for (const Entity* contour : minor) {
        expectOnThePlane(*contour, kFloat32PlaneTolerance);
        // The TIN's vertices are the cell centres, y = 0.5 to 29.5.
        EXPECT_NEAR(lineOf(*contour).length(), 29.0, 1e-9);
    }
}

TEST_F(ContourVerb, MajorEveryTwoFromBaseHundredMakesTheMetreMajor)
{
    // Levels 100 + k x 0.5 for k = 1, 2, 3: only k = 2 (101.0) is a multiple
    // of 2.
    const std::string reply =
        ran("CONTOUR FILE \"" + kPlane + "\" interval=0.5 major=2 base=100 layer=site/levels");
    const auto contours = record(reply, "contours");
    ASSERT_TRUE(contours) << reply;
    EXPECT_EQ(contours->get("major").value_or(""), "1");
    EXPECT_EQ(contours->get("minor").value_or(""), "2");
    EXPECT_EQ(contours->get("layer").value_or(""), "site/levels");
    const auto major = on("site/levels/major");
    ASSERT_EQ(major.size(), 1U);
    EXPECT_DOUBLE_EQ(levelOf(*major[0]), 101.0);
    const auto minor = on("site/levels/minor");
    ASSERT_EQ(minor.size(), 2U);
    EXPECT_DOUBLE_EQ(levelOf(*minor[0]), 100.5);
    EXPECT_DOUBLE_EQ(levelOf(*minor[1]), 101.5);
    EXPECT_TRUE(document.model().layers.contains("site/levels/major"));
}

TEST_F(ContourVerb, EveryVertexCarriesItsContoursLevelAsItsHeight)
{
    ran("CONTOUR FILE \"" + kPlane + "\" interval=0.5");
    for (const Entity* contour : on("terrain/contours/minor")) {
        const std::size_t count = lineOf(*contour).vertices.size();
        const auto heights = katana::entity::heightsOf(contour->properties, count);
        ASSERT_EQ(heights.size(), count);
        for (const auto& height : heights) {
            ASSERT_TRUE(height.has_value());
            EXPECT_DOUBLE_EQ(*height, levelOf(*contour));
        }
    }
}

TEST_F(ContourVerb, OneUndoRemovesEveryContour)
{
    ran("CONTOUR FILE \"" + kPlane + "\" interval=0.25 major=2 base=100");
    ASSERT_GT(document.model().entities.size(), 3U);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.size(), 0U);
}

// ---- boundaries from the scope -------------------------------------------------------------

TEST_F(ContourVerb, AnOptionMayFollowTheFilterButLayerThereIsTheFilters)
{
    // Read as every GIS verb's words are: interval= after the filter is the
    // verb's own (it was refused as "not a WHERE key"), while LAYER= after
    // WHERE is the filter's condition - what the scope widget writes there -
    // so the contours go to the default layer, inside the box on layer 0.
    draw({polyline({{0, 0}, {25, 0}, {25, 15}, {0, 15}}, true)});
    const std::string reply =
        ran("CONTOUR FILE \"" + kPlane + "\" DRAWING WHERE TYPE=polyline interval=0.5 layer=0");
    const auto scope = record(reply, "scope");
    ASSERT_TRUE(scope) << reply;
    EXPECT_EQ(scope->get("where").value_or(""), "TYPE=polyline LAYER=0") << reply;
    // 100.5 at x = 10 and 101.0 at x = 20 cross the box, as in the test below.
    EXPECT_EQ(on("terrain/contours/minor").size(), 2U) << reply;
}

TEST_F(ContourVerb, AClosedBoundaryKeepsOnlyWhatIsInsideItOnBothEngines)
{
    // A 25 x 15 box from the origin: 100.5 (x = 10) and 101.0 (x = 20) cross
    // it from y = 0 to its top at 15; 101.5 (x = 30) lies outside. The open
    // line beside it bounds nothing and is counted.
    draw({polyline({{0, 0}, {25, 0}, {25, 15}, {0, 15}}, true),
          polyline({{30, 0}, {35, 5}}, false)});
    const std::string grid =
        ran("CONTOUR FILE \"" + kPlane + "\" interval=0.5 layer=grid DRAWING WHERE TYPE=polyline");
    const auto areas = record(grid, "areas");
    ASSERT_TRUE(areas) << grid;
    EXPECT_EQ(areas->get("used").value_or(""), "1");
    EXPECT_EQ(areas->get("skipped.open").value_or(""), "1");
    EXPECT_NE(grid.find("warning text="), std::string::npos) << grid;
    const auto cut = on("grid/minor");
    ASSERT_EQ(cut.size(), 2U) << grid;
    for (const Entity* contour : cut) {
        expectOnThePlane(*contour, kFloat32PlaneTolerance);
        // From the raster's edge at y = 0 to the boundary at 15.
        EXPECT_NEAR(lineOf(*contour).length(), 15.0, 1e-9);
    }
    EXPECT_DOUBLE_EQ(levelOf(*cut[0]), 100.5);
    EXPECT_DOUBLE_EQ(levelOf(*cut[1]), 101.0);

    ran("SURFACE FROM FILE \"" + kPlane + "\" NAME plane");
    ran("CONTOUR SURFACE plane interval=0.5 layer=tin LAYERS 0");
    const auto tin = on("tin/minor");
    ASSERT_EQ(tin.size(), 2U);
    for (const Entity* contour : tin) {
        // From the first cell centre at y = 0.5 to the boundary at 15.
        EXPECT_NEAR(lineOf(*contour).length(), 14.5, 1e-9);
    }
}

TEST_F(ContourVerb, AScopeWithNoClosedShapeKeepsNoContourAndSaysSo)
{
    draw({polyline({{0, 0}, {40, 30}}, false)});
    const std::string reply = ran("CONTOUR FILE \"" + kPlane + "\" interval=0.5 DRAWING");
    const auto areas = record(reply, "areas");
    ASSERT_TRUE(areas) << reply;
    EXPECT_EQ(areas->get("used").value_or(""), "0");
    EXPECT_EQ(areas->get("skipped.open").value_or(""), "1");
    EXPECT_EQ(record(reply, "contours")->get("count").value_or(""), "0");
    EXPECT_EQ(document.model().entities.size(), 1U);
}

TEST_F(ContourVerb, SmoothingAPlaneLeavesItsContoursWhereTheyWere)
{
    // A gaussian kernel is symmetric and sums to one, so it gives back a
    // linear surface unchanged away from the edges: the levels at x = 10, 20
    // and 30 are 10 cells from either edge, beyond the 5-cell kernel.
    const std::string reply = ran("CONTOUR FILE \"" + kPlane + "\" interval=0.5 smooth=5");
    EXPECT_EQ(record(reply, "contours")->get("smoothed").value_or(""), "yes") << reply;
    const auto minor = on("terrain/contours/minor");
    ASSERT_EQ(minor.size(), 3U);
    for (const Entity* contour : minor) {
        expectOnThePlane(*contour, kFloat32PlaneTolerance);
    }
}

TEST_F(ContourVerb, PreviewSaysWhatWouldBeReadAndDrawsNothing)
{
    const std::string reply = ran("CONTOUR FILE \"" + kPlane + "\" interval=0.5 PREVIEW");
    EXPECT_NE(reply.find("preview valid=yes changed=no"), std::string::npos) << reply;
    EXPECT_EQ(document.model().entities.size(), 0U);
}

TEST_F(ContourVerb, ACancelledRunDrawsNothing)
{
    auto prepared = geo::prepare(context, "CONTOUR FILE \"" + kPlane + "\" interval=0.5");
    ASSERT_TRUE(prepared.ok());
    std::stop_source stop;
    stop.request_stop();
    auto apply = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(apply.error().message, "cancelled");
    EXPECT_EQ(document.model().entities.size(), 0U);
}

TEST_F(ContourVerb, WhatTheVerbCannotDoIsRefusedNamingIt)
{
    ran("SURFACE FROM FILE \"" + kPlane + "\" NAME plane");
    const std::string plane = "FILE \"" + kPlane + "\"";
    EXPECT_EQ(run("CONTOUR " + plane).error().code, ErrorCode::InvalidArgument); // no interval
    EXPECT_EQ(run("CONTOUR " + plane + " interval=0").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(run("CONTOUR " + plane + " interval=1 smooth=4").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("CONTOUR " + plane + " interval=1 layer=/bad").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("CONTOUR " + plane + " interval=1 colour=red").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("CONTOUR DRAWING interval=1").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(run("CONTOUR SURFACE nothing interval=1").error().code, ErrorCode::NotFound);
    EXPECT_EQ(run("CONTOUR SURFACE plane interval=1 smooth=3").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("CONTOUR SURFACE plane CELL 2 interval=1").error().code,
              ErrorCode::InvalidArgument);
    // A millionth of a metre over 1.95 m of relief is 1.95 million levels:
    // a unit mistake, refused by both engines before a line is drawn.
    EXPECT_EQ(run("CONTOUR SURFACE plane interval=0.000001").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("CONTOUR " + plane + " interval=0.000001").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(document.model().entities.size(), 0U);
}

// ---- the conversion and the clip ------------------------------------------------------------

TEST(ContourEntities, GdalsLinesBecomeContoursAtTheirLevelsRingsClosed)
{
    gp::FeatureTable table;
    table.name = "contour";
    table.kind = katana::gis::GeometryKind::LineString;
    table.fields = {{"ID", gp::FieldType::Integer64}, {"elevation", gp::FieldType::Real}};
    const auto feature = [](std::vector<katana::gis::GeoPoint> points, double level) {
        gp::Feature made;
        katana::gis::VectorGeometry line;
        line.kind = katana::gis::GeometryKind::LineString;
        line.parts = {std::move(points)};
        made.parts = {line};
        made.values = {std::int64_t{0}, level};
        return made;
    };
    table.features.push_back(feature({{0, 0, 3}, {1, 0, 3}, {1, 1, 3}, {0, 0, 3}}, 3.0));
    table.features.push_back(feature({{0, 5, 2}, {9, 5, 2}}, 2.0));
    gp::FeatureSet set{{table}};
    auto contours = igeo::contoursFromFeatures(set, "elevation", 1.0, 0.0, 3);
    ASSERT_TRUE(contours.ok());
    ASSERT_EQ(contours->size(), 2U);
    EXPECT_DOUBLE_EQ((*contours)[0].elevation, 2.0); // sorted by level
    EXPECT_FALSE((*contours)[0].line.closed);
    EXPECT_FALSE((*contours)[0].major);
    EXPECT_DOUBLE_EQ((*contours)[1].elevation, 3.0);
    EXPECT_TRUE((*contours)[1].line.closed);
    EXPECT_EQ((*contours)[1].line.vertices.size(), 3U); // the repeat dropped
    EXPECT_TRUE((*contours)[1].major);                  // k = 3, a multiple of 3

    EXPECT_EQ(igeo::contoursFromFeatures(set, "level", 1.0, 0.0, 3).error().code,
              ErrorCode::InvalidArgument);
}

TEST(ContourEntities, ARingCutByABoundaryIsOnePieceJoinedAcrossItsStart)
{
    // A 10 x 10 square ring starting at (0,0), cut by the half-plane box x
    // <= 5 (a 20 x 20 box from (-5,-5) to (5,15)): what is inside runs from
    // (5,10) along the top to (0,10), down to (0,0) - the ring's start - and
    // along the bottom to (5,0). One piece of 5 + 10 + 5 = 20.
    Contour ring;
    ring.elevation = 7.0;
    ring.line.vertices = {{0, 0}, {10, 0}, {10, 10}, {0, 10}};
    ring.line.closed = true;
    igeo::ContourBoundary box;
    Polyline2 outline;
    outline.vertices = {{-5, -5}, {5, -5}, {5, 15}, {-5, 15}};
    outline.closed = true;
    box.rings.push_back(outline);
    const auto kept = igeo::clipContours({ring}, {box});
    ASSERT_EQ(kept.size(), 1U);
    EXPECT_FALSE(kept[0].line.closed);
    EXPECT_NEAR(kept[0].line.length(), 20.0, 1e-12);
    EXPECT_EQ(kept[0].line.vertices.front(), Point2(5, 10));
    EXPECT_EQ(kept[0].line.vertices.back(), Point2(5, 0));
    EXPECT_DOUBLE_EQ(kept[0].elevation, 7.0);

    // A ring wholly inside stays a ring.
    Polyline2 all;
    all.vertices = {{-1, -1}, {11, -1}, {11, 11}, {-1, 11}};
    all.closed = true;
    const auto whole = igeo::clipContours({ring}, {igeo::ContourBoundary{{all}}});
    ASSERT_EQ(whole.size(), 1U);
    EXPECT_TRUE(whole[0].line.closed);
}

TEST(ContourEntities, AHoleInABoundaryCutsTheContourOut)
{
    // A line along y = 5 from x = 0 to 20 through an area from 0 to 20 with
    // a hole from 5 to 15: kept 0 to 5 and 15 to 20.
    Contour line;
    line.elevation = 1.0;
    line.line.vertices = {{0, 5}, {20, 5}};
    Polyline2 outer;
    outer.vertices = {{0, 0}, {20, 0}, {20, 10}, {0, 10}};
    outer.closed = true;
    Polyline2 hole;
    hole.vertices = {{5, 2}, {15, 2}, {15, 8}, {5, 8}};
    hole.closed = true;
    const auto kept = igeo::clipContours({line}, {igeo::ContourBoundary{{outer, hole}}});
    ASSERT_EQ(kept.size(), 2U);
    EXPECT_NEAR(kept[0].line.length(), 5.0, 1e-12);
    EXPECT_NEAR(kept[1].line.length(), 5.0, 1e-12);
    EXPECT_EQ(kept[1].line.vertices.front(), Point2(15, 5));
}

TEST(ContourEntities, NoContoursMakeNoCommandAndABadLayerIsRefused)
{
    const katana::entity::Model model;
    auto none = igeo::contourCommand(model, {}, {});
    ASSERT_TRUE(none.ok());
    EXPECT_EQ(none->command, nullptr);
    Contour one;
    one.line.vertices = {{0, 0}, {1, 1}};
    igeo::ContourEntityOptions options;
    options.layer = "";
    EXPECT_EQ(igeo::contourCommand(model, {one}, options).error().code, ErrorCode::InvalidArgument);
}

// ---- through a Session, as katana_cli and katana_mcp run it -----------------------------------

TEST(ContourSession, TheSessionDrawsContoursAndReportsTheirLevels)
{
    katana::app::Session session(nullptr);
    testing::internal::CaptureStdout();
    const bool drawn = session.run("CONTOUR FILE \"" + kPlane + "\" interval=0.5 major=2 base=100");
    const std::string out = testing::internal::GetCapturedStdout();
    EXPECT_TRUE(drawn);
    EXPECT_NE(out.find("contours method=grid cell=1 levels=3 count=3 major=1 minor=2"),
              std::string::npos)
        << out;
}

} // namespace
