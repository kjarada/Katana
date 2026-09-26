// The RASTER SHADE verb (src/katana_app/geo/shade_verbs.cpp, docs/terrain.md
// "Shading") and its ramps (include/katana/interop/geo/colour_ramps.hpp): an
// elevation source rendered by GDAL's hillshade, color-map and blend into a
// derived reference raster, drawn as it was rendered, with its legend.
//
// Fixtures: tests/geo/data/plane.asc (40 x 30 cells of 1 m from (0,0),
// z = 100 + 0.05 x at the cell centres, so 100.025 to 101.975), and flat
// grids written here.

#include <array>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <numbers>
#include <optional>
#include <stop_token>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "contract_support.hpp"
#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/geo/colour_ramps.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "session.hpp"

namespace {

namespace geo = katana::app::geo;
namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::interop::RasterDisplayStyle;
using katana::interop::RasterOverlay;

const std::string kData = KATANA_GEO_TEST_DATA;
const std::string kPlane = kData + "/plane.asc";

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-shade-verb-" + name))
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

// An ESRI ASCII grid of `columns` x `rows` 1 m cells from (0,0), every cell
// `height`.
std::string flatGrid(const TempDir& folder, const std::string& name, int columns, int rows,
                     double height)
{
    const std::string path = folder.file(name);
    std::ofstream out(path, std::ios::binary);
    out << "ncols " << columns << "\nnrows " << rows << "\nxllcorner 0\nyllcorner 0\ncellsize 1\n";
    for (int row = 0; row < rows; ++row) {
        for (int column = 0; column < columns; ++column) {
            out << height << (column + 1 < columns ? " " : "\n");
        }
    }
    return path;
}

struct Rgba {
    int r = 0, g = 0, b = 0, a = 0;
};

Rgba pixel(const RasterOverlay& raster, int column, int row)
{
    const std::size_t at = (static_cast<std::size_t>(row) * static_cast<std::size_t>(raster.width) +
                            static_cast<std::size_t>(column)) *
                           4;
    return {raster.rgba[at], raster.rgba[at + 1], raster.rgba[at + 2], raster.rgba[at + 3]};
}

class ShadeVerb : public ::testing::Test {
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

