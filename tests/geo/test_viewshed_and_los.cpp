// RASTER VIEWSHED and LOS (src/katana_app/geo/viewshed_verbs.cpp,
// src/katana_terrain/line_of_sight.cpp; docs/terrain.md "Viewshed and line
// of sight").
//
// Every fixture is flat ground at 0 with, where there is one, a wall: the
// hand-worked shadow is then similar triangles. Curvature is switched off
// (curvature=none) wherever a shadow's edge is worked by hand, since GDAL
// lowers the ground by 0.85714 d^2 / 12 741 994 m otherwise - 7e-6 m at
// 10 m, but not zero, and the expectation must be the true one.
//
// The wall fixture: 60 x 31 cells of 1 m from (0,0), 0 everywhere but the
// column x = 20..21 (centre 20.5), which is 1 m high. An observer at the
// centre (0.5, 15.5) with an eye 1.7 m up sees over the wall top, 1 m high
// 20 m away; the sight grazing it falls 0.7 m in 20 m and meets the ground
// at 20 x 1.7 / 0.7 = 48.571 m. So along the observer's row the cells 21 to
// 48 m out (centres 21.5 to 48.5) are hidden, and those 49 m out and more
// (49.5 on) are seen again. The margins either side are 0.02 m (48 m out:
// 1.7 - 0.035 x 48 = 0.02 above the ground) and 0.015 m (49 m: 0.015
// below), far beyond rounding. GDAL computes the cells on the observer's
// own row from the cell before them on that row, so the row is exact.

#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
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
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/line_of_sight.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

namespace geo = katana::app::geo;
namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::geometry::Point2;
using katana::terrain::GroundAt;
using katana::terrain::lineOfSight;
using katana::terrain::SightOptions;

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-viewshed-" + name))
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
    out << "ncols " << columns << "\nnrows " << rows << "\nxllcorner 0\nyllcorner 0\ncellsize 1\n";
    for (int row = 0; row < rows; ++row) {
        for (int column = 0; column < columns; ++column) {
            out << height(column, row) << (column + 1 < columns ? " " : "\n");
        }
    }
    return path;
}

class Viewshed : public ::testing::Test {
  protected:
    TempDir scratch{::testing::UnitTest::GetInstance()->current_test_info()->name()};
    TempDir files{std::string(::testing::UnitTest::GetInstance()->current_test_info()->name()) +
                  "-files"};
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

    std::string wall()
    {
        return grid(files, "wall.asc", 60, 31, [](int column, int) { return column == 20 ? 1 : 0; });
    }

    // The last viewshed kept: whether the cell whose centre is (x, y) is
    // seen; none when the raster does not reach it.
    struct Kept {
        katana::gis::RasterInfo info;
        std::vector<double> values;
        [[nodiscard]] std::optional<bool> seen(double x, double y) const
        {
            const auto& gt = info.geotransform;
            const auto column = static_cast<long long>(std::floor((x - gt[0]) / gt[1]));
            const auto row = static_cast<long long>(std::floor((y - gt[3]) / gt[5]));
            if (column < 0 || row < 0 || column >= info.width || row >= info.height) {
                return std::nullopt;
            }
            return values[static_cast<std::size_t>(row * info.width + column)] == 1.0;
        }
    };
    [[nodiscard]] Kept kept() const
    {
        Kept out;
        EXPECT_FALSE(reference.rasters().empty());
        if (reference.rasters().empty()) {
            return out;
        }
        auto opened = katana::gis::GdalDataset::open(reference.rasters().back().source);
        EXPECT_TRUE(opened.ok());
        if (!opened.ok()) {
            return out;
        }
        out.info = *(*opened)->rasterInfo();
        out.values = *(*opened)->readBand(1);
        return out;
    }
};

// ---- the contract: what the verb binds of GDAL's ---------------------------------------------

TEST(ViewshedContract, TheVerbStillFindsTheArgumentsItBinds)
{
    using katana::geo_test::expectArgument;
    expectArgument({"raster", "viewshed"}, "input", gp::ArgType::DatasetList, true);
    expectArgument({"raster", "viewshed"}, "position", gp::ArgType::RealList, false);
    expectArgument({"raster", "viewshed"}, "height", gp::ArgType::Real, false);
    expectArgument({"raster", "viewshed"}, "target-height", gp::ArgType::Real, false);
    expectArgument({"raster", "viewshed"}, "max-distance", gp::ArgType::Real, false);
    // The curvature the hand-worked shadows switch off.
    expectArgument({"raster", "viewshed"}, "curvature-coefficient", gp::ArgType::Real, false);
    expectArgument({"raster", "viewshed"}, "visible-value", gp::ArgType::Real, false);
    expectArgument({"raster", "polygonize"}, "attribute-name", gp::ArgType::String, false);
}

