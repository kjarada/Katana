// The reference layers managed and kept (src/katana_app/geo/refs_verb.cpp;
// interop/reference_data.hpp; docs/interop.md, "Reference layers"): REFS
// shows, hides, restyles, renames and removes a layer by its id or name,
// builds overviews only when told to, and a saved project reads its layers
// again when it is opened - through the executor, and through a Session as
// katana_cli and katana_mcp run it.
//
// The samples' facts, by hand: samples/gis/terrain.asc is 120 x 90 cells of
// 1.5 from (-5, -5); survey_scan.las holds 40 000 points (`pdal info`);
// tests/archive12d/data/las_point_cloud.12da two clouds, SITE SCAN of 154
// points and ROAD SCAN of 24 (its las_cloud_data strings).

#include <filesystem>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "geo/geo_verbs.hpp"
#include "geo/references.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "session.hpp"

namespace {

namespace geo = katana::app::geo;
namespace interop = katana::interop;
using katana::core::ErrorCode;

const std::string kSamples = KATANA_GIS_SAMPLES;
const std::string kData = KATANA_GEO_TEST_DATA;
const std::string kArchives = kSamples + "/../../tests/archive12d/data";

std::string quoted(const std::string& path)
{
    return "\"" + path + "\"";
}

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana refs verbs " + name))
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

std::vector<geo::Record> recordsOf(const std::string& reply, const std::string& kind)
{
    std::vector<geo::Record> found;
    for (geo::Record& record : geo::parseRecords(reply)) {
        if (record.kind == kind) {
            found.push_back(std::move(record));
        }
    }
    return found;
}

std::string field(const geo::Record& record, const std::string& key)
{
    return record.get(key).value_or("<absent>");
}

class RefsVerbs : public ::testing::Test {
  protected:
    TempDir scratch{::testing::UnitTest::GetInstance()->current_test_info()->name()};
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, scratch.path(), {}, {}};
    int changes = 0;

    void SetUp() override { context.changed = [this] { ++changes; }; }

    katana::core::Result<std::string> run(const std::string& line)
    {
        return geo::runNow(context, line);
    }

    void importTerrainAndScan()
    {
        ASSERT_TRUE(run("IMPORT " + quoted(kSamples + "/terrain.asc")).ok());
        ASSERT_TRUE(run("IMPORT " + quoted(kSamples + "/survey_scan.las")).ok());
    }
};

TEST_F(RefsVerbs, RefsHideThenShowRoundTripsVisibility)
{
    importTerrainAndScan();
    const int before = changes;
    auto hidden = run("REFS HIDE 1");
    ASSERT_TRUE(hidden.ok()) << hidden.error().describe();
    EXPECT_FALSE(reference.findRaster(1)->visible);
    EXPECT_EQ(field(recordsOf(*hidden, "reference").at(0), "visible"), "no");
    // By name, in any case, the same layer.
    auto shown = run("REFS SHOW Terrain");
    ASSERT_TRUE(shown.ok()) << shown.error().describe();
    EXPECT_TRUE(reference.findRaster(1)->visible);
    EXPECT_EQ(field(recordsOf(*shown, "reference").at(0), "visible"), "yes");
    // Each change redraws: the front end is told.
    EXPECT_EQ(changes, before + 2);

    auto cloud = run("REFS HIDE 2");
    ASSERT_TRUE(cloud.ok());
    EXPECT_FALSE(reference.findPointCloud(2)->visible);
}

TEST_F(RefsVerbs, OpacityOutsideZeroToOneIsRefused)
{
    importTerrainAndScan();
    ASSERT_TRUE(run("REFS OPACITY 1 0.5").ok());
    EXPECT_EQ(reference.findRaster(1)->opacity, 0.5);
    for (const char* bad : {"1.5", "-0.1", "half", "nan"}) {
        auto refused = run(std::string("REFS OPACITY 1 ") + bad);
        ASSERT_FALSE(refused.ok()) << bad;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument) << bad;
    }
    EXPECT_EQ(reference.findRaster(1)->opacity, 0.5) << "a refusal changes nothing";
    // A point cloud has no opacity; a raster no colouring.
    EXPECT_EQ(run("REFS OPACITY 2 0.5").error().code, ErrorCode::Unsupported);
    EXPECT_EQ(run("REFS COLOR 1 intensity").error().code, ErrorCode::Unsupported);
}