    // The one reference raster the line made.
    const RasterOverlay& made() const
    {
        EXPECT_EQ(reference.rasters().size(), 1U);
        return reference.rasters().back();
    }
};

// ---- the contract: what RASTER SHADE binds of GDAL's --------------------------------------

TEST(ShadeContract, TheShadingStillFindsTheArgumentsItBinds)
{
    using katana::geo_test::expectArgument;
    expectArgument({"raster", "hillshade"}, "azimuth", gp::ArgType::Real, false);
    expectArgument({"raster", "hillshade"}, "altitude", gp::ArgType::Real, false);
    expectArgument({"raster", "hillshade"}, "zfactor", gp::ArgType::Real, false);
    expectArgument({"raster", "hillshade"}, "variant", gp::ArgType::String, false);
    expectArgument({"raster", "color-map"}, "color-map", gp::ArgType::String, false);
    expectArgument({"raster", "color-map"}, "add-alpha", gp::ArgType::Boolean, false);
    expectArgument({"raster", "blend"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "blend"}, "overlay", gp::ArgType::Dataset, true);
    expectArgument({"raster", "blend"}, "operator", gp::ArgType::String, false);
    expectArgument({"raster", "resize"}, "size", gp::ArgType::StringList, true);
    expectArgument({"raster", "resize"}, "resampling", gp::ArgType::String, false);
    expectArgument({"raster", "slope"}, "unit", gp::ArgType::String, false);
}

// ---- the styles -----------------------------------------------------------------------------

TEST_F(ShadeVerb, FlatGroundHillshadesTo181EverywhereAtAltitude45)
{
    // GDAL's hillshade is 1 + 254 x (sin(altitude) cos(slope) + ...); on flat
    // ground the slope is 0, so 1 + 254 x sin 45 degrees = 180.6: 181, the
    // edges too (they are interpolated, and flat).
    const std::string flat = flatGrid(files, "flat.asc", 12, 9, 100.0);
    const std::string reply = ran("RASTER SHADE FILE \"" + flat + "\" style=hillshade");
    const RasterOverlay& raster = made();
    ASSERT_EQ(raster.width, 12);
    ASSERT_EQ(raster.height, 9);
    for (int row = 0; row < raster.height; ++row) {
        for (int column = 0; column < raster.width; ++column) {
            const Rgba shade = pixel(raster, column, row);
            ASSERT_EQ(shade.r, 181) << column << "," << row;
            ASSERT_EQ(shade.g, 181);
            ASSERT_EQ(shade.b, 181);
            ASSERT_EQ(shade.a, 255);
        }
    }
    EXPECT_EQ(raster.role, katana::interop::RasterRole::Derived);
    EXPECT_EQ(raster.displayStyle, RasterDisplayStyle::Hillshade);
    EXPECT_EQ(raster.name, "flat-hillshade");
    EXPECT_NE(raster.derivation.find("RASTER SHADE"), std::string::npos);
    EXPECT_TRUE(records(reply, "legend").empty()) << "a hillshade is grey: no legend";
}

TEST_F(ShadeVerb, ARampMapsItsEndpointsExactly)
{
    // Over the data's range, the lowest cells (the column at x = 0.5) take the
    // terrain ramp's first colour and the highest (x = 39.5) its last.
    ran("RASTER SHADE FILE \"" + kPlane + "\" style=relief");
    const RasterOverlay& raster = made();
    EXPECT_EQ(raster.displayStyle, RasterDisplayStyle::Relief);
    const igeo::ColourRamp* terrain = igeo::builtInRamp("terrain");
    ASSERT_NE(terrain, nullptr);
    const igeo::RampStop& first = terrain->stops.front();
    const igeo::RampStop& last = terrain->stops.back();
    for (int row = 0; row < raster.height; ++row) {
        const Rgba low = pixel(raster, 0, row);
        EXPECT_EQ(low.r, first.r);
        EXPECT_EQ(low.g, first.g);
        EXPECT_EQ(low.b, first.b);
        EXPECT_EQ(low.a, 255);
        const Rgba high = pixel(raster, raster.width - 1, row);
        EXPECT_EQ(high.r, last.r);
        EXPECT_EQ(high.g, last.g);
        EXPECT_EQ(high.b, last.b);
    }
}

TEST_F(ShadeVerb, LegendRecordsListTheRampStops)
{
    // The terrain ramp's five stops at 0, 1/4, 1/2, 3/4 and 1 of the data's
    // range. The range is the file's heights as GDAL reads them, Float32:
    // 100.025f and 101.975f, which are 1.5e-6 off the decimals. So the stops
    // are about 100.025, 100.5125, 101.0, 101.4875 and 101.975, and each
    // record is its stop rounded to three decimals: within half a thousandth
    // of the stop worked from the Float32 ends.
    const std::string reply = ran("RASTER SHADE FILE \"" + kPlane + "\" style=relief");
    const auto legend = records(reply, "legend");
    const igeo::ColourRamp* terrain = igeo::builtInRamp("terrain");
    ASSERT_EQ(legend.size(), terrain->stops.size()) << reply;
    const double low = static_cast<double>(100.025f);
    const double high = static_cast<double>(101.975f);
    for (std::size_t i = 0; i < legend.size(); ++i) {
        const double stop = low + terrain->stops[i].position * (high - low);
        EXPECT_NEAR(std::stod(legend[i].get("value").value_or("nan")), stop, 5e-4);
        EXPECT_EQ(legend[i].get("r").value_or(""), std::to_string(terrain->stops[i].r));
        EXPECT_EQ(legend[i].get("g").value_or(""), std::to_string(terrain->stops[i].g));
        EXPECT_EQ(legend[i].get("b").value_or(""), std::to_string(terrain->stops[i].b));
    }
    const auto ramp = records(reply, "ramp");
    ASSERT_EQ(ramp.size(), 1U);
    EXPECT_EQ(ramp[0].get("name").value_or(""), "terrain");
    EXPECT_EQ(ramp[0].get("from").value_or(""), "data");
}

TEST_F(ShadeVerb, ReliefOverHillshadeOnFlatGroundKeepsTheRampHue)
{
    // range=100,200 puts the flat ground's 100 at the terrain ramp's first
    // colour, (38, 115, 0). hsv-value keeps its hue and saturation and takes
    // the hillshade's 181 as the value: the colour scaled so its greatest
    // channel is 181, (38 x 181 / 115, 181, 0) = (59.8, 181, 0). An 8-bit
    // channel rounds it: within 1.
    const std::string flat = flatGrid(files, "flat.asc", 8, 6, 100.0);
    ran("RASTER SHADE FILE \"" + flat + "\" style=relief+hillshade range=100,200");
    const RasterOverlay& raster = made();
    EXPECT_EQ(raster.displayStyle, RasterDisplayStyle::ReliefHillshade);
    for (int row = 0; row < raster.height; ++row) {
        for (int column = 0; column < raster.width; ++column) {
            const Rgba shade = pixel(raster, column, row);
            ASSERT_NEAR(shade.r, 38.0 * 181.0 / 115.0, 1.0) << column << "," << row;
            ASSERT_EQ(shade.g, 181);
            ASSERT_EQ(shade.b, 0);
        }
    }
}

TEST_F(ShadeVerb, SlopeShadingColoursTheSlopeInDegrees)
{
    // The plane rises 1 in 20: atan(0.05) = 2.8624052 degrees wherever Horn's
    // 3 x 3 window lies on it. A range of twice that puts it at the slope
    // ramp's middle stop. The Float32 slope is within about 1e-6 degree of
    // it, which moves the colour by 1e-6 / 5.72 of the ramp: none, to 8 bits.
    const double slope = std::atan(0.05) * 180.0 / std::numbers::pi;
    const std::string reply = ran("RASTER SHADE FILE \"" + kPlane + "\" style=slope range=0," +
                                  katana::core::formatExactReal(2.0 * slope));
    const auto shade = records(reply, "shade");
    ASSERT_EQ(shade.size(), 1U) << reply;
    EXPECT_EQ(shade[0].get("unit").value_or(""), "degree");
    const auto ramp = records(reply, "ramp");
    ASSERT_EQ(ramp.size(), 1U);
    EXPECT_EQ(ramp[0].get("name").value_or(""), "slope");
    EXPECT_EQ(ramp[0].get("from").value_or(""), "range");
    const RasterOverlay& raster = made();
    EXPECT_EQ(raster.displayStyle, RasterDisplayStyle::Slope);
    const igeo::RampStop& middle = igeo::builtInRamp("slope")->stops[2];
    const Rgba inside = pixel(raster, 20, 15);
    EXPECT_NEAR(inside.r, middle.r, 1.0);
    EXPECT_NEAR(inside.g, middle.g, 1.0);
    EXPECT_NEAR(inside.b, middle.b, 1.0);
}

TEST_F(ShadeVerb, SaveWritesAGeoTiffWithTheSourceGeotransform)
{
    const std::string save = files.file("shade.tif");
    const std::string reply =
        ran("RASTER SHADE FILE \"" + kPlane + "\" style=relief save=\"" + save + "\"");
    EXPECT_FALSE(records(reply, "saved").empty()) << reply;
    auto opened = katana::gis::GdalDataset::open(save);
    ASSERT_TRUE(opened.ok());
    EXPECT_EQ((*opened)->driverName(), "GTiff");
    auto info = (*opened)->rasterInfo();
    ASSERT_TRUE(info.ok());
    EXPECT_EQ(info->width, 40);
    EXPECT_EQ(info->height, 30);
    EXPECT_EQ(info->bandCount, 4); // red, green, blue and alpha
    // plane.asc's own: its lower-left corner at (0,0), 30 rows of 1 m.
    const std::array<double, 6> expected{0.0, 1.0, 0.0, 30.0, 0.0, -1.0};
    EXPECT_EQ(info->geotransform, expected);
    // A file in the way is replaced only with OVERWRITE.
    EXPECT_EQ(run("RASTER SHADE FILE \"" + kPlane + "\" save=\"" + save + "\"").error().code,
              ErrorCode::AlreadyExists);
    ran("RASTER SHADE FILE \"" + kPlane + "\" save=\"" + save + "\" OVERWRITE");
}

TEST_F(ShadeVerb, ASurfaceSourceIsRasterisedAtItsCell)
{
    // The surface of plane.asc spans its cell centres, 0.5 to 39.5 by 0.5 to
    // 29.5: 39 x 29 m, so a 2 m cell makes ceil(19.5) x ceil(14.5) = 20 x 15.
    ran("SURFACE FROM FILE \"" + kPlane + "\" NAME plane");
    const std::string reply = ran("RASTER SHADE SURFACE plane CELL 2");
    const auto output = records(reply, "output");
    ASSERT_EQ(output.size(), 1U) << reply;
    EXPECT_EQ(output[0].get("raster").value_or(""), "20x15");
    EXPECT_EQ(made().name, "plane-hillshade");
}

TEST_F(ShadeVerb, AReferenceRasterIsShadedFromItsFile)
{
    auto imported = katana::interop::importRaster(kPlane);
    ASSERT_TRUE(imported.ok());
    const auto id = reference.add(std::move(imported).value());
    const std::string reply =
        ran("RASTER SHADE RASTER " + std::to_string(id) + " style=plain NAME grey-plane");
    ASSERT_EQ(reference.rasters().size(), 2U);
    EXPECT_EQ(reference.rasters().back().name, "grey-plane");
    EXPECT_EQ(reference.rasters().back().displayStyle, RasterDisplayStyle::Plain);
    EXPECT_EQ(records(reply, "legend").size(), 2U); // the grey ramp's two stops
}

TEST_F(ShadeVerb, APictureLongerThanTheDisplayCopyIsAveragedDownToIt)
{
    // 5000 cells is more than the 4096 a reference raster's display copy
    // keeps, so the DEM is averaged by ceil(5000 / 4096) = 2 first: 2500 x 3.
    const std::string wide = flatGrid(files, "wide.asc", 5000, 6, 50.0);
    const std::string reply = ran("RASTER SHADE FILE \"" + wide + "\"");
    const auto resampled = records(reply, "resampled");
    ASSERT_EQ(resampled.size(), 1U) << reply;
    EXPECT_EQ(resampled[0].get("from").value_or(""), "5000x6");
    EXPECT_EQ(resampled[0].get("to").value_or(""), "2500x3");
    EXPECT_EQ(made().width, 2500);
}

TEST_F(ShadeVerb, APersonsColourMapFileIsPaintedAndIsTheLegend)
{
    // Percentages are placed on the data's range, as GDAL places them.
    const std::string map = files.file("blue-red.txt");
    {
        std::ofstream out(map, std::ios::binary);
        out << "0% 0 0 255\n100% 255 0 0\n";
    }
    const std::string reply =
        ran("RASTER SHADE FILE \"" + kPlane + "\" style=relief ramp=\"" + map + "\"");
    const auto legend = records(reply, "legend");
    ASSERT_EQ(legend.size(), 2U) << reply;
    EXPECT_NEAR(std::stod(legend[0].get("value").value_or("nan")), 100.025, 5e-4);
    EXPECT_NEAR(std::stod(legend[1].get("value").value_or("nan")), 101.975, 5e-4);
    const RasterOverlay& raster = made();
    const Rgba low = pixel(raster, 0, 0);
    EXPECT_EQ(low.b, 255);
    EXPECT_EQ(low.r, 0);
    const Rgba high = pixel(raster, raster.width - 1, 0);
    EXPECT_EQ(high.r, 255);
    EXPECT_EQ(high.b, 0);
}

TEST_F(ShadeVerb, PreviewAndACancelledRunAddNothing)
{
    const std::string reply = ran("RASTER SHADE FILE \"" + kPlane + "\" PREVIEW");
    EXPECT_NE(reply.find("preview valid=yes changed=no"), std::string::npos);
    auto prepared = geo::prepare(context, "RASTER SHADE FILE \"" + kPlane + "\" style=relief");
    ASSERT_TRUE(prepared.ok());
    std::stop_source stop;
    stop.request_stop();
    auto apply = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().message, "cancelled");
    EXPECT_TRUE(reference.rasters().empty());
    // Nothing of the chain's is left in the scratch folder.
    EXPECT_TRUE(std::filesystem::is_empty(scratch.path()));
}