// ---- RASTER VIEWSHED ----------------------------------------------------------------------------

TEST_F(Viewshed, OnFlatGroundEveryCellWithinMaxDistanceIsVisible)
{
    // Flat ground seen from 1.7 m up: each cell further out lies at a
    // steeper angle below the eye than any nearer one, so nothing hides
    // anything, and every cell within max= is seen. The edge at 10 m is
    // GDAL's (whether a cell 10 m out counts as within); the assertions
    // keep a metre clear of it.
    const std::string flat =
        grid(files, "flat.asc", 41, 41, [](int, int) { return 10; });
    const std::string reply = ran("RASTER VIEWSHED FILE \"" + flat +
                                  "\" OBSERVER 20.5,20.5 max=10 curvature=none");
    const Kept view = kept();
    ASSERT_GT(view.info.width, 0) << reply;
    std::size_t near = 0;
    for (int row = 0; row < 41; ++row) {
        for (int column = 0; column < 41; ++column) {
            const double x = column + 0.5;
            const double y = row + 0.5;
            const double d = std::hypot(x - 20.5, y - 20.5);
            if (d <= 9.0) {
                ++near;
                EXPECT_EQ(view.seen(x, y), std::optional<bool>(true)) << x << "," << y;
            } else if (d > 11.0) {
                EXPECT_NE(view.seen(x, y), std::optional<bool>(true)) << x << "," << y;
            }
        }
    }
    const auto summary = records(reply, "viewshed");
    ASSERT_EQ(summary.size(), 1U) << reply;
    const auto cells = katana::core::parseInteger(summary[0].get("visible_cells").value_or(""));
    ASSERT_TRUE(cells.has_value());
    EXPECT_GE(static_cast<std::size_t>(*cells), near);
    // 1 m cells: the area is the count.
    EXPECT_EQ(summary[0].get("area").value_or(""), std::to_string(*cells) + ".000");
    EXPECT_EQ(reference.rasters().back().name, "flat-viewshed");
    EXPECT_EQ(reference.rasters().back().role, katana::interop::RasterRole::Derived);
}

TEST_F(Viewshed, BehindARidgeTheShadowEndsWhereSimilarTrianglesSay)
{
    const std::string reply = ran("RASTER VIEWSHED FILE \"" + wall() +
                                  "\" OBSERVER 0.5,15.5 height=1.7 target=0 curvature=none");
    const Kept view = kept();
    ASSERT_GT(view.info.width, 0) << reply;
    // 20 x 1.7 / 0.7 = 48.571 m: hidden 21 to 48 m out, seen from 49 m.
    for (int d = 0; d < 60; ++d) {
        const double x = 0.5 + d;
        const bool hidden = d >= 21 && d <= 48;
        EXPECT_EQ(view.seen(x, 15.5), std::optional<bool>(!hidden)) << "x = " << x;
    }
}

TEST_F(Viewshed, TwoObserversGiveTheUnionOfTheirViewsheds)
{
    // From the east end, (59.5, 15.5), the wall is 39 m off: its grazing
    // sight meets the ground 39 x 1.7 / 0.7 = 94.7 m out, past the raster,
    // so everything west of the wall (centres 0.5 to 19.5) is hidden from
    // there. From the west end the cells 21.5 to 48.5 are hidden. Each sees
    // what the other cannot: together, the whole row.
    const std::string raster = wall();
    ran("RASTER VIEWSHED FILE \"" + raster + "\" OBSERVER 59.5,15.5 curvature=none NAME east");
    const Kept east = kept();
    EXPECT_EQ(east.seen(10.5, 15.5), std::optional<bool>(false));
    EXPECT_EQ(east.seen(30.5, 15.5), std::optional<bool>(true));
    const std::string reply = ran("RASTER VIEWSHED FILE \"" + raster +
                                  "\" OBSERVER 0.5,15.5 OBSERVER 59.5,15.5 curvature=none");
    EXPECT_EQ(records(reply, "observer").size(), 2U) << reply;
    const Kept both = kept();
    for (int d = 0; d < 60; ++d) {
        EXPECT_EQ(both.seen(0.5 + d, 15.5), std::optional<bool>(true)) << "x = " << 0.5 + d;
    }
}

