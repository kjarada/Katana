// The one geoprocessing executor and the GDAL verb (src/katana_app/geo/,
// docs/geoprocessing.md "The executor", "The GDAL verb"): driven through the
// executor's own context, as the window and the session drive it, and
// through a Session, as katana_cli and katana_mcp do.

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <sstream>
#include <stop_token>
#include <string>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "geo/bindings.hpp"
#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "session.hpp"

namespace {

namespace geo = katana::app::geo;
using katana::core::ErrorCode;

const std::string kData = KATANA_GEO_TEST_DATA;

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-geo-verbs-" + name))
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

std::size_t count(const std::string& text, const std::string& word)
{
    std::size_t found = 0;
    for (std::size_t at = text.find(word); at != std::string::npos; at = text.find(word, at + 1)) {
        ++found;
    }
    return found;
}

class GeoExecutor : public ::testing::Test {
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

    void type(const std::string& line) { ASSERT_TRUE(interpreter.run(line).ok()) << line; }

    // The one polyline on `layer`, and its area.
    double areaOn(const std::string& layer)
    {
        double total = 0.0;
        document.model().entities.forEach([&](const katana::entity::Entity& entity) {
            if (entity.layer == layer) {
                if (const auto* line = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
                    total += line->area();
                }
            }
        });
        return total;
    }
};

TEST_F(GeoExecutor, TheExecutorTakesTheGdalVerbAndNothingElse)
{
    EXPECT_TRUE(geo::handles("GDAL VERSION"));
    EXPECT_TRUE(geo::handles("  gdal list raster"));
    EXPECT_FALSE(geo::handles("GDALX VERSION"));
    EXPECT_FALSE(geo::handles("\"GDAL\" VERSION"));
    EXPECT_FALSE(geo::handles("LINE 0,0 1,1"));
    EXPECT_NE(geo::helpText().find("GDAL VERSION"), std::string::npos);
}

TEST_F(GeoExecutor, GdalVersionNamesTheLibrary)
{
    auto reply = run("GDAL VERSION");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    ASSERT_EQ(records.size(), 1u);
    EXPECT_EQ(records.front().kind, "gdal");
    EXPECT_TRUE(records.front().get("version").value_or("").starts_with("3."));
    EXPECT_TRUE(records.front().get("algorithms").has_value());
}

TEST_F(GeoExecutor, GdalListFiltersByGroup)
{
    auto reply = run("GDAL LIST vector grid");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    std::size_t algorithms = 0;
    for (const geo::Record& record : records) {
        if (record.kind == "algorithm") {
            ++algorithms;
            EXPECT_TRUE(record.get("path").value_or("").starts_with("vector grid "));
        }
    }
    // GDAL 3.13's gridding methods: average, average-distance,
    // average-distance-points, count, invdist, invdistnn, linear, maximum,
    // minimum, nearest, range.
    EXPECT_EQ(algorithms, 11u);
    EXPECT_EQ(records.back().kind, "listed");
    EXPECT_EQ(records.back().get("algorithms"), "11");
}

TEST_F(GeoExecutor, GdalListFindsWordsInTheDescriptions)
{
    auto reply = run("GDAL LIST shaded");
    ASSERT_TRUE(reply.ok());
    EXPECT_NE(reply->find("path=\"raster hillshade\""), std::string::npos) << *reply;
    auto json = run("GDAL LIST hillshade JSON");
    ASSERT_TRUE(json.ok());
    const auto list = nlohmann::json::parse(*json);
    ASSERT_TRUE(list.is_array());
    EXPECT_EQ(list.front()["name"], "raster hillshade");
}

TEST_F(GeoExecutor, GdalHelpPrintsOneRecordPerArgument)
{
    auto reply = run("GDAL HELP raster hillshade");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    auto spec = katana::gis::processing::describe({"raster", "hillshade"});
    ASSERT_TRUE(spec.ok());
    EXPECT_EQ(count(*reply, "\narg name="), spec->args.size());
    EXPECT_NE(reply->find("arg name=altitude"), std::string::npos);
    EXPECT_NE(reply->find("max=90 max_inclusive=yes"), std::string::npos);
    EXPECT_NE(reply->find("binding arg=input kinds=raster accepts=name,object "
                          "sources=raster,surface,file"),
              std::string::npos)
        << *reply;
}

TEST_F(GeoExecutor, GdalHelpJsonCarriesTheArgumentsSchema)
{
    auto reply = run("GDAL HELP raster hillshade JSON");
    ASSERT_TRUE(reply.ok());
    const auto described = nlohmann::json::parse(*reply);
    const auto& zfactor = described["arguments_schema"]["properties"]["zfactor"];
    EXPECT_EQ(zfactor["type"], "number");
    EXPECT_EQ(zfactor["exclusiveMinimum"], 0.0);
    const auto& altitude = described["arguments_schema"]["properties"]["altitude"];
    EXPECT_EQ(altitude["minimum"], 0.0);
    EXPECT_EQ(altitude["maximum"], 90.0);
    EXPECT_EQ(described["arguments_schema"]["properties"]["variant"]["enum"].size(), 4u);
    EXPECT_TRUE(described["inputs_schema"]["properties"].contains("input"));
    EXPECT_TRUE(described["gdal_usage"].is_object());
}

// inputs_schema offers an argument only the sources it reads (its kinds, as
// GDAL declares them): hillshade's raster input no drawing scope, buffer's
// vector input no raster or surface; a file is offered to both.
TEST_F(GeoExecutor, TheInputsSchemaOffersOnlyTheSourcesAnArgumentReads)
{
    const auto properties = [&](const std::string& algorithm) {
        auto reply = run("GDAL HELP " + algorithm + " JSON");
        EXPECT_TRUE(reply.ok());
        return reply ? nlohmann::json::parse(
                           *reply)["inputs_schema"]["properties"]["input"]["properties"]
                     : nlohmann::json();
    };
    const auto raster = properties("raster hillshade");
    for (const char* key : {"raster", "surface", "cell", "file", "layer"}) {
        EXPECT_TRUE(raster.contains(key)) << key;
    }
    for (const char* key : {"scope", "area", "layers", "only", "where"}) {
        EXPECT_FALSE(raster.contains(key)) << key;
    }
    const auto vector = properties("vector buffer");
    for (const char* key : {"scope", "area", "layers", "only", "where", "file", "layer"}) {
        EXPECT_TRUE(vector.contains(key)) << key;
    }
    for (const char* key : {"raster", "surface", "cell"}) {
        EXPECT_FALSE(vector.contains(key)) << key;
    }
}

TEST_F(GeoExecutor, GdalRunSaysWhatTheScopeTook)
{
    type("LINE 0,0 100,0");
    type("TEXT 5,5 2.5 \"LOT 7\"");
    auto reply = run("GDAL vector buffer --distance=1 --endcap-style=flat FROM DRAWING TO LAYER "
                     "gis/buffer");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("scope arg=input scope=drawing matched=2 used=1 points=0 lines=1 "
                          "polygons=0 skipped.text=1"),
              std::string::npos)
        << *reply;
    EXPECT_NE(reply->find("output arg=output kind=vector target=layer layer=gis/buffer created=1"),
              std::string::npos)
        << *reply;
    // 100 m by 2 m with flat caps.
    EXPECT_NEAR(areaOn("gis/buffer"), 200.0, 1e-9);
}

