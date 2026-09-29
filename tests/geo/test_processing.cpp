// The GDAL algorithm bridge (include/katana/gis/processing.hpp,
// docs/geoprocessing.md "The bridge"): the catalogue, argument specs, runs
// with in-memory datasets, cancel, errors, staging and sidecars, each against
// a value worked out by hand and quoted beside it.

#include <algorithm>
#include <atomic>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <numbers>
#include <stop_token>
#include <string>
#include <thread>
#include <vector>

#include <gtest/gtest.h>

#include "katana/core/text.hpp"
#include "katana/gis/formats.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"

namespace {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::gis::GeoPoint;
using katana::gis::GeometryKind;
using katana::gis::VectorGeometry;

const std::string kData = KATANA_GEO_TEST_DATA;

// A folder of its own per test, removed afterwards.
class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-geo-" + name))
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

// A north-up grid of `cell` squares whose top-left corner is (0, height).
gp::RasterGrid grid(int width, int height, double cell, const auto& zOf)
{
    gp::RasterGrid out;
    out.info.width = width;
    out.info.height = height;
    out.info.bandCount = 1;
    out.info.geotransform = {0.0, cell, 0.0, height * cell, 0.0, -cell};
    out.info.hasGeotransform = true;
    std::vector<double> values;
    values.reserve(static_cast<std::size_t>(width) * static_cast<std::size_t>(height));
    for (int row = 0; row < height; ++row) {
        for (int column = 0; column < width; ++column) {
            values.push_back(zOf((column + 0.5) * cell, (height - row - 0.5) * cell));
        }
    }
    out.bands.push_back(std::move(values));
    out.noData.emplace_back(std::nullopt);
    return out;
}

double at(const gp::RasterGrid& raster, int column, int row)
{
    return raster.bands.front()[static_cast<std::size_t>(row) *
                                    static_cast<std::size_t>(raster.info.width) +
                                static_cast<std::size_t>(column)];
}

gp::FeatureTable table(const std::string& name, GeometryKind kind)
{
    gp::FeatureTable out;
    out.name = name;
    out.kind = kind;
    return out;
}

gp::Feature feature(GeometryKind kind, std::vector<GeoPoint> points, bool hasZ = false)
{
    VectorGeometry geometry;
    geometry.kind = kind;
    geometry.hasZ = hasZ;
    geometry.parts.push_back(std::move(points));
    gp::Feature out;
    out.parts.push_back(std::move(geometry));
    return out;
}

// Shoelace area of a polygon's exterior less its holes.
double area(const VectorGeometry& polygon)
{
    double total = 0.0;
    for (std::size_t r = 0; r < polygon.parts.size(); ++r) {
        const auto& ring = polygon.parts[r];
        double twice = 0.0;
        for (std::size_t i = 0; i < ring.size(); ++i) {
            const GeoPoint& a = ring[i];
            const GeoPoint& b = ring[(i + 1) % ring.size()];
            twice += a.x * b.y - b.x * a.y;
        }
        total += (r == 0 ? 1.0 : -1.0) * std::abs(twice) / 2.0;
    }
    return total;
}

gp::RunRequest request(std::vector<std::string> path)
{
    gp::RunRequest out;
    out.path = std::move(path);
    return out;
}

const gp::ArgSpec* argNamed(const gp::AlgorithmSpec& spec, const std::string& name)
{
    for (const gp::ArgSpec& arg : spec.args) {
        if (arg.name == name) {
            return &arg;
        }
    }
    return nullptr;
}

// ---- the catalogue --------------------------------------------------------------------------

// GDAL 3.13.2 as MSYS2 builds it has 121 leaf algorithms. One of them,
// `gdal driver pdf list-layers`, is registered by the PDF driver and exists
// only in a GDAL built with it: conda-forge's 3.13.2 has no PDF driver, and
// its own `gdal driver --help` lists no pdf subcommand, so it has 120.
std::size_t expectedLeaves()
{
    return katana::gis::findFormat("PDF") != nullptr ? 121u : 120u;
}

TEST(GdalCatalogue, HoldsEveryLeafFromTheRootIncludingTheDriverFamily)
{
    // 121 leaves in GDAL 3.13.2, walked from the root "gdal": the registry's
    // own top-level list stops at convert, dataset, info, mdim, pipeline,
    // raster, vector and vsi, and the driver family is missed.
    std::size_t leaves = 0;
    bool driverGpkgValidate = false;
    bool rasterGroup = false;
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        leaves += info.container ? 0u : 1u;
        driverGpkgValidate = driverGpkgValidate ||
                             info.path == std::vector<std::string>{"driver", "gpkg", "validate"};
        rasterGroup = rasterGroup || (info.container && info.path == std::vector<std::string>{"raster"});
    }
    EXPECT_GE(leaves, expectedLeaves());
    EXPECT_TRUE(driverGpkgValidate);
    EXPECT_TRUE(rasterGroup);
}