TEST_F(Viewshed, ObserversFromTheScopeAreItsPointsAndTheRestIsCounted)
{
    Entity west;
    west.geometry = katana::entity::PointGeometry{{0.5, 15.5}};
    Entity east;
    east.geometry = katana::entity::PointGeometry{{59.5, 15.5}};
    Entity line;
    line.geometry = katana::geometry::Segment2{{1, 1}, {2, 2}};
    ASSERT_TRUE(document.execute(katana::commands::createEntities({west, east, line})).ok());
    const std::string reply =
        ran("RASTER VIEWSHED FILE \"" + wall() + "\" OBSERVERS DRAWING curvature=none");
    const auto scope = records(reply, "scope");
    ASSERT_EQ(scope.size(), 1U) << reply;
    EXPECT_EQ(scope[0].get("points").value_or(""), "2");
    EXPECT_EQ(scope[0].get("skipped.line").value_or(""), "1");
    const auto observers = records(reply, "observer");
    ASSERT_EQ(observers.size(), 2U);
    EXPECT_TRUE(observers[0].get("entity").has_value());
    EXPECT_EQ(kept().seen(30.5, 15.5), std::optional<bool>(true));
}

TEST_F(Viewshed, AreasDrawTheVisibleAreaInOneStepAndTheReplyMeasuresIt)
{
    const std::string flat = grid(files, "flat.asc", 41, 41, [](int, int) { return 10; });
    const std::size_t before = document.history().undoCount();
    const std::string reply = ran("RASTER VIEWSHED FILE \"" + flat +
                                  "\" OBSERVER 20.5,20.5 max=10 curvature=none areas=site/view");
    EXPECT_EQ(document.history().undoCount(), before + 1) << reply;
    EXPECT_GE(document.model().entities.countOnLayer("site/view"), 1U);
    bool drawn = false;
    for (const geo::Record& output : records(reply, "output")) {
        drawn = drawn || output.get("arg") == std::optional<std::string>("areas");
    }
    EXPECT_TRUE(drawn) << reply;
}

TEST_F(Viewshed, WhatTheVerbCannotDoIsRefusedNamingIt)
{
    const std::string raster = "RASTER VIEWSHED FILE \"" + wall() + "\"";
    for (const std::string& words :
         {std::string(""), std::string(" OBSERVER 5"), std::string(" OBSERVER 1,1 OBSERVERS"),
          std::string(" OBSERVER 1,1 DRAWING"), std::string(" OBSERVER 1,1 height=-1"),
          std::string(" OBSERVER 1,1 curvature=lots"), std::string(" OBSERVER 1,1 max=0"),
          std::string(" OBSERVER 1,1 areas=/bad")}) {
        auto reply = geo::runNow(context, raster + words);
        ASSERT_FALSE(reply.ok()) << words;
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument) << words;
    }
    // Off the 60 x 31 m raster: refused by the work, which knows its extent.
    auto off = geo::runNow(context, raster + " OBSERVER 100,100");
    ASSERT_FALSE(off.ok());
    EXPECT_EQ(off.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(reference.rasters().empty());
}

TEST_F(Viewshed, PreviewAndACancelledRunMakeNothing)
{
    const std::string raster = wall();
    const std::string reply =
        ran("RASTER VIEWSHED FILE \"" + raster + "\" OBSERVER 0.5,15.5 PREVIEW");
    EXPECT_NE(reply.find("preview valid=yes changed=no"), std::string::npos) << reply;
    auto prepared = geo::prepare(context, "RASTER VIEWSHED FILE \"" + raster + "\" OBSERVER 0.5,15.5");
    ASSERT_TRUE(prepared.ok());
    std::stop_source stop;
    stop.request_stop();
    auto apply = prepared->work(stop.get_token(), {});
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().message, "cancelled");
    EXPECT_TRUE(reference.rasters().empty());
    EXPECT_TRUE(std::filesystem::is_empty(scratch.path()));
}

// ---- LOS, the verb -----------------------------------------------------------------------------

TEST_F(Viewshed, LosOverARasterWallIsBlockedAtTheWall)
{
    // Bilinear between centres, the wall rises from 0 at x = 19.5 to 1 at
    // 20.5. From the eye 1.7 m over (0.5, 15.5) to the ground at (30.5,
    // 15.5) the sight is 1.7 x (1 - (x - 0.5) / 30) high: at x = 20 it is
    // 0.595 over ground 0.5 (clear), at 20.5 it is 0.567 under ground 1
    // (hidden). The stations are half a cell apart from the observer, so
    // 20.5 is the first one hidden.
    const std::string reply = ran("LOS FILE \"" + wall() +
                                  "\" OBSERVER 0.5,15.5 TARGET 30.5,15.5 curvature=none");
    const auto sight = records(reply, "sight");
    ASSERT_EQ(sight.size(), 1U) << reply;
    EXPECT_EQ(sight[0].get("visible").value_or(""), "no");
    const std::string blocked = sight[0].get("blocked_at").value_or("");
    const auto comma = blocked.find(',');
    ASSERT_NE(comma, std::string::npos) << reply;
    EXPECT_NEAR(katana::core::parseFiniteDouble(blocked.substr(0, comma)).value_or(0.0), 20.5, 1e-9);
    EXPECT_NEAR(katana::core::parseFiniteDouble(blocked.substr(comma + 1)).value_or(0.0), 15.5, 1e-9);
    EXPECT_EQ(sight[0].get("step").value_or(""), "0.5");
    EXPECT_EQ(sight[0].get("distance").value_or(""), "30.000");
    EXPECT_EQ(document.history().undoCount(), 0U);
}

