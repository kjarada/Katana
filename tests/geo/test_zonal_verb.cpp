// The RASTER ZONAL verb (src/katana_app/geo/zonal_verbs.cpp, docs/terrain.md
// "Statistics by area"): a raster's statistics inside each closed shape a
// scope takes, written on the shapes as <prefix>_<stat> properties in one
// undo step.
//
// Fixtures:
//   samples/gis/terrain.asc  120 x 90 cells of 1.5 m from (-5,-5), no no-data
//                            cell (checked by reading the text grid);
//   tests/geo/data/plane.asc 40 x 30 cells of 1 m from (0,0), z = 100 +
//                            0.05 x at the cell centres.
// With pixels=fractional (exactextract's method) a cell counts by the part
// of it a zone covers, so a count is an area in cells and a mean is weighted
// by those parts.

#include <cstdint>
#include <filesystem>
#include <fstream>
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
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

namespace geo = katana::app::geo;
namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

const std::string kPlane = std::string(KATANA_GEO_TEST_DATA) + "/plane.asc";
const std::string kTerrain = std::string(KATANA_GIS_SAMPLES) + "/terrain.asc";

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-zonal-verb-" + name))
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

class ZonalVerb : public ::testing::Test {
  protected:
    TempDir scratch{::testing::UnitTest::GetInstance()->current_test_info()->name()};
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, scratch.path(), {}, {}};

    std::string ran(const std::string& line)
    {
        auto reply = geo::runNow(context, line);
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

    // A closed rectangle on `layer`, drawn as a person draws it: its id.
    EntityId box(double x0, double y0, double x1, double y1, const std::string& layer = "0")
    {
        Entity entity;
        Polyline2 outline;
        outline.vertices = {{x0, y0}, {x1, y0}, {x1, y1}, {x0, y1}};
        outline.closed = true;
        entity.geometry = outline;
        entity.layer = layer;
        return draw(entity);
    }

    EntityId draw(const Entity& entity)
    {
        if (entity.layer != "0" && document.model().layers.find(entity.layer) == nullptr) {
            katana::entity::Layer made;
            made.name = entity.layer;
            EXPECT_TRUE(document.execute(katana::commands::createLayer(made)).ok());
        }
        EXPECT_TRUE(document.execute(katana::commands::createEntities({entity})).ok());
        const auto made = document.lastCreatedEntities();
        return made.empty() ? katana::entity::kInvalidEntityId : made.front();
    }

    std::optional<double> number(EntityId id, const std::string& key) const
    {
        const Entity* entity = document.model().entities.find(id);
        if (entity == nullptr) {
            return std::nullopt;
        }
        const auto found = entity->properties.find(key);
        if (found == entity->properties.end()) {
            return std::nullopt;
        }
        if (const auto* real = std::get_if<double>(&found->second)) {
            return *real;
        }
        if (const auto* whole = std::get_if<std::int64_t>(&found->second)) {
            return static_cast<double>(*whole);
        }
        return std::nullopt;
    }
};

// ---- the contract: what the verb binds of GDAL's ---------------------------------------------

TEST(ZonalContract, TheVerbStillFindsTheArgumentsItBinds)
{
    using katana::geo_test::expectArgument;
    expectArgument({"raster", "zonal-stats"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "zonal-stats"}, "zones", gp::ArgType::Dataset, true);
    expectArgument({"raster", "zonal-stats"}, "stat", gp::ArgType::StringList, true);
    expectArgument({"raster", "zonal-stats"}, "include-field", gp::ArgType::StringList, false);
    expectArgument({"raster", "zonal-stats"}, "pixels", gp::ArgType::String, false);
}

// ---- the statistics --------------------------------------------------------------------------

TEST_F(ZonalVerb, ZonalCountOfAFortyByThirtyBoxOnA1Point5MetreGridIs533Point333)
{
    // The box covers 40 x 30 = 1200 m2; each cell is 1.5 x 1.5 = 2.25 m2, so
    // fractional coverage counts 1200 / 2.25 = 533.333... cells. Its west
    // and south sides lie on cell edges (10 = -5 + 10 x 1.5), its east side
    // cuts a column of cells (50 = -5 + 36.67 x 1.5): the count is the same
    // either way, which is the point of fractional coverage. Tolerance: the
    // coverage fractions may be single precision (exactextract keeps them as
    // float), 6e-8 relative each over 533 cells: 3.2e-5; 1e-4 allows that.
    const EntityId lot = box(10, 10, 50, 40);
    const std::string reply = ran("RASTER ZONAL FILE \"" + kTerrain + "\" DRAWING stats=count");
    const auto count = number(lot, "zone_count");
    ASSERT_TRUE(count.has_value()) << reply;
    EXPECT_NEAR(*count, 1200.0 / 2.25, 1e-4);
    ASSERT_EQ(records(reply, "zone").size(), 1U) << reply;
    EXPECT_EQ(records(reply, "zone")[0].get("entity").value_or(""), std::to_string(lot));
}

