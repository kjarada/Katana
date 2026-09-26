// IMPORT, EXPORT, INFO <file>, REFS and COPC as the one executor runs them
// (src/katana_app/geo/import_verb.cpp ... copc_verb.cpp; docs/interop.md,
// "IMPORT, EXPORT, INFO, REFS and COPC on every front end"): the records they
// reply with, what they add to the drawing and the session, and a cancel that
// changes nothing. Driven through the executor's context, as the window's
// workbench drives it, and through a Session, as katana_cli and katana_mcp do.
//
// The samples' facts, by hand:
//   samples/gis/parcels.geojson  8 features - two lots, two roads, three
//     control points and the spoil heaps, a MultiPolygon of two - so 9
//     entities; its coordinates span (180, 0) to (365, 165); EPSG:32630.
//   samples/gis/terrain.asc  ncols 120, nrows 90, cellsize 1.5, lower-left
//     corner (-5, -5): it spans (-5, -5) to (175, 130).
//   samples/gis/survey_scan.las  40 000 points (`pdal info`).
//   tests/archive12d/data/multiple_tins.12da  four tins of 9 points and 8
//     triangles each; TIN SOUTH WEST spans (554000, 6883000) to (554100,
//     6883100), heights 10.0 to 12.5.

#include <filesystem>
#include <iostream>
#include <memory>
#include <optional>
#include <sstream>
#include <stop_token>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "geo/geo_verbs.hpp"
#include "geo/gis_records.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "session.hpp"

namespace {

namespace geo = katana::app::geo;
using katana::core::ErrorCode;

const std::string kSamples = KATANA_GIS_SAMPLES;
// The 12d archive fixtures, beside the samples in the source tree.
const std::string kArchives = kSamples + "/../../tests/archive12d/data";

std::string quoted(const std::string& path)
{
    return "\"" + path + "\"";
}

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana gis verbs " + name))
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
    // What the folder holds, by name.
    [[nodiscard]] std::vector<std::string> names() const
    {
        std::vector<std::string> found;
        for (const auto& entry : std::filesystem::directory_iterator(path_)) {
            found.push_back(entry.path().filename().generic_string());
        }
        return found;
    }

  private:
    std::filesystem::path path_;
};

// A front end's side of the executor: a drawing, its interpreter, reference
// layers and surfaces, as the window and the session each hold them.
struct Front {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context;

    explicit Front(const std::filesystem::path& scratch)
        : context{document, interpreter, reference, surfaces, scratch, {}, {}}
    {
    }

    katana::core::Result<std::string> run(const std::string& line)
    {
        return geo::runNow(context, line);
    }
};

// The records of kind `kind` in a reply.
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

class GisVerbs : public ::testing::Test {
  protected:
    TempDir scratch{::testing::UnitTest::GetInstance()->current_test_info()->name()};
    Front front{scratch.path()};

    katana::core::Result<std::string> run(const std::string& line) { return front.run(line); }
    void type(const std::string& line) { ASSERT_TRUE(front.interpreter.run(line).ok()) << line; }
};

TEST_F(GisVerbs, ImportOfParcelsSaysWhatCameInAndWhereItWent)
{
    auto reply = run("IMPORT " + quoted(kSamples + "/parcels.geojson") + " LOCAL");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto records = geo::parseRecords(*reply);
    ASSERT_GE(records.size(), 2u) << *reply;
    // The imported record first, then where LOCAL put it: the lower-left
    // corner (180, 0) moved to 0,0, so everything by -180,0.
    EXPECT_EQ(records[0].kind, "imported");
    EXPECT_EQ(field(records[0], "kind"), "vector");
    EXPECT_EQ(field(records[0], "entities"), "9");
    EXPECT_EQ(field(records[0], "layers"), "1");
    EXPECT_EQ(field(records[0], "bounds"), "0,0,185,165");
    EXPECT_NE(field(records[0], "crs").find("EPSG:32630"), std::string::npos) << *reply;
    EXPECT_EQ(records[1].kind, "placed");
    EXPECT_EQ(field(records[1], "placement"), "local");
    EXPECT_EQ(field(records[1], "east"), "-180");
    EXPECT_EQ(field(records[1], "north"), "0");
    EXPECT_TRUE(field(records[1], "text").starts_with("LOCAL: moved as one piece by -180.000,0.000"));
    EXPECT_EQ(front.document.model().entities.size(), 9u);

    // The whole import is one undo step.
    type("UNDO");
    EXPECT_EQ(front.document.model().entities.size(), 0u);
}

