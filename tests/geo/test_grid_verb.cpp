// RASTER GRID: surveyed points to a DEM (src/katana_app/geo/grid_verbs.cpp,
// docs/terrain.md "Gridding points to a DEM"): through the executor, as the
// window runs it, and through a Session and the MCP server, as katana_cli and
// katana_mcp do. Every expected value is worked by hand from the plane the
// points are surveyed on, z = 100 + x/10 + y/20.

#include <cmath>
#include <filesystem>
#include <iostream>
#include <optional>
#include <sstream>
#include <stop_token>
#include <string>
#include <utility>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "contract_support.hpp"
#include "geo/dem_support.hpp"
#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/interop/geo/raster_products.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "mcp_server.hpp"
#include "session.hpp"

namespace {

namespace geo = katana::app::geo;
namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::geometry::Point2;

double plane(double x, double y)
{
    return 100.0 + x / 10.0 + y / 20.0;
}

Entity spot(double x, double y, std::optional<double> z)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{Point2(x, y)};
    if (z) {
        katana::entity::setHeights(entity.properties, {z});
    }
    return entity;
}

class GridVerb : public ::testing::Test {
  protected:
    std::filesystem::path scratch = std::filesystem::temp_directory_path() /
                                    ("katana-grid-verb-" + std::string(::testing::UnitTest::GetInstance()
                                                                           ->current_test_info()
                                                                           ->name()));
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, scratch, {}, {}};

    void SetUp() override
    {
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
    }
    void TearDown() override
    {
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
    }

    void draw(std::vector<Entity> entities)
    {
        ASSERT_TRUE(document.execute(katana::commands::createEntities(std::move(entities))).ok());
    }

    // Points every `step` metres over [x0, x1] x [y0, y1], surveyed on the plane.
    void lattice(double x0, double y0, double x1, double y1, double step)
    {
        std::vector<Entity> points;
        for (double x = x0; x <= x1 + 1e-9; x += step) {
            for (double y = y0; y <= y1 + 1e-9; y += step) {
                points.push_back(spot(x, y, plane(x, y)));
            }
        }
        draw(std::move(points));
    }

    katana::core::Result<std::string> run(const std::string& line)
    {
        return geo::runNow(context, line);
    }

    // The value of the last reference raster's cell holding (x, y).
    double valueAt(double x, double y)
    {
        EXPECT_FALSE(reference.rasters().empty());
        auto band = geo::readBandOne(reference.rasters().back().source);
        EXPECT_TRUE(band.ok()) << band.error().describe();
        const auto& g = band->geotransform;
        const int column = static_cast<int>(std::floor((x - g[0]) / g[1]));
        const int row = static_cast<int>(std::floor((y - g[3]) / g[5]));
        EXPECT_TRUE(column >= 0 && column < band->width && row >= 0 && row < band->height);
        return band->values[static_cast<std::size_t>(row) * band->width + column];
    }

    static const geo::Record* record(const std::vector<geo::Record>& records, const std::string& kind)
    {
        for (const geo::Record& found : records) {
            if (found.kind == kind) {
                return &found;
            }
        }
        return nullptr;
    }
};

// ---- the contract: what RASTER GRID binds ------------------------------------------------------

TEST(RasterGridContract, VectorGridTakesTheArgumentsTheVerbBinds)
{
    using katana::geo_test::expectArgument;
    const std::vector<std::string> linear{"vector", "grid", "linear"};
    expectArgument(linear, "input", gp::ArgType::DatasetList, true);
    expectArgument(linear, "output", gp::ArgType::Dataset, true);
    expectArgument(linear, "extent", gp::ArgType::RealList, false);
    expectArgument(linear, "resolution", gp::ArgType::RealList, false);
    expectArgument(linear, "size", gp::ArgType::IntegerList, false);
    expectArgument(linear, "zfield", gp::ArgType::String, false);
    expectArgument(linear, "nodata", gp::ArgType::Real, false);
    expectArgument(linear, "input-layer", gp::ArgType::StringList, false);
    expectArgument(linear, "radius", gp::ArgType::Real, false);
    expectArgument({"vector", "grid", "invdist"}, "power", gp::ArgType::Real, false);
    // The output takes a name only: the bridge stages it (docs/geoprocessing.md,
    // "Names only: staging"), which the verb relies on.
    const auto spec = gp::describe(linear);
    ASSERT_TRUE(spec.ok());
    for (const gp::ArgSpec& arg : spec->args) {
        if (arg.name == "output") {
            EXPECT_TRUE(arg.acceptsName);
            EXPECT_FALSE(arg.acceptsObject);
        }
    }
}

