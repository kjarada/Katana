// The DEM tools (src/katana_app/geo/dem_verbs.cpp, docs/terrain.md "The DEM
// tools"): RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT and DIFFERENCE
// through the executor, as the window runs them, and through a Session and
// the MCP server, as katana_cli and katana_mcp do. Every expected value is
// worked by hand from tests/geo/data/plane.asc: 40 x 30 cells of 1 m from
// (0,0), z = 100 + 0.05x at the cell centres, read by GDAL as Float32.

#include <cmath>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iostream>
#include <optional>
#include <sstream>
#include <stop_token>
#include <string>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "contract_support.hpp"
#include "geo/dem_support.hpp"
#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "mcp_server.hpp"
#include "session.hpp"

namespace {

namespace geo = katana::app::geo;
namespace gp = katana::gis::processing;
using katana::core::ErrorCode;

const std::string kPlane = std::string(KATANA_GEO_TEST_DATA) + "/plane.asc";

class DemVerbs : public ::testing::Test {
  protected:
    std::filesystem::path scratch =
        std::filesystem::temp_directory_path() /
        ("katana-dem-verbs-" +
         std::string(::testing::UnitTest::GetInstance()->current_test_info()->name()));
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, scratch / "derived", {}, {}};

    void SetUp() override
    {
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
        std::filesystem::create_directories(scratch, error);
    }
    void TearDown() override
    {
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
    }

    std::string at(const std::string& name) const { return (scratch / name).generic_string(); }

    katana::core::Result<std::string> run(const std::string& line)
    {
        return geo::runNow(context, line);
    }

    std::string ok(const std::string& line)
    {
        auto reply = run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply ? "" : reply.error().describe());
        return reply ? *reply : std::string();
    }

    static std::optional<geo::Record> record(const std::string& reply, const std::string& kind)
    {
        for (const geo::Record& found : geo::parseRecords(reply)) {
            if (found.kind == kind) {
                return found;
            }
        }
        return std::nullopt;
    }

    geo::BandValues band(const std::filesystem::path& file)
    {
        auto read = geo::readBandOne(file);
        EXPECT_TRUE(read.ok()) << file.generic_string() << ": "
                               << (read ? "" : read.error().describe());
        return read ? *read : geo::BandValues{};
    }

    // plane.asc's values, as GDAL reads them, written with `change` applied
    // to each (value, column, row): Float64, the plane's own grid.
    void writePlane(const std::string& file,
                    const std::function<double(double, int, int)>& change,
                    std::optional<double> noData = std::nullopt)
    {
        const geo::BandValues plane = band(kPlane);
        std::vector<double> values(plane.values.size());
        for (int row = 0; row < plane.height; ++row) {
            for (int column = 0; column < plane.width; ++column) {
                const std::size_t i = static_cast<std::size_t>(row) * plane.width + column;
                values[i] = change(plane.values[i], column, row);
            }
        }
        katana::gis::RasterExportOptions options;
        options.width = plane.width;
        options.height = plane.height;
        options.geotransform = plane.geotransform;
        options.noDataValue = noData;
        ASSERT_TRUE(katana::gis::GdalDataset::writeRaster(file, options, values).ok());
    }
};

// ---- the contract: what the DEM tools bind --------------------------------------------------

