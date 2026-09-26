// IMPORT's options (src/katana_app/geo/import_verb.cpp; docs/interop.md,
// "Import options"): what of a file is read - layers, a WHERE filter, a
// statement, the part in a scope, cut at its edge or not - and how - fields,
// the project's coordinate system, a raster's band - run through the one
// executor as katana_cli, katana_mcp and the window's dialogs run it.
//
// The fixtures' facts, by hand:
//   tests/geo/data/lots.geojson  EPSG:28356; three polygons, layer "lots":
//     A  kind=lot       (0, 0) - (50, 40)    area 2000
//     B  kind=lot       (50, 0) - (100, 40)  area 2000
//     C  kind=corridor  (-10, 18) - (110, 22) area 480
//   samples/gis/parcels.geojson  8 features (docs above test_gis_verbs.cpp);
//     two have surface=asphalt|gravel, one each.

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <numbers>
#include <stop_token>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "katana/core/text.hpp"

#include "vector_fixture.hpp"

namespace {

using katana::core::ErrorCode;
using katana::geo_test::VectorFixture;

const std::string kData = KATANA_GEO_TEST_DATA;
const std::string kSamples = KATANA_GIS_SAMPLES;

std::string quoted(const std::string& path)
{
    return "\"" + path + "\"";
}

const std::string kLots = quoted(kData + "/lots.geojson");

class ImportOptions : public VectorFixture {
  protected:
    // The areas of the closed polylines on `layer`, smallest first.
    std::vector<double> areas(const std::string& layer) const
    {
        std::vector<double> found;
        for (const katana::entity::Entity& entity : on(layer)) {
            if (const auto* ring = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
                found.push_back(std::abs(ring->area()));
            }
        }
        std::ranges::sort(found);
        return found;
    }

    // A file written for one test, in its scratch folder.
    std::string write(const std::string& name, const std::string& text) const
    {
        const std::filesystem::path path = scratch_ / name;
        std::ofstream out(path, std::ios::binary);
        out << text;
        return path.generic_string();
    }

    std::string value(const std::string& reply, const std::string& kind, const std::string& key)
    {
        const auto found = record(reply, kind);
        return found ? found->get(key).value_or(std::string("<no ") + key + ">")
                     : "<no " + kind + " record>";
    }
};

// ---- vector: which features ------------------------------------------------------------------

TEST_F(ImportOptions, WhereImportsOnlyMatchingFeatures)
{
    // kind = 'lot' is A and B: two of the three.
    const std::string reply = ok("IMPORT " + kLots + " where=\"kind = 'lot'\"");
    EXPECT_EQ(areas("lots"), (std::vector<double>{2000.0, 2000.0}));
    EXPECT_EQ(value(reply, "matched", "features"), "2");
    EXPECT_EQ(value(reply, "matched", "of"), "3");
}

TEST_F(ImportOptions, AWhereThatMatchesNothingImportsNothingAndSaysSo)
{
    // No feature has kind = 'road': an answer, not a failure to read.
    const std::string reply = ok("IMPORT " + kLots + " where=\"kind = 'road'\"");
    EXPECT_EQ(entityCount(), 0u);
    EXPECT_EQ(value(reply, "imported", "entities"), "0");
    EXPECT_NE(reply.find("the filters took none of the 3 features"), std::string::npos) << reply;
}

TEST_F(ImportOptions, AFilterGdalCannotReadIsRefusedWithItsReason)
{
    const auto reply = run("IMPORT " + kLots + " where=\"kind = = 'lot'\"");
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(entityCount(), 0u);
}

TEST_F(ImportOptions, LayersNamesTheLayersReadAndAnUnknownOneIsRefusedNamingThoseThereAre)
{
    ok("IMPORT " + kLots + " layers=LOTS");
    EXPECT_EQ(on("lots").size(), 3u);
    const auto refused = run("IMPORT " + kLots + " layers=parcels");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::NotFound);
    EXPECT_NE(refused.error().message.find("it has lots"), std::string::npos)
        << refused.error().message;
}

TEST_F(ImportOptions, SqlSelectImportsItsRows)
{
    // C alone: 120 x 4 = 480, on the layer named after the file.
    const std::string reply =
        ok("IMPORT " + kLots + " sql=\"SELECT * FROM lots WHERE name = 'C'\" dialect=ogrsql");
    EXPECT_EQ(areas("lots"), (std::vector<double>{480.0}));
    EXPECT_EQ(value(reply, "matched", "features"), "1");
}

