// The RASTER SLOPE and RASTER ASPECT verbs (src/katana_app/geo/slope_verbs.cpp,
// docs/terrain.md "Slope and aspect"): the slope or aspect kept as a
// derived reference raster of its values, and slope classes drawn as areas
// on <areas>/<class> in one undo step, with each class's area.
//
// Fixtures: tests/geo/data/plane.asc (40 x 30 cells of 1 m from (0,0),
// z = 100 + 0.05 x at the cell centres: a slope of 5 %, falling to the west)
// and grids written here. Slope is Horn's: dz/dx = ((c + 2f + i) - (a + 2d +
// g)) / 8 cells over the 3 x 3 window, which on a surface that varies in x
// alone is the difference of the two neighbours over two cells.

#include <cmath>
#include <filesystem>
#include <fstream>
#include <numbers>
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
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "session.hpp"

namespace {

namespace geo = katana::app::geo;
namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::interop::RasterOverlay;

const std::string kData = KATANA_GEO_TEST_DATA;
const std::string kPlane = kData + "/plane.asc";

// plane.asc is read as Float32: each height within 3.815e-6 of its value
// near 100. Horn's dz/dx is a weighted difference whose weights sum to 8
// over 8 cells, so it is within 3.815e-6 of 0.05: 3.8e-4 % of slope, and
// 3.815e-6 rad x 57.3 = 2.2e-4 degrees. The Float32 result adds half its
// step (2.4e-7 near 5, 1.2e-7 near 2.9).
constexpr double kPercentTolerance = 5e-4;
constexpr double kDegreeTolerance = 2.5e-4;

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-slope-verbs-" + name))
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
    [[nodiscard]] std::string file(const std::string& name) const
    {
        return (path_ / name).generic_string();
    }

  private:
    std::filesystem::path path_;
};

// An ESRI ASCII grid of 1 m cells from (0,0), height(column, row) at each
// cell centre, row 0 the northern (top) row.
template <typename Height>
std::string grid(const TempDir& folder, const std::string& name, int columns, int rows,
                 Height height)
{
    const std::string path = folder.file(name);
    std::ofstream out(path, std::ios::binary);
    out.precision(17);
    out << "ncols " << columns << "\nnrows " << rows << "\nxllcorner 0\nyllcorner 0\ncellsize 1\n";
    for (int row = 0; row < rows; ++row) {
        for (int column = 0; column < columns; ++column) {
            out << height(column, row) << (column + 1 < columns ? " " : "\n");
        }
    }
    return path;
}

class SlopeVerbs : public ::testing::Test {
  protected:
    TempDir scratch{::testing::UnitTest::GetInstance()->current_test_info()->name()};
    TempDir files{std::string(::testing::UnitTest::GetInstance()->current_test_info()->name()) +
                  "-files"};
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

    static std::vector<geo::Record> records(const std::string& reply, const std::string& kind)
    {
        std::vector<geo::Record> found;
        for (const geo::Record& each : geo::parseRecords(reply)) {
            if (each.kind == kind) {
                found.push_back(each);
            }
        }
        return found;
    }

    // The class record of `name`.
    static std::optional<geo::Record> classRecord(const std::string& reply, const std::string& name)
    {
        for (const geo::Record& each : records(reply, "class")) {
            if (each.get("name") == name) {
                return each;
            }
        }
        return std::nullopt;
    }

    // The values of the reference raster the line made, from its file.
    std::vector<double> values(int& width) const
    {
        EXPECT_FALSE(reference.rasters().empty());
        const RasterOverlay& raster = reference.rasters().back();
        auto opened = katana::gis::GdalDataset::open(raster.source);
        EXPECT_TRUE(opened.ok());
        auto info = (*opened)->rasterInfo();
        width = info->width;
        auto band = (*opened)->readBand(1);
        EXPECT_TRUE(band.ok());
        return band.ok() ? *band : std::vector<double>{};
    }

    std::size_t on(const std::string& layer) const
    {
        return document.model().entities.countOnLayer(layer);
    }
};

// ---- the contract: what the verbs bind of GDAL's --------------------------------------------

TEST(SlopeContract, TheAnalysisStillFindsTheArgumentsItBinds)
{
    using katana::geo_test::expectArgument;
    expectArgument({"raster", "slope"}, "unit", gp::ArgType::String, false);
    expectArgument({"raster", "aspect"}, "convention", gp::ArgType::String, false);
    expectArgument({"raster", "reclassify"}, "mapping", gp::ArgType::String, true);
    expectArgument({"raster", "reclassify"}, "output-data-type", gp::ArgType::String, false);
    expectArgument({"raster", "sieve"}, "size-threshold", gp::ArgType::Integer, false);
    expectArgument({"raster", "polygonize"}, "attribute-name", gp::ArgType::String, false);
    expectArgument({"raster", "clip"}, "geometry", gp::ArgType::String, false);
}