TEST_F(GeoExecutor, AResultIsOneUndoStep)
{
    type("LINE 0,0 100,0");
    const std::size_t steps = document.history().undoCount();
    ASSERT_TRUE(run("GDAL vector buffer distance=1 FROM DRAWING").ok());
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    // Without TO, features go to gis/<the algorithm's last word>.
    EXPECT_TRUE(document.model().layers.contains("gis/buffer"));
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.size(), 1u);
    EXPECT_FALSE(document.model().layers.contains("gis/buffer"));
}

TEST_F(GeoExecutor, ViewIsRefusedHeadlessNamingArea)
{
    type("LINE 0,0 100,0");
    auto reply = run("GDAL vector buffer distance=1 FROM VIEW");
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().code, ErrorCode::InvalidState);
    EXPECT_NE(reply.error().message.find("AREA"), std::string::npos);
}

TEST_F(GeoExecutor, AnAreaTakesWhatLiesInIt)
{
    type("LINE 0,0 10,0");
    type("LINE 100,100 110,100");
    auto reply = run("GDAL vector buffer distance=1 FROM AREA -5,-5,20,20 TO LAYER gis/near");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("scope=area area=-5,-5,20,20 matched=1"), std::string::npos) << *reply;
}

TEST_F(GeoExecutor, AnEmptyScopeIsReportedNotAnError)
{
    type("LINE 0,0 10,0");
    auto reply = run("GDAL vector buffer distance=1 FROM DRAWING WHERE TYPE=circle");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("ran=no"), std::string::npos);
    EXPECT_NE(reply->find("matched=0"), std::string::npos) << *reply;
    EXPECT_EQ(document.model().entities.size(), 1u);
}