TEST(DemVerbsContract, GdalsRasterToolsTakeTheArgumentsTheVerbsBind)
{
    using katana::geo_test::expectArgument;
    expectArgument({"raster", "mosaic"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "mosaic"}, "output", gp::ArgType::Dataset, true);
    expectArgument({"raster", "mosaic"}, "resolution", gp::ArgType::String, false);
    expectArgument({"raster", "mosaic"}, "absolute-path", gp::ArgType::Boolean, false);
    expectArgument({"raster", "clip"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "clip"}, "bbox", gp::ArgType::RealList, false);
    expectArgument({"raster", "clip"}, "like", gp::ArgType::Dataset, false);
    expectArgument({"raster", "fill-nodata"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "fill-nodata"}, "max-distance", gp::ArgType::Integer, false);
    expectArgument({"raster", "fill-nodata"}, "smoothing-iterations", gp::ArgType::Integer, false);
    expectArgument({"raster", "fill-nodata"}, "strategy", gp::ArgType::String, false);
    expectArgument({"raster", "footprint"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "footprint"}, "output", gp::ArgType::Dataset, true);
    expectArgument({"raster", "reproject"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "reproject"}, "output-crs", gp::ArgType::String, false);
    expectArgument({"raster", "reproject"}, "input-crs", gp::ArgType::String, false);
    expectArgument({"raster", "reproject"}, "like", gp::ArgType::Dataset, false);
    expectArgument({"raster", "reproject"}, "resampling", gp::ArgType::String, false);
    expectArgument({"raster", "reproject"}, "resolution", gp::ArgType::RealList, false);
    expectArgument({"raster", "calc"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "calc"}, "calc", gp::ArgType::StringList, true);
    expectArgument({"raster", "calc"}, "dialect", gp::ArgType::String, false);
    // DIFFERENCE subtracts with the builtin dialect: this GDAL has neither
    // muparser nor ExprTk (docs/geoprocessing.md).
    const auto calc = gp::describe({"raster", "calc"});
    ASSERT_TRUE(calc.ok());
    for (const gp::ArgSpec& arg : calc->args) {
        if (arg.name == "dialect") {
            EXPECT_NE(std::ranges::find(arg.choices, std::string("builtin")), arg.choices.end());
        }
    }
}

// ---- MOSAIC ---------------------------------------------------------------------------------

TEST_F(DemVerbs, MosaicOfTwoHalvesEqualsTheWhole)
{
    // The plane cut into its west and east halves (columns 0-19 and 20-39),
    // and whole, by the same GDAL clip so all three are the same kind of
    // file; the halves joined again are the whole, value for value.
    std::filesystem::create_directories(scratch / "tiles");
    ok("GDAL raster clip --window=0,0,20,30 FROM FILE \"" + kPlane + "\" TO FILE \"" +
       at("tiles/west.tif") + "\"");
    ok("GDAL raster clip --window=20,0,20,30 FROM FILE \"" + kPlane + "\" TO FILE \"" +
       at("tiles/east.tif") + "\"");
    ok("GDAL raster clip --window=0,0,40,30 FROM FILE \"" + kPlane + "\" TO FILE \"" +
       at("whole.tif") + "\"");

    const std::string reply = ok("RASTER MOSAIC FILE \"" + at("tiles/west.tif") + "\" FILE \"" +
                                 at("tiles/east.tif") + "\" NAME joined");
    const auto mosaic = record(reply, "mosaic");
    ASSERT_TRUE(mosaic) << reply;
    EXPECT_EQ(mosaic->get("tiles"), "2");
    EXPECT_EQ(mosaic->get("virtual"), "yes");
    const auto output = record(reply, "output");
    ASSERT_TRUE(output) << reply;
    EXPECT_EQ(output->get("name"), "joined");
    EXPECT_EQ(output->get("raster"), "40x30");
    ASSERT_EQ(reference.rasters().size(), 1u);
    EXPECT_EQ(reference.rasters().front().source.extension(), ".vrt");

    const std::string compared = ok("GDAL raster compare --skip-binary FROM input RASTER joined "
                                    "FROM reference FILE \"" + at("whole.tif") + "\"");
    EXPECT_NE(compared.find("return code=0"), std::string::npos) << compared;
}

TEST_F(DemVerbs, AFolderOrAPatternIsItsRastersInNameOrder)
{
    std::filesystem::create_directories(scratch / "tiles");
    ok("GDAL raster clip --window=0,0,20,30 FROM FILE \"" + kPlane + "\" TO FILE \"" +
       at("tiles/a.tif") + "\"");
    ok("GDAL raster clip --window=20,0,20,30 FROM FILE \"" + kPlane + "\" TO FILE \"" +
       at("tiles/b.tif") + "\"");
    // A file GDAL does not read as a raster is no tile of a folder.
    {
        std::ofstream note(scratch / "tiles" / "readme.txt");
        note << "survey tiles\n";
    }
    auto folder = ok("RASTER MOSAIC FILE \"" + at("tiles") + "\"");
    EXPECT_EQ(record(folder, "mosaic")->get("tiles"), "2") << folder;
    EXPECT_EQ(record(folder, "output")->get("raster"), "40x30") << folder;
    auto pattern = ok("RASTER MOSAIC FILE \"" + at("tiles/*.tif") + "\"");
    EXPECT_EQ(record(pattern, "mosaic")->get("tiles"), "2") << pattern;
    // The second takes the next name: mosaic-2.
    EXPECT_EQ(record(pattern, "output")->get("name"), "mosaic-2") << pattern;
    auto none = run("RASTER MOSAIC FILE \"" + at("tiles/*.jp2") + "\"");
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::NotFound);
}

