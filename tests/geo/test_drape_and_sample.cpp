// RASTER SAMPLE and DRAPE (src/katana_app/geo/drape_verbs.cpp,
// src/katana_cad/geo/drape.cpp, src/katana_io/geo/raster_sampling.cpp;
// docs/terrain.md "Sampling and drape"): the ground's height at points,
// reported, and given to the drawing's points and vertices in one undo
// step; off the ground, no height - never 0.
//
// Fixture: tests/geo/data/plane.asc, 40 x 30 cells of 1 m from (0,0),
// z = 100 + 0.05 x at the cell centres. Bilinear interpolation of a linear
// function is the function itself, so between any four cell centres the
// sample is exactly the plane: z(x, y) = 100 + 0.05 x. The file is read as
// Float32, each height within 3.815e-6 of its value near 100 (half an ulp,
// 2^-17), and a bilinear sample is a convex combination of four of them:
// within 3.815e-6 as well. 1e-5 allows that.

#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/geo/drape.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/model.hpp"
#include "katana/gis/raster_sampling.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace {

namespace geo = katana::app::geo;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

const std::string kPlane = std::string(KATANA_GEO_TEST_DATA) + "/plane.asc";
constexpr double kFloat32 = 1e-5;

double plane(double x)
{
    return 100.0 + 0.05 * x;
}

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-drape-sample-" + name))
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

class DrapeAndSample : public ::testing::Test {
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

    static std::optional<double> numberIn(const geo::Record& record, const std::string& key)
    {
        const auto text = record.get(key);
        return text ? katana::core::parseFiniteDouble(*text) : std::nullopt;
    }

    EntityId draw(const Entity& entity)
    {
        EXPECT_TRUE(document.execute(katana::commands::createEntities({entity})).ok());
        const auto made = document.lastCreatedEntities();
        return made.empty() ? katana::entity::kInvalidEntityId : made.front();
    }

    EntityId polyline(std::vector<Point2> vertices)
    {
        Entity entity;
        Polyline2 line;
        line.vertices = std::move(vertices);
        entity.geometry = line;
        return draw(entity);
    }

    std::vector<std::optional<double>> heights(EntityId id, std::size_t count) const
    {
        const Entity* entity = document.model().entities.find(id);
        return entity == nullptr ? std::vector<std::optional<double>>{}
                                 : katana::entity::heightsOf(entity->properties, count);
    }

    // Two triangles folded along the diagonal from (0,0) to (10,10):
    //   below it (y < x), through (0,0,10) (10,0,10) (10,10,20): z = 10 + y;
    //   above it (y > x), through (0,0,10) (10,10,20) (0,10,10): z = 10 + x.
    void addFold()
    {
        auto tin = katana::terrain::TinSurface::create(
            {{0, 0, 10}, {10, 0, 10}, {10, 10, 20}, {0, 10, 10}}, {{0, 1, 2}, {0, 2, 3}});
        ASSERT_TRUE(tin.ok()) << tin.error().describe();
        ASSERT_TRUE(surfaces
                        .add({"fold",
                              std::make_shared<const katana::terrain::TinSurface>(
                                  std::move(tin).value()),
                              "test"})
                        .ok());
    }
};

// ---- the sampler itself -----------------------------------------------------------------------

TEST(RasterSampler, BilinearOnAPlaneIsThePlaneAndOffItIsNothing)
{
    auto sampler = katana::gis::RasterSampler::open(kPlane);
    ASSERT_TRUE(sampler.ok()) << sampler.error().describe();
    const auto bilinear = katana::gis::Resampling::Bilinear;
    EXPECT_NEAR((*sampler)->at(12.3, 7.7, bilinear).value_or(0.0), plane(12.3), kFloat32);
    // Nearest: the cell's centre value, column 12's: 100 + 0.05 x 12.5.
    EXPECT_NEAR((*sampler)->at(12.3, 7.7, katana::gis::Resampling::Nearest).value_or(0.0),
                plane(12.5), kFloat32);
    EXPECT_FALSE((*sampler)->at(-0.5, 10, bilinear).has_value());
    EXPECT_FALSE((*sampler)->at(40.5, 10, bilinear).has_value());
    EXPECT_FALSE((*sampler)->at(10, 30.5, bilinear).has_value());
    EXPECT_DOUBLE_EQ((*sampler)->cellSize(), 1.0);
}

