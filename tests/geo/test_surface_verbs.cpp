// The SURFACE verbs (src/katana_app/geo/surface_verbs.cpp, docs/terrain.md
// "Surfaces on every front end"): the session's named surfaces made from a
// raster, a point cloud or the drawing, listed, described, removed and
// written out as a DEM, and TO SURFACE, through the one executor as katana_cli,
// katana_mcp and the window run it.

#include <algorithm>
#include <cfloat>
#include <cmath>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/interop/terrain_io.hpp"
#include "katana/terrain/surface_store.hpp"
#include "session.hpp"

namespace {

namespace geo = katana::app::geo;
namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::geometry::Point2;

const std::string kData = KATANA_GEO_TEST_DATA;
const std::string kPlane = kData + "/plane.asc";

// Float32's step near 100 (2^6 <= z < 2^7): 2^(6 - 23) = 7.63e-6. plane.asc
// is read as Float32 (GDAL's AAIGrid reads a grid of decimals so), so each
// height is within half of it of the value written in the file.
constexpr double kFloat32UlpNear100 = 7.62939453125e-6;

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-surface-verbs-" + name))
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

Entity levelled(double x, double y, std::optional<double> z)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{Point2(x, y)};
    katana::entity::setHeights(entity.properties, {z});
    return entity;
}

class SurfaceVerbs : public ::testing::Test {
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

    // The first record of `kind` in `reply`.
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
};

TEST_F(SurfaceVerbs, TheExecutorTakesTheSurfaceVerbAndHelpNamesIt)
{
    EXPECT_TRUE(geo::handles("SURFACE LIST"));
    EXPECT_TRUE(geo::handles("surface from drawing"));
    EXPECT_NE(geo::helpText().find("SURFACE FROM"), std::string::npos);
    EXPECT_NE(geo::helpText().find("SURFACE EXPORT"), std::string::npos);
}

TEST_F(SurfaceVerbs, SurfaceFromRasterOfAPlaneSpansThePlanesRange)
{
    // plane.asc: 40 x 30 cells of 1 m from (0,0), z = 100 + 0.05 x at the
    // cell centres. The extreme centres are x = 0.5 and 39.5, so by hand
    // zmin = 100.025 and zmax = 101.975, and every one of the 1200 cells is
    // a vertex (under the 400 000 cap). A grid of m x n points in convex
    // position triangulates into 2 (m - 1)(n - 1) = 2 x 39 x 29 = 2262.
    const std::string reply = ran("SURFACE FROM FILE \"" + kPlane + "\"");
    const auto surface = record(reply, "surface");
    ASSERT_TRUE(surface) << reply;
    EXPECT_EQ(surface->get("name").value_or(""), "plane");
    EXPECT_EQ(surface->get("points").value_or(""), "1200");
    EXPECT_EQ(surface->get("triangles").value_or(""), "2262");
    EXPECT_EQ(surface->get("zmin").value_or(""), "100.025");
    EXPECT_EQ(surface->get("zmax").value_or(""), "101.975");
    EXPECT_EQ(surface->get("bounds").value_or(""), "0.500,0.500,39.500,29.500");
    const auto sampled = record(reply, "sampled");
    ASSERT_TRUE(sampled) << reply;
    EXPECT_EQ(sampled->get("stride").value_or(""), "1");
    EXPECT_EQ(sampled->get("nodata").value_or(""), "0");

    const katana::terrain::NamedSurface* kept = surfaces.find("plane");
    ASSERT_NE(kept, nullptr);
    EXPECT_NEAR(kept->surface->minElevation(), 100.025, kFloat32UlpNear100 / 2);
    EXPECT_NEAR(kept->surface->maxElevation(), 101.975, kFloat32UlpNear100 / 2);
}