TEST(GdalCatalogue, AnAliasResolvesToItsAlgorithm)
{
    std::size_t consumed = 0;
    // "warp" is a hidden alias, "neighbours" a public one.
    auto warp = gp::resolve({"raster", "warp"}, consumed);
    ASSERT_TRUE(warp.ok()) << warp.error().describe();
    EXPECT_EQ(*warp, (std::vector<std::string>{"raster", "reproject"}));
    auto neighbours = gp::resolve({"raster", "neighbours"}, consumed);
    ASSERT_TRUE(neighbours.ok());
    EXPECT_EQ(*neighbours, (std::vector<std::string>{"raster", "neighbors"}));
    auto shouted = gp::resolve({"RASTER", "HillShade"}, consumed);
    ASSERT_TRUE(shouted.ok());
    EXPECT_EQ(*shouted, (std::vector<std::string>{"raster", "hillshade"}));
}

TEST(GdalCatalogue, ResolveTakesTheLongestPathAndLeavesTheRest)
{
    std::size_t consumed = 0;
    auto path = gp::resolve({"vector", "grid", "linear", "--resolution", "1,1"}, consumed);
    ASSERT_TRUE(path.ok());
    EXPECT_EQ(*path, (std::vector<std::string>{"vector", "grid", "linear"}));
    EXPECT_EQ(consumed, 3u);
}

TEST(GdalCatalogue, AContainerIsRefusedListingItsChildren)
{
    std::size_t consumed = 0;
    auto group = gp::resolve({"vector", "grid"}, consumed);
    ASSERT_FALSE(group.ok());
    EXPECT_EQ(group.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(group.error().message.find("linear"), std::string::npos) << group.error().message;
    EXPECT_NE(group.error().message.find("invdist"), std::string::npos);
    EXPECT_EQ(consumed, 2u);
}

TEST(GdalCatalogue, AnAlgorithmOfNoSuchNameIsNotFound)
{
    std::size_t consumed = 0;
    auto nothing = gp::resolve({"hillshade"}, consumed);
    ASSERT_FALSE(nothing.ok());
    EXPECT_EQ(nothing.error().code, ErrorCode::NotFound);
    EXPECT_EQ(consumed, 0u);
}

// ---- describing arguments ---------------------------------------------------------------------

TEST(GdalDescribe, ReportsBoundsWithTheirInclusivity)
{
    // GDAL 3.13: hillshade --altitude [0, 90]; contour --interval (0, ...).
    auto hillshade = gp::describe({"raster", "hillshade"});
    ASSERT_TRUE(hillshade.ok());
    const gp::ArgSpec* altitude = argNamed(*hillshade, "altitude");
    ASSERT_NE(altitude, nullptr);
    ASSERT_TRUE(altitude->min && altitude->max);
    EXPECT_EQ(altitude->min->value, 0.0);
    EXPECT_TRUE(altitude->min->inclusive);
    EXPECT_EQ(altitude->max->value, 90.0);
    EXPECT_TRUE(altitude->max->inclusive);

    auto contour = gp::describe({"raster", "contour"});
    ASSERT_TRUE(contour.ok());
    const gp::ArgSpec* interval = argNamed(*contour, "interval");
    ASSERT_NE(interval, nullptr);
    ASSERT_TRUE(interval->min);
    EXPECT_EQ(interval->min->value, 0.0);
    EXPECT_FALSE(interval->min->inclusive);
}

TEST(GdalDescribe, AMissingBoundIsAbsentNotZero)
{
    auto hillshade = gp::describe({"raster", "hillshade"});
    ASSERT_TRUE(hillshade.ok());
    const gp::ArgSpec* azimuth = argNamed(*hillshade, "azimuth");
    ASSERT_NE(azimuth, nullptr);
    EXPECT_FALSE(azimuth->min.has_value());
    EXPECT_FALSE(azimuth->max.has_value());
    ASSERT_TRUE(azimuth->defaultValue.has_value());
    EXPECT_EQ(std::get<double>(*azimuth->defaultValue), 315.0);
}

TEST(GdalDescribe, ArgumentsHiddenFromTheApiAreNotListed)
{
    auto hillshade = gp::describe({"raster", "hillshade"});
    ASSERT_TRUE(hillshade.ok());
    for (const char* hidden : {"help", "help-doc", "json-usage", "config", "progress"}) {
        EXPECT_EQ(argNamed(*hillshade, hidden), nullptr) << hidden;
    }
    const gp::ArgSpec* input = argNamed(*hillshade, "input");
    ASSERT_NE(input, nullptr);
    EXPECT_EQ(input->type, gp::ArgType::DatasetList);
    EXPECT_EQ(input->datasetKinds, static_cast<unsigned>(gp::DatasetKind::Raster));
    EXPECT_TRUE(input->acceptsName && input->acceptsObject);
    EXPECT_FALSE(hillshade->usageJson.empty());
    EXPECT_NE(hillshade->usageJson.find("input_arguments"), std::string::npos);
}

TEST(GdalDescribe, AnAlgorithmOfNoSuchPathIsNotFound)
{
    auto nothing = gp::describe({"raster", "no-such"});
    ASSERT_FALSE(nothing.ok());
    EXPECT_EQ(nothing.error().code, ErrorCode::NotFound);
}

// ---- runs, against values worked by hand ---------------------------------------------------------

TEST(GdalRun, HillshadeOfFlatGroundAtAltitude45Is181)
{
    // Flat ground: slope 0, so GDAL's shade is 1 + 254 * sin(altitude)
    // = 1 + 254 * sin 45 deg = 180.605, rounded to the nearest byte: 181.
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(grid(10, 10, 1.0, [](double, double) {
                                return 50.0;
                            })));
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->raster.has_value());
    EXPECT_EQ(outputs->raster->dataType, "Byte");
    EXPECT_EQ(at(*outputs->raster, 5, 5), 181.0);
    EXPECT_GE(outputs->seconds, 0.0);
}