TEST_F(DemVerbs, AMixedColourInterpretationIsRefusedInGdalsWords)
{
    // plane.asc's band says nothing of its colour (Undefined); a GeoTIFF
    // GDAL writes says Gray. GDAL's mosaic refuses the mix, and says so.
    ok("GDAL raster clip --window=20,0,20,30 FROM FILE \"" + kPlane + "\" TO FILE \"" +
       at("east.tif") + "\"");
    auto reply = run("RASTER MOSAIC FILE \"" + kPlane + "\" FILE \"" + at("east.tif") + "\"");
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().code, ErrorCode::CommandRejected);
    EXPECT_NE(reply.error().describe().find("color interpretation"), std::string::npos)
        << reply.error().describe();
    EXPECT_TRUE(reference.rasters().empty());
}

TEST_F(DemVerbs, SaveWritesTheMosaicAndKeepsThatFile)
{
    ok("GDAL raster clip --window=0,0,20,30 FROM FILE \"" + kPlane + "\" TO FILE \"" +
       at("west.tif") + "\"");
    ok("GDAL raster clip --window=20,0,20,30 FROM FILE \"" + kPlane + "\" TO FILE \"" +
       at("east.tif") + "\"");
    const std::string saved = at("site.tif");
    const std::string reply = ok("RASTER MOSAIC FILE \"" + at("west.tif") + "\" FILE \"" +
                                 at("east.tif") + "\" SAVE \"" + saved + "\"");
    EXPECT_EQ(record(reply, "mosaic")->get("virtual"), "no") << reply;
    ASSERT_EQ(reference.rasters().size(), 1u);
    EXPECT_EQ(reference.rasters().front().source, std::filesystem::path(saved));
    EXPECT_EQ(record(reply, "output")->get("persisted"), "yes");
    // A file in the way is kept unless the line says OVERWRITE.
    auto again = run("RASTER MOSAIC FILE \"" + at("west.tif") + "\" SAVE \"" + saved + "\"");
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, ErrorCode::AlreadyExists);
}

// ---- CLIP -----------------------------------------------------------------------------------

TEST_F(DemVerbs, ClipToAnAreaHasHandComputedSize)
{
    // 0..20 x 0..15 on 1 m cells: 20 x 15 cells, by hand.
    const std::string reply = ok("RASTER CLIP FILE \"" + kPlane + "\" AREA 0,0,20,15");
    const auto clip = record(reply, "clip");
    ASSERT_TRUE(clip) << reply;
    EXPECT_EQ(clip->get("by"), "area");
    EXPECT_EQ(clip->get("area"), "0,0,20,15");
    EXPECT_EQ(record(reply, "output")->get("raster"), "20x15") << reply;
    EXPECT_EQ(record(reply, "output")->get("name"), "clip");
}

TEST_F(DemVerbs, ClipToADrawnBoundaryKeepsTheCellsInsideIt)
{
    // A 10 x 10 m rectangle on whole metres: the 100 cells inside it, whose
    // values are the plane's (x = 10.5 at its first column: 100.525).
    ASSERT_TRUE(interpreter.run("RECT 10,5 20,15").ok());
    ASSERT_TRUE(interpreter.run("LINE 0,0 40,30").ok());
    const std::string reply = ok("RASTER CLIP FILE \"" + kPlane + "\" DRAWING NAME site");
    const auto clip = record(reply, "clip");
    ASSERT_TRUE(clip) << reply;
    EXPECT_EQ(clip->get("by"), "boundaries");
    // The line bounds nothing: one boundary, the rectangle.
    EXPECT_EQ(clip->get("boundaries"), "1");
    EXPECT_EQ(record(reply, "scope")->get("matched"), "2");
    EXPECT_EQ(record(reply, "output")->get("raster"), "10x10") << reply;
    const geo::BandValues clipped = band(reference.rasters().back().source);
    ASSERT_EQ(clipped.values.size(), 100u);
    EXPECT_NEAR(clipped.values.front(), 100.525, 7.6e-6); // Float32 near 100: ulp 7.6e-6
}