// ---- the grid -----------------------------------------------------------------------------------

TEST_F(GridVerb, LinearGridOfPointsOnAPlaneReproducesThePlane)
{
    // Points every 10 m over 0..100 on the plane. Linear interpolation over a
    // triangulation reproduces a plane exactly, so every cell centre holds the
    // plane's value there - (52.5, 47.5): 100 + 5.25 + 2.375 = 107.625 - to
    // Float64 rounding (1e-9; the output is Float64).
    lattice(0, 0, 100, 100, 10);
    auto reply = run("RASTER GRID DRAWING method=linear cell=5");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    const geo::Record* grid = record(records, "grid");
    ASSERT_NE(grid, nullptr) << *reply;
    EXPECT_EQ(grid->get("method"), "linear");
    EXPECT_EQ(grid->get("algorithm"), "vector grid linear");
    EXPECT_EQ(grid->get("z"), "geometry");
    EXPECT_EQ(grid->get("cell"), "5");
    EXPECT_EQ(grid->get("extent"), "0,0,100,100");
    EXPECT_EQ(grid->get("size"), "20x20");
    const geo::Record* scope = record(records, "scope");
    ASSERT_NE(scope, nullptr);
    EXPECT_EQ(scope->get("matched"), "121");
    EXPECT_EQ(scope->get("used"), "121");
    EXPECT_EQ(scope->get("points"), "121");
    const geo::Record* output = record(records, "output");
    ASSERT_NE(output, nullptr);
    EXPECT_EQ(output->get("target"), "reference");
    EXPECT_EQ(output->get("name"), "dem");
    EXPECT_EQ(output->get("raster"), "20x20");
    ASSERT_EQ(reference.rasters().size(), 1u);
    EXPECT_EQ(reference.rasters().front().role, katana::interop::RasterRole::Derived);
    EXPECT_EQ(reference.rasters().front().derivation, "RASTER GRID DRAWING method=linear cell=5");

    EXPECT_NEAR(valueAt(52.5, 47.5), 107.625, 1e-9);
    auto band = geo::readBandOne(reference.rasters().front().source);
    ASSERT_TRUE(band.ok());
    for (int row = 0; row < band->height; ++row) {
        for (int column = 0; column < band->width; ++column) {
            const double x = band->geotransform[0] + (column + 0.5) * band->geotransform[1];
            const double y = band->geotransform[3] + (row + 0.5) * band->geotransform[5];
            ASSERT_NEAR(band->values[static_cast<std::size_t>(row) * band->width + column],
                        plane(x, y), 1e-9)
                << "cell at " << x << "," << y;
        }
    }
    // The no-data value is declared: the surfaces' own, never GDAL's 0.
    ASSERT_TRUE(band->noData.has_value());
    EXPECT_EQ(*band->noData, katana::interop::geo::kGridNoData);
}

TEST_F(GridVerb, AHeightlessPointIsExcludedAndCountedNotReadAsZero)
{
    // A point with no height where the cell (52.5, 47.5) is: read as z = 0 it
    // would pull that cell from 107.625 towards the datum.
    lattice(0, 0, 100, 100, 10);
    draw({spot(52.5, 47.5, std::nullopt)});
    auto reply = run("RASTER GRID DRAWING cell=5");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    const geo::Record* scope = record(records, "scope");
    ASSERT_NE(scope, nullptr) << *reply;
    EXPECT_EQ(scope->get("matched"), "122");
    EXPECT_EQ(scope->get("used"), "121");
    EXPECT_EQ(scope->get("skipped.heightless"), "1");
    EXPECT_NEAR(valueAt(52.5, 47.5), 107.625, 1e-9);
}

