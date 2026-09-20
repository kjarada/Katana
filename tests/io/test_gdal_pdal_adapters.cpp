// The GDAL and PDAL adapters (PLAN.MD Phases 17 and 20).
//
// These are the seam with two large third-party libraries, so the tests drive
// real files rather than mocks: what needs verifying is how GDAL and PDAL
// actually behave, not how we imagine they do.
//
// Failures are reported as Result, never thrown. Both libraries signal errors by
// throwing or by out-of-band error codes, and confining that to these adapters
// is the whole point of Rule 4.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <filesystem>

#include "katana/gis/gdal_adapter.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"

using katana::core::ErrorCode;

namespace {

// Unique per test so the suite can run in parallel with itself.
class TempFile {
  public:
    explicit TempFile(const char* name)
        : path_(std::filesystem::temp_directory_path() / (std::string("katana-io-") + name))
    {
        remove();
    }
    ~TempFile() { remove(); }
    TempFile(const TempFile&) = delete;
    TempFile& operator=(const TempFile&) = delete;

    [[nodiscard]] const std::filesystem::path& path() const { return path_; }

  private:
    void remove()
    {
        std::error_code error;
        std::filesystem::remove(path_, error);
        // Sidecars of the shapefile family.
        for (const char* extension : {".shx", ".dbf", ".prj", ".cpg"}) {
            std::filesystem::path sidecar = path_;
            sidecar.replace_extension(extension);
            std::filesystem::remove(sidecar, error);
        }
    }

    std::filesystem::path path_;
};

} // namespace

// ---- point cloud ----------------------------------------------------------------------------

using namespace katana::pointcloud;

TEST(PointCloudEngine, RejectsZeroDecimationStepWithoutThrowing)
{
    PointCloudReadOptions options;
    options.decimationStep = 0;
    const auto result = PointCloudEngine{}.read("missing.las", options);
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
}

TEST(PointCloudEngine, MissingFileIsNotFound)
{
    const auto result = PointCloudEngine{}.read("definitely-not-here.las");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::NotFound);

    const auto header = PointCloudEngine{}.readHeader("definitely-not-here.las");
    ASSERT_FALSE(header.ok());
    EXPECT_EQ(header.error().code, ErrorCode::NotFound);
}

TEST(PointCloudEngine, EmptyBoundsAreEmptyNotZeroSized)
{
    // A default-constructed bounds must report empty rather than claiming to be
    // a degenerate box at the origin, or an empty cloud would drag Zoom Extents
    // to 0,0.
    PointCloud cloud;
    EXPECT_TRUE(cloud.points.empty());
    EXPECT_TRUE(cloud.bounds.empty());
}