TEST_F(GisVerbs, ExportAfterImportRoundTripsParcels)
{
    ASSERT_TRUE(run("IMPORT " + quoted(kSamples + "/parcels.geojson")).ok());
    const std::string file = scratch.file("round trip.gpkg");
    auto exported = run("EXPORT " + quoted(file));
    ASSERT_TRUE(exported.ok()) << exported.error().describe();
    const auto record = recordsOf(*exported, "exported");
    ASSERT_EQ(record.size(), 1u) << *exported;
    EXPECT_EQ(field(record[0], "file"), file);
    EXPECT_EQ(field(record[0], "kind"), "vector");
    EXPECT_EQ(field(record[0], "driver"), "GPKG");
    EXPECT_EQ(field(record[0], "features"), "9");
    EXPECT_EQ(field(record[0], "skipped"), "0");

    // Read back into a second drawing: the same 9 entities over the same
    // extent.
    Front again(scratch.path());
    auto imported = again.run("IMPORT " + quoted(file));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    const auto back = recordsOf(*imported, "imported");
    ASSERT_EQ(back.size(), 1u);
    EXPECT_EQ(field(back[0], "entities"), "9");
    EXPECT_EQ(field(back[0], "bounds"), "180,0,365,165");
    EXPECT_EQ(again.document.model().entities.size(), 9u);
}

TEST_F(GisVerbs, ImportOfAnArchiveAddsItsSurfacesToTheStore)
{
    auto reply = run("IMPORT " + quoted(kArchives + "/multiple_tins.12da"));
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto imported = recordsOf(*reply, "imported");
    ASSERT_EQ(imported.size(), 1u);
    EXPECT_EQ(field(imported[0], "kind"), "archive");
    EXPECT_EQ(field(imported[0], "entities"), "0");
    EXPECT_EQ(field(imported[0], "surfaces"), "4");

    // Kept, where a session once said it held none.
    ASSERT_EQ(front.surfaces.all().size(), 4u);
    const katana::terrain::NamedSurface* southWest = front.surfaces.find("TIN SOUTH WEST");
    ASSERT_NE(southWest, nullptr);
    EXPECT_EQ(southWest->surface->vertexCount(), 9u);
    EXPECT_EQ(southWest->surface->triangleCount(), 8u);
    const auto surfaces = recordsOf(*reply, "surface");
    ASSERT_EQ(surfaces.size(), 4u);
    EXPECT_EQ(field(surfaces[0], "name"), "TIN SOUTH WEST");
    EXPECT_EQ(field(surfaces[0], "triangles"), "8");
    EXPECT_EQ(field(surfaces[0], "points"), "9");
    EXPECT_EQ(field(surfaces[0], "bounds"), "554000,6883000,554100,6883100");
    EXPECT_EQ(field(surfaces[0], "zmin"), "10");
    EXPECT_EQ(field(surfaces[0], "zmax"), "12.5");

    // And a geoprocessing line finds it by name.
    auto bound = run("GDAL raster hillshade FROM SURFACE \"TIN SOUTH WEST\" CELL 10 PREVIEW");
    EXPECT_TRUE(bound.ok()) << bound.error().describe();

    // A second import of the same archive keeps both: the names are taken,
    // so the new ones are "name (2)".
    ASSERT_TRUE(run("IMPORT " + quoted(kArchives + "/multiple_tins.12da")).ok());
    EXPECT_EQ(front.surfaces.all().size(), 8u);
    EXPECT_NE(front.surfaces.find("TIN SOUTH WEST (2)"), nullptr);
}

