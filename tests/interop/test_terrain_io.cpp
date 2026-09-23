// Terrain to and from GDAL and PDAL data (PLAN.MD Phases 14, 17 and 20):
// the elevations of a DEM, the ground of a point cloud, and a surface written
// out as a DEM.
//
// Real files through GDAL and PDAL, as everywhere in this suite: the formats'
// own rules (a pixel's centre, an ASCII grid read back as Float32, a no-data
// value in a header) are exactly what needs checking. Every expected value is
// worked out by hand from inputs exact in binary.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cfloat>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <limits>
#include <string>
#include <vector>

#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/terrain_io.hpp"
#include "katana/terrain/tin_builder.hpp"

using namespace katana::interop;
using katana::core::ErrorCode;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Point3;
namespace gis = katana::gis;

namespace {

// A directory of its own per test, removed afterwards, so the suite can run in
// parallel with itself and leaves nothing behind.
class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-terrainio-" + name))
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

    [[nodiscard]] std::filesystem::path file(const std::string& name) const
    {
        return path_ / name;
    }

  private:
    std::filesystem::path path_;
};

// EPSG:28356, GDA94 / MGA zone 56, as the EPSG registry publishes it in WKT1
// (epsg.io/28356.wkt). The same text as the adapter tests use.
constexpr const char* kMga56Wkt =
    R"(PROJCS["GDA94 / MGA zone 56",GEOGCS["GDA94",DATUM["Geocentric_Datum_of_Australia_1994",)"
    R"(SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],)"
    R"(TOWGS84[0,0,0,0,0,0,0],AUTHORITY["EPSG","6283"]],PRIMEM["Greenwich",0,)"
    R"(AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],)"
    R"(AUTHORITY["EPSG","4283"]],PROJECTION["Transverse_Mercator"],)"
    R"(PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",153],)"
    R"(PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],)"
    R"(PARAMETER["false_northing",10000000],UNIT["metre",1,AUTHORITY["EPSG","9001"]],)"
    R"(AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","28356"]])";

} // namespace

// ---- strideForCap (audit QT-24) --------------------------------------------------------------