TEST_F(DemVerbs, AScopeWithNoClosedBoundaryIsReportedAndNothingRuns)
{
    ASSERT_TRUE(interpreter.run("LINE 0,0 40,30").ok());
    const std::string reply = ok("RASTER CLIP FILE \"" + kPlane + "\" DRAWING");
    EXPECT_EQ(record(reply, "clip")->get("ran"), "no") << reply;
    EXPECT_EQ(record(reply, "clip")->get("boundaries"), "0");
    EXPECT_TRUE(reference.rasters().empty());
}

// ---- FILL -----------------------------------------------------------------------------------

TEST_F(DemVerbs, FillOfAHoleInAPlaneRestoresThePlaneWithinTolerance)
{
    // A 3 x 3 hole, columns 19-21 and rows 14-16, in the plane. GDAL fills a
    // cell with an inverse-distance mean of the nearest valid cells it finds
    // round it - positive weights, so a value between the least and the
    // greatest of those. They lie on the one-cell ring round the hole,
    // columns 18-22, where the plane runs from 100 + 0.05 x 18.5 = 100.925 to
    // 100 + 0.05 x 22.5 = 101.125: every filled cell lies in that range (the
    // weights are not symmetric, so the centre is not the plane's own value).
    const double noData = -9999.0;
    const auto inHole = [](int column, int row) {
        return column >= 19 && column <= 21 && row >= 14 && row <= 16;
    };
    writePlane(at("hole.tif"),
               [&](double value, int column, int row) { return inHole(column, row) ? noData : value; },
               noData);
    const std::string reply = ok("RASTER FILL FILE \"" + at("hole.tif") + "\"");
    const auto fill = record(reply, "fill");
    ASSERT_TRUE(fill) << reply;
    EXPECT_EQ(fill->get("empty"), "9");
    EXPECT_EQ(fill->get("filled"), "9");
    EXPECT_EQ(fill->get("remaining"), "0");
    const geo::BandValues filled = band(reference.rasters().back().source);
    const geo::BandValues plane = band(kPlane);
    ASSERT_EQ(filled.values.size(), plane.values.size());
    for (int row = 0; row < filled.height; ++row) {
        for (int column = 0; column < filled.width; ++column) {
            const std::size_t i = static_cast<std::size_t>(row) * filled.width + column;
            if (inHole(column, row)) {
                EXPECT_GE(filled.values[i], 100.925 - 1e-9) << column << "," << row;
                EXPECT_LE(filled.values[i], 101.125 + 1e-9) << column << "," << row;
            } else {
                // Everything else is the input, untouched.
                EXPECT_EQ(filled.values[i], plane.values[i]) << column << "," << row;
            }
        }
    }
}

TEST_F(DemVerbs, ARasterWithNothingToFillSaysSoAndMakesNothing)
{
    const std::string reply = ok("RASTER FILL FILE \"" + kPlane + "\"");
    EXPECT_EQ(record(reply, "fill")->get("empty"), "0") << reply;
    EXPECT_EQ(record(reply, "fill")->get("ran"), "no");
    EXPECT_TRUE(reference.rasters().empty());
}

// ---- FOOTPRINT ------------------------------------------------------------------------------

