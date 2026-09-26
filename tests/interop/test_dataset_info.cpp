// What a file holds, read without importing it (PLAN.MD Phase 20).
//
// describeSource is checked against files written here with known contents -
// so every expected fact is one the test put there - and formatDescription,
// which is pure, against descriptions built by hand. The text tests assert on
// the facts appearing, never on a snapshot of the whole text: a rewording is
// not a defect, a missing or wrong fact is.

#include <gtest/gtest.h>

#include <cstdint>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/zip_container.hpp"
#include "katana/interop/dataset_info.hpp"
#include "katana/interop/import.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"

using namespace katana::interop;
using katana::core::ErrorCode;
using katana::geometry::Box2;
using katana::geometry::Point2;
namespace gis = katana::gis;
namespace pc = katana::pointcloud;

namespace {

// A directory of its own per test, removed afterwards, so the suite can run in
// parallel with itself and leaves nothing behind.
class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-datasetinfo-" + name))
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
constexpr const char* kMga56Name = "GDA94 / MGA zone 56 (EPSG:28356)";

bool contains(const std::string& text, const std::string& fact)
{
    return text.find(fact) != std::string::npos;
}

void writeRaster(const std::filesystem::path& path, const gis::RasterExportOptions& options)
{
    const std::vector<double> values(
        static_cast<std::size_t>(options.width) * static_cast<std::size_t>(options.height), 1.0);
    const auto status = gis::GdalDataset::writeRaster(path, options, values);
    ASSERT_TRUE(status.ok()) << status.error().describe();
}

void writeText(const std::filesystem::path& path, const std::string& text)
{
    std::ofstream out(path, std::ios::binary);
    out << text;
}

} // namespace

// ---- rasters ------------------------------------------------------------------------------------

TEST(DatasetInfo, ARasterIsDescribedWithItsGeoreferencingAndNoData)
{
    const TempDir dir("raster");
    const auto path = dir.file("dem.tif");
    gis::RasterExportOptions options;
    options.width = 3;
    options.height = 2;
    options.geotransform = {1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0};
    options.projectionWkt = kMga56Wkt;
    options.noDataValue = -9999.0;
    writeRaster(path, options);

    const auto described = describeSource(path);
    ASSERT_TRUE(described.ok()) << described.error().describe();
    const SourceDescription& d = *described;
    EXPECT_EQ(d.path, path);
    EXPECT_EQ(d.kind, SourceKind::Raster);
    EXPECT_EQ(d.driver, "GTiff");
    EXPECT_EQ(d.crs, kMga56Name);
    EXPECT_FALSE(d.projectionWkt.empty());
    EXPECT_TRUE(d.vectorLayers.empty());
    EXPECT_FALSE(d.pointCloud.has_value());
    ASSERT_TRUE(d.raster.has_value());
    EXPECT_EQ(d.raster->width, 3);
    EXPECT_EQ(d.raster->height, 2);
    EXPECT_EQ(d.raster->bandCount, 1);
    EXPECT_TRUE(d.raster->georeferenced);
    EXPECT_EQ(d.raster->pixelWidth, 2.0);
    EXPECT_EQ(d.raster->pixelHeight, 2.0);
    // 3 pixels of 2 m east of 1000, 2 pixels of 2 m south of 2000.
    EXPECT_EQ(d.raster->bounds.min, Point2(1000.0, 1996.0));
    EXPECT_EQ(d.raster->bounds.max, Point2(1006.0, 2000.0));
    ASSERT_TRUE(d.raster->noDataValue.has_value());
    EXPECT_EQ(*d.raster->noDataValue, -9999.0);

    const std::string text = formatDescription(d);
    for (const std::string fact : {"dem.tif", "raster (GTiff)", kMga56Name, "3 x 2 pixels",
                                   "1 band", "(1000.000, 1996.000) to (1006.000, 2000.000)",
                                   "No-data value: -9999"}) {
        EXPECT_TRUE(contains(text, fact)) << "missing '" << fact << "' in:\n" << text;
    }
}