TEST_F(GisVerbs, AnArchivesMeshesGoToTheFrontEndThatShowsThem)
{
    // all_geometries.12da holds one primitive_3d of 4 triangles and two
    // tins (tests/archive12d/data, its own comments).
    const std::string line = "IMPORT " + quoted(kArchives + "/all_geometries.12da");
    auto held = run(line);
    ASSERT_TRUE(held.ok()) << held.error().describe();
    const auto none = recordsOf(*held, "meshes");
    ASSERT_EQ(none.size(), 1u);
    EXPECT_EQ(field(none[0], "count"), "1");
    EXPECT_EQ(field(none[0], "held"), "no");
    EXPECT_NE(held->find("this session holds no meshes"), std::string::npos);

    Front window(scratch.path());
    std::optional<geo::ImportShown> shown;
    window.context.imported = [&shown](geo::ImportShown&& what) { shown = std::move(what); };
    auto kept = window.run(line);
    ASSERT_TRUE(kept.ok()) << kept.error().describe();
    ASSERT_TRUE(shown.has_value());
    ASSERT_EQ(shown->meshes.size(), 1u);
    EXPECT_EQ(shown->meshes.front().mesh.triangleCount(), 4u);
    EXPECT_EQ(shown->surfaces, 2u);
    EXPECT_EQ(field(recordsOf(*kept, "meshes").at(0), "held"), "yes");
    EXPECT_EQ(kept->find("this session holds no meshes"), std::string::npos);
}

TEST_F(GisVerbs, RefsListsWhatWasImportedAsRecords)
{
    auto empty = run("REFS");
    ASSERT_TRUE(empty.ok());
    EXPECT_EQ(*empty, "references rasters=0 clouds=0");

    ASSERT_TRUE(run("IMPORT " + quoted(kSamples + "/terrain.asc")).ok());
    ASSERT_TRUE(run("IMPORT " + quoted(kSamples + "/survey_scan.las")).ok());
    auto listed = run("REFS LIST");
    ASSERT_TRUE(listed.ok()) << listed.error().describe();
    const auto layers = recordsOf(*listed, "reference");
    ASSERT_EQ(layers.size(), 2u) << *listed;
    EXPECT_EQ(field(layers[0], "id"), "1");
    EXPECT_EQ(field(layers[0], "kind"), "raster");
    EXPECT_EQ(field(layers[0], "name"), "terrain");
    EXPECT_EQ(field(layers[0], "width"), "120");
    EXPECT_EQ(field(layers[0], "height"), "90");
    EXPECT_EQ(field(layers[0], "bounds"), "-5,-5,175,130");
    EXPECT_EQ(field(layers[0], "visible"), "yes");
    EXPECT_EQ(field(layers[1], "id"), "2");
    EXPECT_EQ(field(layers[1], "kind"), "pointcloud");
    EXPECT_EQ(field(layers[1], "points"), "40000");
    EXPECT_EQ(field(layers[1], "source_points"), "40000");
    const auto summary = recordsOf(*listed, "references");
    ASSERT_EQ(summary.size(), 1u);
    EXPECT_EQ(field(summary[0], "rasters"), "1");
    EXPECT_EQ(field(summary[0], "clouds"), "1");

    auto refused = run("REFS SHUFFLE");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
}

TEST_F(GisVerbs, CopcRecordsTheFileAndLeavesNothingBesideIt)
{
    const std::string destination = scratch.file("site scan.copc.laz");
    auto reply = run("COPC " + quoted(kSamples + "/survey_scan.las") + " " + quoted(destination));
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto converted = recordsOf(*reply, "converted");
    ASSERT_EQ(converted.size(), 1u) << *reply;
    EXPECT_EQ(field(converted[0], "file"), destination);
    EXPECT_EQ(field(converted[0], "format"), "copc");
    // The file, and only the file: the folder it was written in is gone.
    EXPECT_EQ(scratch.names(), std::vector<std::string>{"site scan.copc.laz"});

    auto described = run("INFO " + quoted(destination));
    ASSERT_TRUE(described.ok()) << described.error().describe();
    EXPECT_NE(described->find("Point cloud: 40,000 points"), std::string::npos) << *described;
    EXPECT_NE(described->find("COPC: yes"), std::string::npos) << *described;

    auto usage = run("COPC " + quoted(kSamples + "/survey_scan.las"));
    ASSERT_FALSE(usage.ok());
    EXPECT_NE(usage.error().message.find("usage: COPC <source> <destination.copc.laz>"),
              std::string::npos);
}