TEST_F(SurfaceVerbs, FromRasterWithAnAreaOnlyTriangulatesTheArea)
{
    // The window 10,10,20,20 on the 1 m grid is ten columns and ten rows of
    // whole cells, centres x = 10.5 ... 19.5: 100 points, z from
    // 100 + 0.05 x 10.5 = 100.525 to 100 + 0.05 x 19.5 = 100.975.
    const std::string reply =
        ran("SURFACE FROM FILE \"" + kPlane + "\" AREA 10,10,20,20 NAME site");
    const auto surface = record(reply, "surface");
    ASSERT_TRUE(surface) << reply;
    EXPECT_EQ(surface->get("points").value_or(""), "100");
    EXPECT_EQ(surface->get("bounds").value_or(""), "10.500,10.500,19.500,19.500");
    EXPECT_EQ(surface->get("zmin").value_or(""), "100.525");
    EXPECT_EQ(surface->get("zmax").value_or(""), "100.975");
    ASSERT_TRUE(record(reply, "window"));
}

TEST_F(SurfaceVerbs, MaxSpendsTheCapOnAStrideOverTheWholeExtent)
{
    // max=300 on 40 x 30: the smallest stride s with ceil(40/s) ceil(30/s)
    // <= 300 is 2 (20 x 15 = 300), and the samples still span the extent.
    const std::string reply = ran("SURFACE FROM FILE \"" + kPlane + "\" max=300");
    const auto sampled = record(reply, "sampled");
    ASSERT_TRUE(sampled) << reply;
    EXPECT_EQ(sampled->get("stride").value_or(""), "2");
    EXPECT_EQ(sampled->get("points").value_or(""), "300");
}

TEST_F(SurfaceVerbs, SurfaceFromDrawingOfFourLevelledPointsHasTwoTriangles)
{
    draw({levelled(0, 0, 100.0), levelled(10, 0, 101.0), levelled(10, 10, 102.0),
          levelled(0, 10, 101.0)});
    const std::string reply = ran("SURFACE FROM DRAWING");
    const auto surface = record(reply, "surface");
    ASSERT_TRUE(surface) << reply;
    EXPECT_EQ(surface->get("triangles").value_or(""), "2");
    EXPECT_EQ(surface->get("points").value_or(""), "4");
    EXPECT_EQ(surface->get("name").value_or(""), "drawing");
    EXPECT_EQ(surface->get("zmin").value_or(""), "100.000");
    EXPECT_EQ(surface->get("zmax").value_or(""), "102.000");
    const auto scope = record(reply, "scope");
    ASSERT_TRUE(scope) << reply;
    EXPECT_EQ(scope->get("scope").value_or(""), "drawing");
    EXPECT_EQ(scope->get("matched").value_or(""), "4");
    EXPECT_EQ(scope->get("used").value_or(""), "4");
}

TEST_F(SurfaceVerbs, AHeightlessPointIsLeftOutAndCounted)
{
    draw({levelled(0, 0, 100.0), levelled(10, 0, 101.0), levelled(10, 10, 102.0),
          levelled(0, 10, 101.0), levelled(5, 5, std::nullopt)});
    const std::string reply = ran("SURFACE FROM DRAWING");
    const auto scope = record(reply, "scope");
    ASSERT_TRUE(scope) << reply;
    EXPECT_EQ(scope->get("matched").value_or(""), "5");
    EXPECT_EQ(scope->get("used").value_or(""), "4");
    EXPECT_EQ(scope->get("vertices.heightless").value_or(""), "1");
    EXPECT_EQ(scope->get("skipped.heightless").value_or(""), "1");
    // Still two triangles: the heightless point is not a fifth vertex at 0.
    EXPECT_EQ(record(reply, "surface")->get("triangles").value_or(""), "2");
    EXPECT_EQ(record(reply, "surface")->get("zmin").value_or(""), "100.000");
    ASSERT_TRUE(record(reply, "warning"));
}

TEST_F(SurfaceVerbs, TheScopeNarrowsTheDrawingAndAScopeWithTooFewPointsIsRefused)
{
    draw({levelled(0, 0, 100.0), levelled(10, 0, 101.0), levelled(10, 10, 102.0),
          levelled(0, 10, 101.0)});
    // AREA 0,0,10,5 takes the two points along y = 0: too few, and said.
    auto refused = run("SURFACE FROM DRAWING AREA -1,-1,11,5");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("matched=2"), std::string::npos)
        << refused.error().message;
    EXPECT_TRUE(surfaces.empty());
    // DRAWING WHERE TYPE=point: all four.
    EXPECT_TRUE(record(ran("SURFACE FROM DRAWING WHERE TYPE=point NAME points"), "surface"));
}