// ---- slope and aspect -----------------------------------------------------------------------

TEST_F(SlopeVerbs, APlaneRisingOneInTwentyIsFivePercentAndTwoPointEightSixDegrees)
{
    // Inside the edge: GDAL computes the edge cells too, by its own edge
    // rule (a corner reads half the gradient: 2.5 %), which is GDAL's and not
    // the plane's.
    const std::string reply = ran("RASTER SLOPE FILE \"" + kPlane + "\"");
    ASSERT_EQ(records(reply, "slope").size(), 1U) << reply;
    EXPECT_EQ(records(reply, "slope")[0].get("unit").value_or(""), "percent");
    const RasterOverlay& percent = reference.rasters().back();
    EXPECT_EQ(percent.role, katana::interop::RasterRole::Derived);
    EXPECT_EQ(percent.displayStyle, katana::interop::RasterDisplayStyle::Slope);
    EXPECT_EQ(percent.name, "plane-slope");
    int width = 0;
    std::vector<double> slope = values(width);
    ASSERT_EQ(slope.size(), 1200U);
    for (int row = 1; row < 29; ++row) {
        for (int column = 1; column < 39; ++column) {
            ASSERT_NEAR(slope[static_cast<std::size_t>(row * width + column)], 5.0,
                        kPercentTolerance)
                << column << "," << row;
        }
    }
    // Drawn coloured, not as a grey stretch of its values.
    const std::size_t middle = (15 * static_cast<std::size_t>(percent.width) + 20) * 4;
    EXPECT_FALSE(percent.rgba[middle] == percent.rgba[middle + 1] &&
                 percent.rgba[middle + 1] == percent.rgba[middle + 2]);

    ran("RASTER SLOPE FILE \"" + kPlane + "\" unit=degree");
    slope = values(width);
    const double degrees = std::atan(0.05) * 180.0 / std::numbers::pi; // 2.8624052
    for (int row = 1; row < 29; ++row) {
        for (int column = 1; column < 39; ++column) {
            ASSERT_NEAR(slope[static_cast<std::size_t>(row * width + column)], degrees,
                        kDegreeTolerance);
        }
    }
}

TEST_F(SlopeVerbs, ItsAspectIs270)
{
    // It rises to the east, so it faces west: 270 degrees clockwise from
    // north. The direction's error is the cross-slope's over the slope: at
    // most 3.815e-6 / 0.05 rad = 4.4e-3 degrees.
    const std::string reply = ran("RASTER ASPECT FILE \"" + kPlane + "\"");
    ASSERT_EQ(records(reply, "aspect").size(), 1U) << reply;
    EXPECT_EQ(reference.rasters().back().name, "plane-aspect");
    int width = 0;
    const std::vector<double> aspect = values(width);
    for (int row = 1; row < 29; ++row) {
        for (int column = 1; column < 39; ++column) {
            ASSERT_NEAR(aspect[static_cast<std::size_t>(row * width + column)], 270.0, 5e-3);
        }
    }
}

TEST_F(SlopeVerbs, ASurfaceIsSampledAtItsCellAndSloped)
{
    // The surface spans the cell centres, 39 x 29 m: 39 x 29 cells of 1 m.
    ran("SURFACE FROM FILE \"" + kPlane + "\" NAME plane");
    const std::string reply = ran("RASTER SLOPE SURFACE plane CELL 1");
    EXPECT_EQ(records(reply, "slope")[0].get("raster").value_or(""), "39x29") << reply;
    EXPECT_EQ(reference.rasters().back().name, "plane-slope");
}

// ---- classes as areas -----------------------------------------------------------------------

TEST_F(SlopeVerbs, ClassesTwoAndTenPutTheWholePlaneInOneClass)
{
    // Every one of the 40 x 30 cells has a slope - GDAL's edge rule computes
    // the edges, 2.5 % at the corners - and every one lies in [2, 10): one
    // region of 1200 m2.
    const std::string reply = ran("RASTER SLOPE FILE \"" + kPlane + "\" classes=2,10");
    const auto middle = classRecord(reply, "2-10");
    ASSERT_TRUE(middle) << reply;
    EXPECT_EQ(middle->get("area").value_or(""), "1200.000");
    EXPECT_EQ(middle->get("polygons").value_or(""), "1");
    EXPECT_EQ(classRecord(reply, "0-2")->get("area").value_or(""), "0.000");
    EXPECT_EQ(classRecord(reply, "10+")->get("to").value_or("x"), "");
    ASSERT_EQ(on("terrain/slope/2-10"), 1U);
    const Entity* area =
        document.model().entities.find(document.model().entities.idsOnLayer("terrain/slope/2-10")[0]);
    EXPECT_TRUE(std::get<Polyline2>(area->geometry).closed);
    EXPECT_NEAR(std::get<Polyline2>(area->geometry).area(), 1200.0, 1e-9);
    EXPECT_EQ(std::get<std::int64_t>(area->properties.at("slope_class")), 2);
    EXPECT_EQ(std::get<double>(area->properties.at("slope_from")), 2.0);
    EXPECT_EQ(std::get<double>(area->properties.at("slope_to")), 10.0);
    EXPECT_EQ(std::get<std::string>(area->properties.at("slope_unit")), "percent");
    EXPECT_EQ(records(reply, "legend").size(), 3U);
}