TEST_F(Viewshed, LosNeedsBothEndsAndReadsNoScope)
{
    const std::string los = "LOS FILE \"" + wall() + "\"";
    for (const std::string& words :
         {std::string(" OBSERVER 1,1"), std::string(" TARGET 1,1"),
          std::string(" OBSERVER 1,1 TARGET 2,2 DRAWING"),
          std::string(" OBSERVER 1,1 OBSERVER 2,2 TARGET 3,3"),
          std::string(" OBSERVER 1,1 TARGET 2,2 step=0")}) {
        auto reply = geo::runNow(context, los + words);
        ASSERT_FALSE(reply.ok()) << words;
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument) << words;
    }
}

// ---- line of sight, native ---------------------------------------------------------------------

TEST(LineOfSight, LosOnFlatGroundIsVisibleWithClearanceEqualToTheObserverHeight)
{
    // Ground at 50; the eye and the aim both 1.7 m up: the sight is level,
    // 1.7 m over every station, exactly.
    const GroundAt ground = [](const Point2&) { return std::optional<double>(50.0); };
    SightOptions options;
    options.observerHeight = 1.7;
    options.targetHeight = 1.7;
    auto line = lineOfSight(ground, {0, 0}, {100, 0}, options);
    ASSERT_TRUE(line.ok()) << line.error().describe();
    EXPECT_TRUE(line->visible);
    EXPECT_DOUBLE_EQ(line->distance, 100.0);
    EXPECT_DOUBLE_EQ(line->observerZ, 51.7);
    ASSERT_TRUE(line->clearance.has_value());
    EXPECT_NEAR(*line->clearance, 1.7, 1e-12);
    EXPECT_EQ(line->stations, 99U);
    EXPECT_FALSE(line->blockedAt.has_value());
}

TEST(LineOfSight, LosOverTheRidgeIsBlockedAtTheRidge)
{
    // A 5 m ridge from x = 19.4 to 20.6; the eye 1.7 m over flat ground at
    // 0, the target on the ground 40 m off. Stations every 0.25 m: the first
    // on the ridge is x = 19.5, where the sight is 1.7 x (1 - 19.5 / 40) =
    // 0.87125 high, 4.12875 under the ridge top. The least clearance is at
    // the far side, 20.5 (the last station on it): 1.7 x (1 - 20.5 / 40) - 5
    // = -4.17125.
    const GroundAt ground = [](const Point2& at) {
        return std::optional<double>(at.x >= 19.4 && at.x <= 20.6 ? 5.0 : 0.0);
    };
    SightOptions options;
    options.step = 0.25;
    auto line = lineOfSight(ground, {0, 0}, {40, 0}, options);
    ASSERT_TRUE(line.ok());
    EXPECT_FALSE(line->visible);
    ASSERT_TRUE(line->blockedAt.has_value());
    EXPECT_NEAR(line->blockedAt->x, 19.5, 1e-12);
    EXPECT_NEAR(*line->blockedDistance, 19.5, 1e-12);
    EXPECT_DOUBLE_EQ(*line->blockedGround, 5.0);
    EXPECT_NEAR(*line->clearance, -4.17125, 1e-12);
    EXPECT_NEAR(line->clearanceAt->x, 20.5, 1e-12);
}