TEST(GdalRun, SlopeOfAPlaneRisingOneInTwentyIsFivePercent)
{
    // plane.asc: z = 100 + 0.05 x, so the slope is 0.05 = 5 %. GDAL reads the
    // grid as Float32 (ulp 7.6e-6 near 100) and Horn's gradient sums six
    // values over 8 cell widths: its dz/dx is within 8 * 3.8e-6 / 8 = 3.8e-6
    // of 0.05, and the percent within 100 * 3.8e-6 = 3.8e-4. Hence 5e-4.
    gp::RunRequest run = request({"raster", "slope"});
    run.values.emplace_back("input", gp::DatasetValue(gp::DatasetPath{kData + "/plane.asc", {}, {}}));
    run.tokens = {"--unit=percent"};
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->raster.has_value());
    EXPECT_NEAR(at(*outputs->raster, 20, 15), 5.0, 5e-4);
}

TEST(GdalRun, AspectOfAPlaneFallingWestIs270Degrees)
{
    // The plane rises to the east and every row is the same, so dz/dy is
    // exactly 0 and the ground faces due west: azimuth 270. Float32 carries
    // 270 to 3e-5 (half an ulp 1.5e-5); 1e-4 allows the degree conversion.
    gp::RunRequest run = request({"raster", "aspect"});
    run.values.emplace_back("input", gp::DatasetValue(gp::DatasetPath{kData + "/plane.asc", {}, {}}));
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->raster.has_value());
    EXPECT_NEAR(at(*outputs->raster, 20, 15), 270.0, 1e-4);
}

TEST(GdalRun, BufferOfAHundredMetreLineOneMetreWithFlatCapsIs200SquareMetres)
{
    // Flat caps end the buffer at the line's ends: a 100 x 2 rectangle.
    gp::FeatureTable lines = table("lines", GeometryKind::LineString);
    lines.features.push_back(feature(GeometryKind::LineString, {{0, 0, 0}, {100, 0, 0}}));
    gp::RunRequest run = request({"vector", "buffer"});
    run.values.emplace_back("input", gp::DatasetValue(gp::FeatureSet{{lines}}));
    run.tokens = {"--distance=1", "--endcap-style=flat"};
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->features.has_value());
    ASSERT_EQ(outputs->features->tables.size(), 1u);
    ASSERT_EQ(outputs->features->tables.front().features.size(), 1u);
    const gp::Feature& buffer = outputs->features->tables.front().features.front();
    ASSERT_EQ(buffer.parts.size(), 1u);
    EXPECT_EQ(buffer.parts.front().kind, GeometryKind::Polygon);
    EXPECT_NEAR(area(buffer.parts.front()), 200.0, 1e-9);
}