TEST_F(GridVerb, TheGdalVerbLeavesAHeightlessPointOutOfAGridThatReadsZ)
{
    // Three points at z = 10 and one with no height at (10, 10). Every
    // surveyed height is 10, so every cell of a nearest-neighbour grid is 10;
    // the heightless point, read as z = 0, made the cell nearest it 0.
    draw({spot(0, 0, 10.0), spot(10, 0, 10.0), spot(0, 10, 10.0), spot(10, 10, std::nullopt)});
    auto reply = run("GDAL vector grid nearest --extent 0,0,10,10 --size 2,2 FROM DRAWING");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    const geo::Record* scope = record(records, "scope");
    ASSERT_NE(scope, nullptr) << *reply;
    EXPECT_EQ(scope->get("used"), "3");
    EXPECT_EQ(scope->get("skipped.heightless"), "1");
    for (const auto& [x, y] :
         {std::pair{2.5, 2.5}, std::pair{7.5, 2.5}, std::pair{2.5, 7.5}, std::pair{7.5, 7.5}}) {
        EXPECT_EQ(valueAt(x, y), 10.0) << x << "," << y;
    }
    // Buffering reads no Z: the heightless point is given to it.
    auto buffered = run("GDAL vector buffer --distance 1 FROM DRAWING");
    ASSERT_TRUE(buffered.ok()) << buffered.error().describe();
    const auto bufferRecords = geo::parseRecords(*buffered);
    const geo::Record* taken = record(bufferRecords, "scope");
    ASSERT_NE(taken, nullptr);
    EXPECT_EQ(taken->get("used"), "4");
}

TEST_F(GridVerb, ZFromAPropertyIsUsed)
{
    // The heights are in the property rl, not in the geometry. One point has
    // no rl and one has a word there: both are left out and counted, where
    // GDAL would read either as 0.
    std::vector<Entity> points;
    for (double x = 0; x <= 100; x += 10) {
        for (double y = 0; y <= 100; y += 10) {
            Entity entity = spot(x, y, std::nullopt);
            entity.properties["rl"] = plane(x, y);
            points.push_back(std::move(entity));
        }
    }
    points.push_back(spot(52.5, 47.5, std::nullopt));
    Entity worded = spot(57.5, 47.5, std::nullopt);
    worded.properties["rl"] = std::string("not surveyed");
    points.push_back(std::move(worded));
    draw(std::move(points));

    auto reply = run("RASTER GRID DRAWING z=rl cell=5 NAME ground");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    ASSERT_NE(record(records, "grid"), nullptr);
    EXPECT_EQ(record(records, "grid")->get("z"), "rl");
    const geo::Record* scope = record(records, "scope");
    ASSERT_NE(scope, nullptr);
    EXPECT_EQ(scope->get("used"), "121");
    EXPECT_EQ(scope->get("skipped.no_z"), "2");
    EXPECT_EQ(record(records, "output")->get("name"), "ground");
    EXPECT_NEAR(valueAt(52.5, 47.5), 107.625, 1e-9);
    EXPECT_NEAR(valueAt(57.5, 47.5), plane(57.5, 47.5), 1e-9);
}

TEST_F(GridVerb, TheExtentDefaultsToTheScopeBounds)
{
    // Points over 10..90 x 20..80 at a 5 m cell: exactly 16 x 12 cells from
    // (10, 20), since the bounds already fall on whole cells.
    lattice(10, 20, 90, 80, 10);
    auto reply = run("RASTER GRID DRAWING cell=5");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    auto records = geo::parseRecords(*reply);
    EXPECT_EQ(record(records, "grid")->get("extent"), "10,20,90,80");
    EXPECT_EQ(record(records, "grid")->get("size"), "16x12");
    auto band = geo::readBandOne(reference.rasters().back().source);
    ASSERT_TRUE(band.ok());
    EXPECT_DOUBLE_EQ(band->geotransform[0], 10.0);
    EXPECT_DOUBLE_EQ(band->geotransform[3], 80.0);
    EXPECT_DOUBLE_EQ(band->geotransform[1], 5.0);
    EXPECT_DOUBLE_EQ(band->geotransform[5], -5.0);
}