TEST_F(GeoExecutor, PreviewChangesNothing)
{
    type("LINE 0,0 100,0");
    const std::size_t steps = document.history().undoCount();
    auto reply = run("GDAL vector buffer --distance=1 FROM DRAWING TO LAYER gis/buffer PREVIEW");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("preview valid=yes changed=no"), std::string::npos);
    EXPECT_NE(reply->find("scope arg=input scope=drawing matched=1"), std::string::npos);
    EXPECT_EQ(document.history().undoCount(), steps);
    EXPECT_EQ(document.model().entities.size(), 1u);
    EXPECT_FALSE(document.model().layers.contains("gis/buffer"));
}

TEST_F(GeoExecutor, PreviewRefusesWhatARunWouldRefuse)
{
    type("LINE 0,0 100,0");
    auto reply = run("GDAL vector buffer FROM DRAWING PREVIEW");
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(reply.error().message.find("distance"), std::string::npos) << reply.error().message;
}

TEST_F(GeoExecutor, ConfirmIsRequiredForADestructiveAlgorithm)
{
    const std::string file = scratch.file("doomed.txt");
    std::ofstream(file) << "x";
    auto refused = run("GDAL vsi delete \"" + file + "\"");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::Unsupported);
    EXPECT_NE(refused.error().message.find("CONFIRM"), std::string::npos);
    EXPECT_TRUE(std::filesystem::exists(file));
    auto confirmed = run("GDAL vsi delete \"" + file + "\" CONFIRM");
    ASSERT_TRUE(confirmed.ok()) << confirmed.error().describe();
    EXPECT_FALSE(std::filesystem::exists(file));
}

TEST_F(GeoExecutor, OverwriteIsRequiredToReplaceAFile)
{
    const std::string out = scratch.file("shade.tif");
    std::ofstream(out) << "not a raster";
    const std::string line =
        "GDAL raster hillshade FROM FILE \"" + kData + "/plane.asc\" TO FILE \"" + out + "\"";
    auto refused = run(line);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::AlreadyExists);
    auto replaced = run(line + " OVERWRITE");
    ASSERT_TRUE(replaced.ok()) << replaced.error().describe();
    auto written = katana::gis::GdalDataset::open(out);
    ASSERT_TRUE(written.ok());
    EXPECT_EQ((*written)->rasterInfo()->width, 40);
}

TEST_F(GeoExecutor, GdalsOwnWordsThatChangeAFileNeedOverwrite)
{
    auto reply = run("GDAL raster hillshade \"" + kData + "/plane.asc\" \"" +
                     scratch.file("out.tif") + "\" --overwrite");
    ASSERT_FALSE(reply.ok());
    EXPECT_NE(reply.error().message.find("OVERWRITE"), std::string::npos);
}