TEST(GdalRun, IntersectionOfAFourMetreCorridorWithAFiftyMetreLotIs200SquareMetres)
{
    // lots.geojson: lots A (0,0)-(50,40) and B (50,0)-(100,40), and a
    // corridor 4 m wide across both, y 18 to 22: 50 x 4 = 200 m2 in each.
    const auto filtered = [](const char* kind) {
        gp::RunRequest run = request({"vector", "filter"});
        run.values.emplace_back("input",
                                gp::DatasetValue(gp::DatasetPath{kData + "/lots.geojson", {}, {}}));
        run.tokens = {"--where", std::string("kind = '") + kind + "'"};
        return gp::run(run);
    };
    auto lots = filtered("lot");
    auto corridor = filtered("corridor");
    ASSERT_TRUE(lots.ok() && corridor.ok());
    ASSERT_TRUE(lots->features && corridor->features);
    gp::RunRequest run = request({"vector", "layer-algebra"});
    run.values.emplace_back("input", gp::DatasetValue(*lots->features));
    run.values.emplace_back("method", gp::DatasetValue(*corridor->features));
    run.tokens = {"intersection"};
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->features.has_value());
    ASSERT_EQ(outputs->features->tables.size(), 1u);
    const auto& pieces = outputs->features->tables.front().features;
    ASSERT_EQ(pieces.size(), 2u);
    for (const gp::Feature& piece : pieces) {
        double total = 0.0;
        for (const VectorGeometry& part : piece.parts) {
            total += area(part);
        }
        EXPECT_NEAR(total, 200.0, 1e-9);
    }
}

TEST(GdalRun, ALinearGridOfPointsOnAPlaneReproducesThePlane)
{
    // Points every 10 m on z = 100 + x/10 + y/20. Linear interpolation over
    // a triangulation reproduces a plane, so the cell centred on (52.5, 47.5)
    // holds 100 + 5.25 + 2.375 = 107.625, in Float64 to rounding (1e-9).
    gp::FeatureTable points = table("points", GeometryKind::Point);
    points.hasZ = true;
    for (int i = 0; i <= 10; ++i) {
        for (int j = 0; j <= 10; ++j) {
            const double x = 10.0 * i;
            const double y = 10.0 * j;
            points.features.push_back(
                feature(GeometryKind::Point, {{x, y, 100.0 + x / 10.0 + y / 20.0}}, true));
        }
    }
    gp::RunRequest run = request({"vector", "grid", "linear"});
    run.values.emplace_back("input", gp::DatasetValue(gp::FeatureSet{{points}}));
    run.tokens = {"--extent=0,0,100,100", "--resolution=5,5"};
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->raster.has_value());
    const auto& g = outputs->raster->info.geotransform;
    const int column = static_cast<int>(std::floor((52.5 - g[0]) / g[1]));
    const int row = static_cast<int>(std::floor((47.5 - g[3]) / g[5]));
    // The cell's centre is the point asked about, so the hand value applies.
    EXPECT_DOUBLE_EQ(g[0] + (column + 0.5) * g[1], 52.5);
    EXPECT_DOUBLE_EQ(g[3] + (row + 0.5) * g[5], 47.5);
    EXPECT_NEAR(at(*outputs->raster, column, row), 107.625, 1e-9);
}

TEST(GdalRun, AnInfoAlgorithmPrintsItsText)
{
    gp::RunRequest run = request({"raster", "info"});
    run.values.emplace_back("input", gp::DatasetValue(gp::DatasetPath{kData + "/plane.asc", {}, {}}));
    run.tokens = {"--format=text"};
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->text.has_value());
    EXPECT_NE(outputs->text->find("Size is 40, 30"), std::string::npos) << *outputs->text;
    EXPECT_FALSE(outputs->raster.has_value());
}

// ---- cancel ---------------------------------------------------------------------------------

gp::RasterGrid rollingGround(int size)
{
    return grid(size, size, 1.0, [](double x, double y) {
        return 20.0 + 10.0 * std::sin(x * 0.02) * std::cos(y * 0.017);
    });
}

TEST(GdalRun, ACancelledRunIsCancelledEvenWhenGdalReportsSuccess)
{
    // raster hillshade returns true after its progress was refused, with a
    // partial output (measured): the run must still say "cancelled" and hand
    // back nothing.
    std::stop_source stop;
    int calls = 0;
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(600)));
    auto outputs = gp::run(run, stop.get_token(), [&](double) {
        if (++calls == 2) {
            stop.request_stop();
        }
    });
    ASSERT_FALSE(outputs.ok());
    EXPECT_EQ(outputs.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(outputs.error().message, "cancelled");
    EXPECT_GE(calls, 2);
}