TEST_F(ImportOptions, SqlThatWouldChangeTheFileIsRefused)
{
    const auto refused = run("IMPORT " + kLots + " sql=\"DELETE FROM lots\"");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("only a SELECT"), std::string::npos);
}

TEST_F(ImportOptions, SqlWithLayersIsRefused)
{
    const auto refused = run("IMPORT " + kLots + " layers=lots sql=\"SELECT * FROM lots\"");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
}

TEST_F(ImportOptions, FieldsKeepsOnlyThoseProperties)
{
    ok("IMPORT " + kLots + " fields=NAME");
    const auto lots = on("lots");
    ASSERT_EQ(lots.size(), 3u);
    for (const katana::entity::Entity& entity : lots) {
        EXPECT_TRUE(entity.properties.contains("name"));
        EXPECT_FALSE(entity.properties.contains("kind"));
    }
    const auto refused = run("IMPORT " + kLots + " fields=owner");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("'owner'"), std::string::npos);
}

TEST_F(ImportOptions, AttributesNoKeepsNoProperties)
{
    ok("IMPORT " + kLots + " attributes=no");
    for (const katana::entity::Entity& entity : on("lots")) {
        EXPECT_FALSE(entity.properties.contains("name"));
        EXPECT_FALSE(entity.properties.contains("kind"));
    }
}

TEST_F(ImportOptions, TargetAndMaxPutAtMostThatManyOnOneLayer)
{
    ok("IMPORT " + kLots + " target=parcels max=2");
    EXPECT_EQ(on("parcels").size(), 2u);
    EXPECT_TRUE(on("lots").empty());
}

// ---- vector: where in the drawing ------------------------------------------------------------

TEST_F(ImportOptions, AnAreaFilterImportsOnlyFeaturesTouchingTheBox)
{
    // The box (60, 5) - (90, 15) lies inside B only: A ends at x = 50, and
    // C runs between y = 18 and 22.
    const std::string reply = ok("IMPORT " + kLots + " AREA 60,5,90,15");
    EXPECT_EQ(areas("lots"), (std::vector<double>{2000.0}));
    EXPECT_EQ(value(reply, "matched", "features"), "1");
    EXPECT_EQ(value(reply, "scope", "scope"), "area");
    EXPECT_EQ(value(reply, "scope", "clip"), "no");
}

TEST_F(ImportOptions, ClipCutsAtTheBoundary)
{
    // (40, 10) - (60, 30) meets all three; cut at it, A keeps (40..50) x
    // (10..30) = 200, B (50..60) x (10..30) = 200, C (40..60) x (18..22) = 80.
    ok("IMPORT " + kLots + " AREA 40,10,60,30 clip");
    const std::vector<double> cut = areas("lots");
    ASSERT_EQ(cut.size(), 3u);
    EXPECT_NEAR(cut[0], 80.0, 1e-9);
    EXPECT_NEAR(cut[1], 200.0, 1e-9);
    EXPECT_NEAR(cut[2], 200.0, 1e-9);
}

TEST_F(ImportOptions, ClipByTheClosedShapesALayersScopeTakes)
{
    // The boundary (0, 0) - (25, 40): its box leaves B out (B starts at
    // x = 50); A is cut to 25 x 40 = 1000 and C to (0..25) x (18..22) = 100.
    rect(0.0, 0.0, 25.0, 40.0, "boundary");
    ok("IMPORT " + kLots + " LAYERS boundary clip");
    const std::vector<double> cut = areas("lots");
    ASSERT_EQ(cut.size(), 2u);
    EXPECT_NEAR(cut[0], 100.0, 1e-9);
    EXPECT_NEAR(cut[1], 1000.0, 1e-9);
}

TEST_F(ImportOptions, AScopeThatTakesNothingIsReportedAndReadsNothing)
{
    // An empty drawing: DRAWING takes nothing, so there is no box to read.
    const std::string reply = ok("IMPORT " + kLots + " DRAWING");
    EXPECT_EQ(entityCount(), 0u);
    EXPECT_EQ(value(reply, "import", "ran"), "no");
    EXPECT_EQ(value(reply, "scope", "matched"), "0");
}

TEST_F(ImportOptions, ClipWithoutClosedShapesIsRefused)
{
    line(0.0, 0.0, 30.0, 30.0, "guide");
    const auto refused = run("IMPORT " + kLots + " LAYERS guide clip");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("none of them closed"), std::string::npos);
}