TEST_F(DemVerbs, FootprintOfAFullRasterIsItsExtentRectangle)
{
    // Every cell of the plane holds a value, so where it has data is its
    // extent: the rectangle 0,0 - 40,30, 1200 m2.
    const std::string reply = ok("RASTER FOOTPRINT FILE \"" + kPlane + "\"");
    const auto output = record(reply, "output");
    ASSERT_TRUE(output) << reply;
    EXPECT_EQ(output->get("layer"), "gis/footprint");
    EXPECT_EQ(output->get("created"), "1");
    ASSERT_EQ(document.model().entities.size(), 1u);
    const katana::entity::Entity* footprint =
        document.model().entities.find(document.model().entities.ids().front());
    ASSERT_NE(footprint, nullptr);
    const auto* polyline = std::get_if<katana::geometry::Polyline2>(&footprint->geometry);
    ASSERT_NE(polyline, nullptr);
    EXPECT_TRUE(polyline->closed);
    const katana::geometry::Box2 box = katana::entity::boundingBox(footprint->geometry);
    EXPECT_DOUBLE_EQ(box.min.x, 0.0);
    EXPECT_DOUBLE_EQ(box.min.y, 0.0);
    EXPECT_DOUBLE_EQ(box.max.x, 40.0);
    EXPECT_DOUBLE_EQ(box.max.y, 30.0);
    double twice = 0.0; // the shoelace formula
    const auto& vertices = polyline->vertices;
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        const auto& p = vertices[i];
        const auto& q = vertices[(i + 1) % vertices.size()];
        twice += p.x * q.y - q.x * p.y;
    }
    EXPECT_DOUBLE_EQ(std::abs(twice) / 2.0, 1200.0);
}

// ---- REPROJECT ------------------------------------------------------------------------------

TEST_F(DemVerbs, ReprojectToTheSameCrsKeepsValues)
{
    // The plane placed in GDA94 / MGA zone 56, then reprojected into the
    // system it is in: the same grid, the same values.
    ok("GDAL raster reproject --input-crs=EPSG:28356 --output-crs=EPSG:28356 FROM FILE \"" +
       kPlane + "\" TO FILE \"" + at("mga.tif") + "\"");
    const std::string reply = ok("RASTER REPROJECT FILE \"" + at("mga.tif") + "\" crs=EPSG:28356");
    EXPECT_EQ(record(reply, "reproject")->get("crs"), "EPSG:28356") << reply;
    const geo::BandValues before = band(at("mga.tif"));
    const geo::BandValues after = band(reference.rasters().back().source);
    ASSERT_EQ(after.width, before.width);
    ASSERT_EQ(after.height, before.height);
    EXPECT_EQ(after.geotransform, before.geotransform);
    EXPECT_EQ(after.values, before.values);
}

TEST_F(DemVerbs, ARasterWithNoCoordinateSystemIsRefusedRatherThanGuessed)
{
    // plane.asc has no .prj. GDAL would hand it back unchanged as if it were
    // in the system asked for; the verb asks where it is instead.
    auto refused = run("RASTER REPROJECT FILE \"" + kPlane + "\" crs=EPSG:28356");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidCRS);
    EXPECT_NE(refused.error().describe().find("from="), std::string::npos);
    EXPECT_TRUE(reference.rasters().empty());
    // Said where it is, it runs.
    const std::string reply =
        ok("RASTER REPROJECT FILE \"" + kPlane + "\" from=EPSG:28356 crs=EPSG:28356");
    EXPECT_EQ(record(reply, "output")->get("raster"), "40x30") << reply;
    // And a drawing with no coordinate system has none to reproject to.
    auto nowhere = run("RASTER REPROJECT FILE \"" + kPlane + "\"");
    ASSERT_FALSE(nowhere.ok());
    EXPECT_EQ(nowhere.error().code, ErrorCode::InvalidCRS);
}

// ---- DIFFERENCE -----------------------------------------------------------------------------

TEST_F(DemVerbs, DifferenceOfADemAndItselfRaisedThreeTenthsIs0Point3Everywhere)
{
    // The plane raised 0.3 m, minus the plane: 0.3 in every cell. The raised
    // copy is written from the very values GDAL reads the plane as, so each
    // cell is (v + 0.3) - v in Float64: 0.3 to within an ulp of 100 (1.4e-14).
    writePlane(at("raised.tif"), [](double value, int, int) { return value + 0.3; });
    const std::string reply =
        ok("RASTER DIFFERENCE FILE \"" + at("raised.tif") + "\" FILE \"" + kPlane + "\"");
    const auto difference = record(reply, "difference");
    ASSERT_TRUE(difference) << reply;
    EXPECT_EQ(difference->get("aligned"), "yes");
    const geo::BandValues result = band(reference.rasters().back().source);
    ASSERT_EQ(result.values.size(), 1200u);
    for (const double cell : result.values) {
        ASSERT_NEAR(cell, 0.3, 1e-9);
    }
}