// The bytes of a file, to show a refused line left it as it was.
std::string bytesOf(const std::string& path)
{
    std::ifstream in(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
}

TEST_F(GeoExecutor, AQuotedPipelineThatOverwritesAFileNeedsOverwrite)
{
    const std::string victim = scratch.file("victim.tif");
    const std::string raster = kData + "/plane.asc";
    ASSERT_TRUE(run("GDAL raster hillshade \"" + raster + "\" \"" + victim + "\"").ok());
    const std::string before = bytesOf(victim);
    ASSERT_FALSE(before.empty());
    // One quoted text is one word to the line, and still steps to GDAL.
    const std::string line =
        "GDAL pipeline \"read " + raster + " ! write --overwrite " + victim + "\"";
    auto refused = run(line);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("OVERWRITE"), std::string::npos)
        << refused.error().describe();
    EXPECT_EQ(bytesOf(victim), before);
    auto allowed = run(line + " OVERWRITE");
    ASSERT_TRUE(allowed.ok()) << allowed.error().describe();
    EXPECT_NE(bytesOf(victim), before);
}

TEST_F(GeoExecutor, APipelineUpdateStepNeedsConfirmQuotedOrNot)
{
    const std::string points = scratch.file("points.geojson");
    std::ofstream(points)
        << R"({"type":"FeatureCollection","features":[{"type":"Feature","properties":{"id":"1",)"
        << R"("name":"new"},"geometry":{"type":"Point","coordinates":[5,5]}}]})";
    const std::string target = scratch.file("target.gpkg");
    const std::string source = scratch.file("source.gpkg");
    ASSERT_TRUE(
        run("GDAL vector convert \"" + points + "\" \"" + target + "\" --output-layer=pts").ok());
    ASSERT_TRUE(
        run("GDAL vector convert \"" + points + "\" \"" + source + "\" --output-layer=pts").ok());
    const std::string before = bytesOf(target);
    for (const std::string& line :
         {"GDAL vector pipeline read \"" + source + "\" ! update \"" + target + "\"",
          "GDAL vector pipeline \"read " + source + " ! update " + target + "\"",
          "GDAL pipeline \"read " + source + " ! tee [ update " + target + " ] ! write " +
              scratch.file("copy.gpkg") + "\""}) {
        auto refused = run(line);
        ASSERT_FALSE(refused.ok()) << line;
        EXPECT_EQ(refused.error().code, ErrorCode::Unsupported) << line;
        EXPECT_NE(refused.error().message.find("CONFIRM"), std::string::npos)
            << refused.error().describe();
        EXPECT_EQ(refused.error().context, "update");
        EXPECT_EQ(bytesOf(target), before) << line;
    }
    // Said, it runs.
    auto confirmed =
        run("GDAL vector pipeline read \"" + source + "\" ! update \"" + target + "\" CONFIRM");
    ASSERT_TRUE(confirmed.ok()) << confirmed.error().describe();
}

TEST_F(GeoExecutor, RasterizeAddBurnsIntoAnExistingRasterOnlyWithOverwrite)
{
    const std::string raster = scratch.file("burn.tif");
    ASSERT_TRUE(run("GDAL raster hillshade \"" + kData + "/plane.asc\" \"" + raster + "\"").ok());
    const std::string before = bytesOf(raster);
    const std::string line =
        "GDAL vector rasterize --burn 5 --add \"" + kData + "/lots.geojson\" \"" + raster + "\"";
    auto refused = run(line);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().context, "--add");
    EXPECT_EQ(bytesOf(raster), before);
}

TEST_F(GeoExecutor, AResultInAnotherCrsThanTheProjectsIsSaidToBe)
{
    // GDAL vector reproject into EPSG:4326 makes degrees, which TO LAYER
    // draws into an MGA drawing as they are: 151 E, 34 S among 330000 m
    // eastings, and nothing said so.
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    type("LINE 330000,6250000 330100,6250000");
    auto reply = run("GDAL vector reproject --output-crs=EPSG:4326 FROM DRAWING TO LAYER lonlat");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("warning text=\"the result is in WGS 84 (EPSG:4326), not the project's "
                          "GDA94 / MGA zone 56 (EPSG:28356)"),
              std::string::npos)
        << *reply;
    // A result in the project's own system says nothing of it.
    auto same = run("GDAL vector buffer --distance 1 FROM DRAWING TO LAYER buffered");
    ASSERT_TRUE(same.ok()) << same.error().describe();
    EXPECT_EQ(same->find("not the project's"), std::string::npos) << *same;
}