TEST(TerrainIoStride, TheStrideIsTheSmallestThatMeetsTheCap)
{
    // The audit's case: 1200 x 999 = 1 198 800 pixels against 400 000. The
    // GUI's floor(sqrt(ceil(1 198 800 / 400 000))) = floor(sqrt 3) = 1 kept
    // every pixel. Stride 2 keeps 600 x 500 = 300 000, which is under the cap.
    EXPECT_EQ(strideForCap(1200, 999, 400'000), 2);
    // Its other case: 2000 x 2000 = 4 M. Stride 3 keeps 667 x 667 = 444 889,
    // over the cap (the GUI's floor(sqrt 10) = 3 gave exactly that); stride 4
    // keeps 500 x 500 = 250 000.
    EXPECT_EQ(strideForCap(2000, 2000, 400'000), 4);

    // Exactly at the cap is within it; one under is not.
    EXPECT_EQ(strideForCap(100, 100, 10'000), 1);
    EXPECT_EQ(strideForCap(100, 100, 9'999), 2); // 50 x 50 = 2 500

    // A strip, where the ceilings put the answer far above sqrt(w*h / cap):
    // ceil(10^9 / 10^8) = 10 meets a cap of 10, ceil(10^9 / (10^8 - 1)) = 11
    // does not.
    EXPECT_EQ(strideForCap(1, 1'000'000'000, 10), 100'000'000);

    // The largest grid GDAL can describe, down to one sample: only the whole
    // side as the stride leaves one, since INT_MAX - 1 leaves 2 x 2.
    const int largest = std::numeric_limits<int>::max();
    EXPECT_EQ(strideForCap(largest, largest, 1), largest);

    // Nothing to sample, and a cap nothing meets (callers refuse it first).
    EXPECT_EQ(strideForCap(0, 5, 10), 1);
    EXPECT_EQ(strideForCap(5, 0, 10), 1);
    EXPECT_EQ(strideForCap(30, 7, 0), 30);
}

TEST(TerrainIoStride, TheStrideAgreesWithABruteForceSearchOverEverySmallGrid)
{
    const auto samples = [](int w, int h, int s) {
        return static_cast<std::uint64_t>((w + s - 1) / s) *
               static_cast<std::uint64_t>((h + s - 1) / s);
    };
    std::uint64_t cases = 0;
    std::uint64_t oldFormulaOverCap = 0;
    for (int w = 1; w <= 30; ++w) {
        for (int h = 1; h <= 30; ++h) {
            for (std::uint64_t cap = 1; cap <= static_cast<std::uint64_t>(w * h) + 1; ++cap) {
                int expected = 1;
                while (samples(w, h, expected) > cap) {
                    ++expected;
                }
                ASSERT_EQ(strideForCap(w, h, cap), expected)
                    << w << " x " << h << " capped at " << cap;
                ++cases;

                // The formula the GUI used (audit QT-24), to show this sweep
                // reaches the cases that broke it rather than only easy ones.
                const std::uint64_t pixels = static_cast<std::uint64_t>(w) * h;
                const std::uint64_t thinning = (pixels + cap - 1) / cap;
                const int old = std::max(
                    1, static_cast<int>(std::floor(std::sqrt(static_cast<double>(thinning)))));
                if (samples(w, h, old) > cap) {
                    ++oldFormulaOverCap;
                }
            }
        }
    }
    // sum over w, h of (w h + 1) = (1 + ... + 30)^2 + 900 = 465^2 + 900.
    EXPECT_EQ(cases, 217'125u);
    // A sweep that never found the old formula over the cap would prove
    // nothing about the fix. Measured (2026-09-23, and by an independent
    // Python replay of the same loops): 199 085 of the 217 125 cases.
    EXPECT_GT(oldFormulaOverCap, 100'000u) << "of " << cases;
}

// ---- readRasterElevations (audit QT-23) ------------------------------------------------------

namespace {

constexpr int kDemWidth = 10;
constexpr int kDemHeight = 9;
constexpr double kDemNoData = -9999.0;

// Every value a multiple of 0.25, exact in binary: v(i, j) = 10.5 + 0.75 i + 0.25 j.
double demValue(int column, int row)
{
    return 10.5 + 0.75 * column + 0.25 * row;
}

bool isNoDataPixel(int column, int row)
{
    return column == 3 && row == 3;
}

// A 10 x 9 GeoTIFF whose top-left corner is (1000, 2000) with 2 m pixels,
// holding demValue except at pixel (3, 3), which is no-data.
void writeDem(const std::filesystem::path& path)
{
    gis::RasterExportOptions options;
    options.width = kDemWidth;
    options.height = kDemHeight;
    options.geotransform = {1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0};
    options.noDataValue = kDemNoData;
    options.projectionWkt = kMga56Wkt;
    std::vector<double> values;
    for (int row = 0; row < kDemHeight; ++row) {
        for (int column = 0; column < kDemWidth; ++column) {
            values.push_back(isNoDataPixel(column, row) ? kDemNoData : demValue(column, row));
        }
    }
    const auto status = gis::GdalDataset::writeRaster(path, options, values);
    ASSERT_TRUE(status.ok()) << status.error().describe();
}

// The centre of pixel (i, j): 1000 + 2 (i + 0.5) east, 2000 - 2 (j + 0.5) north.
Point3 demPoint(int column, int row)
{
    return Point3(1001.0 + 2.0 * column, 1999.0 - 2.0 * row, demValue(column, row));
}

} // namespace

TEST(TerrainIoDem, ElevationsArePixelCentresCarryingTheBandsOwnValues)
{
    const TempDir dir("dem-centres");
    const auto path = dir.file("dem.tif");
    writeDem(path);

    const auto read = readRasterElevations(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->stride, 1);
    EXPECT_EQ(read->sampled, 90u);
    EXPECT_EQ(read->noData, 1u);
    EXPECT_NE(gis::describeCrs(read->projectionWkt).find("EPSG:28356"), std::string::npos);

    // Every point exactly: its position is the pixel CENTRE through the
    // file's own geotransform, and its height is the band value unchanged -
    // not an 8-bit grey rescaled between two typed numbers (audit QT-23).
    std::vector<Point3> expected;
    for (int row = 0; row < kDemHeight; ++row) {
        for (int column = 0; column < kDemWidth; ++column) {
            if (!isNoDataPixel(column, row)) {
                expected.push_back(demPoint(column, row));
            }
        }
    }
    ASSERT_EQ(read->points.size(), expected.size());
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_EQ(read->points[i].x, expected[i].x) << "point " << i;
        EXPECT_EQ(read->points[i].y, expected[i].y) << "point " << i;
        EXPECT_EQ(read->points[i].z, expected[i].z) << "point " << i;
    }
}

TEST(TerrainIoDem, ACapIsMetByAStrideAcrossTheWholeExtentNotByTruncation)
{
    const TempDir dir("dem-cap");
    const auto path = dir.file("dem.tif");
    writeDem(path);

    // 90 pixels capped at 20: stride 2 keeps 5 x 5 = 25, stride 3 keeps
    // ceil(10/3) x ceil(9/3) = 4 x 3 = 12 - columns 0, 3, 6, 9 and rows 0, 3,
    // 6, of which (3, 3) is the no-data pixel.
    RasterElevationOptions options;
    options.maxPoints = 20;
    const auto read = readRasterElevations(path, options);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->stride, 3);
    EXPECT_EQ(read->sampled, 12u);
    EXPECT_EQ(read->noData, 1u);
    ASSERT_EQ(read->points.size(), 11u);
    EXPECT_LE(read->points.size(), options.maxPoints);

    std::vector<Point3> expected;
    for (const int row : {0, 3, 6}) {
        for (const int column : {0, 3, 6, 9}) {
            if (!isNoDataPixel(column, row)) {
                expected.push_back(demPoint(column, row));
            }
        }
    }
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_EQ(read->points[i].x, expected[i].x) << "point " << i;
        EXPECT_EQ(read->points[i].y, expected[i].y) << "point " << i;
        EXPECT_EQ(read->points[i].z, expected[i].z) << "point " << i;
    }
    // The far corner is there: pixel (9, 6), within one stride of the last
    // row and on the last column - not the first 20 pixels of the file.
    EXPECT_EQ(read->points.back().x, 1019.0);
    EXPECT_EQ(read->points.back().y, 1987.0);
    EXPECT_EQ(read->points.back().z, 18.75);
}

TEST(TerrainIoDem, WhatCannotBePlacedOnTheGroundIsRefused)
{
    const TempDir dir("dem-refused");
    const auto dem = dir.file("dem.tif");
    writeDem(dem);

    EXPECT_EQ(readRasterElevations(dir.file("missing.tif")).error().code, ErrorCode::NotFound);

    RasterElevationOptions zero;
    zero.maxPoints = 0;
    EXPECT_EQ(readRasterElevations(dem, zero).error().code, ErrorCode::InvalidArgument);

    for (const int band : {0, 2}) {
        RasterElevationOptions options;
        options.band = band;
        EXPECT_EQ(readRasterElevations(dem, options).error().code, ErrorCode::InvalidArgument)
            << "band " << band;
    }

    // A raster with no georeferencing: its pixels would land at (0, 0) one
    // unit each, a surface in the wrong place. Written by hand as a VRT with no
    // <GeoTransform> - GTiff records even GDAL's default transform as real
    // georeferencing, so writeRaster cannot make this fixture.
    const auto plain = dir.file("plain.vrt");
    {
        std::ofstream(plain) << R"(<VRTDataset rasterXSize="4" rasterYSize="3">)"
                             << R"(<VRTRasterBand dataType="Float64" band="1"/></VRTDataset>)";
    }
    ASSERT_FALSE((*(*gis::GdalDataset::open(plain))->rasterInfo()).hasGeotransform)
        << "the fixture must really be without georeferencing";
    const auto refused = readRasterElevations(plain);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("georeferenced"), std::string::npos);

    // A file GDAL cannot read at all.
    const auto broken = dir.file("broken.tif");
    {
        std::ofstream(broken) << "not a raster";
    }
    EXPECT_EQ(readRasterElevations(broken).error().code, ErrorCode::FileImportFailure);
}

// ---- surfacePoints (audit QT-10) --------------------------------------------------------------

namespace {

katana::pointcloud::PointCloudPoint cloudPoint(double x, double y, double z,
                                               std::uint8_t classification)
{
    katana::pointcloud::PointCloudPoint point;
    point.x = x;
    point.y = y;
    point.z = z;
    point.classification = classification;
    return point;
}

} // namespace

TEST(TerrainIoCloud, AClassifiedCloudGivesItsGroundReturnsAlone)
{
    // ASPRS LAS 1.4 R15 table 17: 2 ground, 5 high vegetation, 6 building,
    // 7 low noise. A canopy or a roof in a ground surface is what QT-10 found.
    PointCloudLayer cloud;
    cloud.points = {cloudPoint(0, 0, 10, 2), cloudPoint(1, 0, 25, 5), cloudPoint(2, 0, 11, 2),
                    cloudPoint(3, 0, 18, 6), cloudPoint(4, 0, -40, 7)};
    const CloudSurfacePoints surface = surfacePoints(cloud);
    EXPECT_TRUE(surface.groundOnly);
    EXPECT_EQ(surface.excluded, 3u);
    ASSERT_EQ(surface.points.size(), 2u);
    EXPECT_EQ(surface.points[0], Point3(0, 0, 10));
    EXPECT_EQ(surface.points[1], Point3(2, 0, 11));
}

TEST(TerrainIoCloud, AnUnclassifiedCloudGivesEveryReturnAndSaysSo)
{
    // Photogrammetry and terrestrial scans carry no classes (0 never
    // classified, 1 unclassified). Refusing them would leave them no way to a
    // surface; taking every return is right, provided the caller is TOLD.
    PointCloudLayer cloud;
    cloud.points = {cloudPoint(0, 0, 10, 0), cloudPoint(1, 0, 25, 1), cloudPoint(2, 0, 11, 1)};
    const CloudSurfacePoints surface = surfacePoints(cloud);
    EXPECT_FALSE(surface.groundOnly);
    EXPECT_EQ(surface.excluded, 0u);
    ASSERT_EQ(surface.points.size(), 3u);
    EXPECT_EQ(surface.points[1], Point3(1, 0, 25));
}

TEST(TerrainIoCloud, AnEmptyCloudGivesNothingAndClaimsNothing)
{
    const CloudSurfacePoints surface = surfacePoints(PointCloudLayer{});
    EXPECT_TRUE(surface.points.empty());
    EXPECT_FALSE(surface.groundOnly) << "no ground was found, so none can be claimed";
    EXPECT_EQ(surface.excluded, 0u);
}

// ---- exportSurfaceRaster ----------------------------------------------------------------------

namespace {

// The plane z = 0.5 x + 0.25 y + 3. Linear interpolation reproduces a plane
// exactly, so every cell of the exported grid has a value worked out by hand,
// and at the half-integer cell centres used here it is a multiple of 1/8 -
// exact in Float64 and in Float32 alike.
double plane(double x, double y)
{
    return 0.5 * x + 0.25 * y + 3.0;
}

// The TIN's interpolation error. At these small integer vertices the edge
// functions and their sum are exact, so each barycentric weight is one
// correctly rounded division; the value is then three products and two sums.
// Per operation the relative error is at most u = DBL_EPSILON / 2, which
// bounds the whole by about 4u x max|z| = 2 DBL_EPSILON x max|z|. The plane
// peaks at 9 on the 8 m square (0.5 x 8 + 0.25 x 8 + 3); twice that bound is
// the tolerance.
constexpr double kPlaneTolerance = 4.0 * DBL_EPSILON * 9.0;

// Survey points on the plane at every whole metre of the 8 m square, or of the
// triangle below its diagonal x + y = 8 when `triangle`.
katana::terrain::TinSurface planeSurface(bool triangle)
{
    katana::terrain::TinInput input;
    for (int j = 0; j <= 8; ++j) {
        for (int i = 0; i <= 8; ++i) {
            if (!triangle || i + j <= 8) {
                input.points.emplace_back(i, j, plane(i, j));
            }
        }
    }
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok()) << built.error().describe();
    return built.ok() ? built->surface : katana::terrain::TinSurface{};
}

struct ReadBack {
    gis::RasterInfo info;
    gis::RasterSamples samples;
};

ReadBack readBack(const std::filesystem::path& path)
{
    const auto dataset = gis::GdalDataset::open(path);
    EXPECT_TRUE(dataset.ok()) << dataset.error().describe();
    if (!dataset.ok()) {
        return {};
    }
    const auto info = (*dataset)->rasterInfo();
    const auto samples = (*dataset)->readBandSampled(1, 1);
    EXPECT_TRUE(info.ok() && samples.ok());
    if (!info.ok() || !samples.ok()) {
        return {};
    }
    return {*info, *samples};
}

constexpr double kFloatNoData = std::numeric_limits<float>::lowest();

} // namespace

TEST(TerrainIoSurfaceRaster, EachCellIsTheSurfaceAtItsCentre)
{
    const TempDir dir("surface-square");
    const auto path = dir.file("surface.tif");
    const auto surface = planeSurface(false);
    SurfaceRasterOptions options;
    options.projectionWkt = kMga56Wkt;
    const auto written = exportSurfaceRaster(surface, path, options);
    ASSERT_TRUE(written.ok()) << written.error().describe();

    // 8 m at 1 m cells, north-up from the top-left corner (0, 8).
    EXPECT_EQ(written->columns, 8);
    EXPECT_EQ(written->rows, 8);
    EXPECT_EQ(written->cellsWithData, 64u);
    EXPECT_EQ(written->driver, "GTiff");
    EXPECT_EQ(written->noDataValue, kFloatNoData);
    EXPECT_EQ(written->geotransform, (std::array<double, 6>{0.0, 1.0, 0.0, 8.0, 0.0, -1.0}));

    const ReadBack back = readBack(path);
    EXPECT_EQ(back.info.geotransform, written->geotransform);
    ASSERT_TRUE(back.info.noDataValue.has_value());
    EXPECT_EQ(*back.info.noDataValue, kFloatNoData);
    EXPECT_NE(gis::describeCrs(back.info.projectionWkt).find("EPSG:28356"), std::string::npos);
    ASSERT_EQ(back.samples.values.size(), 64u);
    for (int row = 0; row < 8; ++row) {
        for (int column = 0; column < 8; ++column) {
            const double x = column + 0.5;
            const double y = 8.0 - (row + 0.5);
            EXPECT_NEAR(back.samples.values[static_cast<std::size_t>(row * 8 + column)],
                        plane(x, y), kPlaneTolerance)
                << "cell (" << column << ", " << row << ")";
        }
    }
}

TEST(TerrainIoSurfaceRaster, CellsOffTheSurfaceAreNoDataInEveryFormat)
{
    // The triangle below x + y = 8. Cell (c, r) has its centre at
    // (c + 0.5, 7.5 - r), so x + y = 8 + c - r: inside for c < r, exactly on
    // the rim for c == r (on the surface: tin_surface.hpp takes the rim as
    // part of it), outside for c > r. 28 + 8 = 36 of the 64 cells.
    const auto surface = planeSurface(true);
    for (const char* name : {"triangle.tif", "triangle.asc", "triangle.img"}) {
        SCOPED_TRACE(name);
        const TempDir dir(std::string("surface-") + name);
        const auto path = dir.file(name);
        const auto written = exportSurfaceRaster(surface, path);
        ASSERT_TRUE(written.ok()) << written.error().describe();
        EXPECT_EQ(written->cellsWithData, 36u);

        const ReadBack back = readBack(path);
        ASSERT_TRUE(back.info.noDataValue.has_value());
        EXPECT_EQ(*back.info.noDataValue, kFloatNoData);
        EXPECT_EQ(back.info.geotransform, (std::array<double, 6>{0.0, 1.0, 0.0, 8.0, 0.0, -1.0}));
        ASSERT_EQ(back.samples.values.size(), 64u);
        std::uint64_t withData = 0;
        for (int row = 0; row < 8; ++row) {
            for (int column = 0; column < 8; ++column) {
                const double value =
                    back.samples.values[static_cast<std::size_t>(row * 8 + column)];
                if (column > row) {
                    EXPECT_EQ(value, kFloatNoData) << "cell (" << column << ", " << row << ")";
                    continue;
                }
                ++withData;
                EXPECT_NEAR(value, plane(column + 0.5, 7.5 - row), kPlaneTolerance)
                    << "cell (" << column << ", " << row << ")";
            }
        }
        EXPECT_EQ(withData, 36u);
    }
}

TEST(TerrainIoSurfaceRaster, EveryOfferedFormatIsOneTheExporterWrites)
{
    // The save dialog offers rasterExportFormats(); each must be an extension
    // the exporter can then write, and read back as the driver it named.
    const auto surface = planeSurface(false);
    const auto formats = rasterExportFormats();
    ASSERT_EQ(formats.size(), 3u);
    for (const FormatChoice& format : formats) {
        SCOPED_TRACE(format.description);
        const TempDir dir("offered-" + format.extension);
        const auto path = dir.file("surface." + format.extension);
        const auto written = exportSurfaceRaster(surface, path);
        ASSERT_TRUE(written.ok()) << written.error().describe();
        EXPECT_EQ(written->driver, *gis::GdalDataset::rasterDriverForPath(path));
        const auto dataset = gis::GdalDataset::open(path);
        ASSERT_TRUE(dataset.ok()) << dataset.error().describe();
        EXPECT_EQ((*dataset)->driverName(), written->driver);
    }
}

TEST(TerrainIoSurfaceRaster, AnExplicitDriverOverridesTheExtension)
{
    const TempDir dir("surface-driver");
    const auto path = dir.file("surface.grid");
    SurfaceRasterOptions options;
    options.driver = "AAIGrid";
    const auto written = exportSurfaceRaster(planeSurface(false), path, options);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(written->driver, "AAIGrid");
    EXPECT_EQ((*gis::GdalDataset::open(path))->driverName(), "AAIGrid");
}

TEST(TerrainIoSurfaceRaster, WhatCannotBeWrittenIsRefusedAndNothingIsLeftBehind)
{
    const TempDir dir("surface-refused");
    const auto surface = planeSurface(false);
    const auto path = dir.file("surface.tif");

    EXPECT_EQ(exportSurfaceRaster(katana::terrain::TinSurface{}, path).error().code,
              ErrorCode::InvalidArgument);
    for (const double cell : {0.0, -1.0, std::numeric_limits<double>::quiet_NaN(),
                              std::numeric_limits<double>::infinity()}) {
        SurfaceRasterOptions options;
        options.cellSize = cell;
        EXPECT_EQ(exportSurfaceRaster(surface, path, options).error().code,
                  ErrorCode::InvalidArgument)
            << "cell size " << cell;
    }

    // 8 x 8 = 64 cells against a limit of 63: refused, and the count named.
    SurfaceRasterOptions small;
    small.maxCells = 63;
    const auto tooMany = exportSurfaceRaster(surface, path, small);
    ASSERT_FALSE(tooMany.ok());
    EXPECT_EQ(tooMany.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(tooMany.error().message.find("64"), std::string::npos) << tooMany.error().message;
    small.maxCells = 64;
    EXPECT_TRUE(exportSurfaceRaster(surface, dir.file("exactly.tif"), small).ok())
        << "exactly the limit is within it";

    // A cell so fine the count overflows every integer type is still named,
    // not wrapped round to something small.
    SurfaceRasterOptions absurd;
    absurd.cellSize = 1e-12;
    EXPECT_EQ(exportSurfaceRaster(surface, path, absurd).error().code,
              ErrorCode::InvalidArgument);

    const auto unknown = exportSurfaceRaster(surface, dir.file("surface.xyzzy"));
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::Unsupported);

    EXPECT_FALSE(std::filesystem::exists(path));
    EXPECT_FALSE(std::filesystem::exists(dir.file("surface.xyzzy")));
}

// ---- suggestedCellSize -----------------------------------------------------------------------

TEST(TerrainIoSurfaceRaster, TheSuggestedCellIsARoundNumberNearTheTarget)
{
    const auto square = [](double side) { return Box2(Point2(0, 0), Point2(side, side)); };
    // side / 1000 cells, to the nearest of 1, 2, 5 x 10^n by ratio.
    EXPECT_EQ(suggestedCellSize(square(1000.0)), 1.0); // 1
    EXPECT_EQ(suggestedCellSize(square(2300.0)), 2.0); // 2.3
    EXPECT_EQ(suggestedCellSize(square(480.0)), 0.5);  // 0.48
    EXPECT_EQ(suggestedCellSize(square(12.0)), 0.01);  // 0.012

    // The boundaries are sqrt(2), sqrt(10) and sqrt(50) - geometric, not the
    // arithmetic 1.5, 3.5 and 7.5 - so each of these rounds UP.
    EXPECT_EQ(suggestedCellSize(square(1450.0)), 2.0);  // 1.45 > 1.414
    EXPECT_EQ(suggestedCellSize(square(3300.0)), 5.0);  // 3.3 > 3.162
    EXPECT_EQ(suggestedCellSize(square(7300.0)), 10.0); // 7.3 > 7.071
    // ...and these down.
    EXPECT_EQ(suggestedCellSize(square(1400.0)), 1.0);  // 1.4 < 1.414
    EXPECT_EQ(suggestedCellSize(square(7000.0)), 5.0);  // 7.0 < 7.071

    // The LONGER side decides, whichever it is.
    EXPECT_EQ(suggestedCellSize(Box2(Point2(0, 0), Point2(100, 5000))), 5.0);
    EXPECT_EQ(suggestedCellSize(Box2(Point2(0, 0), Point2(5000, 100))), 5.0);
    EXPECT_EQ(suggestedCellSize(square(1000.0), 100), 10.0);

    // Nothing to size: 1.0.
    EXPECT_EQ(suggestedCellSize(Box2{}), 1.0);
    EXPECT_EQ(suggestedCellSize(Box2(Point2(5, 5), Point2(5, 5))), 1.0);
    EXPECT_EQ(suggestedCellSize(square(1000.0), 0), 1.0);
}

// ---- point-cloud export formats --------------------------------------------------------------

TEST(TerrainIoCloudExport, EveryOfferedPointCloudFormatIsWrittenAndReadBack)
{
    PointCloudLayer cloud;
    cloud.name = "grid";
    for (int j = 0; j < 100; ++j) {
        for (int i = 0; i < 100; ++i) {
            cloud.points.push_back(cloudPoint(0.5 * i, 0.5 * j, 10.0 + 0.125 * i, 2));
        }
    }
    const auto formats = pointCloudExportFormats();
    ASSERT_EQ(formats.size(), 2u);
    const TempDir dir("cloud-formats");
    std::vector<std::uintmax_t> sizes;
    for (const FormatChoice& format : formats) {
        SCOPED_TRACE(format.description);
        const auto path = dir.file("cloud." + format.extension);
        const auto status = exportPointCloud(cloud, path);
        ASSERT_TRUE(status.ok()) << status.error().describe();
        const auto back = importPointCloud(path);
        ASSERT_TRUE(back.ok()) << back.error().describe();
        EXPECT_EQ(back->points.size(), cloud.points.size());
        sizes.push_back(std::filesystem::file_size(path));
    }
    // LAS then LAZ: "compressed LAS" must be compressed. A regular grid
    // compresses far better than two to one.
    EXPECT_LT(sizes[1] * 2, sizes[0]) << "LAS " << sizes[0] << " bytes, LAZ " << sizes[1];
}