TEST(GdalRun, AStopBeforeTheRunStartsRunsNothing)
{
    std::stop_source stop;
    stop.request_stop();
    int calls = 0;
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(50)));
    auto outputs = gp::run(run, stop.get_token(), [&](double) { ++calls; });
    ASSERT_FALSE(outputs.ok());
    EXPECT_EQ(outputs.error().message, "cancelled");
    EXPECT_EQ(calls, 0);
}

TEST(GdalRun, AViewshedCancelledOnGdalsOwnThreadsStillSaysCancelled)
{
    // The viewshed works on threads GDAL starts, whose errors skip a run's
    // own collector and reach the process-wide quiet handler. What the run
    // says must not depend on them.
    std::stop_source stop;
    int calls = 0;
    gp::RunRequest run = request({"raster", "viewshed"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(600)));
    run.tokens = {"--position=300,300", "--height=1.7"};
    auto outputs = gp::run(run, stop.get_token(), [&](double) {
        if (++calls == 2) {
            stop.request_stop();
        }
    });
    ASSERT_FALSE(outputs.ok());
    EXPECT_EQ(outputs.error().message, "cancelled");
}

// ---- refusals --------------------------------------------------------------------------------

TEST(GdalRun, AnUnknownArgumentIsRefusedByName)
{
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(10)));
    run.values.emplace_back("no-such-arg", gp::ArgValue(gp::Scalar(1.0)));
    auto outputs = gp::run(run);
    ASSERT_FALSE(outputs.ok());
    EXPECT_EQ(outputs.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(outputs.error().message.find("no-such-arg"), std::string::npos)
        << outputs.error().message;
}

TEST(GdalRun, AnUnknownOptionInGdalsWordsIsRefusedByName)
{
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(10)));
    run.tokens = {"--nosuch=2"};
    auto outputs = gp::run(run);
    ASSERT_FALSE(outputs.ok());
    EXPECT_EQ(outputs.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(outputs.error().message.find("nosuch"), std::string::npos) << outputs.error().message;
}

TEST(GdalRun, AValueOutsideTheChoicesIsRefusedWithGdalsMessage)
{
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(10)));
    run.values.emplace_back("variant", gp::ArgValue(gp::Scalar(std::string("bogus"))));
    auto outputs = gp::run(run);
    ASSERT_FALSE(outputs.ok());
    EXPECT_EQ(outputs.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(outputs.error().message.find("Should be one among"), std::string::npos)
        << outputs.error().message;
}

TEST(GdalRun, AValueBeyondItsBoundIsRefused)
{
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(10)));
    run.values.emplace_back("altitude", gp::ArgValue(gp::Scalar(100.0)));
    auto outputs = gp::run(run);
    ASSERT_FALSE(outputs.ok());
    EXPECT_EQ(outputs.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(outputs.error().message.find("90"), std::string::npos) << outputs.error().message;
}

TEST(GdalRun, TheConfigTokenIsRefused)
{
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(10)));
    run.tokens = {"--config", "GDAL_ENABLE_EXTERNAL=YES"};
    auto outputs = gp::run(run);
    ASSERT_FALSE(outputs.ok());
    EXPECT_EQ(outputs.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(outputs.error().context, "--config");
    run.tokens = {"--config=GDAL_ENABLE_EXTERNAL=YES"};
    EXPECT_FALSE(gp::run(run).ok());
}

TEST(GdalRun, OnlyAllowListedConfigurationIsSetForARun)
{
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(10)));
    run.config = {{"SPATIALITE_SECURITY", "relaxed"}};
    auto refused = gp::run(run);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().context, "SPATIALITE_SECURITY");
    run.config = {{"GDAL_NUM_THREADS", "2"}};
    EXPECT_TRUE(gp::run(run).ok());
}

TEST(GdalRun, APipelineWithAnExternalStepIsRefused)
{
    gp::RunRequest run = request({"pipeline"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(10)));
    run.tokens = {"read ! external --command \"calc.exe\" ! write"};
    auto byWords = gp::run(run);
    ASSERT_FALSE(byWords.ok());
    EXPECT_EQ(byWords.error().code, ErrorCode::InvalidArgument);
    run.tokens.clear();
    run.values.emplace_back("pipeline",
                            gp::ArgValue(gp::Scalar(std::string("read ! [ external ] ! write"))));
    EXPECT_FALSE(gp::run(run).ok());
}