TEST(PointCloudEngine, DecimationForBudgetRoundsUpAndNeverReturnsZero)
{
    // A step of 0 would be an infinite loop or a division by zero downstream, so
    // the floor is 1 in every case.
    EXPECT_EQ(PointCloudEngine::decimationForBudget(0, 1000), 1u);
    EXPECT_EQ(PointCloudEngine::decimationForBudget(500, 1000), 1u);
    EXPECT_EQ(PointCloudEngine::decimationForBudget(1000, 1000), 1u);
    EXPECT_EQ(PointCloudEngine::decimationForBudget(1000, 0), 1u); // 0 = no budget

    // Rounded UP, so the result is at or under the budget rather than just over:
    // 2001/1000 must be 3, not 2, since a step of 2 would yield 1001 points.
    EXPECT_EQ(PointCloudEngine::decimationForBudget(2001, 1000), 3u);
    EXPECT_EQ(PointCloudEngine::decimationForBudget(2000, 1000), 2u);

    // A billion-point file against a two-million budget: this is the case
    // Phase 17 exists for.
    const std::uint32_t step = PointCloudEngine::decimationForBudget(1'000'000'000, 2'000'000);
    EXPECT_EQ(step, 500u);
    EXPECT_LE(1'000'000'000ull / step, 2'000'000ull);
}

TEST(PointCloudEngine, WritesAndReadsLasPreservingFields)
{
    const TempFile file("cloud.las");
    PointCloud source;
    source.points.push_back({1.0, 2.0, 3.0, 10.0, 2, 0, 0, 0, false});
    source.points.push_back({4.0, 5.0, 6.0, 20.0, 5, 0, 0, 0, false});

    const PointCloudEngine engine;
    const auto written = engine.write(file.path(), source);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    ASSERT_TRUE(std::filesystem::exists(file.path()));

    const auto loaded = engine.read(file.path());
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    ASSERT_EQ(loaded->points.size(), 2u);

    // LAS stores coordinates as scaled integers, so exact equality is not
    // guaranteed by the format; the default scale is 0.001, and these values
    // are chosen to be representable.
    EXPECT_NEAR(loaded->points[0].x, 1.0, 1e-6);
    EXPECT_NEAR(loaded->points[1].z, 6.0, 1e-6);
    EXPECT_EQ(loaded->points[0].classification, 2);
    EXPECT_EQ(loaded->points[1].classification, 5);

    // Bounds are derived from the points that were actually read.
    EXPECT_NEAR(loaded->bounds.minX, 1.0, 1e-6);
    EXPECT_NEAR(loaded->bounds.maxX, 4.0, 1e-6);
    EXPECT_FALSE(loaded->bounds.empty());
}

TEST(PointCloudEngine, ColorSurvivesTheRoundTrip)
{
    const TempFile file("colour.las");
    PointCloud source;
    source.points.push_back({0.0, 0.0, 0.0, 1.0, 2, 255, 128, 0, true});

    const PointCloudEngine engine;
    ASSERT_TRUE(engine.write(file.path(), source).ok());
    const auto loaded = engine.read(file.path());
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    ASSERT_EQ(loaded->points.size(), 1u);

    // Colour goes out as 16-bit and comes back scaled to 8-bit, so it is
    // preserved to within the rounding of that conversion rather than exactly.
    EXPECT_TRUE(loaded->points[0].hasColor);
    EXPECT_NEAR(loaded->points[0].red, 255, 1);
    EXPECT_NEAR(loaded->points[0].green, 128, 1);
    EXPECT_EQ(loaded->points[0].blue, 0);
}

TEST(PointCloudEngine, HeaderReportsTheCountWithoutReadingPoints)
{
    const TempFile file("header.las");
    PointCloud source;
    for (int i = 0; i < 500; ++i) {
        source.points.push_back({static_cast<double>(i), 0.0, 0.0, 0.0, 2, 0, 0, 0, false});
    }
    const PointCloudEngine engine;
    ASSERT_TRUE(engine.write(file.path(), source).ok());

    const auto header = engine.readHeader(file.path());
    ASSERT_TRUE(header.ok()) << header.error().describe();
    EXPECT_EQ(header->pointCount, 500u);
    EXPECT_NEAR(header->bounds.maxX, 499.0, 1e-6);
}

TEST(PointCloudEngine, DecimationAndClassificationFiltersApply)
{
    const TempFile file("filtered.las");
    PointCloud source;
    for (int i = 0; i < 100; ++i) {
        source.points.push_back(
            {static_cast<double>(i), 0.0, 0.0, 0.0,
             static_cast<std::uint8_t>(i % 2 == 0 ? 2 : 5), 0, 0, 0, false});
    }
    const PointCloudEngine engine;
    ASSERT_TRUE(engine.write(file.path(), source).ok());

    PointCloudReadOptions groundOnly;
    groundOnly.classification = 2;
    const auto ground = engine.read(file.path(), groundOnly);
    ASSERT_TRUE(ground.ok()) << ground.error().describe();
    EXPECT_EQ(ground->points.size(), 50u);
    EXPECT_TRUE(std::all_of(ground->points.begin(), ground->points.end(),
                            [](const PointCloudPoint& p) { return p.classification == 2; }));
    // The source count reports the FILE's size, not the filtered result, so a
    // caller can say "50 of 100" honestly.
    EXPECT_EQ(ground->sourcePointCount, 100u);

    PointCloudReadOptions everyTenth;
    everyTenth.decimationStep = 10;
    const auto sampled = engine.read(file.path(), everyTenth);
    ASSERT_TRUE(sampled.ok()) << sampled.error().describe();
    EXPECT_LE(sampled->points.size(), 11u);
    EXPECT_GE(sampled->points.size(), 9u);
}

TEST(PointCloudEngine, RefusesToWriteAnEmptyCloud)
{
    const TempFile file("empty.las");
    const auto status = PointCloudEngine{}.write(file.path(), PointCloud{});
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    EXPECT_FALSE(std::filesystem::exists(file.path()));
}

// ---- GDAL raster ------------------------------------------------------------------------------

namespace gis = katana::gis;

TEST(GdalAdapter, MissingFileIsNotFound)
{
    const auto dataset = gis::GdalDataset::open("definitely-not-here.tif");
    ASSERT_FALSE(dataset.ok());
    EXPECT_EQ(dataset.error().code, ErrorCode::NotFound);
}

TEST(GdalAdapter, WritesAndReadsRaster)
{
    const TempFile file("raster.tif");
    gis::RasterExportOptions options;
    options.width = 2;
    options.height = 2;
    options.geotransform = {100.0, 1.0, 0.0, 200.0, 0.0, -1.0};

    ASSERT_TRUE(gis::GdalDataset::writeRaster(file.path(), options, {1.0, 2.0, 3.0, 4.0}).ok());

    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok()) << dataset.error().describe();
    EXPECT_TRUE((*dataset)->hasRaster());

    const auto info = (*dataset)->rasterInfo();
    ASSERT_TRUE(info.ok()) << info.error().describe();
    EXPECT_EQ(info->width, 2);
    EXPECT_EQ(info->height, 2);
    EXPECT_TRUE(info->hasGeotransform);
    EXPECT_DOUBLE_EQ(info->geotransform[0], 100.0);
    EXPECT_DOUBLE_EQ(info->geotransform[5], -1.0);

    const auto values = (*dataset)->readBand(1);
    ASSERT_TRUE(values.ok()) << values.error().describe();
    ASSERT_EQ(values->size(), 4u);
    EXPECT_DOUBLE_EQ((*values)[2], 3.0);
}

TEST(GdalAdapter, BandIndexOutOfRangeIsRejected)
{
    const TempFile file("oneband.tif");
    gis::RasterExportOptions options;
    options.width = options.height = 1;
    ASSERT_TRUE(gis::GdalDataset::writeRaster(file.path(), options, {1.0}).ok());

    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok());
    for (const int index : {0, 2, -1}) {
        const auto values = (*dataset)->readBand(index);
        ASSERT_FALSE(values.ok()) << "band " << index << " should not be readable";
        EXPECT_EQ(values.error().code, ErrorCode::InvalidArgument);
    }
}