TEST(DatasetInfo, ARotatedRasterReportsItsTruePixelSizeAndAllFourCorners)
{
    // Columns step (3, 4) and rows (4, -3): 5 m pixels, turned. Corners of a
    // 2 x 1 grid through the full affine: (0,0) -> (1000, 2000), (2,0) ->
    // (1006, 2008), (0,1) -> (1004, 1997), (2,1) -> (1010, 2005). Reading
    // only gt[1] and gt[5] would report 3 x 3 pixels and a box missing two
    // of those corners.
    const TempDir dir("rotated");
    const auto path = dir.file("rotated.tif");
    gis::RasterExportOptions options;
    options.width = 2;
    options.height = 1;
    options.geotransform = {1000.0, 3.0, 4.0, 2000.0, 4.0, -3.0};
    writeRaster(path, options);

    const auto described = describeSource(path);
    ASSERT_TRUE(described.ok()) << described.error().describe();
    ASSERT_TRUE(described->raster.has_value());
    EXPECT_EQ(described->raster->pixelWidth, 5.0);
    EXPECT_EQ(described->raster->pixelHeight, 5.0);
    EXPECT_EQ(described->raster->bounds.min, Point2(1000.0, 1997.0));
    EXPECT_EQ(described->raster->bounds.max, Point2(1010.0, 2008.0));
}

TEST(DatasetInfo, ARasterWithoutGeoreferencingOrCrsSaysSo)
{
    // Written by hand as a VRT with no <GeoTransform>: GTiff records even
    // GDAL's default transform as real georeferencing, so writeRaster cannot
    // make a raster without it.
    const TempDir dir("plain");
    const auto path = dir.file("scan.vrt");
    writeText(path, R"(<VRTDataset rasterXSize="4" rasterYSize="3">)"
                    R"(<VRTRasterBand dataType="Float64" band="1"/></VRTDataset>)");

    const auto described = describeSource(path);
    ASSERT_TRUE(described.ok()) << described.error().describe();
    EXPECT_EQ(described->driver, "VRT");
    ASSERT_TRUE(described->raster.has_value());
    EXPECT_EQ(described->raster->width, 4);
    EXPECT_EQ(described->raster->height, 3);
    EXPECT_FALSE(described->raster->georeferenced);
    EXPECT_TRUE(described->raster->bounds.empty())
        << "the origin is GDAL's guess, not the raster's position";
    EXPECT_TRUE(described->crs.empty());
    EXPECT_FALSE(described->raster->noDataValue.has_value());

    const std::string text = formatDescription(*described);
    EXPECT_TRUE(contains(text, "not georeferenced")) << text;
    EXPECT_TRUE(contains(text, "no coordinate system declared")) << text;
    EXPECT_TRUE(contains(text, "No-data value: none declared")) << text;
}

// ---- vector layers ------------------------------------------------------------------------------

TEST(DatasetInfo, EveryVectorLayerIsListedWithItsCountTypeAndCrs)
{
    const TempDir dir("vector");
    const auto path = dir.file("survey.gpkg");
    std::vector<gis::VectorFeature> features;
    for (int i = 0; i < 3; ++i) {
        gis::VectorFeature point;
        point.geometry.kind = gis::GeometryKind::Point;
        point.geometry.parts.push_back({gis::GeoPoint{255440.0 + i, 7410850.0, 0.0}});
        features.push_back(point);
    }
    gis::VectorExportOptions options;
    options.layerName = "control";
    options.projectionWkt = kMga56Wkt;
    ASSERT_TRUE(gis::GdalDataset::writeVector(path, features, options).ok());

    const auto described = describeSource(path);
    ASSERT_TRUE(described.ok()) << described.error().describe();
    EXPECT_EQ(described->kind, SourceKind::Vector);
    EXPECT_EQ(described->driver, "GPKG");
    EXPECT_FALSE(described->raster.has_value());
    ASSERT_EQ(described->vectorLayers.size(), 1u);
    const VectorLayerDescription& layer = described->vectorLayers.front();
    EXPECT_EQ(layer.name, "control");
    EXPECT_EQ(layer.featureCount, 3u);
    EXPECT_EQ(layer.geometryType, "Point");
    EXPECT_EQ(layer.crs, kMga56Name);
    // The layers agree, so the file's CRS is theirs.
    EXPECT_EQ(described->crs, kMga56Name);

    const std::string text = formatDescription(*described);
    EXPECT_TRUE(contains(text, "vector (GPKG)")) << text;
    EXPECT_TRUE(contains(text, std::string("control: 3 features, Point, ") + kMga56Name)) << text;
}

TEST(DatasetInfo, AShapefileWithoutAPrjDeclaresNoCoordinateSystem)
{
    const TempDir dir("shapefile");
    const auto path = dir.file("marks.shp");
    gis::VectorFeature point;
    point.geometry.kind = gis::GeometryKind::Point;
    point.geometry.parts.push_back({gis::GeoPoint{1.0, 2.0, 0.0}});
    ASSERT_TRUE(gis::GdalDataset::writeVector(path, {point}, {}).ok());

    const auto described = describeSource(path);
    ASSERT_TRUE(described.ok()) << described.error().describe();
    EXPECT_EQ(described->driver, "ESRI Shapefile");
    ASSERT_EQ(described->vectorLayers.size(), 1u);
    EXPECT_EQ(described->vectorLayers.front().featureCount, 1u);
    EXPECT_TRUE(described->vectorLayers.front().crs.empty());
    EXPECT_TRUE(described->crs.empty());
    const std::string text = formatDescription(*described);
    EXPECT_TRUE(contains(text, "Coordinate system: no coordinate system declared")) << text;
    EXPECT_TRUE(contains(text, "1 feature, Point, no coordinate system declared")) << text;
}