TEST_F(GeoExecutor, GdalTokensAfterAClauseAreRefused)
{
    type("LINE 0,0 100,0");
    auto reply = run("GDAL vector buffer FROM DRAWING --distance=1");
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(reply.error().context, "--distance=1");
}

TEST_F(GeoExecutor, AFileOnlyGdalLineWritesTheFile)
{
    const std::string out = scratch.file("shade.tif");
    auto reply = run("GDAL raster hillshade \"" + kData + "/plane.asc\" \"" + out + "\" --zfactor 2");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("kind=file target=file"), std::string::npos) << *reply;
    EXPECT_TRUE(std::filesystem::exists(out));
    EXPECT_TRUE(reference.rasters().empty());
    EXPECT_EQ(document.model().entities.size(), 0u);
}

TEST_F(GeoExecutor, AnOutputGdalsWordsNameIsReplacedOnlyWithOverwrite)
{
    const std::string line =
        "GDAL raster hillshade \"" + kData + "/plane.asc\" \"" + scratch.file("shade.tif") + "\"";
    ASSERT_TRUE(run(line).ok());
    // GDAL refuses to write over it by itself, and so the line does too.
    auto again = run(line);
    ASSERT_FALSE(again.ok());
    EXPECT_NE(again.error().message.find("already exists"), std::string::npos)
        << again.error().describe();
    auto replaced = run(line + " OVERWRITE");
    EXPECT_TRUE(replaced.ok()) << replaced.error().describe();
}

TEST_F(GeoExecutor, ARasterResultBecomesADerivedReferenceRaster)
{
    const std::string line = "GDAL raster hillshade \"" + kData + "/plane.asc\"";
    auto reply = run(line);
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    ASSERT_EQ(reference.rasters().size(), 1u);
    const katana::interop::RasterOverlay& shade = reference.rasters().front();
    EXPECT_EQ(shade.name, "hillshade");
    EXPECT_EQ(shade.role, katana::interop::RasterRole::Derived);
    EXPECT_EQ(shade.derivation, line);
    EXPECT_EQ(shade.width, 40);
    // No project: kept in the scratch folder, and said so.
    EXPECT_NE(reply->find("persisted=no"), std::string::npos);
    EXPECT_EQ(shade.source.parent_path(), scratch.path());
    // A second takes the next free name.
    ASSERT_TRUE(run(line).ok());
    ASSERT_EQ(reference.rasters().size(), 2u);
    EXPECT_EQ(reference.rasters().back().name, "hillshade-2");
}

TEST_F(GeoExecutor, ARasterCannotGoToALayerNorFeaturesToAReference)
{
    type("LINE 0,0 100,0");
    auto raster = run("GDAL raster hillshade FROM FILE \"" + kData + "/plane.asc\" TO LAYER gis/x");
    ASSERT_FALSE(raster.ok());
    EXPECT_EQ(raster.error().code, ErrorCode::Unsupported);
    auto vector = run("GDAL vector buffer distance=1 FROM DRAWING TO REFERENCE b");
    ASSERT_FALSE(vector.ok());
    EXPECT_EQ(vector.error().code, ErrorCode::Unsupported);
}