TEST_F(DemVerbs, ItsFillVolumeIsPoint3TimesTheArea)
{
    // 0.3 m over the plane's 40 x 30 m: 0.3 x 1200 = 360 m3 of fill, no cut.
    writePlane(at("raised.tif"), [](double value, int, int) { return value + 0.3; });
    const std::string reply =
        ok("RASTER DIFFERENCE FILE \"" + at("raised.tif") + "\" FILE \"" + kPlane + "\"");
    const auto difference = record(reply, "difference");
    ASSERT_TRUE(difference) << reply;
    EXPECT_EQ(difference->get("cells"), "1200");
    EXPECT_EQ(difference->get("area"), "1200.000");
    EXPECT_EQ(difference->get("fill"), "360.000");
    EXPECT_EQ(difference->get("cut"), "0.000");
    EXPECT_EQ(difference->get("net"), "360.000");
    EXPECT_EQ(difference->get("cell"), "1");
    EXPECT_EQ(difference->get("label"), "grid method, cell 1 m");
    // The other way round it is cut.
    const std::string reversed =
        ok("RASTER DIFFERENCE FILE \"" + kPlane + "\" FILE \"" + at("raised.tif") + "\"");
    EXPECT_EQ(record(reversed, "difference")->get("cut"), "360.000") << reversed;
    EXPECT_EQ(record(reversed, "difference")->get("net"), "-360.000");
}

TEST_F(DemVerbs, AScopeLimitsTheDifferenceToItsBoundaries)
{
    // A 10 x 10 m rectangle: 100 cells of 0.3 m, 30 m3.
    writePlane(at("raised.tif"), [](double value, int, int) { return value + 0.3; });
    ASSERT_TRUE(interpreter.run("RECT 0,0 10,10").ok());
    const std::string reply = ok("RASTER DIFFERENCE FILE \"" + at("raised.tif") + "\" FILE \"" +
                                 kPlane + "\" DRAWING");
    const auto difference = record(reply, "difference");
    ASSERT_TRUE(difference) << reply;
    EXPECT_EQ(difference->get("cells"), "100");
    EXPECT_EQ(difference->get("fill"), "30.000");
    EXPECT_TRUE(record(reply, "scope").has_value());
}

TEST_F(DemVerbs, ASecondRasterOnAnotherGridIsAlignedToTheFirst)
{
    // The raised plane on 2 m cells over the same 40 x 30 m. Aligned to the
    // plane's 1 m grid by bilinear interpolation, which reproduces a linear
    // function exactly between the centres it interpolates: every 1 m cell
    // whose centre has 2 m centres on both sides (x from 1 to 39) differs by
    // 0.3. Both are written from the plane's exact values in Float64, so only
    // the interpolation's own rounding is left (1e-9).
    std::vector<double> coarse;
    for (int row = 0; row < 15; ++row) {
        for (int column = 0; column < 20; ++column) {
            coarse.push_back(100.0 + 0.05 * (2.0 * column + 1.0) + 0.3);
        }
    }
    katana::gis::RasterExportOptions options;
    options.width = 20;
    options.height = 15;
    options.geotransform = {0.0, 2.0, 0.0, 30.0, 0.0, -2.0};
    ASSERT_TRUE(katana::gis::GdalDataset::writeRaster(at("coarse.tif"), options, coarse).ok());
    writePlane(at("fine.tif"), [](double, int column, int) { return 100.0 + 0.05 * (column + 0.5); });
    const std::string reply =
        ok("RASTER DIFFERENCE FILE \"" + at("coarse.tif") + "\" FILE \"" + at("fine.tif") + "\"");
    // The first is the grid: the coarse one.
    ASSERT_TRUE(record(reply, "difference")) << reply;
    EXPECT_EQ(record(reply, "difference")->get("aligned"), "no");
    EXPECT_EQ(record(reply, "difference")->get("cell"), "2");
    const std::string back =
        ok("RASTER DIFFERENCE FILE \"" + at("fine.tif") + "\" FILE \"" + at("coarse.tif") + "\"");
    const auto difference = record(back, "difference");
    ASSERT_TRUE(difference) << back;
    EXPECT_EQ(difference->get("aligned"), "no");
    EXPECT_EQ(difference->get("resampling"), "bilinear");
    const geo::BandValues result = band(reference.rasters().back().source);
    ASSERT_EQ(result.width, 40);
    for (int row = 1; row < 29; ++row) {
        for (int column = 1; column < 39; ++column) {
            const double cell = result.values[static_cast<std::size_t>(row) * 40 + column];
            ASSERT_NEAR(cell, -0.3, 1e-9) << column << "," << row;
        }
    }
}