TEST(GdalAdapter, RasterExportChecksItsDimensions)
{
    const TempFile file("bad.tif");
    gis::RasterExportOptions options;
    options.width = 3;
    options.height = 3;
    // Nine cells claimed, four supplied.
    const auto status = gis::GdalDataset::writeRaster(file.path(), options, {1.0, 2.0, 3.0, 4.0});
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
}

TEST(GdalAdapter, DisplayImageIsDecimatedAndKeepsItsGeoreferencing)
{
    const TempFile file("big.tif");
    constexpr int kSize = 64;
    gis::RasterExportOptions options;
    options.width = kSize;
    options.height = kSize;
    options.geotransform = {1000.0, 2.0, 0.0, 5000.0, 0.0, -2.0};
    std::vector<double> values(static_cast<std::size_t>(kSize) * kSize);
    for (std::size_t i = 0; i < values.size(); ++i) {
        values[i] = static_cast<double>(i % 256);
    }
    ASSERT_TRUE(gis::GdalDataset::writeRaster(file.path(), options, values).ok());

    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok());

    const auto image = (*dataset)->readImage(16);
    ASSERT_TRUE(image.ok()) << image.error().describe();
    EXPECT_LE(image->width, 16);
    EXPECT_LE(image->height, 16);
    EXPECT_EQ(image->rgba.size(),
              static_cast<std::size_t>(image->width) * image->height * 4);

    // The geotransform must be rescaled for the decimated grid, or the overlay
    // would be drawn at a quarter of its true size. The image still has to span
    // the same ground: origin unchanged, and width * pixelSize preserved.
    EXPECT_DOUBLE_EQ(image->geotransform[0], 1000.0);
    EXPECT_DOUBLE_EQ(image->geotransform[3], 5000.0);
    EXPECT_DOUBLE_EQ(image->width * image->geotransform[1], kSize * 2.0);
    EXPECT_DOUBLE_EQ(image->height * image->geotransform[5], kSize * -2.0);

    const auto full = (*dataset)->readImage(4096);
    ASSERT_TRUE(full.ok()) << full.error().describe();
    EXPECT_EQ(full->width, kSize) << "a raster below the budget is not decimated at all";
    EXPECT_DOUBLE_EQ(full->geotransform[1], 2.0);
}