TEST_F(GridVerb, BoundsOffTheCellGrowOutwardsToWholeCells)
{
    // Points over 12..92 x 22..82: outwards to the 5 m cells round them,
    // 10..95 x 20..85 - 17 x 13 cells, each exactly 5 m (GDAL itself would
    // stretch the cells to fit the bounds).
    lattice(12, 22, 92, 82, 10);
    auto reply = run("RASTER GRID DRAWING cell=5");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    auto records = geo::parseRecords(*reply);
    EXPECT_EQ(record(records, "grid")->get("extent"), "10,20,95,85");
    EXPECT_EQ(record(records, "grid")->get("size"), "17x13");
    auto band = geo::readBandOne(reference.rasters().back().source);
    ASSERT_TRUE(band.ok());
    EXPECT_DOUBLE_EQ(band->geotransform[1], 5.0);
}

TEST_F(GridVerb, BoundsOnTheCellAtAnMgaNorthingStayOnIt)
{
    // Four points 1 m apart at easting 330000.1 and northing 6250000.3 (MGA
    // zone 56 magnitudes), gridded at 0.1 m: the bounds lie on 0.1 m lines,
    // so the grid is 10 x 10 cells from (330000.1, 6250000.3). In binary64,
    // 6250000.3 / 0.1 is 62500002.99999999 (one ulp there is 7.5e-9), so a
    // floor that allows 1e-9 of a cell put the grid's foot at 6250000.2: 11
    // rows, the extra one holding no datum.
    std::vector<Entity> points;
    for (const double x : {330000.1, 330001.1}) {
        for (const double y : {6250000.3, 6250001.3}) {
            points.push_back(spot(x, y, 100.0));
        }
    }
    draw(std::move(points));
    auto reply = run("RASTER GRID DRAWING cell=0.1");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(record(geo::parseRecords(*reply), "grid")->get("size"), "10x10") << *reply;
    auto band = geo::readBandOne(reference.rasters().back().source);
    ASSERT_TRUE(band.ok());
    // Within 1e-6 m: k x 0.1 in binary64 is off the decimal by an ulp or two
    // (1e-9 m here), far below the 0.1 m a wrong row would be.
    EXPECT_NEAR(band->geotransform[0], 330000.1, 1e-6);
    EXPECT_NEAR(band->geotransform[3] + 10 * band->geotransform[5], 6250000.3, 1e-6);
}

TEST_F(GridVerb, AGivenExtentKeepsItsCornerAndSizeTakesItAsItIs)
{
    lattice(0, 0, 100, 100, 10);
    // 0..99 at 5 m is 19.8 cells: 20, so 0..100.
    auto reply = run("RASTER GRID DRAWING cell=5 extent=0,0,99,99");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(record(geo::parseRecords(*reply), "grid")->get("extent"), "0,0,100,100");
    // 10 x 10 cells over 0..100: 10 m each.
    reply = run("RASTER GRID DRAWING size=10x10 extent=0,0,100,100 NAME coarse");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(record(geo::parseRecords(*reply), "grid")->get("cell"), "10");
    EXPECT_NEAR(valueAt(55, 45), plane(55, 45), 1e-9);
}

TEST_F(GridVerb, InverseDistanceTakesPowerAndRadius)
{
    // Every 10 m cell's centre is 7.07 m from the four lattice points round it
    // and 15.8 m from the next; a radius of 8 m takes the four, equally
    // weighted, and the mean of a plane at four symmetric corners is the
    // plane at the centre: (55, 45) holds 100 + 5.5 + 2.25 = 107.75.
    lattice(0, 0, 100, 100, 10);
    auto reply = run("RASTER GRID DRAWING method=invdist power=2 radius=8 cell=10");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(record(geo::parseRecords(*reply), "grid")->get("method"), "invdist");
    EXPECT_NEAR(valueAt(55, 45), 107.75, 1e-9);
}

TEST_F(GridVerb, AnEmptyScopeIsReported)
{
    lattice(0, 0, 100, 100, 10);
    auto reply = run("RASTER GRID AREA 1000,1000,1010,1010 cell=5");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    EXPECT_EQ(record(records, "grid")->get("ran"), "no");
    EXPECT_EQ(record(records, "scope")->get("matched"), "0");
    EXPECT_TRUE(reference.rasters().empty());
}