// ---- what every tool shares ---------------------------------------------------------------

TEST_F(DemVerbs, WhatTheToolsCannotDoIsRefusedBeforeAnythingRuns)
{
    const auto refused = [&](const std::string& line, ErrorCode code, const std::string& says) {
        auto reply = run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, code) << line << ": " << reply.error().describe();
        EXPECT_NE(reply.error().describe().find(says), std::string::npos)
            << line << ": " << reply.error().describe();
    };
    const std::string plane = "FILE \"" + kPlane + "\"";
    refused("RASTER CLIP " + plane, ErrorCode::InvalidArgument, "AREA");
    refused("RASTER CLIP " + plane + " AREA 0,0,10,10 WHERE TYPE=polyline",
            ErrorCode::InvalidArgument, "box");
    refused("RASTER CLIP " + plane + " AREA 0,0,10,10 TO LAYER dem", ErrorCode::Unsupported,
            "raster");
    refused("RASTER FOOTPRINT " + plane + " TO REFERENCE fp", ErrorCode::Unsupported, "LAYER");
    refused("RASTER DIFFERENCE " + plane, ErrorCode::InvalidArgument, "2 rasters");
    refused("RASTER DIFFERENCE " + plane + " " + plane + " " + plane, ErrorCode::InvalidArgument,
            "takes 2");
    refused("RASTER FILL " + plane + " distance=0", ErrorCode::InvalidArgument, "whole number");
    refused("RASTER FILL " + plane + " bogus", ErrorCode::InvalidArgument, "bogus");
    refused("RASTER REPROJECT " + plane + " crs=EPSG:28356 like=dem", ErrorCode::InvalidArgument,
            "give one");
    refused("RASTER MOSAIC SURFACE ground", ErrorCode::InvalidArgument, "surface");
    refused("RASTER CLIP RASTER nosuch AREA 0,0,10,10", ErrorCode::NotFound, "nosuch");
    refused("RASTER FILL", ErrorCode::InvalidArgument, "needs a raster");
    EXPECT_TRUE(reference.rasters().empty());
    EXPECT_EQ(document.model().entities.size(), 0u);
}

TEST_F(DemVerbs, APreviewChecksTheLineAndChangesNothing)
{
    const std::string reply = ok("RASTER CLIP FILE \"" + kPlane + "\" AREA 0,0,20,15 PREVIEW");
    EXPECT_EQ(record(reply, "clip")->get("preview"), "yes") << reply;
    EXPECT_EQ(record(reply, "preview")->get("valid"), "yes");
    // GDAL's own check opens what the line names: a file that is not there
    // is refused before anything runs.
    auto missing = run("RASTER CLIP FILE \"" + at("nosuch.tif") + "\" AREA 0,0,20,15 PREVIEW");
    EXPECT_FALSE(missing.ok());
    EXPECT_TRUE(reference.rasters().empty());
}

TEST_F(DemVerbs, ACancelledToolAddsNothing)
{
    auto prepared = geo::prepare(context, "RASTER CLIP FILE \"" + kPlane + "\" AREA 0,0,20,15");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    ASSERT_FALSE(prepared->reply.has_value());
    std::stop_source stop;
    stop.request_stop();
    auto apply = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().code, ErrorCode::InvalidState);
    EXPECT_TRUE(reference.rasters().empty());
}