TEST_F(ZonalVerb, ZonalMeanOnAPlaneIsThePlaneAtTheCentroid)
{
    // plane.asc is z = 100 + 0.05 x at the cell centres (x + 0.5 for column
    // x). The box from x = 10.25 to 30.25 covers column 10 by 0.75, columns
    // 11 to 29 whole and column 30 by 0.25; y from 5 to 25 is 20 whole rows.
    // The weighted mean of the centres' x:
    //   (0.75 x 10.5 + (11.5 + ... + 29.5) + 0.25 x 30.5) / (0.75 + 19 + 0.25)
    //   = (7.875 + 399.5 + 7.625) / 20 = 20.25,
    // the box's centroid, so the mean height is 100 + 0.05 x 20.25 = 101.0125.
    // The least centre inside is 10.5 (100.525), the greatest 30.5
    // (101.525); the count is 20 columns x 20 rows = 400.
    // Tolerance: plane.asc is read as Float32, each height within 3.815e-6
    // of its value near 100, and a weighted mean of them is too: 1e-5.
    const EntityId lot = box(10.25, 5, 30.25, 25);
    const std::string reply = ran("RASTER ZONAL FILE \"" + kPlane + "\" DRAWING");
    ASSERT_TRUE(number(lot, "zone_mean").has_value()) << reply;
    EXPECT_NEAR(*number(lot, "zone_mean"), 101.0125, 1e-5);
    EXPECT_NEAR(*number(lot, "zone_min"), 100.525, 1e-5);
    EXPECT_NEAR(*number(lot, "zone_max"), 101.525, 1e-5);
    EXPECT_NEAR(*number(lot, "zone_count"), 400.0, 1e-4);
    // The sum is the mean times the count: 101.0125 x 400 = 40405.
    EXPECT_NEAR(*number(lot, "zone_sum"), 40405.0, 400 * 1e-5);
    EXPECT_NE(reply.find("zonal stats=mean,min,max,count,sum prefix=zone pixels=fractional "
                         "zones=1"),
              std::string::npos)
        << reply;
}

TEST_F(ZonalVerb, AnOpenLineInTheScopeIsSkippedAndCounted)
{
    const EntityId lot = box(10, 5, 30, 25, "site");
    Entity open;
    Polyline2 line;
    line.vertices = {{2, 2}, {8, 8}};
    open.geometry = line;
    open.layer = "site";
    const EntityId path = draw(open);
    const std::string reply = ran("RASTER ZONAL FILE \"" + kPlane + "\" LAYERS site stats=mean");
    const auto zones = records(reply, "zones");
    ASSERT_EQ(zones.size(), 1U) << reply;
    EXPECT_EQ(zones[0].get("used").value_or(""), "1");
    EXPECT_EQ(zones[0].get("skipped.open").value_or(""), "1");
    EXPECT_FALSE(records(reply, "warning").empty()) << reply;
    // Columns 10 to 29 whole: centres 10.5 to 29.5, mean 20, height 101.
    EXPECT_NEAR(number(lot, "zone_mean").value_or(0.0), 101.0, 1e-5);
    EXPECT_FALSE(number(path, "zone_mean").has_value());
}

TEST_F(ZonalVerb, PropertiesAreOneUndoStep)
{
    const EntityId west = box(2, 2, 12, 12);
    const EntityId east = box(20, 2, 30, 12);
    const std::size_t before = document.history().undoCount();
    const std::string reply =
        ran("RASTER ZONAL FILE \"" + kPlane + "\" DRAWING stats=mean prefix=ground");
    EXPECT_EQ(document.history().undoCount(), before + 1) << reply;
    // Centres 2.5 .. 11.5: mean 7, height 100.35; 20.5 .. 29.5: 25, 101.25.
    EXPECT_NEAR(number(west, "ground_mean").value_or(0.0), 100.35, 1e-5);
    EXPECT_NEAR(number(east, "ground_mean").value_or(0.0), 101.25, 1e-5);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(number(west, "ground_mean").has_value());
    EXPECT_FALSE(number(east, "ground_mean").has_value());
}