// ---- point clouds -------------------------------------------------------------------------------

namespace {

// 1 234 coloured points on a grid 100 wide: x 250 000 to 250 099, y 7 400 000
// to 7 400 012 (rows 0 to 12, the last one partial), z 10 to 10.75. Every
// coordinate is a multiple of 0.25, exact at the writer's 0.001 scale.
pc::PointCloud colouredCloud()
{
    pc::PointCloud cloud;
    for (int i = 0; i < 1234; ++i) {
        pc::PointCloudPoint point;
        point.x = 250000.0 + (i % 100);
        point.y = 7400000.0 + (i / 100);
        point.z = 10.0 + 0.25 * (i % 4);
        point.classification = 2;
        point.red = 200;
        point.green = 100;
        point.blue = 50;
        point.hasColor = true;
        cloud.points.push_back(point);
    }
    return cloud;
}

// A 100 m square of ground at 0.25 m spacing, dense enough that a COPC octree
// has levels to choose between (see the adapter tests' denseGround).
pc::PointCloud denseGround()
{
    pc::PointCloud cloud;
    for (int j = 0; j < 400; ++j) {
        for (int i = 0; i < 400; ++i) {
            pc::PointCloudPoint point;
            point.x = 0.25 * i;
            point.y = 0.25 * j;
            point.z = 10.0;
            point.classification = 2;
            cloud.points.push_back(point);
        }
    }
    return cloud;
}

} // namespace

TEST(DatasetInfo, APointCloudIsDescribedFromItsHeader)
{
    const TempDir dir("cloud");
    const auto path = dir.file("scan.las");
    ASSERT_TRUE(pc::PointCloudEngine{}.write(path, colouredCloud()).ok());

    const auto described = describeSource(path);
    ASSERT_TRUE(described.ok()) << described.error().describe();
    EXPECT_EQ(described->kind, SourceKind::PointCloud);
    EXPECT_EQ(described->driver, "readers.las");
    ASSERT_TRUE(described->pointCloud.has_value());
    const PointCloudDescription& cloud = *described->pointCloud;
    EXPECT_EQ(cloud.pointCount, 1234u);
    EXPECT_TRUE(cloud.hasColor);
    EXPECT_FALSE(cloud.copc);
    EXPECT_EQ(cloud.bounds.minX, 250000.0);
    EXPECT_EQ(cloud.bounds.maxX, 250099.0);
    EXPECT_EQ(cloud.bounds.minY, 7400000.0);
    EXPECT_EQ(cloud.bounds.maxY, 7400012.0);
    EXPECT_EQ(cloud.bounds.minZ, 10.0);
    EXPECT_EQ(cloud.bounds.maxZ, 10.75);

    const std::string text = formatDescription(*described);
    for (const std::string fact :
         {"point cloud (readers.las)", "1,234 points", "x 250000.000 to 250099.000",
          "y 7400000.000 to 7400012.000", "z 10.000 to 10.750", "Colour: yes", "COPC: no",
          "no coordinate system declared"}) {
        EXPECT_TRUE(contains(text, fact)) << "missing '" << fact << "' in:\n" << text;
    }
}