TEST_F(DemVerbs, TheToolsAreInTheHelpEveryFrontEndShows)
{
    for (const char* verb : {"MOSAIC", "CLIP", "FILL", "FOOTPRINT", "REPROJECT", "DIFFERENCE"}) {
        EXPECT_TRUE(geo::handles(std::string("RASTER ") + verb)) << verb;
        EXPECT_NE(geo::helpText().find(std::string("RASTER ") + verb), std::string::npos) << verb;
    }
}

// ---- katana_cli and katana_mcp ------------------------------------------------------------------

TEST(DemSession, TheDemToolsRunThroughTheSessionAndTheMcpServer)
{
    katana::app::Session session(nullptr);
    katana::app::mcp::Server server{session, "9.9.9"};
    const nlohmann::json message{
        {"jsonrpc", "2.0"},
        {"id", 1},
        {"method", "tools/call"},
        {"params",
         {{"name", "katana_run_commands"},
          {"arguments",
           {{"commands",
             {"RASTER CLIP FILE \"" + kPlane + "\" AREA 0,0,20,15 NAME west",
              "RASTER FOOTPRINT RASTER west"}}}}}}};
    const auto reply = server.handle(message.dump());
    ASSERT_TRUE(reply.has_value());
    const nlohmann::json result = nlohmann::json::parse(*reply)["result"];
    ASSERT_FALSE(result.value("isError", true)) << result.dump();
    const std::string text = result["content"][0]["text"].get<std::string>();
    EXPECT_NE(text.find("name=west raster=20x15"), std::string::npos) << text;
    EXPECT_NE(text.find("layer=gis/footprint created=1"), std::string::npos) << text;
}

// A drawing with no project keeps its derived rasters in a scratch folder,
// and a result never replaces a file already there. While the folder was
// one per process, a second Session in the same process - katana_geo_tests
// runs dozens - found the first one's `west` there, and its own came back as
// `west-2`: a name not asked for, and a reply that depended on which tests
// ran before (this test failed so whenever the whole binary ran in one
// process). Before that the folder was named by the process id alone, and
// Windows gives a new process the id of one that has ended, so a later run
// found an earlier one's files (docs/geoprocessing.md, "Derived rasters").
TEST(DemSession, ARasterAnotherSessionMadeDoesNotRenameTheResult)
{
#if defined(_WIN32)
    const auto process = static_cast<long long>(_getpid());
#else
    const auto process = static_cast<long long>(getpid());
#endif
    // Each front end has a folder of its own, inside one of the process's
    // named by its id and when it started.
    const std::filesystem::path one = geo::ownScratch();
    const std::filesystem::path two = geo::ownScratch();
    EXPECT_NE(one, two);
    EXPECT_EQ(one.parent_path(), two.parent_path());
    EXPECT_TRUE(one.parent_path().filename().string().starts_with(std::to_string(process) + "-"))
        << one;

    const auto clipWest = [](katana::app::Session& session) {
        katana::app::mcp::Server server{session, "9.9.9"};
        const nlohmann::json message{
            {"jsonrpc", "2.0"},
            {"id", 1},
            {"method", "tools/call"},
            {"params",
             {{"name", "katana_run_commands"},
              {"arguments",
               {{"commands", {"RASTER CLIP FILE \"" + kPlane + "\" AREA 0,0,20,15 NAME west"}}}}}}};
        const auto reply = server.handle(message.dump());
        EXPECT_TRUE(reply.has_value());
        if (!reply) {
            return std::string();
        }
        const nlohmann::json result = nlohmann::json::parse(*reply)["result"];
        EXPECT_FALSE(result.value("isError", true)) << result.dump();
        return result["content"][0]["text"].get<std::string>();
    };
    katana::app::Session first(nullptr);
    katana::app::Session second(nullptr);
    const std::string made = clipWest(first);
    const std::string again = clipWest(second);
    EXPECT_NE(made.find("name=west raster=20x15"), std::string::npos) << made;
    EXPECT_NE(again.find("name=west raster=20x15"), std::string::npos) << again;
}

} // namespace