TEST(GdalRun, AGroupIsRefusedListingWhatIsInIt)
{
    auto outputs = gp::run(request({"vector", "grid"}));
    ASSERT_FALSE(outputs.ok());
    EXPECT_EQ(outputs.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(outputs.error().message.find("linear"), std::string::npos);
}

// ---- staging, spilling, sidecars ---------------------------------------------------------------

TEST(GdalRun, ANameOnlyInputIsStagedAndRemoved)
{
    // raster calc takes its inputs by name only, so a grid in memory is
    // written under /vsimem/katana for it and removed when the run ends.
    // The builtin dialect's sum of one input is that input.
    auto calc = gp::describe({"raster", "calc"});
    ASSERT_TRUE(calc.ok());
    const gp::ArgSpec* input = argNamed(*calc, "input");
    ASSERT_NE(input, nullptr);
    ASSERT_TRUE(input->acceptsName && !input->acceptsObject) << "raster calc now takes objects";

    gp::RunRequest run = request({"raster", "calc"});
    run.values.emplace_back("input", gp::ArgValue(std::vector<gp::DatasetValue>{
                                         gp::DatasetValue(grid(8, 6, 1.0, [](double, double) {
                                             return 7.5;
                                         }))}));
    run.tokens = {"--dialect=builtin", "--calc=sum"};
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->raster.has_value());
    EXPECT_EQ(at(*outputs->raster, 3, 2), 7.5);
    EXPECT_EQ(gp::stagedDatasetCount(), 0u);
}

TEST(GdalRun, ARasterLargerThanTheMemoryLimitSpillsToATiledGeoTiff)
{
    TempDir folder("spill");
    gp::RunRequest run = request({"raster", "hillshade"});
    run.values.emplace_back("input", gp::DatasetValue(rollingGround(20)));
    run.maxMemoryCells = 399; // 20 x 20 x 1 = 400 cells
    run.spillDirectory = folder.path().generic_string();
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    EXPECT_FALSE(outputs->raster.has_value());
    ASSERT_TRUE(outputs->file.has_value());
    auto written = katana::gis::GdalDataset::open(*outputs->file);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ((*written)->driverName(), "GTiff");
    EXPECT_EQ((*written)->rasterInfo()->width, 20);
}

TEST(GdalRun, ARunOnAUserFileLeavesNoSidecarBesideIt)
{
    // raster info --stats makes GDAL write plane.asc.aux.xml when the file
    // closes; a person's folder must not gain one from a run that only read.
    TempDir folder("sidecar");
    std::filesystem::copy_file(kData + "/plane.asc", folder.path() / "plane.asc");
    gp::RunRequest run = request({"raster", "info"});
    run.values.emplace_back("input", gp::DatasetValue(gp::DatasetPath{folder.file("plane.asc"), {}, {}}));
    run.tokens = {"--stats"};
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->text.has_value());
    EXPECT_NE(outputs->text->find("STATISTICS_MEAN"), std::string::npos) << *outputs->text;
    EXPECT_FALSE(std::filesystem::exists(folder.path() / "plane.asc.aux.xml"));
    // The same through GDAL's own words, the file named in them.
    gp::RunRequest words = request({"raster", "info"});
    words.tokens = {"--stats", folder.file("plane.asc")};
    ASSERT_TRUE(gp::run(words).ok());
    EXPECT_FALSE(std::filesystem::exists(folder.path() / "plane.asc.aux.xml"));
}

TEST(GdalRun, APipelinesReadStepLeavesNoSidecarBesideItsFile)
{
    // A file a pipeline's read step names is bound to no dataset argument,
    // so it was not watched: info --stats left plane.asc.aux.xml beside it.
    TempDir folder("sidecar-pipeline");
    std::filesystem::copy_file(kData + "/plane.asc", folder.path() / "plane.asc");
    const std::string file = folder.file("plane.asc");
    const std::filesystem::path sidecar = folder.path() / "plane.asc.aux.xml";
    // One quoted text, and words one by one.
    for (const std::vector<std::string>& tokens : std::vector<std::vector<std::string>>{
             {"read " + file + " ! info --stats"}, {"read", file, "!", "info", "--stats"}}) {
        gp::RunRequest run = request({"raster", "pipeline"});
        run.tokens = tokens;
        auto outputs = gp::run(run);
        ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
        ASSERT_TRUE(outputs->text.has_value());
        EXPECT_NE(outputs->text->find("STATISTICS_MEAN"), std::string::npos) << *outputs->text;
        EXPECT_FALSE(std::filesystem::exists(sidecar)) << tokens.front();
    }
}