TEST(DatasetInfo, ACopcFileIsDescribedAsOneAndIsImportedByResolution)
{
    const TempDir dir("copc");
    const auto las = dir.file("ground.las");
    const auto copc = dir.file("ground.copc.laz");
    const pc::PointCloudEngine engine;
    ASSERT_TRUE(engine.write(las, denseGround()).ok());
    ASSERT_TRUE(engine.convertToCopc(las, copc).ok());

    const auto described = describeSource(copc);
    ASSERT_TRUE(described.ok()) << described.error().describe();
    EXPECT_EQ(described->driver, "readers.copc");
    ASSERT_TRUE(described->pointCloud.has_value());
    EXPECT_TRUE(described->pointCloud->copc);
    EXPECT_EQ(described->pointCloud->pointCount, 160000u);
    EXPECT_TRUE(contains(formatDescription(*described), "COPC: yes"));

    // What the description promises: a resolution query instead of a
    // decimation. The octree does the thinning, so no decimation step is
    // applied on top, and the coarse read still spans the whole square.
    PointCloudImportOptions coarse;
    coarse.resolution = 5.0;
    const auto layer = importPointCloud(copc, coarse);
    ASSERT_TRUE(layer.ok()) << layer.error().describe();
    EXPECT_EQ(layer->decimationStep, 1u);
    EXPECT_GT(layer->points.size(), 0u);
    EXPECT_LT(layer->points.size() * 4u, 160000u) << "a 5 m query on 0.25 m data";
    EXPECT_EQ(layer->sourcePointCount, 160000u);
    EXPECT_TRUE(layer->isDecimated());
    EXPECT_LT(layer->bounds.minX, 10.0);
    EXPECT_GT(layer->bounds.maxX, 90.0);

    // The budget still caps what is kept.
    PointCloudImportOptions capped;
    capped.resolution = 0.25;
    capped.budget = 100;
    const auto small = importPointCloud(copc, capped);
    ASSERT_TRUE(small.ok()) << small.error().describe();
    EXPECT_LE(small->points.size(), 100u);

    // A plain LAS cannot answer a resolution; the import says so rather than
    // quietly reading the whole file.
    const auto refused = importPointCloud(las, coarse);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(refused.error().describe(), "COPC")) << refused.error().describe();
}

// ---- refusals -----------------------------------------------------------------------------------

TEST(DatasetInfo, WhatCannotBeDescribedIsRefusedWithTheReason)
{
    const TempDir dir("refused");
    EXPECT_EQ(describeSource(dir.file("missing.tif")).error().code, ErrorCode::NotFound);

    const auto unknown = dir.file("notes.xyzzy");
    writeText(unknown, "hello");
    EXPECT_EQ(describeSource(unknown).error().code, ErrorCode::Unsupported);

    // A 12d archive's header says nothing its contents do not; it is
    // described by importing it.
    const auto archive = dir.file("job.12da");
    writeText(archive, "// 12d archive\n");
    EXPECT_EQ(describeSource(archive).error().code, ErrorCode::Unsupported);

    const auto brokenRaster = dir.file("broken.tif");
    writeText(brokenRaster, "not a raster at all");
    EXPECT_EQ(describeSource(brokenRaster).error().code, ErrorCode::FileImportFailure);

    const auto brokenCloud = dir.file("broken.las");
    writeText(brokenCloud, "not a point cloud at all");
    EXPECT_EQ(describeSource(brokenCloud).error().code, ErrorCode::FileImportFailure);
}

// ---- formatDescription --------------------------------------------------------------------

TEST(DatasetInfo, CountsAreGroupedInThousandsWhateverTheLocale)
{
    const auto cloudText = [](std::uint64_t count) {
        SourceDescription d;
        d.path = "scan.laz";
        d.kind = SourceKind::PointCloud;
        d.pointCloud = PointCloudDescription{};
        d.pointCloud->pointCount = count;
        return formatDescription(d);
    };
    EXPECT_TRUE(contains(cloudText(0), "0 points"));
    EXPECT_TRUE(contains(cloudText(1), "1 point\n"));
    EXPECT_TRUE(contains(cloudText(999), "999 points"));
    EXPECT_TRUE(contains(cloudText(1000), "1,000 points"));
    EXPECT_TRUE(contains(cloudText(1234567), "1,234,567 points"));
    EXPECT_TRUE(contains(cloudText(18446744073709551615ull), "18,446,744,073,709,551,615 points"));
    // No bounds in a header is said, not printed as infinities.
    EXPECT_TRUE(contains(cloudText(5), "Bounds: none declared"));
}

TEST(DatasetInfo, CoordinatesArePrintedToTheMillimetre)
{
    SourceDescription d;
    d.path = "site.tif";
    d.kind = SourceKind::Raster;
    d.raster = RasterDescription{};
    d.raster->georeferenced = true;
    d.raster->pixelWidth = 0.5;
    d.raster->pixelHeight = 0.5;
    d.raster->bounds = Box2(Point2(255440.1234, 7410850.4567), Point2(255500.0, 7410900.9996));
    const std::string text = formatDescription(d);
    // 0.1234 -> .123, 0.4567 -> .457, 0.9996 -> 1.000 (carrying into the unit).
    EXPECT_TRUE(contains(text, "(255440.123, 7410850.457) to (255500.000, 7410901.000)")) << text;
    EXPECT_TRUE(contains(text, "Pixel size: 0.5 x 0.5")) << text;
}