TEST_F(GeoExecutor, FromBindsItsNamedDatasetArgument)
{
    // Two 50 x 40 lots and a 4 m corridor across both, drawn on layers of
    // their own: the corridor's intersection with each lot is 200 m2.
    type("LAYER NEW lots");
    type("LAYER NEW corridor");
    type("LAYER SET lots");
    type("PLINE 0,0 50,0 50,40 0,40 CLOSE");
    type("PLINE 50,0 100,0 100,40 50,40 CLOSE");
    type("LAYER SET corridor");
    type("PLINE -10,18 110,18 110,22 -10,22 CLOSE");
    auto reply = run("GDAL vector layer-algebra intersection FROM input LAYERS lots FROM method "
                     "LAYERS corridor TO LAYER gis/overlay");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("scope arg=input scope=layers layers=lots"), std::string::npos) << *reply;
    EXPECT_NE(reply->find("scope arg=method scope=layers layers=corridor"), std::string::npos);
    EXPECT_NE(reply->find("created=2"), std::string::npos) << *reply;
    EXPECT_NEAR(areaOn("gis/overlay"), 400.0, 1e-9);
}

TEST_F(GeoExecutor, AnInputGivenTwiceIsRefused)
{
    auto reply = run("GDAL raster hillshade --input=\"" + kData + "/plane.asc\" FROM FILE \"" +
                     kData + "/plane.asc\"");
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(reply.error().message.find("twice"), std::string::npos) << reply.error().message;
}

// A source of a kind the argument does not read is refused as the line is
// read, before GDAL is given anything: hillshade's input reads rasters
// (GDAL HELP says kinds=raster), so the drawing is refused, previewed or
// run, and nothing is drawn or kept; buffer's reads vectors, so a raster is
// refused. GDAL itself answered the first only "Unable to fetch band #1",
// from the worker.
TEST_F(GeoExecutor, ASourceOfAKindTheArgumentDoesNotReadIsRefusedBeforeItRuns)
{
    type("RECT 0,0 10,10");
    ASSERT_TRUE(run("IMPORT \"" + kData + "/plane.asc\"").ok());
    const std::size_t steps = document.history().undoCount();
    for (const std::string line :
         {"GDAL raster hillshade FROM DRAWING", "GDAL raster hillshade FROM DRAWING PREVIEW",
          "GDAL raster hillshade FROM input LAYERS 0 TO REFERENCE shade",
          "GDAL vector buffer --distance=1 FROM RASTER plane",
          "GDAL vector buffer --distance=1 FROM input SURFACE nothing"}) {
        auto reply = run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument) << line;
        EXPECT_EQ(reply.error().context, "input") << line;
        EXPECT_NE(reply.error().message.find("; it takes "), std::string::npos)
            << reply.error().message;
    }
    EXPECT_EQ(document.history().undoCount(), steps);
    EXPECT_EQ(reference.rasters().size(), 1u);
}

TEST_F(GeoExecutor, FromNamesOnlyAnInputDataset)
{
    auto reply = run("GDAL raster hillshade FROM zfactor FILE x.tif");
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().context, "zfactor");
}

TEST_F(GeoExecutor, ARasterSourceIsTheReferenceRastersOwnFile)
{
    ASSERT_TRUE(run("GDAL raster hillshade \"" + kData + "/plane.asc\"").ok());
    auto reply = run("GDAL raster info --format=text FROM RASTER hillshade");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("Size is 40, 30"), std::string::npos) << *reply;
    EXPECT_NE(reply->find("input arg=input source=raster name=hillshade"), std::string::npos);
    auto missing = run("GDAL raster info FROM RASTER 99");
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
}