TEST_F(ImportOptions, AScopeWithAPlacementIsRefused)
{
    const auto refused = run("IMPORT " + kLots + " LOCAL AREA 0,0,10,10");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
}

TEST_F(ImportOptions, ViewHeadlessIsRefusedNamingArea)
{
    const auto refused = run("IMPORT " + kLots + " VIEW");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("AREA"), std::string::npos);
}

// ---- vector: open options ---------------------------------------------------------------------

TEST_F(ImportOptions, AnUnknownOpenOptionIsRefusedByName)
{
    const auto refused = run("IMPORT " + kLots + " oo=NO_SUCH_OPTION=YES");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("NO_SUCH_OPTION"), std::string::npos)
        << refused.error().message;
    EXPECT_EQ(entityCount(), 0u);
    // One GeoJSON declares (GDAL's GeoJSON driver page) reads as asked.
    ok("IMPORT " + kLots + " oo=FLATTEN_NESTED_ATTRIBUTES=YES");
    EXPECT_EQ(on("lots").size(), 3u);
}

// ---- coordinate systems -----------------------------------------------------------------------

TEST_F(ImportOptions, CrsProjectReprojectsAKnownPoint)
{
    // A point in Web Mercator (EPSG:3857) moved into a project in WGS 84
    // longitude and latitude. Expected by the spherical Mercator inverse
    // (EPSG guidance note 7-2, method 1024), independent of PROJ:
    //   lon = x / R,  lat = 2 atan(exp(y / R)) - pi / 2,  R = 6378137 m.
    const double x = 1'113'194.9079327357;
    const double y = 1'118'889.9748579594;
    const std::string file = write(
        "mercator.geojson",
        R"({"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::3857"}},)"
        R"("features":[{"type":"Feature","properties":{"name":"P"},"geometry":{"type":"Point","coordinates":[)" +
            katana::core::formatExactReal(x) + "," + katana::core::formatExactReal(y) + "]}}]}");
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:4326").ok());
    ok("IMPORT " + quoted(file) + " crs=project");
    const auto points = on("mercator");
    ASSERT_EQ(points.size(), 1u);
    const auto* point = std::get_if<katana::entity::PointGeometry>(&points.front().geometry);
    ASSERT_NE(point, nullptr);
    constexpr double kRadius = 6'378'137.0;
    const double lon = x / kRadius * 180.0 / std::numbers::pi;
    const double lat = (2.0 * std::atan(std::exp(y / kRadius)) - std::numbers::pi / 2.0) * 180.0 /
                       std::numbers::pi;
    // 1e-9 degrees is about 0.1 mm on the ground: far below any survey's
    // concern, far above double rounding through PROJ's pipeline.
    EXPECT_NEAR(point->position.x, lon, 1e-9);
    EXPECT_NEAR(point->position.y, lat, 1e-9);
}

TEST_F(ImportOptions, CrsProjectWithoutAProjectCrsIsRefused)
{
    const auto refused = run("IMPORT " + kLots + " crs=project");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidCRS);
    EXPECT_NE(refused.error().message.find("CRS SET"), std::string::npos);
}

TEST_F(ImportOptions, SrsAssumesACrsForAFileWithout)
{
    // A CSV of WKT declares no coordinate system (GDAL's CSV driver).
    const std::string file = write("pegs.csv", "WKT,name\n\"POINT (330000 6250000)\",P1\n");
    const std::string reply = ok("IMPORT " + quoted(file) + " srs=EPSG:28356");
    EXPECT_NE(value(reply, "imported", "crs").find("EPSG:28356"), std::string::npos) << reply;
}