TEST_F(GisVerbs, ACancelledImportImportsNothing)
{
    const std::string line = "IMPORT " + quoted(kSamples + "/parcels.geojson");
    auto prepared = geo::prepare(front.context, line);
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    ASSERT_FALSE(prepared->reply.has_value());
    EXPECT_EQ(prepared->title, "IMPORT parcels.geojson");
    std::stop_source stop;
    stop.request_stop();
    auto apply = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(apply.error().message, "cancelled");

    // A job cancelled after its read never applies: the Apply it made is
    // dropped, and the drawing never sees what was read.
    auto again = geo::prepare(front.context, line);
    ASSERT_TRUE(again.ok());
    {
        auto dropped = again->work({}, {});
        ASSERT_TRUE(dropped.ok()) << dropped.error().describe();
    }
    EXPECT_EQ(front.document.model().entities.size(), 0u);
    EXPECT_TRUE(front.document.model().layers.contains("0"));
    EXPECT_FALSE(front.document.model().layers.contains("parcels"));
}

TEST_F(GisVerbs, ACancelledExportWritesNothing)
{
    type("RECT 0,0 10,5");
    const std::string file = scratch.file("out.geojson");
    auto prepared = geo::prepare(front.context, "EXPORT " + quoted(file));
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    {
        // Written by the work - beside its place - and dropped with the job.
        auto apply = prepared->work({}, {});
        ASSERT_TRUE(apply.ok()) << apply.error().describe();
        EXPECT_FALSE(std::filesystem::exists(file));
    }
    EXPECT_FALSE(std::filesystem::exists(file));
    EXPECT_TRUE(scratch.names().empty());

    std::stop_source stop;
    stop.request_stop();
    auto stopped = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(stopped.ok());
    EXPECT_EQ(stopped.error().message, "cancelled");
    EXPECT_TRUE(scratch.names().empty());

    // Applied, it is in place.
    auto apply = prepared->work({}, {});
    ASSERT_TRUE(apply.ok());
    auto reply = (*apply)(front.context);
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_TRUE(std::filesystem::exists(file));
}

TEST_F(GisVerbs, AnExportReplacesAFileOfTheSameName)
{
    const std::string file = scratch.file("replaced.geojson");
    type("RECT 0,0 10,5");
    ASSERT_TRUE(run("EXPORT " + quoted(file)).ok());
    type("CIRCLE 50,50 5");
    auto second = run("EXPORT " + quoted(file));
    ASSERT_TRUE(second.ok()) << second.error().describe();
    EXPECT_EQ(field(recordsOf(*second, "exported").at(0), "features"), "2");

    Front again(scratch.path());
    auto imported = again.run("IMPORT " + quoted(file));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    EXPECT_EQ(again.document.model().entities.size(), 2u);
    EXPECT_EQ(scratch.names(), std::vector<std::string>{"replaced.geojson"});
}