TEST(DatasetInfo, LayersThatDisagreeAreNotReportedAsHavingNoCrs)
{
    SourceDescription d;
    d.path = "mixed.gpkg";
    d.kind = SourceKind::Vector;
    d.vectorLayers.push_back({"a", 1, "Point", kMga56Name});
    d.vectorLayers.push_back({"b", 2, "LineString", "WGS 84 (EPSG:4326)"});
    const std::string text = formatDescription(d);
    EXPECT_FALSE(contains(text, "Coordinate system: no coordinate system declared")) << text;
    EXPECT_TRUE(contains(text, "differs between layers")) << text;
    EXPECT_TRUE(contains(text, "b: 2 features, LineString, WGS 84 (EPSG:4326)")) << text;
}

// ---- GDAL's own reading: fields, bands, /vsi paths ---------------------------------------------

TEST(DatasetInfo, TheFieldsAndBandsAreReadFromGdalsOwnDescription)
{
    // A layer whose fields GDAL's GeoJSON driver types from their values: a
    // whole number Integer, a fraction Real (the driver's documentation).
    const TempDir dir("fields");
    const auto path = dir.file("plots.geojson");
    writeText(path, R"({"type":"FeatureCollection","name":"plots","features":[
        {"type":"Feature","properties":{"plot":"P1","count":3,"area":12.5},
         "geometry":{"type":"Point","coordinates":[1,2]}}]})");
    const auto described = describeSource(path);
    ASSERT_TRUE(described.ok()) << described.error().describe();
    ASSERT_EQ(described->vectorLayers.size(), 1u);
    const std::vector<FieldDescription>& fields = described->vectorLayers.front().fields;
    ASSERT_EQ(fields.size(), 3u);
    EXPECT_EQ(fields[0].name, "plot");
    EXPECT_EQ(fields[0].type, "String");
    EXPECT_EQ(fields[1].type, "Integer");
    EXPECT_EQ(fields[2].type, "Real");
    EXPECT_EQ(described->vectorLayers.front().extent.min, Point2(1.0, 2.0));
    EXPECT_FALSE(described->vectorJson.empty());
    EXPECT_TRUE(described->rasterJson.empty());

    // A band: its type and no-data, and no statistics until asked for.
    const auto raster = dir.file("dem.tif");
    gis::RasterExportOptions options;
    options.width = 3;
    options.height = 2;
    options.geotransform = {1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0};
    options.noDataValue = -9999.0;
    writeRaster(raster, options);
    const auto band = describeSource(raster);
    ASSERT_TRUE(band.ok()) << band.error().describe();
    ASSERT_EQ(band->raster->bands.size(), 1u);
    EXPECT_EQ(band->raster->bands.front().band, 1);
    EXPECT_EQ(band->raster->bands.front().dataType, "Float64");
    EXPECT_EQ(band->raster->bands.front().noData, -9999.0);
    EXPECT_FALSE(band->raster->bands.front().min.has_value());
    // writeRaster wrote every cell 1.0: the statistics are exactly that.
    DescribeOptions statistics;
    statistics.statistics = true;
    const auto computed = describeSource(raster, statistics);
    ASSERT_TRUE(computed.ok()) << computed.error().describe();
    const BandDescription& one = computed->raster->bands.front();
    EXPECT_EQ(one.min, 1.0);
    EXPECT_EQ(one.max, 1.0);
    EXPECT_EQ(one.mean, 1.0);
    EXPECT_EQ(one.stdDev, 0.0);
    EXPECT_FALSE(std::filesystem::exists(dir.file("dem.tif.aux.xml")));
}

TEST(DatasetInfo, AVsiPathIsGdalsToFindAndIsDescribed)
{
    // Inside a zip: no such file is on the disk, and GDAL reads it all the
    // same, so it is not refused as missing first.
    const TempDir dir("vsizip");
    const auto archive = dir.file("control.zip");
    ASSERT_TRUE(gis::writeZip(archive, "control.geojson",
                              R"({"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"name":"CP1"},"geometry":{"type":"Point","coordinates":[5,6]}}]})")
                    .ok());
    const std::filesystem::path inside = "/vsizip/" + archive.generic_string() + "/control.geojson";
    EXPECT_TRUE(isVirtualPath(inside));
    EXPECT_FALSE(isVirtualPath(archive));
    const auto described = describeSource(inside);
    ASSERT_TRUE(described.ok()) << described.error().describe();
    ASSERT_EQ(described->vectorLayers.size(), 1u);
    EXPECT_EQ(described->vectorLayers.front().featureCount, 1u);

    const auto missing = describeSource(dir.file("absent.geojson"));
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
}