TEST_F(ImportOptions, SrsSaysWhatAGeoJsonWithoutACrsMemberIsIn)
{
    // GDAL declares a GeoJSON without a crs member WGS 84 (RFC 7946), so
    // srs= - read only for a file that declares nothing - never applied, and
    // crs=project moved MGA metres as degrees. The point is in the
    // project's own system, so it stays exactly where it is.
    const std::string file =
        write("pegs.geojson",
              R"({"type":"FeatureCollection","features":[{"type":"Feature","properties":{},)"
              R"("geometry":{"type":"Point","coordinates":[330000,6250000]}}]})");
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    const std::string reply = ok("IMPORT " + quoted(file) + " srs=EPSG:28356 crs=project");
    const auto points = on("pegs");
    ASSERT_EQ(points.size(), 1u) << reply;
    const auto* point = std::get_if<katana::entity::PointGeometry>(&points.front().geometry);
    ASSERT_NE(point, nullptr);
    // No move at all: an identity, to a micrometre.
    EXPECT_NEAR(point->position.x, 330000.0, 1e-6);
    EXPECT_NEAR(point->position.y, 6250000.0, 1e-6);
    // Without crs=project, srs= is what the reply says the file is in.
    const std::string kept = ok("IMPORT " + quoted(file) + " srs=EPSG:28356");
    EXPECT_NE(value(kept, "imported", "crs").find("EPSG:28356"), std::string::npos) << kept;
    // A file that declares another system is read as srs= says, and said.
    const std::string said = ok("IMPORT " + kLots + " srs=EPSG:7856");
    EXPECT_NE(said.find("declares GDA94 / MGA zone 56 (EPSG:28356); srs= says it is in "
                        "GDA2020 / MGA zone 56 (EPSG:7856)"),
              std::string::npos)
        << said;
}

TEST_F(ImportOptions, AFileInAnotherCrsThanTheProjectsIsSaidToBe)
{
    // A peg in EPSG:28356 drawn unmoved into an EPSG:7856 project lies
    // about 1.5 m from where the project's system puts it (GDA94 to
    // GDA2020), and nothing said so.
    const std::string file = write(
        "site.geojson",
        R"({"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::28356"}},)"
        R"("features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[330000,6250000]}}]})");
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:7856").ok());
    const std::string unmoved = ok("IMPORT " + quoted(file));
    EXPECT_NE(unmoved.find("warning text=\"the file is in GDA94 / MGA zone 56 (EPSG:28356), not "
                           "the project's GDA2020 / MGA zone 56 (EPSG:7856)"),
              std::string::npos)
        << unmoved;
    // Moved into it, or in the project's own, nothing is said.
    const std::string moved = ok("IMPORT " + quoted(file) + " crs=project target=moved");
    EXPECT_EQ(moved.find("not the project's"), std::string::npos) << moved;
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    const std::string same = ok("IMPORT " + quoted(file) + " target=same");
    EXPECT_EQ(same.find("not the project's"), std::string::npos) << same;
}

TEST_F(ImportOptions, CrsProjectRefusesWhenTheProjectsCrsChangedWhileTheImportRan)
{
    const std::string file = write(
        "site.geojson",
        R"({"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::28356"}},)"
        R"("features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[330000,6250000]}}]})");
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:7856").ok());
    auto prepared = katana::app::geo::prepare(context, "IMPORT " + quoted(file) + " crs=project");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    ASSERT_TRUE(prepared->work);
    auto apply = prepared->work(std::stop_token{}, {});
    ASSERT_TRUE(apply.ok()) << apply.error().describe();
    // CRS SET while the worker read: the data is in the old system.
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:4326").ok());
    auto applied = (*apply)(context);
    ASSERT_FALSE(applied.ok());
    EXPECT_EQ(applied.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(entityCount(), 0u);
}

TEST_F(ImportOptions, AdoptSetsTheProjectCrsOnlyWhenItHasNone)
{
    // No project CRS: the file's (EPSG:28356) is adopted with the entities,
    // ONE undo step - undone, both go.
    const std::string reply = ok("IMPORT " + kLots + " crs=adopt");
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:28356");
    EXPECT_EQ(value(reply, "crs", "adopted"), "yes");
    EXPECT_EQ(on("lots").size(), 3u);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.metadata().coordinateSystem, "");
    EXPECT_EQ(entityCount(), 0u);

    // A project that has one keeps it.
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:7856").ok());
    const std::string kept = ok("IMPORT " + kLots + " crs=adopt");
    EXPECT_EQ(document.metadata().coordinateSystem, "EPSG:7856");
    EXPECT_EQ(value(kept, "crs", "adopted"), "no");
}

// ---- preview ----------------------------------------------------------------------------------

TEST_F(ImportOptions, PreviewImportsNothingAndCountsTheMatch)
{
    const std::string reply = ok("IMPORT " + kLots + " where=\"kind = 'lot'\" PREVIEW");
    EXPECT_EQ(entityCount(), 0u);
    EXPECT_EQ(value(reply, "import", "preview"), "yes");
    EXPECT_EQ(value(reply, "import", "features"), "2");
    EXPECT_EQ(value(reply, "import", "of"), "3");
    EXPECT_EQ(value(reply, "import", "entities"), "2");
    EXPECT_FALSE(document.history().canUndo());
}