TEST_F(ZonalVerb, AZoneOffTheRasterGetsNoMeanRatherThanZero)
{
    // plane.asc ends at x = 40: a box from 100 to 110 covers no cell, so it
    // has no mean - absent, not 0 - and its count, 0, is a count. The mean an
    // earlier run left on it goes: it was the mean of somewhere else.
    const EntityId away = box(100, 5, 110, 15);
    ASSERT_TRUE(document
                    .execute(katana::commands::setEntityProperty({away}, "zone_mean",
                                                                 katana::entity::PropertyValue(5.0)))
                    .ok());
    const std::string reply = ran("RASTER ZONAL FILE \"" + kPlane + "\" DRAWING stats=mean,count");
    EXPECT_FALSE(number(away, "zone_mean").has_value()) << reply;
    EXPECT_NEAR(number(away, "zone_count").value_or(-1.0), 0.0, 1e-12) << reply;
}

TEST_F(ZonalVerb, AZoneEditedWhileTheJobRanIsNotWrittenOver)
{
    const EntityId lot = box(10, 5, 30, 25);
    auto prepared = geo::prepare(context, "RASTER ZONAL FILE \"" + kPlane + "\" DRAWING");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    ASSERT_FALSE(prepared->reply.has_value());
    auto apply = prepared->work({}, {});
    ASSERT_TRUE(apply.ok()) << apply.error().describe();
    // Moved while the job ran: the statistics are of where it was.
    ASSERT_TRUE(document.execute(katana::commands::moveEntities({lot}, {1.0, 0.0})).ok());
    auto applied = (*apply)(context);
    ASSERT_FALSE(applied.ok());
    EXPECT_EQ(applied.error().code, ErrorCode::InvalidState);
    EXPECT_FALSE(number(lot, "zone_mean").has_value());
}

TEST_F(ZonalVerb, PreviewAScopeOfNoShapeAndACancelledRunChangeNothing)
{
    const EntityId lot = box(10, 5, 30, 25);
    const std::size_t before = document.history().undoCount();
    std::string reply = ran("RASTER ZONAL FILE \"" + kPlane + "\" DRAWING PREVIEW");
    EXPECT_NE(reply.find("preview valid=yes changed=no"), std::string::npos) << reply;
    // An empty layer: a scope that takes nothing is said, not refused.
    katana::entity::Layer empty;
    empty.name = "empty";
    ASSERT_TRUE(document.execute(katana::commands::createLayer(empty)).ok());
    const std::size_t afterLayer = document.history().undoCount();
    reply = ran("RASTER ZONAL FILE \"" + kPlane + "\" LAYERS empty");
    EXPECT_NE(reply.find("ran=no zones=0"), std::string::npos) << reply;
    auto prepared = geo::prepare(context, "RASTER ZONAL FILE \"" + kPlane + "\" DRAWING");
    ASSERT_TRUE(prepared.ok());
    std::stop_source stop;
    stop.request_stop();
    auto apply = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().message, "cancelled");
    EXPECT_EQ(document.history().undoCount(), afterLayer);
    EXPECT_EQ(afterLayer, before + 1);
    EXPECT_FALSE(number(lot, "zone_mean").has_value());
}

TEST_F(ZonalVerb, TheCsvHasARowPerZoneAndIsNotReplacedWithoutOverwrite)
{
    const EntityId lot = box(10, 5, 30, 25);
    const std::filesystem::path csv = scratch.path() / "zones.csv";
    const std::string line = "RASTER ZONAL FILE \"" + kPlane + "\" DRAWING stats=count csv=\"" +
                             csv.generic_string() + "\"";
    ran(line);
    std::ifstream in(csv);
    std::string header;
    std::string row;
    std::getline(in, header);
    std::getline(in, row);
    EXPECT_EQ(header, "entity,count");
    // The row names its zone.
    EXPECT_EQ(row.substr(0, row.find(',')), std::to_string(lot));
    auto again = geo::runNow(context, line);
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, ErrorCode::AlreadyExists);
    EXPECT_TRUE(geo::runNow(context, line + " OVERWRITE").ok());
}

TEST_F(ZonalVerb, WhatTheVerbCannotDoIsRefusedNamingIt)
{
    (void)box(10, 5, 30, 25);
    const std::string zonal = "RASTER ZONAL FILE \"" + kPlane + "\" DRAWING";
    for (const char* words : {" stats=values", " stats=mean,mean", " stats=", " prefix=a!b",
                              " pixels=middle", " colour=red"}) {
        auto reply = geo::runNow(context, zonal + words);
        ASSERT_FALSE(reply.ok()) << words;
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument) << words;
    }
    // A drawing is no raster to measure.
    auto drawing = geo::runNow(context, "RASTER ZONAL DRAWING");
    ASSERT_FALSE(drawing.ok());
    EXPECT_EQ(drawing.error().code, ErrorCode::InvalidArgument);
}

} // namespace