TEST_F(RefsVerbs, AColourRenameAndRemoveAreByIdOrName)
{
    importTerrainAndScan();
    auto coloured = run("REFS COLOUR survey_scan classification");
    ASSERT_TRUE(coloured.ok()) << coloured.error().describe();
    EXPECT_EQ(reference.findPointCloud(2)->colorMode, interop::PointColorMode::Classification);
    EXPECT_EQ(field(recordsOf(*coloured, "reference").at(0), "color"), "classification");
    EXPECT_EQ(run("REFS COLOR 2 purple").error().code, ErrorCode::InvalidArgument);

    auto renamed = run("REFS RENAME 1 \"site dem\"");
    ASSERT_TRUE(renamed.ok()) << renamed.error().describe();
    EXPECT_EQ(reference.findRaster(1)->name, "site dem");

    auto removed = run("REFS REMOVE \"site dem\"");
    ASSERT_TRUE(removed.ok()) << removed.error().describe();
    EXPECT_EQ(*removed, "removed id=1 kind=raster name=\"site dem\"");
    EXPECT_EQ(reference.findRaster(1), nullptr);
    EXPECT_EQ(run("REFS REMOVE 1").error().code, ErrorCode::NotFound);

    // Two layers of one name are named by their ids.
    ASSERT_TRUE(run("IMPORT " + quoted(kSamples + "/terrain.asc")).ok());
    ASSERT_TRUE(run("IMPORT " + quoted(kSamples + "/terrain.asc")).ok());
    auto ambiguous = run("REFS HIDE terrain");
    ASSERT_FALSE(ambiguous.ok());
    EXPECT_EQ(ambiguous.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(ambiguous.error().message.find("3, 4"), std::string::npos)
        << ambiguous.error().message;
}

TEST_F(RefsVerbs, RefsJsonAndInfoGiveEachLayersFacts)
{
    importTerrainAndScan();
    auto json = run("REFS JSON");
    ASSERT_TRUE(json.ok()) << json.error().describe();
    const nlohmann::json parsed = nlohmann::json::parse(*json);
    ASSERT_EQ(parsed["references"].size(), 2u);
    EXPECT_EQ(parsed["references"][0]["id"], 1);
    EXPECT_EQ(parsed["references"][0]["width"], 120);
    EXPECT_EQ(parsed["references"][1]["points"], 40000);
    EXPECT_TRUE(parsed["missing"].empty());

    auto info = run("REFS INFO 1");
    ASSERT_TRUE(info.ok()) << info.error().describe();
    const auto source = recordsOf(*info, "source");
    ASSERT_EQ(source.size(), 1u) << *info;
    EXPECT_EQ(field(source[0], "file"), kSamples + "/terrain.asc");
    EXPECT_NE(field(source[0], "crs").find("EPSG:32630"), std::string::npos);
}

TEST_F(RefsVerbs, OverviewsNeedConfirm)
{
    // A copy: the overviews are written beside the file.
    std::filesystem::copy_file(kSamples + "/terrain.asc", scratch.path() / "terrain.asc");
    std::filesystem::copy_file(kSamples + "/terrain.prj", scratch.path() / "terrain.prj");
    ASSERT_TRUE(run("IMPORT " + quoted(scratch.file("terrain.asc"))).ok());
    const std::filesystem::path ovr = scratch.path() / "terrain.asc.ovr";

    auto refused = run("REFS OVERVIEWS 1 levels=2,4");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::Unsupported);
    EXPECT_NE(refused.error().message.find("add CONFIRM"), std::string::npos);
    EXPECT_FALSE(std::filesystem::exists(ovr)) << "refused, nothing written";

    // 120 x 90 halved is 60 x 45; quartered 30 x 22.5, which GDAL rounds up
    // to 23 (an overview covers every pixel of the full image).
    auto built = run("REFS OVERVIEWS 1 levels=2,4 CONFIRM");
    ASSERT_TRUE(built.ok()) << built.error().describe();
    EXPECT_TRUE(std::filesystem::exists(ovr));
    const auto head = recordsOf(*built, "overviews");
    ASSERT_EQ(head.size(), 1u) << *built;
    EXPECT_EQ(field(head[0], "count"), "2");
    const auto levels = recordsOf(*built, "overview");
    ASSERT_EQ(levels.size(), 2u);
    EXPECT_EQ(field(levels[0], "width"), "60");
    EXPECT_EQ(field(levels[0], "height"), "45");
    EXPECT_EQ(field(levels[1], "width"), "30");
    EXPECT_EQ(field(levels[1], "height"), "23");
    // The source itself is untouched: external overviews, and no sidecar.
    EXPECT_FALSE(std::filesystem::exists(scratch.path() / "terrain.asc.aux.xml"));
}