TEST_F(GisVerbs, TheFarApartQuestionDecidesWhereTheDataLands)
{
    // A drawing near the origin and tins some 6.9 million metres away:
    // merged as they are, one would be a dot at a zoom that shows both.
    const std::string line = "IMPORT " + quoted(kArchives + "/multiple_tins.12da");
    type("RECT 0,0 10,5");

    // Nobody to ask - a session: kept, and said what to type instead.
    auto kept = run(line);
    ASSERT_TRUE(kept.ok()) << kept.error().describe();
    const auto warnings = recordsOf(*kept, "warning");
    ASSERT_FALSE(warnings.empty());
    EXPECT_NE(field(warnings.back(), "text").find("IMPORT <file> LOCAL"), std::string::npos);
    EXPECT_TRUE(recordsOf(*kept, "placed").empty());
    EXPECT_EQ(front.surfaces.find("TIN SOUTH WEST")->surface->bounds().min.x, 554000.0);

    // The window's question, answered Cancel: nothing comes in.
    Front cancelled(scratch.path());
    ASSERT_TRUE(cancelled.interpreter.run("RECT 0,0 10,5").ok());
    cancelled.context.farApart = [](const katana::geometry::Box2&, const katana::geometry::Box2&,
                                    const std::string&) { return geo::FarApartChoice::Cancel; };
    auto refused = cancelled.run(line);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidState);
    EXPECT_TRUE(cancelled.surfaces.all().empty());

    // Answered Shift Alongside: the tins' lower-left corner (554000,
    // 6883000) onto the drawing's (0, 0), and the move said.
    Front alongside(scratch.path());
    ASSERT_TRUE(alongside.interpreter.run("RECT 0,0 10,5").ok());
    std::string advised;
    alongside.context.farApart = [&advised](const katana::geometry::Box2&,
                                            const katana::geometry::Box2& incoming,
                                            const std::string& advice) {
        advised = advice;
        EXPECT_EQ(incoming.min.x, 554000.0);
        return geo::FarApartChoice::Alongside;
    };
    auto moved = alongside.run(line);
    ASSERT_TRUE(moved.ok()) << moved.error().describe();
    EXPECT_FALSE(advised.empty());
    const auto placed = recordsOf(*moved, "placed");
    ASSERT_EQ(placed.size(), 1u) << *moved;
    EXPECT_EQ(field(placed[0], "placement"), "alongside");
    EXPECT_EQ(field(placed[0], "east"), "-554000");
    EXPECT_EQ(field(placed[0], "north"), "-6883000");
    const katana::geometry::Box2 box = alongside.surfaces.find("TIN SOUTH WEST")->surface->bounds();
    EXPECT_EQ(box.min.x, 0.0);
    EXPECT_EQ(box.min.y, 0.0);
    EXPECT_EQ(box.max.x, 100.0);
    EXPECT_EQ(box.max.y, 100.0);
}