TEST_F(GridVerb, PointsWithNoHeightAtAllAreReportedAndNothingRuns)
{
    draw({spot(0, 0, std::nullopt), spot(10, 0, std::nullopt), spot(0, 10, std::nullopt)});
    auto reply = run("RASTER GRID DRAWING cell=5");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    EXPECT_EQ(record(records, "grid")->get("ran"), "no");
    EXPECT_EQ(record(records, "scope")->get("skipped.heightless"), "3");
    EXPECT_TRUE(reference.rasters().empty());
}

TEST_F(GridVerb, WhatTheVerbCannotDoIsRefusedBeforeAnythingRuns)
{
    lattice(0, 0, 20, 20, 10);
    const auto refused = [&](const std::string& line, ErrorCode code, const std::string& says) {
        auto reply = run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, code) << line;
        EXPECT_NE(reply.error().describe().find(says), std::string::npos)
            << line << ": " << reply.error().describe();
    };
    // The methods are the catalogue's, and the refusal lists them.
    refused("RASTER GRID DRAWING method=kriging", ErrorCode::InvalidArgument, "linear");
    // power is for the inverse-distance methods only.
    refused("RASTER GRID DRAWING method=linear power=2", ErrorCode::InvalidArgument, "invdist");
    refused("RASTER GRID DRAWING cell=5 size=10x10", ErrorCode::InvalidArgument, "give one");
    refused("RASTER GRID DRAWING cell=0", ErrorCode::InvalidArgument, "positive");
    refused("RASTER GRID DRAWING cell=5 cell=2", ErrorCode::InvalidArgument, "once");
    refused("RASTER GRID DRAWING extent=0,0,0,10", ErrorCode::InvalidArgument, "no area");
    refused("RASTER GRID DRAWING TO LAYER terrain/dem", ErrorCode::Unsupported, "raster");
    refused("RASTER GRID DRAWING bogus", ErrorCode::InvalidArgument, "bogus");
    // A grid of more than 25 million cells, and the cell that would fit.
    refused("RASTER GRID DRAWING cell=0.001", ErrorCode::InvalidArgument, "0.004");
    EXPECT_TRUE(reference.rasters().empty());
}

TEST_F(GridVerb, AWhereFilterAndTheOptionsMayComeInAnyOrder)
{
    // The lattice on the layer spots, and a stray point at the datum on 0
    // where the cell (52.5, 47.5) is. WHERE LAYER=spots leaves the stray out,
    // so the cell holds the plane's 107.625; and method= after the filter is
    // the verb's option, not a condition.
    ASSERT_TRUE(interpreter.run("LAYER NEW spots").ok());
    std::vector<Entity> points;
    for (double x = 0; x <= 100; x += 10) {
        for (double y = 0; y <= 100; y += 10) {
            Entity entity = spot(x, y, plane(x, y));
            entity.layer = "spots";
            points.push_back(std::move(entity));
        }
    }
    draw(std::move(points));
    draw({spot(52.5, 47.5, 0.0)});
    auto reply = run("RASTER GRID cell=5 DRAWING WHERE LAYER=spots method=linear");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    EXPECT_EQ(record(records, "grid")->get("method"), "linear");
    EXPECT_EQ(record(records, "scope")->get("matched"), "121");
    EXPECT_NEAR(valueAt(52.5, 47.5), 107.625, 1e-9);
}