TEST_F(RefsVerbs, ARecordRoundTripsEveryDisplaySetting)
{
    importTerrainAndScan();
    ASSERT_TRUE(run("REFS OPACITY 1 0.25").ok());
    ASSERT_TRUE(run("REFS HIDE 1").ok());
    ASSERT_TRUE(run("REFS RENAME 1 dem").ok());
    ASSERT_TRUE(run("REFS COLOR 2 rgb").ok());
    const std::vector<std::string> records = interop::referenceRecords(reference);
    ASSERT_EQ(records.size(), 2u);
    for (const std::string& record : records) {
        EXPECT_EQ(record.find('\n'), std::string::npos) << "one line: " << record;
    }
    auto raster = interop::parseReferenceRecord(records[0]);
    ASSERT_TRUE(raster.ok()) << raster.error().describe();
    EXPECT_EQ(raster->kind, interop::ReferenceSource::Kind::Raster);
    EXPECT_EQ(raster->name, "dem");
    EXPECT_EQ(raster->source, std::filesystem::path(kSamples + "/terrain.asc"));
    EXPECT_FALSE(raster->visible);
    EXPECT_EQ(raster->opacity, 0.25);
    EXPECT_EQ(raster->maxPixels, 120);
    auto cloud = interop::parseReferenceRecord(records[1]);
    ASSERT_TRUE(cloud.ok()) << cloud.error().describe();
    EXPECT_EQ(cloud->kind, interop::ReferenceSource::Kind::PointCloud);
    EXPECT_EQ(cloud->colorMode, interop::PointColorMode::SourceColor);
    EXPECT_EQ(cloud->budget, 40000u);

    // A newer record is refused by name, not half read.
    auto newer = interop::parseReferenceRecord(
        R"({"version":2,"kind":"raster","name":"x","source":"x.tif"})");
    ASSERT_FALSE(newer.ok());
    EXPECT_NE(newer.error().message.find("newer"), std::string::npos);
}

TEST_F(RefsVerbs, ADerivedRasterPersistsItsDerivation)
{
    // A raster a GDAL line made keeps the line: recorded with the project,
    // and read again with it.
    const std::string line = "GDAL raster hillshade FROM FILE " + quoted(kData + "/plane.asc") +
                             " TO REFERENCE shade";
    auto made = run(line);
    ASSERT_TRUE(made.ok()) << made.error().describe();
    geo::recordReferences(context);
    ASSERT_EQ(document.metadata().referenceLayers.size(), 1u);
    auto recorded = interop::parseReferenceRecord(document.metadata().referenceLayers.front());
    ASSERT_TRUE(recorded.ok());
    EXPECT_EQ(recorded->role, interop::RasterRole::Derived);
    EXPECT_EQ(recorded->derivation, line);

    reference.clear();
    auto restored = run("REFS RESTORE");
    ASSERT_TRUE(restored.ok()) << restored.error().describe();
    ASSERT_EQ(reference.rasters().size(), 1u);
    EXPECT_EQ(reference.rasters().front().name, "shade");
    EXPECT_EQ(reference.rasters().front().role, interop::RasterRole::Derived);
    EXPECT_EQ(reference.rasters().front().derivation, line);
    auto info = run("REFS INFO shade");
    ASSERT_TRUE(info.ok());
    EXPECT_EQ(field(recordsOf(*info, "derivation").at(0), "line"), line);
}