TEST(RasterSampler, AMissingFileAndAnUnknownMethodAreRefused)
{
    auto missing = katana::gis::RasterSampler::open(std::filesystem::path(kPlane + ".nothing"));
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    EXPECT_FALSE(katana::gis::resamplingNamed("lanczos").ok());
    EXPECT_EQ(*katana::gis::resamplingNamed("CubicSpline"), katana::gis::Resampling::CubicSpline);
}

// ---- RASTER SAMPLE ----------------------------------------------------------------------------

TEST_F(DrapeAndSample, SampleOnAPlaneIsExactWithBilinear)
{
    const std::string reply =
        ran("RASTER SAMPLE FILE \"" + kPlane + "\" AT 12.3,7.7 AT 30.05,20.5");
    const auto samples = records(reply, "sample");
    ASSERT_EQ(samples.size(), 2U) << reply;
    // 100 + 0.05 x 12.3 = 100.615; 100 + 0.05 x 30.05 = 101.5025.
    EXPECT_EQ(samples[0].get("at").value_or(""), "12.3,7.7");
    EXPECT_NEAR(numberIn(samples[0], "z").value_or(0.0), 100.615, kFloat32);
    EXPECT_NEAR(numberIn(samples[1], "z").value_or(0.0), 101.5025, kFloat32);
    EXPECT_NE(reply.find("samples method=bilinear count=2 on=2 off=0"), std::string::npos) << reply;
    EXPECT_EQ(document.history().undoCount(), 0U);
}

TEST_F(DrapeAndSample, APointOffTheDataHasNoHeightNotZero)
{
    const std::string reply = ran("RASTER SAMPLE FILE \"" + kPlane + "\" AT 50,10 AT 10,10");
    const auto samples = records(reply, "sample");
    ASSERT_EQ(samples.size(), 2U) << reply;
    EXPECT_FALSE(samples[0].get("z").has_value()) << reply;
    EXPECT_EQ(samples[0].get("ground").value_or(""), "no");
    EXPECT_NEAR(numberIn(samples[1], "z").value_or(0.0), plane(10), kFloat32);
    EXPECT_NE(reply.find("count=2 on=1 off=1"), std::string::npos) << reply;
    EXPECT_FALSE(records(reply, "warning").empty());
}

TEST_F(DrapeAndSample, SampleReadsTheScopesPointsAndSaysWhatElseItTook)
{
    Entity mark;
    mark.geometry = katana::entity::PointGeometry{{20, 5}};
    const EntityId id = draw(mark);
    (void)polyline({{1, 1}, {2, 2}});
    const std::string reply = ran("RASTER SAMPLE FILE \"" + kPlane + "\" DRAWING");
    const auto samples = records(reply, "sample");
    ASSERT_EQ(samples.size(), 1U) << reply;
    EXPECT_EQ(samples[0].get("entity").value_or(""), std::to_string(id));
    EXPECT_NEAR(numberIn(samples[0], "z").value_or(0.0), plane(20), kFloat32);
    const auto scope = records(reply, "scope");
    ASSERT_EQ(scope.size(), 1U);
    EXPECT_EQ(scope[0].get("points").value_or(""), "1");
    EXPECT_EQ(scope[0].get("skipped.polyline").value_or(""), "1");
}

// ---- DRAPE ------------------------------------------------------------------------------------