TEST_F(GisVerbs, InfoOfAnEntityIsTheInterpretersAndOfAFileTheExecutors)
{
    EXPECT_FALSE(geo::handles("INFO #1"));
    EXPECT_FALSE(geo::handles("INFO 987654321")); // no file of that name here
    EXPECT_TRUE(geo::handles("INFO terrain.tif"));
    EXPECT_TRUE(geo::handles("INFO \"C:/a b/terrain.tif\""));
    EXPECT_TRUE(geo::handles("info")); // the executor's, to refuse with its usage
    auto usage = run("INFO");
    ASSERT_FALSE(usage.ok());
    EXPECT_EQ(usage.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(usage.error().message.find("INFO <file> | INFO <id>"), std::string::npos);

    auto described = run("INFO " + kSamples + "/terrain.asc");
    ASSERT_TRUE(described.ok()) << described.error().describe();
    EXPECT_NE(described->find("Raster: 120 x 90 pixels, 1 band"), std::string::npos) << *described;
    EXPECT_NE(described->find("Bounds: (-5.000, -5.000) to (175.000, 130.000)"), std::string::npos);
}

TEST_F(GisVerbs, ImportRefusesWhatItCannotRead)
{
    auto usage = run("IMPORT");
    ASSERT_FALSE(usage.ok());
    EXPECT_EQ(usage.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(usage.error().message.find("usage: IMPORT <file>"), std::string::npos);

    auto unknown = run("IMPORT notes.xyzzy");
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::Unsupported);
    EXPECT_NE(unknown.error().message.find("no importer for '.xyzzy'"), std::string::npos);

    auto placed = run("IMPORT " + quoted(kSamples + "/terrain.asc") + " LOCAL");
    ASSERT_FALSE(placed.ok());
    EXPECT_EQ(placed.error().message,
              "LOCAL is not supported for rasters and point clouds, which are reference data "
              "drawn at their own coordinates");

    auto missing = run("IMPORT " + quoted(scratch.file("absent.geojson")));
    ASSERT_FALSE(missing.ok());
    EXPECT_TRUE(front.document.model().entities.empty());
}

TEST_F(GisVerbs, ADxfComesInAndGoesOutInRecords)
{
    // A 30 x 20 rectangle, a circle and a text on a nested layer, out and
    // back: three entities, three layers (0, Survey, Survey/Kerb).
    type("LAYER NEW Survey/Kerb #FF0000");
    type("LAYER SET Survey/Kerb");
    type("RECT 0,0 30,20");
    type("CIRCLE 15,10 5");
    type("TEXT 5,5 2.5 \"LOT 42\"");
    const std::string file = scratch.file("site plan.dxf");
    auto exported = run("EXPORT " + quoted(file));
    ASSERT_TRUE(exported.ok()) << exported.error().describe();
    const auto out = recordsOf(*exported, "exported");
    ASSERT_EQ(out.size(), 1u) << *exported;
    EXPECT_EQ(field(out[0], "kind"), "dxf");
    EXPECT_EQ(field(out[0], "format"), "DXF R2000");
    EXPECT_EQ(field(out[0], "entities"), "3");
    EXPECT_EQ(field(out[0], "layers"), "3");

    Front again(scratch.path());
    auto imported = again.run("IMPORT " + quoted(file) + " OFFSET=10,-20.5");
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    const auto in = recordsOf(*imported, "imported");
    ASSERT_EQ(in.size(), 1u) << *imported;
    EXPECT_EQ(field(in[0], "kind"), "dxf");
    EXPECT_EQ(field(in[0], "entities"), "3");
    EXPECT_EQ(field(in[0], "bounds"), "10,-20.5,40,-0.5");
    EXPECT_EQ(field(recordsOf(*imported, "placed").at(0), "placement"), "offset");
    EXPECT_EQ(recordsOf(*imported, "tally").size(), 3u); // LWPOLYLINE, CIRCLE, TEXT
}

TEST(GisRecords, ARecordReadsAsJsonWithItsNumbersAndItsWords)
{
    const auto records = geo::parseRecords(
        "reference id=12 kind=raster name=12 width=120 bounds=-5,-5,175,130 visible=yes "
        "opacity=0.5 file= role=imagery");
    ASSERT_EQ(records.size(), 1u);
    const nlohmann::json json = geo::recordJson(records.front());
    EXPECT_EQ(json["record"], "reference");
    EXPECT_EQ(json["id"], 12);
    EXPECT_EQ(json["name"], "12"); // a name, whatever it holds
    EXPECT_EQ(json["width"], 120);
    EXPECT_EQ(json["bounds"], nlohmann::json({-5.0, -5.0, 175.0, 130.0}));
    EXPECT_EQ(json["visible"], true);
    EXPECT_EQ(json["opacity"], 0.5);
    EXPECT_EQ(json["file"], ""); // a word, empty
    EXPECT_EQ(json["role"], "imagery");
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

TEST(GisSession, TheSessionRepliesWithTheExecutorsOwnRecords)
{
    // One IMPORT line, through a Session and through the executor as the
    // window's workbench runs it: the same text, record for record.
    const std::string line = "IMPORT " + quoted(kSamples + "/parcels.geojson") + " LOCAL";
    const TempDir scratch("session");
    Front window(scratch.path());
    auto direct = window.run(line);
    ASSERT_TRUE(direct.ok()) << direct.error().describe();

    katana::app::Session session(nullptr);
    Captured captured;
    ASSERT_TRUE(session.run(line));
    EXPECT_EQ(captured.out(), *direct + "\n");
    EXPECT_EQ(captured.err(), "");
    EXPECT_EQ(session.document().model().entities.size(), 9u);

    // HELP says the verbs once, from the executor's table.
    const std::string help = katana::app::Session::helpText();
    EXPECT_NE(help.find("IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]"), std::string::npos);
    EXPECT_NE(help.find("REFS [LIST]"), std::string::npos);
    EXPECT_EQ(help.find("Interop   IMPORT"), std::string::npos);
}

TEST(GisSession, ARefusalGoesToStderrAndChangesNothing)
{
    katana::app::Session session(nullptr);
    Captured captured;
    EXPECT_FALSE(session.run("IMPORT " + quoted(kSamples + "/terrain.asc") + " OFFSET=1,2"));
    EXPECT_EQ(captured.out(), "");
    EXPECT_EQ(captured.err(), "error: InvalidArgument: OFFSET is not supported for rasters and "
                              "point clouds, which are reference data drawn at their own "
                              "coordinates\n");
    EXPECT_TRUE(session.document().model().entities.empty());
}

} // namespace