TEST(LineOfSight, BehindAFenceTheTargetIsSeenAgainWhereSimilarTrianglesSay)
{
    // A 1 m fence from x = 19.5 to 20.5, the eye 1.7 m up at 0, targets on
    // the ground. The sight grazing the fence's far top edge, (20.5, 1),
    // meets the ground at 20.5 x 1.7 / 0.7 = 49.786: a target at 49 m is
    // hidden, one at 50.5 m seen (its sight clears the edge by
    // 1.7 x (1 - 20.5 / 50.5) - 1 = 0.0099 m, the station nearest the edge
    // by more, being nearer the eye).
    const GroundAt ground = [](const Point2& at) {
        return std::optional<double>(at.x >= 19.5 && at.x <= 20.5 ? 1.0 : 0.0);
    };
    SightOptions options;
    options.step = 0.1;
    auto near = lineOfSight(ground, {0, 0}, {49, 0}, options);
    ASSERT_TRUE(near.ok());
    EXPECT_FALSE(near->visible);
    auto far = lineOfSight(ground, {0, 0}, {50.5, 0}, options);
    ASSERT_TRUE(far.ok());
    EXPECT_TRUE(far->visible);
}

TEST(LineOfSight, TheEarthsCurveHidesALowTargetTenKilometresAway)
{
    // Flat ground at 0, the eye 2 m up, the target on the ground L = 10 km
    // off, De = 12 741 994 m. Without curvature the sight is clear all the
    // way. With a coefficient of 1 the ground d m out sinks d^2 / De - at
    // the target L^2 / De = 7.848 m - and the clearance d m out is
    //   C(d) = 2 (1 - d / L) - d (L - d) / De,
    // which is 0 where (d - L)(d - 2 De / L) = 0: hidden from
    // 2 De / L = 2548.399 m on, so the first station hidden (10 m apart) is
    // 2550. C is least where C'(d) = 0, at L / 2 + De / L = 6274.199; of
    // the stations the nearest below, 6270, is the least (C(6270) =
    // -1.089435, C(6280) = -1.089434).
    const double length = 10000.0;
    const double diameter = katana::terrain::kEarthDiameter;
    const GroundAt ground = [](const Point2&) { return std::optional<double>(0.0); };
    SightOptions options;
    options.observerHeight = 2.0;
    options.step = 10.0;
    auto flat = lineOfSight(ground, {0, 0}, {length, 0}, options);
    ASSERT_TRUE(flat.ok());
    EXPECT_TRUE(flat->visible);
    options.curvature = 1.0;
    auto curved = lineOfSight(ground, {0, 0}, {length, 0}, options);
    ASSERT_TRUE(curved.ok());
    EXPECT_FALSE(curved->visible);
    EXPECT_NEAR(curved->targetZ, -length * length / diameter, 1e-9);
    EXPECT_NEAR(*curved->blockedDistance, 2550.0, 1e-9);
    const double least = 6270.0;
    EXPECT_NEAR(*curved->clearance,
                2.0 * (1.0 - least / length) - least * (length - least) / diameter, 1e-9);
    EXPECT_NEAR(curved->clearanceAt->x, least, 1e-9);
}

TEST(LineOfSight, StationsOffTheGroundHideNothingAndAreCounted)
{
    // No ground between 10 and 20 m: unknown there, never a wall at 0 -
    // nor a pit that would make the clearance look larger than it is.
    const GroundAt ground = [](const Point2& at) {
        return at.x > 10.0 && at.x < 20.0 ? std::nullopt : std::optional<double>(0.0);
    };
    SightOptions options;
    options.targetHeight = 1.7;
    auto line = lineOfSight(ground, {0, 0}, {30, 0}, options);
    ASSERT_TRUE(line.ok());
    EXPECT_TRUE(line->visible);
    EXPECT_EQ(line->stations, 29U);
    // Stations at 11 .. 19 m.
    EXPECT_EQ(line->unknown, 9U);
    EXPECT_NEAR(*line->clearance, 1.7, 1e-12);
}

TEST(LineOfSight, CoincidentEndsAnEndOffTheGroundAndABadStepAreRefused)
{
    const GroundAt ground = [](const Point2& at) {
        return at.x < 100.0 ? std::optional<double>(0.0) : std::nullopt;
    };
    SightOptions options;
    EXPECT_EQ(lineOfSight(ground, {1, 1}, {1, 1}, options).error().code, ErrorCode::InvalidArgument);
    auto off = lineOfSight(ground, {0, 0}, {150, 0}, options);
    ASSERT_FALSE(off.ok());
    EXPECT_NE(off.error().message.find("target"), std::string::npos);
    auto eye = lineOfSight(ground, {150, 0}, {0, 0}, options);
    ASSERT_FALSE(eye.ok());
    EXPECT_NE(eye.error().message.find("observer"), std::string::npos);
    options.step = 0.0;
    EXPECT_FALSE(lineOfSight(ground, {0, 0}, {10, 0}, options).ok());
    options.step = 1.0;
    options.curvature = -1.0;
    EXPECT_FALSE(lineOfSight(ground, {0, 0}, {10, 0}, options).ok());
}

} // namespace