TEST(GdalAdapter, ReadImageRejectsANonsensePixelBudget)
{
    const TempFile file("tiny.tif");
    gis::RasterExportOptions options;
    options.width = options.height = 1;
    ASSERT_TRUE(gis::GdalDataset::writeRaster(file.path(), options, {1.0}).ok());
    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok());

    const auto image = (*dataset)->readImage(0);
    ASSERT_FALSE(image.ok());
    EXPECT_EQ(image.error().code, ErrorCode::InvalidArgument);
}

// ---- GDAL vector ------------------------------------------------------------------------------

TEST(GdalAdapter, WritesAndReadsVectorFeatures)
{
    const TempFile file("points.geojson");
    gis::VectorFeature point;
    point.geometry.kind = gis::GeometryKind::Point;
    point.geometry.parts.push_back({gis::GeoPoint{1.0, 2.0, 0.0}});
    point.attributes.emplace("name", "origin");

    gis::VectorExportOptions options;
    options.layerName = "points";
    ASSERT_TRUE(gis::GdalDataset::writeVector(file.path(), {point}, options).ok());

    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok()) << dataset.error().describe();
    EXPECT_TRUE((*dataset)->hasVector());

    const auto layers = (*dataset)->vectorLayers();
    ASSERT_TRUE(layers.ok()) << layers.error().describe();
    ASSERT_EQ(layers->size(), 1u);
    EXPECT_EQ(layers->front().featureCount, 1u);

    // Reading the GEOMETRY back, not merely the layer's metadata: the adapter
    // previously reported layer names and nothing else, which made importing
    // vector data impossible.
    const auto features = (*dataset)->readFeatures(0);
    ASSERT_TRUE(features.ok()) << features.error().describe();
    ASSERT_EQ(features->size(), 1u);
    EXPECT_EQ(features->front().geometry.kind, gis::GeometryKind::Point);
    ASSERT_EQ(features->front().geometry.parts.size(), 1u);
    ASSERT_EQ(features->front().geometry.parts.front().size(), 1u);
    EXPECT_DOUBLE_EQ(features->front().geometry.parts.front().front().x, 1.0);

    const auto name = features->front().attributes.find("name");
    ASSERT_NE(name, features->front().attributes.end());
    EXPECT_EQ(name->second, "origin");
}

TEST(GdalAdapter, PolygonRingsRoundTripWithTheirHoles)
{
    const TempFile file("rings.geojson");
    gis::VectorFeature polygon;
    polygon.geometry.kind = gis::GeometryKind::Polygon;
    polygon.geometry.parts.push_back({{0, 0, 0}, {10, 0, 0}, {10, 10, 0}, {0, 10, 0}});
    polygon.geometry.parts.push_back({{3, 3, 0}, {7, 3, 0}, {7, 7, 0}, {3, 7, 0}}); // hole

    ASSERT_TRUE(gis::GdalDataset::writeVector(file.path(), {polygon}, {}).ok());

    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok());
    const auto features = (*dataset)->readFeatures(0);
    ASSERT_TRUE(features.ok()) << features.error().describe();
    ASSERT_EQ(features->size(), 1u);
    EXPECT_EQ(features->front().geometry.kind, gis::GeometryKind::Polygon);
    ASSERT_EQ(features->front().geometry.parts.size(), 2u) << "the hole must survive";
    // Rings come back closed, so the first point repeats at the end.
    EXPECT_EQ(features->front().geometry.parts[0].size(), 5u);
}