TEST_F(DrapeAndSample, DrapeOfAPolylineSetsEveryVertexHeight)
{
    // 100 + 0.05 x: 5.5 -> 100.275, 20.25 -> 101.0125, 35 -> 101.75.
    const EntityId line = polyline({{5.5, 5}, {20.25, 10}, {35, 25}});
    const std::size_t before = document.history().undoCount();
    const std::string reply = ran("DRAPE FILE \"" + kPlane + "\" DRAWING");
    EXPECT_EQ(document.history().undoCount(), before + 1) << reply;
    const auto z = heights(line, 3);
    ASSERT_EQ(z.size(), 3U);
    ASSERT_TRUE(z[0] && z[1] && z[2]) << reply;
    EXPECT_NEAR(*z[0], 100.275, kFloat32);
    EXPECT_NEAR(*z[1], 101.0125, kFloat32);
    EXPECT_NEAR(*z[2], 101.75, kFloat32);
    EXPECT_NE(reply.find("drape method=bilinear entities=1 vertices=3 off=0"), std::string::npos)
        << reply;
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(heights(line, 3)[0].has_value());
}

TEST_F(DrapeAndSample, DrapeFromASurfaceUsesTheTinNotAGrid)
{
    // On the fold, the point (5.2, 5) is below the diagonal: z = 10 + 5 =
    // 15 exactly. A grid of the surface at 1 m, sampled bilinearly, would
    // not give it: its centres (4.5,4.5) 14.5, (5.5,4.5) 14.5, (4.5,5.5)
    // 14.5 and (5.5,5.5) 15.5 give 14.5 + 0.7 x 0.5 x 1 = 14.85. The line
    // from (2,6) (above: 10 + 2 = 12) to (8,3) (below: 10 + 3 = 13) is
    // exact too. The TIN's barycentric arithmetic is exact to rounding:
    // 1e-9.
    addFold();
    Entity mark;
    mark.geometry = katana::entity::PointGeometry{{5.2, 5.0}};
    const EntityId point = draw(mark);
    Entity segment;
    segment.geometry = katana::geometry::Segment2{{2, 6}, {8, 3}};
    const EntityId line = draw(segment);
    const std::string reply = ran("DRAPE SURFACE fold DRAWING");
    EXPECT_NE(reply.find("input arg=input source=surface name=fold read=triangles"),
              std::string::npos)
        << reply;
    EXPECT_NE(reply.find("drape method=triangles"), std::string::npos) << reply;
    EXPECT_NEAR(heights(point, 1)[0].value_or(0.0), 15.0, 1e-9);
    const auto ends = heights(line, 2);
    EXPECT_NEAR(ends[0].value_or(0.0), 12.0, 1e-9);
    EXPECT_NEAR(ends[1].value_or(0.0), 13.0, 1e-9);
}

TEST_F(DrapeAndSample, AVertexOffTheGroundIsLeftWithoutAHeightAndCounted)
{
    // plane.asc ends at x = 40: the second vertex has no ground under it.
    const EntityId line = polyline({{35, 10}, {45, 10}});
    const std::string reply = ran("DRAPE FILE \"" + kPlane + "\" DRAWING");
    const auto z = heights(line, 2);
    ASSERT_EQ(z.size(), 2U);
    EXPECT_NEAR(z[0].value_or(0.0), plane(35), kFloat32);
    EXPECT_FALSE(z[1].has_value());
    EXPECT_NE(reply.find("vertices=2 off=1 entities_off=0"), std::string::npos) << reply;
    EXPECT_FALSE(records(reply, "warning").empty());
}

TEST_F(DrapeAndSample, ASecondDrapeOnTheSameGroundChangesNothing)
{
    (void)polyline({{5, 5}, {15, 5}});
    ran("DRAPE FILE \"" + kPlane + "\" DRAWING");
    const std::size_t after = document.history().undoCount();
    const std::string reply = ran("DRAPE FILE \"" + kPlane + "\" DRAWING");
    EXPECT_EQ(document.history().undoCount(), after) << reply;
    EXPECT_NE(reply.find("updated=0"), std::string::npos) << reply;
    EXPECT_NE(reply.find("unchanged=1"), std::string::npos) << reply;
}