TEST_F(GeoExecutor, ASurfaceIsSampledAtItsCellCentres)
{
    // Two triangles over 40 x 30 on z = 100 + 0.05 x: the plane of
    // plane.asc, so its slope is 5 % wherever the grid is inside it. The
    // TIN interpolates the plane exactly; the tolerance is the Float32
    // analysis of GdalRun.SlopeOfAPlaneRisingOneInTwentyIsFivePercent.
    auto tin = katana::terrain::TinSurface::create(
        {{0, 0, 100}, {40, 0, 102}, {40, 30, 102}, {0, 30, 100}}, {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(tin.ok());
    ASSERT_TRUE(surfaces.add({"ground", std::make_shared<const katana::terrain::TinSurface>(
                                            std::move(tin).value()),
                              "test"})
                    .ok());
    auto reply = run("GDAL raster slope --unit=percent FROM SURFACE ground CELL 1 TO REFERENCE s");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("input arg=input source=surface name=ground cell=1"), std::string::npos);
    ASSERT_EQ(reference.rasters().size(), 1u);
    auto slope = katana::gis::GdalDataset::open(reference.rasters().front().source);
    ASSERT_TRUE(slope.ok());
    auto band = (*slope)->readBand(1);
    ASSERT_TRUE(band.ok());
    EXPECT_NEAR((*band)[static_cast<std::size_t>(15 * 40 + 20)], 5.0, 5e-4);
    auto unknown = run("GDAL raster slope FROM SURFACE nowhere");
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::NotFound);
}

TEST_F(GeoExecutor, ACancelledRunChangesNothing)
{
    type("LINE 0,0 100,0");
    auto prepared = geo::prepare(context, "GDAL vector buffer distance=1 FROM DRAWING");
    ASSERT_TRUE(prepared.ok());
    ASSERT_FALSE(prepared->reply.has_value());
    std::stop_source stop;
    stop.request_stop();
    auto apply = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().message, "cancelled");
    EXPECT_EQ(document.model().entities.size(), 1u);
    EXPECT_FALSE(document.model().layers.contains("gis/buffer"));
}

TEST_F(GeoExecutor, ACancelledRunLeavesTheFileItWouldHaveReplacedAsItWas)
{
    // OVERWRITE let GDAL write over the file from the start, so a run
    // cancelled part way left a partial file where the original had been.
    const std::string out = scratch.file("shade.tif");
    ASSERT_TRUE(run("GDAL raster hillshade \"" + kData + "/plane.asc\" \"" + out + "\"").ok());
    const std::string before = bytesOf(out);
    ASSERT_FALSE(before.empty());
    auto prepared = geo::prepare(context, "GDAL raster hillshade --zfactor=3 FROM FILE \"" + kData +
                                              "/plane.asc\" TO FILE \"" + out + "\" OVERWRITE");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    std::stop_source stop;
    bool reported = false;
    auto apply = prepared->work(stop.get_token(), [&](double) {
        reported = true;
        stop.request_stop();
    });
    ASSERT_TRUE(reported) << "the run never reported progress, so it was never stopped part way";
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().message, "cancelled");
    EXPECT_EQ(bytesOf(out), before);
    // Nothing is left beside it either.
    std::size_t files = 0;
    for (const auto& entry : std::filesystem::directory_iterator(scratch.path())) {
        files += entry.path().filename().string().starts_with(".katana-staging") ? 1u : 0u;
    }
    EXPECT_EQ(files, 0u);
    // Run to its end, it replaces the file, and the reply names it.
    auto replaced = run("GDAL raster hillshade --zfactor=3 FROM FILE \"" + kData +
                        "/plane.asc\" TO FILE \"" + out + "\" OVERWRITE");
    ASSERT_TRUE(replaced.ok()) << replaced.error().describe();
    EXPECT_NE(bytesOf(out), before);
    EXPECT_NE(replaced->find("file=" + out), std::string::npos) << *replaced;
}

TEST_F(GeoExecutor, TheWorkReadsOnlyWhatPrepareCopied)
{
    // The work runs on another thread in the window; what it reads was
    // copied at prepare, so a drawing changed meanwhile does not reach it,
    // and a result that only creates is applied all the same.
    type("LINE 0,0 100,0");
    auto prepared = geo::prepare(context, "GDAL vector buffer distance=1 --endcap-style=flat FROM DRAWING");
    ASSERT_TRUE(prepared.ok());
    type("SELECT ALL");
    type("ERASE");
    auto apply = prepared->work({}, {});
    ASSERT_TRUE(apply.ok()) << apply.error().describe();
    auto reply = (*apply)(context);
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NEAR(areaOn("gis/buffer"), 200.0, 1e-9);
}