TEST(GdalAdapter, LayerIndexOutOfRangeIsRejected)
{
    const TempFile file("one.geojson");
    gis::VectorFeature point;
    point.geometry.kind = gis::GeometryKind::Point;
    point.geometry.parts.push_back({gis::GeoPoint{0.0, 0.0, 0.0}});
    ASSERT_TRUE(gis::GdalDataset::writeVector(file.path(), {point}, {}).ok());

    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok());
    for (const int index : {-1, 1, 99}) {
        const auto features = (*dataset)->readFeatures(index);
        ASSERT_FALSE(features.ok()) << "layer " << index;
        EXPECT_EQ(features.error().code, ErrorCode::InvalidArgument);
    }
}

TEST(GdalAdapter, DriverIsChosenByExtensionAndUnknownOnesAreNamed)
{
    const auto shapefile = gis::GdalDataset::vectorDriverForPath("a.shp");
    ASSERT_TRUE(shapefile.ok());
    EXPECT_EQ(*shapefile, "ESRI Shapefile");

    const auto geojson = gis::GdalDataset::vectorDriverForPath("a.GeoJSON");
    ASSERT_TRUE(geojson.ok()) << "extension matching must be case insensitive";
    EXPECT_EQ(*geojson, "GeoJSON");

    const auto unknown = gis::GdalDataset::vectorDriverForPath("a.xyzzy");
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::Unsupported);
    EXPECT_NE(unknown.error().message.find("xyzzy"), std::string::npos);

    const auto none = gis::GdalDataset::vectorDriverForPath("a");
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::InvalidArgument);
}

TEST(GdalAdapter, FormatCapabilitiesAreReported)
{
    // A shapefile fixes its shape type in the header; GeoPackage and GeoJSON do
    // not. The exporter relies on this to refuse a mixed collection up front
    // rather than failing halfway through writing one.
    EXPECT_TRUE(gis::driverHoldsOneGeometryType("ESRI Shapefile"));
    EXPECT_FALSE(gis::driverHoldsOneGeometryType("GPKG"));
    EXPECT_FALSE(gis::driverHoldsOneGeometryType("GeoJSON"));

    // RFC 7946 pins GeoJSON to WGS 84, so coordinates in any other system are
    // silently relabelled by it and the user has to be warned.
    EXPECT_TRUE(gis::driverAssumesWgs84("GeoJSON"));
    EXPECT_FALSE(gis::driverAssumesWgs84("GPKG"));
}

TEST(GdalAdapter, SeveralDatasetsCanBeOpenAtOnce)
{
    // Regression. The destructor used to call GDALDestroyDriverManager(), which
    // tears down state shared by the whole process: closing one dataset broke
    // every other open dataset and every subsequent open.
    const TempFile first("a.geojson");
    const TempFile second("b.geojson");
    gis::VectorFeature point;
    point.geometry.kind = gis::GeometryKind::Point;
    point.geometry.parts.push_back({gis::GeoPoint{1.0, 1.0, 0.0}});
    ASSERT_TRUE(gis::GdalDataset::writeVector(first.path(), {point}, {}).ok());
    ASSERT_TRUE(gis::GdalDataset::writeVector(second.path(), {point}, {}).ok());

    auto a = gis::GdalDataset::open(first.path());
    ASSERT_TRUE(a.ok());
    {
        auto b = gis::GdalDataset::open(second.path());
        ASSERT_TRUE(b.ok());
    } // b closes here

    // a must still work...
    const auto features = (*a)->readFeatures(0);
    EXPECT_TRUE(features.ok()) << "closing one dataset broke another: " << features.error().describe();

    // ...and opening another afterwards must still work.
    const auto c = gis::GdalDataset::open(second.path());
    EXPECT_TRUE(c.ok()) << "the driver manager was destroyed: " << c.error().describe();
}

TEST(GdalAdapter, VersionsAreReported)
{
    EXPECT_FALSE(gis::gdalVersion().empty());
    EXPECT_FALSE(pdalVersion().empty());
}