TEST(GdalRun, AFileSourcesLayerIsTheLayerOfItsOwnDataset)
{
    // like.gpkg holds two areas: decoy, far away, and m, the square 5..15.
    // Clipping (0,0) and (10,10) by like's layer m keeps (10,10) alone. The
    // layer was set as --input-layer, the points' own, which have no m.
    TempDir folder("layer-of-like");
    const auto area = [](double x0, double x1) {
        const std::string a = katana::core::formatExactReal(x0),
                          b = katana::core::formatExactReal(x1);
        return R"({"type":"FeatureCollection","features":[{"type":"Feature","properties":{},)"
               R"("geometry":{"type":"Polygon","coordinates":[[[)" +
               a + "," + a + "],[" + b + "," + a + "],[" + b + "," + b + "],[" + a + "," + b +
               "],[" + a + "," + a + "]]]}}]}";
    };
    std::ofstream(folder.path() / "decoy.geojson") << area(100, 101);
    std::ofstream(folder.path() / "m.geojson") << area(5, 15);
    std::ofstream(folder.path() / "points.geojson")
        << R"({"type":"FeatureCollection","features":[)"
        << R"({"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[0,0]}},)"
        << R"({"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[10,10]}}]})";
    const std::string like = folder.file("like.gpkg");
    for (const char* layer : {"decoy", "m"}) {
        gp::RunRequest made = request({"vector", "convert"});
        made.tokens = {folder.file(std::string(layer) + ".geojson"), like,
                       "--output-layer=" + std::string(layer)};
        if (std::string(layer) == "m") {
            made.tokens.push_back("--update");
        }
        ASSERT_TRUE(gp::run(made).ok()) << layer;
    }
    gp::RunRequest clip = request({"vector", "clip"});
    clip.values.emplace_back(
        "input", gp::DatasetValue(gp::DatasetPath{folder.file("points.geojson"), {}, {}}));
    clip.values.emplace_back("like", gp::DatasetValue(gp::DatasetPath{like, {}, "m"}));
    auto outputs = gp::run(clip);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->features.has_value());
    std::size_t kept = 0;
    for (const gp::FeatureTable& table : outputs->features->tables) {
        kept += table.features.size();
    }
    EXPECT_EQ(kept, 1u);
    // A dataset argument with no layer argument of its own refuses a layer
    // rather than giving it to another dataset: raster reproject's like has
    // none, and its input is a raster.
    gp::RunRequest refused = request({"raster", "reproject"});
    refused.values.emplace_back("input",
                                gp::DatasetValue(gp::DatasetPath{kData + "/plane.asc", {}, {}}));
    refused.values.emplace_back("like", gp::DatasetValue(gp::DatasetPath{like, {}, "m"}));
    auto no = gp::run(refused);
    ASSERT_FALSE(no.ok());
    EXPECT_EQ(no.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_EQ(no.error().context, "like");
}

TEST(GdalRun, ASidecarAlreadyBesideAFileIsReadAndLeftAsItWas)
{
    // Turning GDAL's sidecars off for a run would stop this one being READ
    // (measured): its metadata must reach the run, and the file stay as it was.
    TempDir folder("sidecar-kept");
    std::filesystem::copy_file(kData + "/plane.asc", folder.path() / "plane.asc");
    const std::string sidecar =
        "<PAMDataset><Metadata><MDI key=\"sidecar_mark\">kept</MDI></Metadata></PAMDataset>\n";
    std::ofstream(folder.path() / "plane.asc.aux.xml", std::ios::binary) << sidecar;
    gp::RunRequest run = request({"raster", "info"});
    run.values.emplace_back("input", gp::DatasetValue(gp::DatasetPath{folder.file("plane.asc"), {}, {}}));
    run.tokens = {"--stats"};
    auto outputs = gp::run(run);
    ASSERT_TRUE(outputs.ok()) << outputs.error().describe();
    ASSERT_TRUE(outputs->text.has_value());
    EXPECT_NE(outputs->text->find("sidecar_mark"), std::string::npos);
    std::ifstream in(folder.path() / "plane.asc.aux.xml", std::ios::binary);
    EXPECT_EQ(std::string(std::istreambuf_iterator<char>(in), {}), sidecar);
}