TEST_F(GeoExecutor, AnInPlaceApplyRefusesWhenTheDrawingChangedWhileTheJobRan)
{
    type("LINE 0,0 100,0");
    const katana::entity::Entity copy = *document.model().entities.find(1);
    EXPECT_TRUE(geo::unchangedSince(context, {copy}).ok());
    type("SELECT 1");
    type("MOVE 5,5");
    const auto changed = geo::unchangedSince(context, {copy});
    ASSERT_FALSE(changed.ok());
    EXPECT_EQ(changed.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(changed.error().message, "the drawing changed while the job ran");
}

TEST_F(GeoExecutor, TheSourceAndTargetClausesReadWhatTheGrammarSays)
{
    auto tokens = geo::tokenize("FROM SURFACE g CELL 0.5 FROM FILE \"a b.tif\" LAYER roads "
                                "FROM LAYERS a,b ONLY WHERE TYPE=polyline TO REFERENCE shade");
    ASSERT_TRUE(tokens.ok());
    std::size_t at = 1;
    auto surface = geo::parseSource(*tokens, at);
    ASSERT_TRUE(surface.ok());
    EXPECT_EQ(surface->kind, geo::Source::Kind::Surface);
    EXPECT_EQ(surface->surface, "g");
    EXPECT_EQ(surface->cell, 0.5);
    ++at;
    auto file = geo::parseSource(*tokens, at);
    ASSERT_TRUE(file.ok());
    EXPECT_EQ(file->path, "a b.tif");
    EXPECT_EQ(file->layer, "roads");
    ++at;
    auto layers = geo::parseSource(*tokens, at);
    ASSERT_TRUE(layers.ok());
    EXPECT_EQ(layers->kind, geo::Source::Kind::Drawing);
    EXPECT_EQ(layers->scope.layers, (std::vector<std::string>{"a", "b"}));
    EXPECT_FALSE(layers->scope.sublayers);
    EXPECT_EQ(layers->scope.filter.types.size(), 1u);
    ASSERT_TRUE(tokens->is(at, "TO"));
    ++at;
    auto target = geo::parseTarget(*tokens, at);
    ASSERT_TRUE(target.ok());
    EXPECT_EQ(target->kind, geo::Target::Kind::Reference);
    EXPECT_EQ(target->name, "shade");
    EXPECT_EQ(at, tokens->size());
}

TEST_F(GeoExecutor, AQuotedKeywordIsAValue)
{
    auto tokens = geo::tokenize("GDAL \"FROM\" FROM");
    ASSERT_TRUE(tokens.ok());
    EXPECT_FALSE(tokens->is(1, "FROM"));
    EXPECT_TRUE(tokens->is(2, "FROM"));
    EXPECT_FALSE(geo::tokenize("GDAL \"open").ok());
}

// ---- through a Session, as katana_cli and katana_mcp run it ----------------------------------

// std::cout and std::cerr swapped for strings while it lives.
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

TEST(GeoSession, AGdalLineRunsThroughTheSessionAndItsRasterIsAReference)
{
    katana::app::Session session(nullptr);
    Captured captured;
    EXPECT_TRUE(session.run("GDAL raster hillshade \"" + kData + "/plane.asc\""));
    EXPECT_TRUE(session.run("REFS"));
    EXPECT_NE(captured.out().find("target=reference id=1 name=hillshade"), std::string::npos)
        << captured.out();
    // REFS replies in records since IMPORT, EXPORT, INFO, REFS and COPC became
    // the executor's (docs/interop.md).
    EXPECT_NE(captured.out().find("reference id=1 kind=raster name=hillshade width=40 height=30"),
              std::string::npos)
        << captured.out();
    EXPECT_FALSE(session.run("GDAL raster hillshade --config X=Y"));
    EXPECT_NE(captured.err().find("error: InvalidArgument: --config is refused"), std::string::npos)
        << captured.err();
    EXPECT_NE(katana::app::Session::helpText().find("GDAL VERSION"), std::string::npos);
}

} // namespace