TEST_F(RefsVerbs, AMissingSourceIsWarnedNotFatalAndKeptForTheNextSave)
{
    std::filesystem::copy_file(kSamples + "/terrain.asc", scratch.path() / "gone.asc");
    ASSERT_TRUE(run("IMPORT " + quoted(scratch.file("gone.asc"))).ok());
    ASSERT_TRUE(run("IMPORT " + quoted(kSamples + "/survey_scan.las")).ok());
    geo::recordReferences(context);
    const std::vector<std::string> recorded = document.metadata().referenceLayers;
    ASSERT_EQ(recorded.size(), 2u);
    std::filesystem::remove(scratch.path() / "gone.asc");

    auto restored = run("REFS RESTORE");
    ASSERT_TRUE(restored.ok()) << restored.error().describe();
    const auto warnings = recordsOf(*restored, "warning");
    ASSERT_EQ(warnings.size(), 1u) << *restored;
    EXPECT_NE(field(warnings[0], "text").find("is gone"), std::string::npos);
    EXPECT_EQ(field(recordsOf(*restored, "restored").at(0), "layers"), "1");
    EXPECT_EQ(field(recordsOf(*restored, "restored").at(0), "missing"), "1");
    EXPECT_TRUE(reference.rasters().empty());
    ASSERT_EQ(reference.pointClouds().size(), 1u);

    // Listed as missing, and written back as it was by the next save: the
    // drive may simply not be there today.
    auto listed = run("REFS");
    ASSERT_TRUE(listed.ok());
    ASSERT_EQ(recordsOf(*listed, "missing").size(), 1u);
    EXPECT_EQ(field(recordsOf(*listed, "missing")[0], "name"), "gone");
    geo::recordReferences(context);
    ASSERT_EQ(document.metadata().referenceLayers.size(), 2u);
    EXPECT_EQ(document.metadata().referenceLayers.back(), recorded.front());
    // Until it is removed by name.
    ASSERT_TRUE(run("REFS REMOVE gone").ok());
    geo::recordReferences(context);
    EXPECT_EQ(document.metadata().referenceLayers.size(), 1u);
}

TEST_F(RefsVerbs, ACloudFromAnArchiveIsReadAgainFromTheArchive)
{
    ASSERT_TRUE(run("IMPORT " + quoted(kArchives + "/las_point_cloud.12da")).ok());
    ASSERT_TRUE(run("REFS COLOR \"ROAD SCAN\" flat").ok());
    geo::recordReferences(context);
    reference.clear();
    auto restored = run("REFS RESTORE");
    ASSERT_TRUE(restored.ok()) << restored.error().describe();
    ASSERT_EQ(reference.pointClouds().size(), 2u);
    EXPECT_EQ(reference.pointClouds()[0].name, "SITE SCAN");
    EXPECT_EQ(reference.pointClouds()[0].points.size(), 154u);
    EXPECT_EQ(reference.pointClouds()[1].name, "ROAD SCAN");
    EXPECT_EQ(reference.pointClouds()[1].points.size(), 24u);
    EXPECT_EQ(reference.pointClouds()[1].colorMode, interop::PointColorMode::Flat);
    EXPECT_TRUE(document.model().entities.empty()) << "only the clouds are taken from it";
}

// ---- through a Session, saved and opened, as katana_cli and katana_mcp run it -------------

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

TEST(RefsSession, ASavedProjectReopensWithItsReferenceRasters)
{
    const TempDir scratch("session");
    const std::string project = scratch.file("site.katana");
    std::string listedFirst;
    {
        katana::app::Session session(nullptr);
        Captured captured;
        ASSERT_TRUE(session.run("RECT 0,0 10,5"));
        ASSERT_TRUE(session.run("IMPORT " + quoted(kSamples + "/terrain.asc")));
        ASSERT_TRUE(session.run("IMPORT " + quoted(kSamples + "/survey_scan.las")));
        ASSERT_TRUE(session.run("REFS OPACITY 1 0.75"));
        ASSERT_TRUE(session.run("SAVE " + quoted(project))) << captured.err();
        const std::size_t mark = captured.out().size();
        ASSERT_TRUE(session.run("REFS"));
        listedFirst = captured.out().substr(mark);
    }
    katana::app::Session reopened(nullptr);
    Captured captured;
    ASSERT_TRUE(reopened.run("OPEN " + quoted(project))) << captured.err();
    ASSERT_TRUE(reopened.run("REFS"));
    const auto before = recordsOf(listedFirst, "reference");
    const auto after = recordsOf(captured.out(), "reference");
    ASSERT_EQ(before.size(), 2u) << listedFirst;
    // The OPEN's restore and the REFS after it each list both layers.
    ASSERT_EQ(after.size(), 4u) << captured.out();
    for (std::size_t k = 0; k < 2; ++k) {
        EXPECT_EQ(field(after[2 + k], "name"), field(before[k], "name"));
        EXPECT_EQ(field(after[2 + k], "kind"), field(before[k], "kind"));
        EXPECT_EQ(field(after[2 + k], "file"), field(before[k], "file"));
        EXPECT_EQ(field(after[2 + k], "visible"), field(before[k], "visible"));
    }
    EXPECT_EQ(field(after[2], "opacity"), "0.75");
    EXPECT_EQ(field(after[3], "points"), "40000");
    EXPECT_EQ(captured.err(), "");

    // NEW lets them go with the drawing.
    ASSERT_TRUE(reopened.run("NEW"));
    ASSERT_TRUE(reopened.run("REFS"));
    EXPECT_NE(captured.out().find("references rasters=0 clouds=0 missing=0"), std::string::npos);
}