TEST_F(SurfaceVerbs, ADuplicateNameIsRefused)
{
    (void)ran("SURFACE FROM FILE \"" + kPlane + "\" NAME ground");
    auto again = run("SURFACE FROM FILE \"" + kPlane + "\" NAME Ground");
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, ErrorCode::AlreadyExists);
    // Without NAME the source's name is made unique instead.
    (void)ran("SURFACE FROM FILE \"" + kPlane + "\"");
    (void)ran("SURFACE FROM FILE \"" + kPlane + "\"");
    EXPECT_NE(surfaces.find("plane"), nullptr);
    EXPECT_NE(surfaces.find("plane (2)"), nullptr);
    EXPECT_EQ(surfaces.all().size(), 3u);
}

TEST_F(SurfaceVerbs, AReferenceRasterIsReadAtFullPrecisionFromItsFile)
{
    // The raster as IMPORT keeps it: an 8-bit display copy beside its source
    // path. RASTER <id> reads the path, so the range is the file's, not 256
    // grey steps.
    auto overlay = katana::interop::importRaster(kPlane, {});
    ASSERT_TRUE(overlay.ok()) << overlay.error().describe();
    const katana::interop::ReferenceId id = reference.add(std::move(*overlay));
    const std::string reply = ran("SURFACE FROM RASTER " + std::to_string(id));
    EXPECT_EQ(record(reply, "surface")->get("zmin").value_or(""), "100.025");
    EXPECT_EQ(record(reply, "input")->get("source").value_or(""), "raster");
    const auto missing = run("SURFACE FROM RASTER 99");
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
}

TEST_F(SurfaceVerbs, ACloudsGroundIsTriangulatedAndClassesChooseOthers)
{
    // Four ground returns on a 10 m square and one roof return (ASPRS 6) in
    // the middle, 8 m up: the ground surface leaves the roof out and says so.
    katana::interop::PointCloudLayer cloud;
    cloud.name = "scan";
    const auto add = [&](double x, double y, double z, std::uint8_t cls) {
        katana::pointcloud::PointCloudPoint point;
        point.x = x;
        point.y = y;
        point.z = z;
        point.classification = cls;
        cloud.points.push_back(point);
    };
    add(0, 0, 10, 2);
    add(10, 0, 11, 2);
    add(10, 10, 12, 2);
    add(0, 10, 11, 2);
    add(5, 5, 18, 6);
    add(4, 4, 18, 6);
    add(6, 4, 18, 6);
    const katana::interop::ReferenceId id = reference.add(std::move(cloud));
    const std::string ground = ran("SURFACE FROM CLOUD " + std::to_string(id));
    const auto input = record(ground, "input");
    ASSERT_TRUE(input) << ground;
    EXPECT_EQ(input->get("ground").value_or(""), "yes");
    EXPECT_EQ(input->get("points").value_or(""), "4");
    EXPECT_EQ(input->get("excluded").value_or(""), "3");
    EXPECT_EQ(record(ground, "surface")->get("zmax").value_or(""), "12.000");

    const std::string roofs = ran("SURFACE FROM CLOUD scan classes=6 NAME roofs");
    EXPECT_EQ(record(roofs, "input")->get("points").value_or(""), "3");
    EXPECT_EQ(record(roofs, "surface")->get("zmin").value_or(""), "18.000");
}