TEST_F(ShadeVerb, WhatTheVerbCannotDoIsRefusedNamingIt)
{
    const std::string plane = "RASTER SHADE FILE \"" + kPlane + "\"";
    for (const char* words :
         {" style=glow", " azimuth=400", " altitude=95", " z=0", " variant=soft", " range=5,1",
          " range=5", " save=picture.png", " style=relief azimuth=270",
          " style=hillshade ramp=terrain", " colour=red"}) {
        const auto refused = run(plane + words);
        ASSERT_FALSE(refused.ok()) << words;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument) << words;
    }
    EXPECT_EQ(run(plane + " style=relief ramp=no-such-ramp").error().code, ErrorCode::NotFound);
    EXPECT_EQ(run("RASTER SHADE DRAWING").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(run("RASTER SHADE SURFACE nothing").error().code, ErrorCode::NotFound);
    EXPECT_TRUE(reference.rasters().empty());
}

// ---- the ramps ------------------------------------------------------------------------------

TEST(ColourRamps, ASpreadRampHitsItsEndsExactlyAndWritesGdalsText)
{
    const igeo::ColourRamp* slope = igeo::builtInRamp("SLOPE");
    ASSERT_NE(slope, nullptr);
    // 0.1 + 1 x (0.3 - 0.1) is not 0.3 in binary; the end is taken as given.
    const auto entries = igeo::spread(*slope, 0.1, 0.3);
    ASSERT_EQ(entries.size(), 5U);
    EXPECT_EQ(entries.front().value, 0.1);
    EXPECT_EQ(entries.back().value, 0.3);
    EXPECT_EQ(igeo::colourMapText({{1.5, 1, 2, 3}}, true), "1.5 1 2 3 255\nnv 0 0 0 0\n");
    EXPECT_EQ(igeo::colourMapText({{-2, 0, 0, 0}}, false), "-2 0 0 0 255\n");
    EXPECT_EQ(igeo::builtInRampNames(),
              (std::vector<std::string>{"terrain", "diverging", "slope", "grey"}));
    EXPECT_EQ(igeo::builtInRamp("rainbow"), nullptr);
}