// The working folder, changed while it lives and put back after.
class InFolder {
  public:
    explicit InFolder(const std::filesystem::path& folder) : was_(std::filesystem::current_path())
    {
        std::filesystem::current_path(folder);
    }
    ~InFolder()
    {
        std::error_code error;
        std::filesystem::current_path(was_, error);
    }
    InFolder(const InFolder&) = delete;
    InFolder& operator=(const InFolder&) = delete;

  private:
    std::filesystem::path was_;
};

TEST(RefsSession, ALayerImportedByARelativePathReopensFromAnotherFolder)
{
    // katana_cli run in the data's folder, IMPORT terrain.asc: the record
    // kept "terrain.asc" as typed, and an OPEN from any other folder found
    // nothing there (docs/interop.md said the source was absolute).
    const TempDir scratch("relative");
    std::filesystem::create_directories(scratch.path() / "data");
    std::filesystem::create_directories(scratch.path() / "elsewhere");
    std::filesystem::copy_file(kSamples + "/terrain.asc", scratch.path() / "data" / "terrain.asc");
    const std::string project = scratch.file("site.katana");
    {
        const InFolder there(scratch.path() / "data");
        katana::app::Session session(nullptr);
        Captured captured;
        ASSERT_TRUE(session.run("IMPORT terrain.asc")) << captured.err();
        ASSERT_TRUE(session.run("SAVE " + quoted(project))) << captured.err();
    }
    const InFolder elsewhere(scratch.path() / "elsewhere");
    katana::app::Session reopened(nullptr);
    Captured captured;
    ASSERT_TRUE(reopened.run("OPEN " + quoted(project))) << captured.err();
    const auto restored = recordsOf(captured.out(), "restored");
    ASSERT_EQ(restored.size(), 1u) << captured.out();
    EXPECT_EQ(field(restored[0], "layers"), "1") << captured.out();
    EXPECT_EQ(field(restored[0], "missing"), "0") << captured.out();
}

TEST(RefsSession, ALayerInsideTheProjectFollowsTheProjectWhenItMoves)
{
    // The data kept in the project's own folder, and the folder moved: the
    // absolute source is gone, and the layer is found where the record
    // places it inside the project.
    const TempDir scratch("moved");
    const std::filesystem::path first = scratch.path() / "first.katana";
    const std::filesystem::path moved = scratch.path() / "moved.katana";
    {
        katana::app::Session session(nullptr);
        Captured captured;
        ASSERT_TRUE(session.run("SAVE " + quoted(first.generic_string()))) << captured.err();
        std::filesystem::create_directories(first / "data");
        std::filesystem::copy_file(kSamples + "/terrain.asc", first / "data" / "terrain.asc");
        ASSERT_TRUE(
            session.run("IMPORT " + quoted((first / "data" / "terrain.asc").generic_string())))
            << captured.err();
        ASSERT_TRUE(session.run("SAVE")) << captured.err();
    }
    std::filesystem::rename(first, moved);
    katana::app::Session reopened(nullptr);
    Captured captured;
    ASSERT_TRUE(reopened.run("OPEN " + quoted(moved.generic_string()))) << captured.err();
    const auto restored = recordsOf(captured.out(), "restored");
    ASSERT_EQ(restored.size(), 1u) << captured.out();
    EXPECT_EQ(field(restored[0], "layers"), "1") << captured.out();
    const auto listed = recordsOf(captured.out(), "reference");
    ASSERT_FALSE(listed.empty()) << captured.out();
    EXPECT_NE(field(listed[0], "file").find("moved.katana/data/terrain.asc"), std::string::npos)
        << captured.out();
}

} // namespace