TEST_F(SurfaceVerbs, ListInfoAndRemoveSayWhatTheSessionHolds)
{
    EXPECT_NE(ran("SURFACE LIST").find("listed surfaces=0 rasters=0"), std::string::npos);
    (void)ran("SURFACE FROM FILE \"" + kPlane + "\" NAME ground");
    const std::string listed = ran("SURFACE LIST");
    EXPECT_NE(listed.find("surface name=ground triangles=2262 points=1200"), std::string::npos)
        << listed;
    EXPECT_NE(listed.find("listed surfaces=1 rasters=0"), std::string::npos) << listed;

    const auto json = nlohmann::json::parse(ran("SURFACE LIST JSON"));
    ASSERT_EQ(json["surfaces"].size(), 1u);
    EXPECT_EQ(json["surfaces"][0]["name"], "ground");
    EXPECT_EQ(json["surfaces"][0]["triangles"], 2262);
    EXPECT_NEAR(json["surfaces"][0]["zmin"].get<double>(), 100.025, kFloat32UlpNear100 / 2);
    EXPECT_TRUE(json["rasters"].empty());

    // The plan area of the TIN is its hull, the rectangle of the extreme
    // centres: 39 x 29 = 1131 m2.
    const std::string info = ran("SURFACE INFO ground");
    EXPECT_NE(info.find("area plan=1131.000"), std::string::npos) << info;

    EXPECT_EQ(run("SURFACE INFO nothing").error().code, ErrorCode::NotFound);
    EXPECT_NE(ran("SURFACE REMOVE GROUND").find("removed surface=ground surfaces=0"),
              std::string::npos);
    EXPECT_TRUE(surfaces.empty());
    EXPECT_EQ(run("SURFACE REMOVE ground").error().code, ErrorCode::NotFound);
}

TEST_F(SurfaceVerbs, PreviewSaysWhatWouldBeReadAndMakesNothing)
{
    const std::string reply = ran("SURFACE FROM FILE \"" + kPlane + "\" PREVIEW");
    EXPECT_NE(reply.find("preview valid=yes changed=no"), std::string::npos);
    EXPECT_TRUE(surfaces.empty());
}