TEST_F(GridVerb, PreviewChangesNothingAndAFileNeedsOverwriteToBeReplaced)
{
    lattice(0, 0, 100, 100, 10);
    auto preview = run("RASTER GRID DRAWING cell=5 PREVIEW");
    ASSERT_TRUE(preview.ok()) << preview.error().describe();
    EXPECT_NE(preview->find("preview valid=yes changed=no"), std::string::npos) << *preview;
    EXPECT_TRUE(reference.rasters().empty());

    std::filesystem::create_directories(scratch);
    const std::string file = (scratch / "dem.tif").generic_string();
    auto written = run("RASTER GRID DRAWING cell=5 TO FILE \"" + file + "\"");
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_NE(written->find("kind=file target=file"), std::string::npos) << *written;
    EXPECT_TRUE(std::filesystem::exists(scratch / "dem.tif"));
    auto again = run("RASTER GRID DRAWING cell=5 TO FILE \"" + file + "\"");
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, ErrorCode::AlreadyExists);
    EXPECT_TRUE(run("RASTER GRID DRAWING cell=5 TO FILE \"" + file + "\" OVERWRITE").ok());
}

TEST_F(GridVerb, ACancelledGridAddsNothing)
{
    lattice(0, 0, 100, 100, 10);
    auto prepared = geo::prepare(context, "RASTER GRID DRAWING cell=5");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    ASSERT_FALSE(prepared->reply.has_value());
    std::stop_source stop;
    stop.request_stop();
    auto apply = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().code, ErrorCode::InvalidState);
    EXPECT_TRUE(reference.rasters().empty());
}

TEST_F(GridVerb, TheVerbIsInTheHelpEveryFrontEndShows)
{
    EXPECT_TRUE(geo::handles("RASTER GRID DRAWING"));
    EXPECT_TRUE(geo::handles("raster grid"));
    EXPECT_FALSE(geo::handles("RASTER NOSUCH"));
    EXPECT_NE(geo::helpText().find("RASTER GRID"), std::string::npos);
}

// ---- katana_cli and katana_mcp ------------------------------------------------------------------

class Captured {
  public:
    Captured() : out_(std::cout.rdbuf(outText_.rdbuf())), err_(std::cerr.rdbuf(errText_.rdbuf())) {}
    ~Captured()
    {
        std::cout.rdbuf(out_);
        std::cerr.rdbuf(err_);
    }
    Captured(const Captured&) = delete;
    Captured& operator=(const Captured&) = delete;
    [[nodiscard]] std::string out() const { return outText_.str(); }
    [[nodiscard]] std::string err() const { return errText_.str(); }

  private:
    std::ostringstream outText_;
    std::ostringstream errText_;
    std::streambuf* out_;
    std::streambuf* err_;
};

TEST(GridSession, RasterGridRunsThroughTheSessionAndTheMcpServer)
{
    katana::app::Session session(nullptr);
    katana::app::mcp::Server server{session, "9.9.9"};
    const auto call = [&](const std::string& tool, nlohmann::json arguments) {
        const nlohmann::json message{
            {"jsonrpc", "2.0"},
            {"id", 1},
            {"method", "tools/call"},
            {"params", {{"name", tool}, {"arguments", std::move(arguments)}}}};
        const auto reply = server.handle(message.dump());
        EXPECT_TRUE(reply.has_value());
        return reply ? nlohmann::json::parse(*reply)["result"] : nlohmann::json();
    };
    // Three points given a height of 100 by MODIFY, as a person would.
    for (const char* line : {"POINT 0,0", "POINT 20,0", "POINT 0,20"}) {
        ASSERT_TRUE(session.run(line));
    }
    ASSERT_TRUE(session.run("MODIFY DRAWING WHERE TYPE=point SET PROP=elevation:100"));
    const nlohmann::json result =
        call("katana_run_commands", {{"commands", {"RASTER GRID DRAWING cell=5 NAME ground"}}});
    ASSERT_FALSE(result.value("isError", true)) << result.dump();
    const std::string text = result["content"][0]["text"].get<std::string>();
    EXPECT_NE(text.find("grid method=linear"), std::string::npos) << text;
    EXPECT_NE(text.find("target=reference id=1 name=ground"), std::string::npos) << text;
    Captured captured;
    EXPECT_TRUE(session.run("RASTER GRID DRAWING cell=5 NAME ground PREVIEW"));
    EXPECT_NE(captured.out().find("preview valid=yes"), std::string::npos) << captured.out();
    EXPECT_FALSE(session.run("RASTER GRID DRAWING method=kriging"));
    EXPECT_NE(captured.err().find("error: InvalidArgument"), std::string::npos) << captured.err();
}

} // namespace