TEST(ColourRamps, AColourMapFileIsReadAsGdalReadsIt)
{
    TempDir folder("colour-map");
    const std::filesystem::path map = folder.path() / "map.txt";
    {
        std::ofstream out(map, std::ios::binary);
        out << "# a comment\nnv 0 0 0 0\n50% 10,20,30\n200 1 2 3 255\nwhite 255 255 255\n0\t4 5 6\n";
    }
    auto entries = igeo::readColourMap(map, 100.0, 300.0);
    ASSERT_TRUE(entries.ok());
    ASSERT_EQ(entries->size(), 3U); // sorted by value
    EXPECT_EQ((*entries)[0].value, 0.0);
    EXPECT_EQ((*entries)[1].value, 200.0); // 50% of 100 to 300
    EXPECT_EQ((*entries)[1].r, 10);
    EXPECT_EQ((*entries)[2].value, 200.0);
    EXPECT_EQ(igeo::readColourMap(folder.path() / "none.txt", 0, 1).error().code,
              ErrorCode::NotFound);
}

// ---- through a Session, as katana_cli and katana_mcp run it -----------------------------------

TEST(ShadeSession, TheSessionAddsTheShadingAsAReferenceRaster)
{
    katana::app::Session session(nullptr);
    testing::internal::CaptureStdout();
    const bool shaded = session.run("RASTER SHADE FILE \"" + kPlane + "\" style=relief+hillshade");
    const bool listed = session.run("SURFACE LIST");
    const std::string out = testing::internal::GetCapturedStdout();
    EXPECT_TRUE(shaded);
    EXPECT_TRUE(listed);
    EXPECT_NE(out.find("output arg=output kind=raster target=reference id=1 "
                       "name=plane-relief-hillshade raster=40x30"),
              std::string::npos)
        << out;
    EXPECT_NE(out.find("raster id=1 name=plane-relief-hillshade kind=derived"), std::string::npos)
        << out;
}

} // namespace