TEST_F(SurfaceVerbs, WhatTheVerbCannotReadIsRefusedNamingIt)
{
    EXPECT_EQ(run("SURFACE").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(run("SURFACE BUILD").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(run("SURFACE FROM").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(run("SURFACE FROM FILE \"" + kPlane + "\" max=2").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("SURFACE FROM FILE \"" + kPlane + "\" classes=2").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("SURFACE FROM FILE \"" + kPlane + "\" AREA 1,1,1,5").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("SURFACE FROM CLOUD 7").error().code, ErrorCode::NotFound);
    EXPECT_EQ(run("SURFACE FROM FILE \"" + scratch.file("none.tif") + "\"").error().code,
              ErrorCode::NotFound);
    EXPECT_TRUE(surfaces.empty());
}

// ---- EXPORT -------------------------------------------------------------------------------------

nlohmann::json rasterInfo(const std::string& path);

// The data type of band 1, as GDAL's own raster info reports it.
std::string bandType(const std::string& path)
{
    const nlohmann::json json = rasterInfo(path);
    return json.is_discarded() || json.is_null() ? std::string()
                                                 : json["bands"][0]["type"].get<std::string>();
}

TEST_F(SurfaceVerbs, SurfaceExportFloat32ReadsBackTheSurfaceWithinFloat32Precision)
{
    (void)ran("SURFACE FROM FILE \"" + kPlane + "\" NAME ground");
    const std::string dem = scratch.file("ground.tif");
    const std::string reply = ran("SURFACE EXPORT ground \"" + dem + "\" cell=1");
    const auto exported = record(reply, "exported");
    ASSERT_TRUE(exported) << reply;
    EXPECT_EQ(exported->get("driver").value_or(""), "GTiff");
    EXPECT_EQ(exported->get("type").value_or(""), "Float32");
    // The surface spans 0.5 ... 39.5 by 0.5 ... 29.5: 39 x 29 cells of 1 m.
    EXPECT_EQ(exported->get("raster").value_or(""), "39x29");
    EXPECT_EQ(bandType(dem), "Float32");

    // Each cell is the TIN at its centre (x0 + 0.5 + c, y1 - 0.5 - r), which
    // on the plane is 100 + 0.05 x. Error: each vertex is within half a
    // Float32 step of the plane (the file's Float32 values); the TIN's
    // barycentric weights are non-negative and sum to one, so the
    // interpolated value is too; storing it as Float32 rounds once more by at
    // most half a step. Together at most one step, 7.63e-6, plus the
    // double arithmetic's 1e-12.
    auto dataset = katana::gis::GdalDataset::open(dem);
    ASSERT_TRUE(dataset.ok()) << dataset.error().describe();
    const auto samples = (*dataset)->readBandSampled(1, 1);
    dataset->reset(); // closed, so the file can be replaced below
    ASSERT_TRUE(samples.ok());
    ASSERT_EQ(samples->columns, 39);
    ASSERT_EQ(samples->rows, 29);
    double worst = 0.0;
    for (int row = 0; row < samples->rows; ++row) {
        for (int column = 0; column < samples->columns; ++column) {
            const double x = 0.5 + 0.5 + column;
            const double value = samples->values[static_cast<std::size_t>(row * 39 + column)];
            worst = std::max(worst, std::abs(value - (100.0 + 0.05 * x)));
        }
    }
    EXPECT_LE(worst, kFloat32UlpNear100 + 1e-12);

    // Float64 on request; and the file in the way needs OVERWRITE.
    auto refused = run("SURFACE EXPORT ground \"" + dem + "\" cell=1 type=Float64");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::AlreadyExists);
    (void)ran("SURFACE EXPORT ground \"" + dem + "\" cell=1 type=Float64 OVERWRITE");
    EXPECT_EQ(bandType(dem), "Float64");
}

// GDAL's own raster info of `path`, as JSON.
nlohmann::json rasterInfo(const std::string& path)
{
    gp::RunRequest info;
    info.path = {"raster", "info"};
    info.values.emplace_back("input", gp::DatasetValue(gp::DatasetPath{path, {}, {}}));
    info.tokens = {"--format=json"};
    auto ran = gp::run(info);
    if (!ran || !ran->text) {
        return {};
    }
    return nlohmann::json::parse(*ran->text, nullptr, false);
}

TEST_F(SurfaceVerbs, ACogExportIsLaidOutAsACloudOptimisedGeoTiff)
{
    // At 0.05 m the 39 x 29 m surface is 780 x 580 cells, more than one of
    // the COG driver's 512-cell tiles, so a COG has an overview and 512 x 512
    // tiles; the plain GeoTIFF the export writes otherwise has 256 x 256
    // tiles and no overview - which is what tells the two apart here.
    (void)ran("SURFACE FROM FILE \"" + kPlane + "\" NAME ground");
    const std::string cog = scratch.file("ground_cog.tif");
    const std::string reply = ran("SURFACE EXPORT ground \"" + cog + "\" cell=0.05 COG");
    EXPECT_EQ(record(reply, "exported")->get("driver").value_or(""), "COG");
    EXPECT_EQ(record(reply, "exported")->get("raster").value_or(""), "780x580");
    const nlohmann::json info = rasterInfo(cog);
    ASSERT_FALSE(info.is_discarded());
    EXPECT_EQ(info["metadata"]["IMAGE_STRUCTURE"]["LAYOUT"], "COG") << info.dump();
    EXPECT_EQ(info["metadata"]["IMAGE_STRUCTURE"]["COMPRESSION"], "DEFLATE");
    EXPECT_EQ(info["bands"][0]["block"], nlohmann::json({512, 512}));
    EXPECT_FALSE(info["bands"][0]["overviews"].empty()) << info["bands"][0].dump();

    const std::string plain = scratch.file("ground_plain.tif");
    (void)ran("SURFACE EXPORT ground \"" + plain + "\" cell=0.05");
    const nlohmann::json tiff = rasterInfo(plain);
    EXPECT_EQ(tiff["bands"][0]["block"], nlohmann::json({256, 256}));
    EXPECT_TRUE(!tiff["bands"][0].contains("overviews") || tiff["bands"][0]["overviews"].empty());
}

TEST_F(SurfaceVerbs, ACogExportPassesGdalsCogValidation)
{
    (void)ran("SURFACE FROM FILE \"" + kPlane + "\" NAME ground");
    const std::string cog = scratch.file("ground_cog.tif");
    (void)ran("SURFACE EXPORT ground \"" + cog + "\" cell=0.05 COG");
    // GDAL's own validator, through the GDAL verb: return code 0 is a valid
    // Cloud Optimised GeoTIFF. It is a script of GDAL's Python utilities,
    // which a GDAL built without them cannot run.
    auto validated = run("GDAL driver cog validate \"" + cog + "\"");
    if (!validated.ok() && validated.error().message.find("osgeo_utils") != std::string::npos) {
        GTEST_SKIP() << "this GDAL has no Python utilities to validate a COG with: "
                     << validated.error().message;
    }
    ASSERT_TRUE(validated.ok()) << validated.error().describe();
    const auto code = record(*validated, "return");
    ASSERT_TRUE(code) << *validated;
    EXPECT_EQ(code->get("code").value_or(""), "0") << *validated;
}

TEST_F(SurfaceVerbs, ExportTakesCreationOptionsAndRefusesWhatItCannotWrite)
{
    (void)ran("SURFACE FROM FILE \"" + kPlane + "\" NAME ground");
    (void)ran("SURFACE EXPORT ground \"" + scratch.file("lzw.tif") + "\" co=COMPRESS=LZW");
    EXPECT_TRUE(std::filesystem::exists(scratch.path() / "lzw.tif"));
    EXPECT_EQ(run("SURFACE EXPORT ground \"" + scratch.file("x.xyzzy") + "\"").error().code,
              ErrorCode::Unsupported);
    EXPECT_EQ(run("SURFACE EXPORT ground \"" + scratch.file("x.tif") + "\" cell=0").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("SURFACE EXPORT ground \"" + scratch.file("x.tif") + "\" type=Int16").error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(run("SURFACE EXPORT nothing \"" + scratch.file("x.tif") + "\"").error().code,
              ErrorCode::NotFound);
    EXPECT_FALSE(std::filesystem::exists(scratch.path() / "x.tif"));
    EXPECT_FALSE(std::filesystem::exists(scratch.path() / "x.xyzzy"));
}

// ---- TO SURFACE ----------------------------------------------------------------------------

TEST_F(SurfaceVerbs, ARasterResultToASurfaceIsTriangulatedAndKept)
{
    // GDAL's clip of the plane to 0,0,10,10: 10 x 10 whole cells, their
    // centres 0.5 ... 9.5, z 100.025 ... 100.475.
    const std::string reply = ran("GDAL raster clip --bbox=0,0,10,10 FROM FILE \"" + kPlane +
                                  "\" TO SURFACE corner");
    const auto output = record(reply, "output");
    ASSERT_TRUE(output) << reply;
    EXPECT_EQ(output->get("target").value_or(""), "surface");
    const auto surface = record(reply, "surface");
    ASSERT_TRUE(surface) << reply;
    EXPECT_EQ(surface->get("points").value_or(""), "100");
    EXPECT_EQ(surface->get("zmax").value_or(""), "100.475");
    ASSERT_NE(surfaces.find("corner"), nullptr);
    // The spilled raster is gone once read.
    std::size_t left = 0;
    for (const auto& entry : std::filesystem::directory_iterator(scratch.path())) {
        left += entry.path().extension() == ".tif" ? 1u : 0u;
    }
    EXPECT_EQ(left, 0u);
    // A name in use is refused.
    EXPECT_EQ(run("GDAL raster clip --bbox=0,0,10,10 FROM FILE \"" + kPlane + "\" TO SURFACE corner")
                  .error()
                  .code,
              ErrorCode::AlreadyExists);
}

// ---- through a Session, as katana_cli and katana_mcp run it -----------------------------------

TEST(SurfaceSession, TheSessionKeepsItsSurfacesBetweenLines)
{
    katana::app::Session session(nullptr);
    testing::internal::CaptureStdout();
    const bool made = session.run("SURFACE FROM FILE \"" + kPlane + "\" NAME ground");
    const bool listed = session.run("SURFACE LIST");
    const std::string out = testing::internal::GetCapturedStdout();
    EXPECT_TRUE(made);
    EXPECT_TRUE(listed);
    EXPECT_NE(out.find("surface name=ground triangles=2262"), std::string::npos) << out;
    EXPECT_NE(out.find("listed surfaces=1"), std::string::npos) << out;
}

} // namespace