// ---- concurrency ------------------------------------------------------------------------------

TEST(GdalRun, ConcurrentRunsOnTheirOwnDatasetsMatchTheSingleThreadedResult)
{
    // One shared input gave 63 wrong results in 120 (measured); each run
    // makes its own, so 4 threads x 10 runs must all equal one run alone.
    const gp::RasterGrid ground = rollingGround(64);
    const auto shade = [&ground]() {
        gp::RunRequest run = request({"raster", "hillshade"});
        run.values.emplace_back("input", gp::DatasetValue(ground));
        run.tokens = {"--zfactor=2"};
        return gp::run(run);
    };
    auto reference = shade();
    ASSERT_TRUE(reference.ok() && reference->raster);
    std::atomic<int> identical{0};
    std::atomic<int> failed{0};
    std::vector<std::thread> threads;
    for (int t = 0; t < 4; ++t) {
        threads.emplace_back([&] {
            for (int i = 0; i < 10; ++i) {
                auto again = shade();
                if (!again.ok() || !again->raster) {
                    ++failed;
                } else if (again->raster->bands == reference->raster->bands) {
                    ++identical;
                }
            }
        });
    }
    for (std::thread& thread : threads) {
        thread.join();
    }
    EXPECT_EQ(failed.load(), 0);
    EXPECT_EQ(identical.load(), 40);
}

// ---- the tail's words -----------------------------------------------------------------------------

TEST(GdalArguments, WhatATailGivesIsReadFromItsPositionalsAndOptions)
{
    const std::vector<std::string> hillshade{"raster", "hillshade"};
    auto both = gp::argumentsGiven(hillshade, {"dem.tif", "shade.tif", "--zfactor", "2"}, {});
    ASSERT_TRUE(both.ok());
    EXPECT_NE(std::ranges::find(*both, std::string("input")), both->end());
    EXPECT_NE(std::ranges::find(*both, std::string("output")), both->end());
    EXPECT_NE(std::ranges::find(*both, std::string("zfactor")), both->end());
    // With the input bound, GDAL gives the one positional to the output.
    auto outputOnly = gp::argumentsGiven(hillshade, {"shade.tif"}, {"input"});
    ASSERT_TRUE(outputOnly.ok());
    EXPECT_EQ(*outputOnly, std::vector<std::string>{"output"});
    // An option's value is not a positional.
    auto options = gp::argumentsGiven(hillshade, {"-z", "2"}, {"input"});
    ASSERT_TRUE(options.ok());
    EXPECT_EQ(*options, std::vector<std::string>{"zfactor"});
}

TEST(GdalArguments, APipelineNamesItsInputAndOutputInItsReadAndWriteSteps)
{
    auto named = gp::argumentsGiven({"raster", "pipeline"}, {"read dem.tif ! slope ! write out.tif"}, {});
    ASSERT_TRUE(named.ok());
    EXPECT_NE(std::ranges::find(*named, std::string("input")), named->end());
    EXPECT_NE(std::ranges::find(*named, std::string("output")), named->end());
    auto bare = gp::argumentsGiven({"raster", "pipeline"}, {"read ! slope ! write"}, {"input"});
    ASSERT_TRUE(bare.ok());
    EXPECT_EQ(std::ranges::find(*bare, std::string("output")), bare->end());
}

TEST(GdalVersions, NameTheLibraryAndCountItsAlgorithms)
{
    const gp::Versions versions = gp::versions();
    EXPECT_TRUE(versions.gdal.starts_with("3.")) << versions.gdal;
    EXPECT_FALSE(versions.proj.empty());
    EXPECT_GT(versions.rasterDrivers, 0);
    EXPECT_GT(versions.vectorDrivers, 0);
    EXPECT_GE(static_cast<std::size_t>(versions.algorithms), expectedLeaves());
}

TEST(GdalWords, AValueIsWrittenAsGdalsCommandLineWritesIt)
{
    EXPECT_EQ(gp::toString(gp::Scalar(true)), "true");
    EXPECT_EQ(gp::toString(gp::Scalar(0.1)), "0.1");
    EXPECT_EQ(gp::toString(gp::Scalar(std::vector<int>{100, 50})), "100,50");
    EXPECT_EQ(gp::toString(gp::Scalar(std::vector<std::string>{"a", "b"})), "a,b");
    EXPECT_EQ(gp::datasetKindsText(gp::DatasetKind::Raster | gp::DatasetKind::Vector), "raster,vector");
}

} // namespace