// ---- rasters and point clouds -----------------------------------------------------------------

class RasterBands : public ImportOptions {
  protected:
    // A 2 x 1 raster of three byte bands, band-sequential (GDAL's EHdr):
    // band 1 = [0, 100], band 2 = [100, 0], band 3 = [50, 50].
    std::string threeBands()
    {
        write("bands.hdr", "NROWS 1\nNCOLS 2\nNBANDS 3\nNBITS 8\nBYTEORDER I\nLAYOUT BSQ\n"
                           "ULXMAP 0.5\nULYMAP 0.5\nXDIM 1\nYDIM 1\n");
        return write("bands.bil", std::string("\x00\x64\x64\x00\x32\x32", 6));
    }
};

TEST_F(RasterBands, BandTwoOfAThreeBandRasterIsRead)
{
    const std::string file = threeBands();
    ok("IMPORT " + quoted(file) + " band=2");
    ASSERT_EQ(reference.rasters().size(), 1u);
    const auto& grey = reference.rasters().front().rgba;
    ASSERT_EQ(grey.size(), 8u);
    // Band 2 alone, stretched over its own range 0..100: the first pixel,
    // 100, is white; the second, 0, black.
    EXPECT_EQ(grey[0], 255);
    EXPECT_EQ(grey[1], 255);
    EXPECT_EQ(grey[4], 0);

    // Without band=, the three bands are red, green and blue as they are.
    ok("IMPORT " + quoted(file) + " name=colour");
    ASSERT_EQ(reference.rasters().size(), 2u);
    const auto& colour = reference.rasters().back().rgba;
    EXPECT_EQ(colour[0], 0);
    EXPECT_EQ(colour[1], 100);
    EXPECT_EQ(colour[2], 50);
    EXPECT_EQ(reference.rasters().back().name, "colour");
}

TEST_F(RasterBands, ABandTheFileDoesNotHaveIsRefused)
{
    const auto refused = run("IMPORT " + quoted(threeBands()) + " band=4");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("3 bands"), std::string::npos);
    EXPECT_TRUE(reference.rasters().empty());
}

TEST_F(ImportOptions, ASubdatasetOfAFileWithNoneIsRefused)
{
    const auto refused = run("IMPORT " + quoted(kSamples + "/terrain.asc") + " subdataset=1");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("no subdatasets"), std::string::npos);
}

TEST_F(ImportOptions, ABudgetWithAClassIsSizedToThePointsOfThatClass)
{
    // samples/gis/survey_scan.las: 40000 points, 29512 of class 2 (counted
    // from the file's point records, format 7, byte 16). The step was sized
    // to the whole file, 40000 / 1000 = 40, and applied after the class
    // filter: ceil(29512 / 40) = 738 of the 1000 asked. Sized to the class
    // as that first read measures it, 738 x 40 = 29520 points, the step is
    // ceil(29520 / 1000) = 30: ceil(29512 / 30) = 984, within the budget.
    const std::string reply =
        ok("IMPORT " + quoted(kSamples + "/survey_scan.las") + " budget=1000 class=2 name=ground");
    ASSERT_EQ(reference.pointClouds().size(), 1u) << reply;
    EXPECT_EQ(reference.pointClouds().front().points.size(), 984u) << reply;
    EXPECT_EQ(reference.pointClouds().front().decimationStep, 30u);
}

TEST_F(ImportOptions, AnOptionOfAnotherKindOfDataIsRefusedByName)
{
    const auto band = run("IMPORT " + kLots + " band=1");
    ASSERT_FALSE(band.ok());
    EXPECT_NE(band.error().message.find("band= is not an option of a vector import"),
              std::string::npos);
    const auto where = run("IMPORT " + quoted(kSamples + "/terrain.asc") + " where=\"a = 1\"");
    ASSERT_FALSE(where.ok());
    const auto classes = run("IMPORT " + quoted(kSamples + "/survey_scan.las") + " class=2,6");
    ASSERT_FALSE(classes.ok());
    EXPECT_EQ(classes.error().code, ErrorCode::Unsupported);
}

TEST_F(ImportOptions, APlainImportStillReadsAsItAlwaysDid)
{
    // No option: the whole file, as before the options came.
    const std::string reply = ok("IMPORT " + kLots);
    EXPECT_EQ(on("lots").size(), 3u);
    EXPECT_FALSE(record(reply, "matched").has_value());
}

} // namespace