TEST_F(SlopeVerbs, ARasterWithAStepGivesTwoClassesWithHandComputedAreas)
{
    // 20 columns rising 2 % (z = 0.02 x), then 20 rising 20 % (z = 0.4 +
    // 0.2 (x - 20)), each 20 x 30 m. Horn reads the two neighbours across the
    // step: at x = 19.5 (0.5 - 0.37) / 2 = 6.5 %, at x = 20.5 (0.7 - 0.39) / 2
    // = 15.5 %; the 20 % plane's corners read half, 10 %. With a break at 8,
    // the first 20 columns are gentle and the rest steep: 600 m2 each.
    const std::string step = grid(files, "step.asc", 40, 30, [](int column, int) {
        const double x = column + 0.5;
        return x < 20.0 ? 0.02 * x : 0.4 + 0.2 * (x - 20.0);
    });
    const std::string reply = ran("RASTER SLOPE FILE \"" + step + "\" classes=8 areas=site/grade");
    const auto gentle = classRecord(reply, "0-8");
    const auto steep = classRecord(reply, "8+");
    ASSERT_TRUE(gentle && steep) << reply;
    EXPECT_EQ(gentle->get("area").value_or(""), "600.000");
    EXPECT_EQ(steep->get("area").value_or(""), "600.000");
    EXPECT_EQ(on("site/grade/0-8"), 1U);
    EXPECT_EQ(on("site/grade/8+"), 1U);
}

TEST_F(SlopeVerbs, MinAreaSievesSpecks)
{
    // One cell raised 1 m in flat ground: Horn's window gives its four side
    // neighbours 2 x 1 / 8 = 25 % and its four diagonal ones sqrt(2) / 8 =
    // 17.7 %, and the raised cell itself 0 (its own height is not in its
    // window). So a ring of 8 steep cells - 8 m2, a region with a hole - and
    // the 1 m2 top inside it, gentle. min_area=10 sieves both specks into the
    // ground around them: 900 m2 gentle, nothing steep.
    const std::string bump = grid(files, "bump.asc", 30, 30, [](int column, int row) {
        return column == 15 && row == 15 ? 101.0 : 100.0;
    });
    const std::string specks = ran("RASTER SLOPE FILE \"" + bump + "\" classes=10 areas=specks");
    EXPECT_EQ(classRecord(specks, "10+")->get("area").value_or(""), "8.000") << specks;
    EXPECT_EQ(classRecord(specks, "10+")->get("polygons").value_or(""), "1");
    EXPECT_EQ(classRecord(specks, "0-10")->get("area").value_or(""), "892.000");
    EXPECT_EQ(classRecord(specks, "0-10")->get("polygons").value_or(""), "2");

    const std::string sieved =
        ran("RASTER SLOPE FILE \"" + bump + "\" classes=10 min_area=10 areas=sieved");
    ASSERT_EQ(records(sieved, "sieved").size(), 1U) << sieved;
    EXPECT_EQ(records(sieved, "sieved")[0].get("cells").value_or(""), "10");
    EXPECT_EQ(classRecord(sieved, "10+")->get("area").value_or(""), "0.000");
    EXPECT_EQ(classRecord(sieved, "0-10")->get("area").value_or(""), "900.000");
    EXPECT_EQ(on("sieved/10+"), 0U);
    EXPECT_EQ(on("sieved/0-10"), 1U);
}

TEST_F(SlopeVerbs, OneUndoRemovesEveryClassPolygon)
{
    const std::string bump = grid(files, "bump.asc", 30, 30, [](int column, int row) {
        return column == 15 && row == 15 ? 101.0 : 100.0;
    });
    ran("RASTER SLOPE FILE \"" + bump + "\" classes=10");
    // The ring and its hole, the ground and its hole, the top: 5 rings.
    ASSERT_EQ(document.model().entities.size(), 5U);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.size(), 0U);
}