TEST_F(DrapeAndSample, WhatHasNoVerticesIsCountedAndLeft)
{
    Entity ring;
    ring.geometry = katana::geometry::Circle2{{10, 10}, 3};
    const EntityId circle = draw(ring);
    (void)polyline({{5, 5}, {15, 5}});
    const std::string reply = ran("DRAPE FILE \"" + kPlane + "\" DRAWING");
    EXPECT_NE(reply.find("used=1 skipped.circle=1"), std::string::npos) << reply;
    EXPECT_TRUE(document.model().entities.find(circle)->properties.empty());
}

TEST_F(DrapeAndSample, AnEntityEditedWhileTheJobRanIsNotWrittenOver)
{
    const EntityId line = polyline({{5, 5}, {15, 5}});
    auto prepared = geo::prepare(context, "DRAPE FILE \"" + kPlane + "\" DRAWING");
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    auto apply = prepared->work({}, {});
    ASSERT_TRUE(apply.ok()) << apply.error().describe();
    ASSERT_TRUE(document.execute(katana::commands::moveEntities({line}, {0.0, 2.0})).ok());
    auto applied = (*apply)(context);
    ASSERT_FALSE(applied.ok());
    EXPECT_EQ(applied.error().code, ErrorCode::InvalidState);
    EXPECT_FALSE(heights(line, 2)[0].has_value());
}

TEST_F(DrapeAndSample, WhatTheVerbsCannotDoIsRefusedNamingIt)
{
    addFold();
    (void)polyline({{5, 5}, {15, 5}});
    for (const std::string& line : std::vector<std::string>{
             "DRAPE SURFACE fold DRAWING method=cubic", "DRAPE SURFACE fold CELL 1 DRAWING",
          "DRAPE DRAWING", "DRAPE FILE \"" + kPlane + "\" DRAWING method=lanczos",
          "RASTER SAMPLE FILE \"" + kPlane + "\" AT 10", "RASTER SAMPLE FILE \"" + kPlane + "\" AT",
          "RASTER SAMPLE SURFACE nothing AT 1,1"}) {
        auto reply = geo::runNow(context, line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_TRUE(reply.error().code == ErrorCode::InvalidArgument ||
                    reply.error().code == ErrorCode::NotFound)
            << line << ": " << reply.error().describe();
    }
}

// ---- the domain edit, without GDAL --------------------------------------------------------------

TEST(Drape, HeightsAreComputedFirstAndWrittenAsOneCommand)
{
    katana::cad::Document document;
    Entity entity;
    Polyline2 line;
    line.vertices = {{0, 0}, {10, 0}, {20, 0}};
    entity.geometry = line;
    ASSERT_TRUE(document.execute(katana::commands::createEntities({entity})).ok());
    const EntityId id = document.lastCreatedEntities().front();
    const Entity copy = *document.model().entities.find(id);
    // Ground rising 1 in 10 east, and nothing beyond x = 15.
    const katana::cad::geo::HeightAt ground = [](const Point2& at) -> std::optional<double> {
        return at.x <= 15.0 ? std::optional<double>(at.x / 10.0) : std::nullopt;
    };
    const auto heights = katana::cad::geo::drapeHeights({copy}, ground);
    ASSERT_EQ(heights.entities.size(), 1U);
    EXPECT_EQ(heights.vertices, 3U);
    EXPECT_EQ(heights.verticesOff, 1U);
    auto made = katana::cad::geo::drapeCommand(document.model(), heights.entities, "DRAPE test");
    ASSERT_NE(made.command, nullptr);
    EXPECT_EQ(made.changed, 1U);
    ASSERT_TRUE(document.execute(std::move(made.command)).ok());
    const auto z = katana::entity::heightsOf(document.model().entities.find(id)->properties, 3);
    EXPECT_DOUBLE_EQ(z[0].value_or(-1.0), 0.0);
    EXPECT_DOUBLE_EQ(z[1].value_or(-1.0), 1.0);
    EXPECT_FALSE(z[2].has_value());
}

} // namespace