TEST_F(SlopeVerbs, AScopeKeepsTheAnalysisInsideItsClosedShapes)
{
    // A triangle with its right angle at (5,5) and its legs 10.25 long: the
    // plane lies in [2, 10) all over it, so that class's area is the
    // triangle's, 10.25 x 10.25 / 2 = 52.53125 m2 - not the 66 cells GDAL's
    // raster clip keeps (every cell it touches: i + j <= 10 of the 11 x 11
    // cells from (5,5)), nor the 121 of its box. The open line beside it
    // bounds nothing and is counted.
    Entity square;
    Polyline2 outline;
    outline.vertices = {{5, 5}, {15.25, 5}, {5, 15.25}};
    outline.closed = true;
    square.geometry = outline;
    square.layer = "site";
    Entity open;
    Polyline2 line;
    line.vertices = {{20, 5}, {30, 5}};
    open.geometry = line;
    open.layer = "site";
    katana::entity::Layer site;
    site.name = "site";
    ASSERT_TRUE(document.execute(katana::commands::createLayer(site)).ok());
    ASSERT_TRUE(document.execute(katana::commands::createEntities({square, open})).ok());
    const std::string reply =
        ran("RASTER SLOPE FILE \"" + kPlane + "\" classes=2,10 areas=inside LAYERS site");
    const auto areas = records(reply, "areas");
    ASSERT_EQ(areas.size(), 1U) << reply;
    EXPECT_EQ(areas[0].get("used").value_or(""), "1");
    EXPECT_EQ(areas[0].get("skipped.open").value_or(""), "1");
    // The record rounds to the thousandth: 52.531.
    EXPECT_EQ(classRecord(reply, "2-10")->get("area").value_or(""), "52.531");
    EXPECT_EQ(on("inside/2-10"), 1U);
}

TEST_F(SlopeVerbs, AScopeWithNoClosedShapeMakesNothingAndSaysSo)
{
    Entity open;
    Polyline2 line;
    line.vertices = {{0, 0}, {10, 10}};
    open.geometry = line;
    ASSERT_TRUE(document.execute(katana::commands::createEntities({open})).ok());
    const std::string reply = ran("RASTER SLOPE FILE \"" + kPlane + "\" classes=5 DRAWING");
    EXPECT_NE(reply.find("made=no"), std::string::npos) << reply;
    EXPECT_TRUE(reference.rasters().empty());
    EXPECT_EQ(document.model().entities.size(), 1U);
}

TEST_F(SlopeVerbs, PreviewAndACancelledRunMakeNothing)
{
    const std::string reply = ran("RASTER SLOPE FILE \"" + kPlane + "\" classes=5 PREVIEW");
    EXPECT_NE(reply.find("preview valid=yes changed=no"), std::string::npos);
    auto prepared = geo::prepare(context, "RASTER SLOPE FILE \"" + kPlane + "\" classes=5");
    ASSERT_TRUE(prepared.ok());
    std::stop_source stop;
    stop.request_stop();
    auto apply = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().message, "cancelled");
    EXPECT_TRUE(reference.rasters().empty());
    EXPECT_EQ(document.model().entities.size(), 0U);
    EXPECT_TRUE(std::filesystem::is_empty(scratch.path()));
}

TEST_F(SlopeVerbs, WhatTheVerbsCannotDoIsRefusedNamingIt)
{
    const std::string slope = "RASTER SLOPE FILE \"" + kPlane + "\"";
    for (const char* words :
         {" unit=radian", " classes=10,5", " classes=0", " classes=5,5", " classes=95 unit=degree",
          " min_area=10", " areas=grades", " classes=5 min_area=0", " classes=5 areas=/bad",
          " colour=red"}) {
        const auto refused = run(slope + words);
        ASSERT_FALSE(refused.ok()) << words;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument) << words;
    }
    EXPECT_EQ(run("RASTER ASPECT FILE \"" + kPlane + "\" unit=degree").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("RASTER SLOPE DRAWING").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(run("RASTER SLOPE SURFACE nothing").error().code, ErrorCode::NotFound);
    EXPECT_TRUE(reference.rasters().empty());
}

// ---- through a Session, as katana_cli and katana_mcp run it -----------------------------------

TEST(SlopeSession, TheSessionDrawsTheClassesAndReportsTheirAreas)
{
    katana::app::Session session(nullptr);
    testing::internal::CaptureStdout();
    const bool made = session.run("RASTER SLOPE FILE \"" + kPlane + "\" classes=2,10");
    const std::string out = testing::internal::GetCapturedStdout();
    EXPECT_TRUE(made);
    EXPECT_NE(out.find("class name=2-10 from=2 to=10 unit=percent area=1200.000 polygons=1"),
              std::string::npos)
        << out;
}

} // namespace
