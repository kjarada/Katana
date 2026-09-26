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

#include <cpl_conv.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <string>
#include <vector>

#include "katana/core/library_data.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"

using katana::core::ErrorCode;

namespace {

std::filesystem::path auxiliaryOf(const std::filesystem::path& path)
{
    std::filesystem::path aux = path;
    aux += ".aux.xml";
    return aux;
}

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
        // Sidecars of the shapefile family, and the .prj an Esri ASCII grid
        // keeps its coordinate system in.
        for (const char* extension : {".shx", ".dbf", ".prj", ".cpg"}) {
            std::filesystem::path sidecar = path_;
            sidecar.replace_extension(extension);
            std::filesystem::remove(sidecar, error);
        }
        // GDAL's auxiliary metadata, which is named by APPENDING to the whole
        // file name (dem.tif.aux.xml). One left behind by a run of an older
        // build would be trusted by the next run (audit IO-13), so it goes too.
        std::filesystem::remove(auxiliaryOf(path_), error);
    }

    std::filesystem::path path_;
};

} // namespace

// ---- point cloud ----------------------------------------------------------------------------

using namespace katana::pointcloud;

// A relocated Linux or macOS tree (docs/release.md) keeps GDAL's data in
// share/gdal and libcurl's certificates in ssl/cacert.pem beside the lib/ the
// libraries are in, while the paths compiled into them name the machine that
// built them. Registering GDAL points it at both, unless the environment
// already has. Where to look is worked out here from where the libraries
// were loaded, independently of the adapter.
namespace {

#if defined(__APPLE__)
constexpr const char* kGdalLibrary = "libgdal.";
constexpr const char* kCurlLibrary = "libcurl.";
#else
constexpr const char* kGdalLibrary = "libgdal.so";
constexpr const char* kCurlLibrary = "libcurl.so";
#endif

} // namespace

TEST(GdalAdapter, GdalIsPointedAtTheDataBesideItsLibrary)
{
#if defined(__linux__) || defined(__APPLE__)
    if (std::getenv("GDAL_DATA") != nullptr) {
        GTEST_SKIP() << "GDAL_DATA names GDAL's data already";
    }
    (void)katana::gis::gdalVersion(); // registers GDAL
    const auto data = katana::core::dataBesideLibrary(kGdalLibrary, "share/gdal");
    if (!data) {
        GTEST_SKIP() << "a distribution's GDAL: no share/gdal beside its directory";
    }
    const char* given = CPLGetConfigOption("GDAL_DATA", nullptr);
    ASSERT_NE(given, nullptr);
    EXPECT_TRUE(std::filesystem::equivalent(given, *data)) << given;
#else
    GTEST_SKIP() << "Windows finds GDAL's data by the DLL's name";
#endif
}

TEST(GdalAdapter, GdalIsPointedAtTheCertificatesBesideLibcurl)
{
#if defined(__linux__) || defined(__APPLE__)
    if (std::getenv("CURL_CA_BUNDLE") != nullptr || std::getenv("SSL_CERT_FILE") != nullptr) {
        GTEST_SKIP() << "the environment names the certificates already";
    }
    (void)katana::gis::gdalVersion(); // registers GDAL
    const auto certificates = katana::core::dataBesideLibrary(kCurlLibrary, "ssl/cacert.pem");
    if (!certificates) {
        GTEST_SKIP() << "a distribution's libcurl: no ssl/cacert.pem beside its directory";
    }
    const char* given = CPLGetConfigOption("CURL_CA_BUNDLE", nullptr);
    ASSERT_NE(given, nullptr);
    EXPECT_TRUE(std::filesystem::equivalent(given, *certificates)) << given;
#else
    GTEST_SKIP() << "MSYS2's libcurl finds the certificates the deploy puts beside it";
#endif
}

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
    // guaranteed by the format. The writer sets a 0.001 scale (PDAL's own
    // default is 0.01; audit IO-17), and whole numbers are representable at
    // either - SurveyCoordinatesSurviveToTheMillimetre is the test of the scale.
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

// ---- COPC: level of detail as a query (PLAN.MD Phase 17) -------------------------------------

namespace {

// A 100 m square of ground at 0.25 m spacing: 160 000 points.
//
// The spacing is what makes this a test of level of detail at all. A COPC
// octree node keeps at most one point per cell of a 128-cell grid across its
// span, so on a 100 m root a point every 0.78 m or coarser lands ENTIRELY in
// the root node, and every resolution query then returns the whole file - a
// fixture that could never show a coarse read being coarser. A quarter of a
// metre forces the octree to at least two further levels.
PointCloud denseGround()
{
    PointCloud cloud;
    cloud.points.reserve(400u * 400u);
    for (int j = 0; j < 400; ++j) {
        for (int i = 0; i < 400; ++i) {
            const double x = 0.25 * i;
            const double y = 0.25 * j;
            cloud.points.push_back({x, y, 10.0 + 0.01 * x, 1.0, 2, 0, 0, 0, false});
        }
    }
    return cloud;
}

} // namespace

TEST(PointCloudEngine, ACoarseCopcQueryReturnsFewerPointsOverTheSameExtent)
{
    const TempFile las("lod-source.las");
    const TempFile copc("lod.copc.laz");
    const PointCloudEngine engine;
    ASSERT_TRUE(engine.write(las.path(), denseGround()).ok());
    const auto converted = engine.convertToCopc(las.path(), copc.path());
    ASSERT_TRUE(converted.ok()) << converted.error().describe();

    // Converting loses nothing: with no resolution asked, every point is there.
    const auto full = engine.read(copc.path());
    ASSERT_TRUE(full.ok()) << full.error().describe();
    EXPECT_EQ(full->points.size(), 160000u);

    PointCloudReadOptions coarse;
    coarse.resolution = 5.0;
    PointCloudReadOptions medium;
    medium.resolution = 0.5;
    const auto coarseRead = engine.read(copc.path(), coarse);
    const auto mediumRead = engine.read(copc.path(), medium);
    ASSERT_TRUE(coarseRead.ok()) << coarseRead.error().describe();
    ASSERT_TRUE(mediumRead.ok()) << mediumRead.error().describe();

    // The properties of level of detail, not the counts one PDAL version
    // happens to produce: coarser is never more, the coarse read is a real
    // reduction, and it is still a sample of the WHOLE extent rather than the
    // first corner of it - which is the difference between a level of detail
    // and the `maxPoints` truncation this replaces.
    EXPECT_GT(coarseRead->points.size(), 0u);
    EXPECT_LE(coarseRead->points.size(), mediumRead->points.size());
    EXPECT_LE(mediumRead->points.size(), full->points.size());
    EXPECT_LT(coarseRead->points.size() * 4u, full->points.size())
        << "a 5 m query on 0.25 m data should be a small fraction of it";
    EXPECT_LT(coarseRead->bounds.minX, 10.0);
    EXPECT_GT(coarseRead->bounds.maxX, 90.0);
    EXPECT_LT(coarseRead->bounds.minY, 10.0);
    EXPECT_GT(coarseRead->bounds.maxY, 90.0);
    // The honest count is unchanged by the query: it is what the FILE holds.
    EXPECT_EQ(coarseRead->sourcePointCount, 160000u);
}

TEST(PointCloudEngine, OnlyACopcFileIsReportedAsOne)
{
    const TempFile las("plain.las");
    const TempFile copc("real.copc.laz");
    const PointCloudEngine engine;
    ASSERT_TRUE(engine.write(las.path(), denseGround()).ok());
    ASSERT_TRUE(engine.convertToCopc(las.path(), copc.path()).ok());

    const auto plain = engine.isCopc(las.path());
    ASSERT_TRUE(plain.ok());
    EXPECT_FALSE(*plain);
    const auto real = engine.isCopc(copc.path());
    ASSERT_TRUE(real.ok()) << real.error().describe();
    EXPECT_TRUE(*real);
    EXPECT_EQ(engine.isCopc("definitely-not-here.copc.laz").error().code, ErrorCode::NotFound);
}

TEST(PointCloudEngine, AResolutionAskedOfAPlainLasIsRefusedRatherThanIgnored)
{
    // readers.las has no resolution option. Passing one through would return
    // the whole file to a caller who asked for a coarse sample of a
    // billion-point cloud - the silent failure PLAN.MD section 36 forbids.
    const TempFile las("no-lod.las");
    const PointCloudEngine engine;
    ASSERT_TRUE(engine.write(las.path(), denseGround()).ok());
    PointCloudReadOptions options;
    options.resolution = 5.0;
    const auto refused = engine.read(las.path(), options);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().describe().find("COPC"), std::string::npos)
        << refused.error().describe();
}

TEST(PointCloudEngine, CopcConversionRefusesWhatItCannotDo)
{
    const TempFile las("convert-source.las");
    const TempFile copc("convert.copc.laz");
    const TempFile wrongName("convert-wrong.laz");
    const PointCloudEngine engine;
    ASSERT_TRUE(engine.write(las.path(), denseGround()).ok());

    // The extension is what makes every later read infer readers.copc; a COPC
    // file called .laz would be read as plain LAZ and never answer a query.
    EXPECT_EQ(engine.convertToCopc(las.path(), wrongName.path()).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_FALSE(std::filesystem::exists(wrongName.path()));
    EXPECT_EQ(engine.convertToCopc("definitely-not-here.las", copc.path()).error().code,
              ErrorCode::NotFound);

    ASSERT_TRUE(engine.convertToCopc(las.path(), copc.path()).ok());
    PointCloudReadOptions zero;
    zero.resolution = 0.0;
    EXPECT_EQ(engine.read(copc.path(), zero).error().code, ErrorCode::InvalidArgument);
    PointCloudReadOptions negative;
    negative.resolution = -1.0;
    EXPECT_EQ(engine.read(copc.path(), negative).error().code, ErrorCode::InvalidArgument);
}

// ---- coordinate resolution (audit IO-17) -------------------------------------------------------

namespace {

// Projected coordinates of survey magnitude - an MGA northing is seven digits -
// quoted to the millimetre, with fractions that no centimetre grid holds. More
// than one point, and not all on one grid line: with an automatic offset a
// LONE point is stored as zero from its own offset and comes back exact at any
// scale, which would make the test pass over the defect.
PointCloud surveyMagnitudeCloud()
{
    PointCloud cloud;
    cloud.points.push_back({255440.1234, 7410850.4567, 12.3456, 1.0, 2, 0, 0, 0, false});
    cloud.points.push_back({255441.9876, 7410851.0021, 13.0009, 1.0, 2, 0, 0, 0, false});
    cloud.points.push_back({255439.5555, 7410849.7777, 11.1111, 1.0, 2, 0, 0, 0, false});
    return cloud;
}

// ASPRS LAS 1.4 R15 stores a coordinate as round((value - offset) / scale), so
// at a 0.001 scale it comes back within half a millimetre. The 1e-9 is the
// rounding of reconstructing int * scale + offset in doubles: one half-ulp at
// 7.4 x 10^6 is 4.7e-10. PDAL's default 0.01 scale misses it on every one of
// these coordinates, by 0.9 to 4.5 mm (measured with the scale removed:
// 255440.1234 came back as 255440.12).
constexpr double kMillimetreRoundTrip = 0.0005 + 1e-9;

void expectMillimetres(const PointCloud& source, const PointCloud& loaded)
{
    ASSERT_EQ(loaded.points.size(), source.points.size());
    for (const PointCloudPoint& expected : source.points) {
        // Matched by position, not by index: nothing in LAS promises order.
        const auto found = std::find_if(
            loaded.points.begin(), loaded.points.end(), [&expected](const PointCloudPoint& p) {
                return std::abs(p.x - expected.x) < 0.1 && std::abs(p.y - expected.y) < 0.1;
            });
        ASSERT_NE(found, loaded.points.end()) << "no point near x = " << expected.x;
        EXPECT_NEAR(found->x, expected.x, kMillimetreRoundTrip);
        EXPECT_NEAR(found->y, expected.y, kMillimetreRoundTrip);
        EXPECT_NEAR(found->z, expected.z, kMillimetreRoundTrip);
    }
}

} // namespace

TEST(PointCloudEngine, SurveyCoordinatesSurviveToTheMillimetre)
{
    const TempFile file("survey.las");
    const PointCloudEngine engine;
    const PointCloud source = surveyMagnitudeCloud();
    ASSERT_TRUE(engine.write(file.path(), source).ok());
    const auto loaded = engine.read(file.path());
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    expectMillimetres(source, *loaded);
}

TEST(PointCloudEngine, CopcConversionKeepsTheSourcesMillimetres)
{
    // writers.copc defaults to a 0.01 scale too. Converting a millimetre LAS
    // must not coarsen it: the whole point of the conversion is to index the
    // data, not to lose some of it.
    const TempFile las("survey-source.las");
    const TempFile copc("survey.copc.laz");
    const PointCloudEngine engine;
    const PointCloud source = surveyMagnitudeCloud();
    ASSERT_TRUE(engine.write(las.path(), source).ok());
    const auto converted = engine.convertToCopc(las.path(), copc.path());
    ASSERT_TRUE(converted.ok()) << converted.error().describe();
    const auto loaded = engine.read(copc.path());
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    expectMillimetres(source, *loaded);
}

TEST(PointCloudEngine, ALazIsCompressedAndReadsBackAsTheSamePoints)
{
    // The extension is all a caller says; writers.las has to be told to
    // compress, and a .laz that is really an uncompressed LAS is a file every
    // other reader rejects.
    const TempFile las("compress.las");
    const TempFile laz("compress.laz");
    const PointCloudEngine engine;
    const PointCloud source = denseGround();
    ASSERT_TRUE(engine.write(las.path(), source).ok());
    const auto written = engine.write(laz.path(), source);
    ASSERT_TRUE(written.ok()) << written.error().describe();

    const auto lasSize = std::filesystem::file_size(las.path());
    const auto lazSize = std::filesystem::file_size(laz.path());
    // A regular grid compresses by far more than this; half is a floor that
    // no uncompressed file of the same points can meet.
    EXPECT_LT(lazSize * 2, lasSize) << "LAS " << lasSize << " bytes, LAZ " << lazSize;

    const auto fromLas = engine.read(las.path());
    const auto fromLaz = engine.read(laz.path());
    ASSERT_TRUE(fromLas.ok()) << fromLas.error().describe();
    ASSERT_TRUE(fromLaz.ok()) << fromLaz.error().describe();
    ASSERT_EQ(fromLaz->points.size(), source.points.size());
    ASSERT_EQ(fromLaz->points.size(), fromLas->points.size());
    // Compression is lossless: the same integers, so the same doubles.
    for (std::size_t i = 0; i < fromLas->points.size(); ++i) {
        ASSERT_EQ(fromLaz->points[i].x, fromLas->points[i].x) << i;
        ASSERT_EQ(fromLaz->points[i].y, fromLas->points[i].y) << i;
        ASSERT_EQ(fromLaz->points[i].z, fromLas->points[i].z) << i;
    }
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

namespace {

// EPSG:28356, GDA94 / MGA zone 56, as the EPSG registry publishes it in WKT1
// (epsg.io/28356.wkt): its name and code are the independent answer
// describeCrs has to find.
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

// A band whose value names its pixel: 10 * row + column. Every sample
// then says which source pixel it came from, exactly, in any binary format.
std::vector<double> namedPixels(int width, int height)
{
    std::vector<double> values;
    for (int row = 0; row < height; ++row) {
        for (int column = 0; column < width; ++column) {
            values.push_back(10.0 * row + column);
        }
    }
    return values;
}

} // namespace

TEST(GdalAdapter, RasterInfoReportsTheBandsNoDataValueOnlyWhenOneIsDeclared)
{
    const TempFile with("nodata.tif");
    const TempFile without("no-nodata.tif");
    gis::RasterExportOptions options;
    options.width = 2;
    options.height = 1;
    ASSERT_TRUE(gis::GdalDataset::writeRaster(without.path(), options, {1.0, 2.0}).ok());
    options.noDataValue = -9999.0;
    ASSERT_TRUE(gis::GdalDataset::writeRaster(with.path(), options, {1.0, -9999.0}).ok());

    const auto declared = gis::GdalDataset::open(with.path());
    ASSERT_TRUE(declared.ok());
    const auto declaredInfo = (*declared)->rasterInfo();
    ASSERT_TRUE(declaredInfo.ok());
    ASSERT_TRUE(declaredInfo->noDataValue.has_value());
    EXPECT_EQ(*declaredInfo->noDataValue, -9999.0);

    // Absent is not a value: a DEM without a no-data value has every pixel
    // meaningful, and zero is a real height.
    const auto undeclared = gis::GdalDataset::open(without.path());
    ASSERT_TRUE(undeclared.ok());
    const auto undeclaredInfo = (*undeclared)->rasterInfo();
    ASSERT_TRUE(undeclaredInfo.ok());
    EXPECT_FALSE(undeclaredInfo->noDataValue.has_value());
}

TEST(GdalAdapter, TheDriverIsReportedByItsShortName)
{
    const TempFile raster("driver.tif");
    const TempFile vector("driver.geojson");
    gis::RasterExportOptions options;
    options.width = options.height = 1;
    ASSERT_TRUE(gis::GdalDataset::writeRaster(raster.path(), options, {1.0}).ok());
    gis::VectorFeature point;
    point.geometry.kind = gis::GeometryKind::Point;
    point.geometry.parts.push_back({gis::GeoPoint{1.0, 2.0, 0.0}});
    ASSERT_TRUE(gis::GdalDataset::writeVector(vector.path(), {point}, {}).ok());

    EXPECT_EQ((*gis::GdalDataset::open(raster.path()))->driverName(), "GTiff");
    EXPECT_EQ((*gis::GdalDataset::open(vector.path()))->driverName(), "GeoJSON");
}

TEST(GdalAdapter, ASampledReadKeepsExactSourcePixelsOnTheStride)
{
    const TempFile file("sampled.tif");
    gis::RasterExportOptions options;
    options.width = 7;
    options.height = 5;
    ASSERT_TRUE(gis::GdalDataset::writeRaster(file.path(), options, namedPixels(7, 5)).ok());
    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok());

    // Stride 3 keeps columns 0, 3, 6 and rows 0, 3: ceil(7/3) = 3 by
    // ceil(5/3) = 2, and each value is its source pixel's, not an average.
    const auto everyThird = (*dataset)->readBandSampled(1, 3);
    ASSERT_TRUE(everyThird.ok()) << everyThird.error().describe();
    EXPECT_EQ(everyThird->columns, 3);
    EXPECT_EQ(everyThird->rows, 2);
    EXPECT_EQ(everyThird->stride, 3);
    EXPECT_EQ(everyThird->values, (std::vector<double>{0, 3, 6, 30, 33, 36}));
    EXPECT_FALSE(everyThird->noDataValue.has_value());

    // Stride 1 is the whole band, identical to readBand.
    const auto all = (*dataset)->readBandSampled(1, 1);
    ASSERT_TRUE(all.ok());
    EXPECT_EQ(all->columns, 7);
    EXPECT_EQ(all->rows, 5);
    EXPECT_EQ(all->values, *(*dataset)->readBand(1));

    // A stride at or beyond both sides keeps the first pixel alone.
    for (const int stride : {7, 1000}) {
        const auto one = (*dataset)->readBandSampled(1, stride);
        ASSERT_TRUE(one.ok());
        EXPECT_EQ(one->columns, 1);
        EXPECT_EQ(one->rows, 1);
        EXPECT_EQ(one->values, (std::vector<double>{0}));
    }
}

TEST(GdalAdapter, ASampledReadRefusesABadBandOrStrideAndReportsNoData)
{
    const TempFile file("sampled-bad.tif");
    gis::RasterExportOptions options;
    options.width = 2;
    options.height = 2;
    options.noDataValue = -32768.0;
    ASSERT_TRUE(
        gis::GdalDataset::writeRaster(file.path(), options, {1.0, -32768.0, 3.0, 4.0}).ok());
    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok());

    for (const int band : {0, 2, -1}) {
        const auto refused = (*dataset)->readBandSampled(band, 1);
        ASSERT_FALSE(refused.ok()) << "band " << band;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    }
    for (const int stride : {0, -3}) {
        const auto refused = (*dataset)->readBandSampled(1, stride);
        ASSERT_FALSE(refused.ok()) << "stride " << stride;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    }

    // The sentinel is delivered as the file holds it, with the band's value
    // beside it: deciding what no-data means is the caller's business.
    const auto samples = (*dataset)->readBandSampled(1, 1);
    ASSERT_TRUE(samples.ok());
    ASSERT_TRUE(samples->noDataValue.has_value());
    EXPECT_EQ(*samples->noDataValue, -32768.0);
    EXPECT_EQ(samples->values, (std::vector<double>{1.0, -32768.0, 3.0, 4.0}));
}

TEST(GdalAdapter, RasterDriverIsChosenByExtensionAndUnknownOnesAreNamed)
{
    EXPECT_EQ(*gis::GdalDataset::rasterDriverForPath("dem.tif"), "GTiff");
    EXPECT_EQ(*gis::GdalDataset::rasterDriverForPath("dem.TIFF"), "GTiff")
        << "extension matching must be case insensitive";
    EXPECT_EQ(*gis::GdalDataset::rasterDriverForPath("dem.asc"), "AAIGrid");
    EXPECT_EQ(*gis::GdalDataset::rasterDriverForPath("dem.img"), "HFA");

    const auto unknown = gis::GdalDataset::rasterDriverForPath("dem.xyzzy");
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::Unsupported);
    EXPECT_NE(unknown.error().message.find("xyzzy"), std::string::npos);

    const auto none = gis::GdalDataset::rasterDriverForPath("dem");
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::Unsupported);
}

TEST(GdalAdapter, ACoordinateSystemIsDescribedByItsNameAndAuthorityCode)
{
    EXPECT_EQ(gis::describeCrs(kMga56Wkt), "GDA94 / MGA zone 56 (EPSG:28356)");

    // No AUTHORITY node anywhere, as an Esri .prj is written, yet each is an
    // EPSG system by its parameters, and GDAL can say which. The codes are the
    // EPSG registry's: 4326 is WGS 84 geographic (a = 6378137, 1/f =
    // 298.257223563); 28356 is GDA94 / MGA zone 56 (GRS 1980, transverse
    // Mercator, central meridian 153, k 0.9996, false origin 500 000 m E,
    // 10 000 000 m N). `gdalsrsinfo -e` names the same codes.
    constexpr const char* kBareWgs84 =
        R"(GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],)"
        R"(PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]])";
    EXPECT_EQ(gis::describeCrs(kBareWgs84), "WGS 84 (EPSG:4326)");
    constexpr const char* kEsriMga56 =
        R"(PROJCS["GDA_1994_MGA_Zone_56",GEOGCS["GCS_GDA_1994",DATUM["D_GDA_1994",)"
        R"(SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],)"
        R"(UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],)"
        R"(PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",10000000.0],)"
        R"(PARAMETER["Central_Meridian",153.0],PARAMETER["Scale_Factor",0.9996],)"
        R"(PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]])";
    const std::string esri = gis::describeCrs(kEsriMga56);
    EXPECT_NE(esri.find("(EPSG:28356)"), std::string::npos) << esri;

    // A local site grid is no EPSG system: its name, and no invented code.
    EXPECT_EQ(gis::describeCrs(R"(LOCAL_CS["site grid",UNIT["metre",1]])"), "site grid");

    // Nothing declared is nothing; something unreadable is NOT nothing.
    EXPECT_EQ(gis::describeCrs(""), "");
    const std::string unreadable = gis::describeCrs("this is not a coordinate system");
    EXPECT_FALSE(unreadable.empty());
    EXPECT_NE(unreadable.find("unrecognised"), std::string::npos) << unreadable;
}

TEST(GdalAdapter, AFormatThatCanOnlyCopyIsWrittenThroughAnInMemoryDataset)
{
    // AAIGrid has no Create - GDAL can only copy a finished dataset into it.
    // Values exact in binary and at Float32 (AAIGrid reads decimals as
    // Float32 by default), so they must come back exactly.
    const TempFile file("copied.asc");
    gis::RasterExportOptions options;
    options.driver = "AAIGrid";
    options.width = 3;
    options.height = 2;
    options.geotransform = {1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0};
    options.noDataValue = -9999.0;
    options.projectionWkt = kMga56Wkt;
    const auto written = gis::GdalDataset::writeRaster(
        file.path(), options, {10.5, 11.25, -9999.0, 12.125, 13.0, 14.75});
    ASSERT_TRUE(written.ok()) << written.error().describe();
    // The grid and its .prj, and nothing else: GDAL copying from a GeoTIFF
    // leaves a .asc.aux.xml of metadata the format cannot hold, but the
    // in-memory dataset has none to leave.
    EXPECT_FALSE(std::filesystem::exists(auxiliaryOf(file.path())));

    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok()) << dataset.error().describe();
    EXPECT_EQ((*dataset)->driverName(), "AAIGrid");
    const auto info = (*dataset)->rasterInfo();
    ASSERT_TRUE(info.ok());
    EXPECT_EQ(info->width, 3);
    EXPECT_EQ(info->height, 2);
    EXPECT_TRUE(info->hasGeotransform);
    EXPECT_EQ(info->geotransform, (std::array<double, 6>{1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0}));
    ASSERT_TRUE(info->noDataValue.has_value());
    EXPECT_EQ(*info->noDataValue, -9999.0);
    EXPECT_NE(gis::describeCrs(info->projectionWkt).find("MGA zone 56"), std::string::npos)
        << "the .prj beside the grid carries the coordinate system";
    EXPECT_EQ(*(*dataset)->readBand(1),
              (std::vector<double>{10.5, 11.25, -9999.0, 12.125, 13.0, 14.75}));
}

TEST(GdalAdapter, AnErdasImagineRasterIsWrittenWithItsGeoreferencing)
{
    const TempFile file("created.img");
    gis::RasterExportOptions options;
    options.driver = "HFA";
    options.width = 2;
    options.height = 2;
    options.geotransform = {500.0, 0.5, 0.0, 800.0, 0.0, -0.5};
    const auto written = gis::GdalDataset::writeRaster(file.path(), options, {1.5, 2.5, 3.5, 4.5});
    ASSERT_TRUE(written.ok()) << written.error().describe();
    const auto dataset = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(dataset.ok()) << dataset.error().describe();
    EXPECT_EQ((*dataset)->driverName(), "HFA");
    const auto info = (*dataset)->rasterInfo();
    ASSERT_TRUE(info.ok());
    EXPECT_EQ(info->geotransform, (std::array<double, 6>{500.0, 0.5, 0.0, 800.0, 0.0, -0.5}));
    EXPECT_EQ(*(*dataset)->readBand(1), (std::vector<double>{1.5, 2.5, 3.5, 4.5}));
}

TEST(GdalAdapter, ARasterWhoseCoordinateSystemCannotBeSetIsRefusedAndNotLeftBehind)
{
    // Regression, audit IO-14: the georeferencing setters' answers were
    // ignored, so this wrote a GeoTIFF with no coordinate system and reported
    // success.
    const TempFile file("bad-crs.tif");
    gis::RasterExportOptions options;
    options.width = options.height = 1;
    options.projectionWkt = "this is not a coordinate system";
    const auto status = gis::GdalDataset::writeRaster(file.path(), options, {1.0});
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::FileExportFailure);
    EXPECT_FALSE(std::filesystem::exists(file.path()))
        << "a raster that failed to write must not be left looking finished";

    // The in-memory route (a copy-only format) refuses it the same way.
    const TempFile copied("bad-crs.asc");
    options.driver = "AAIGrid";
    const auto viaCopy = gis::GdalDataset::writeRaster(copied.path(), options, {1.0});
    ASSERT_FALSE(viaCopy.ok());
    EXPECT_EQ(viaCopy.error().code, ErrorCode::FileExportFailure);
    EXPECT_FALSE(std::filesystem::exists(copied.path()));
}

TEST(GdalAdapter, AVectorWhoseCoordinateSystemCannotBeReadIsRefusedBeforeWriting)
{
    // The WKT used to be dropped when GDAL could not parse it: the file was
    // written with no CRS and the export reported success.
    const TempFile file("bad-crs.gpkg");
    gis::VectorFeature point;
    point.geometry.kind = gis::GeometryKind::Point;
    point.geometry.parts.push_back({gis::GeoPoint{1.0, 2.0, 0.0}});
    gis::VectorExportOptions options;
    options.projectionWkt = "this is not a coordinate system";
    const auto status = gis::GdalDataset::writeVector(file.path(), {point}, options);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidCRS);
    EXPECT_FALSE(std::filesystem::exists(file.path()));
}

TEST(GdalAdapter, DisplayingARasterWritesNothingBesideTheUsersFile)
{
    // Regression, audit IO-13: readImage forced GDAL's band statistics, and
    // GDAL persisted them to <file>.aux.xml on close - into the user's data
    // folder, on a read-only open.
    const TempFile file("no-sidecar.tif");
    gis::RasterExportOptions options;
    options.width = 4;
    options.height = 4;
    options.geotransform = {100.0, 1.0, 0.0, 200.0, 0.0, -1.0};
    std::vector<double> values(16);
    for (std::size_t i = 0; i < values.size(); ++i) {
        values[i] = 300.0 + static_cast<double>(i);
    }
    ASSERT_TRUE(gis::GdalDataset::writeRaster(file.path(), options, values).ok());
    ASSERT_FALSE(std::filesystem::exists(auxiliaryOf(file.path())));
    {
        const auto dataset = gis::GdalDataset::open(file.path());
        ASSERT_TRUE(dataset.ok());
        ASSERT_TRUE((*dataset)->readImage(16).ok());
        ASSERT_TRUE((*dataset)->readBandSampled(1, 2).ok());
    } // closed here, which is when GDAL would write it
    EXPECT_FALSE(std::filesystem::exists(auxiliaryOf(file.path())))
        << auxiliaryOf(file.path()).string() << " was written";
}

TEST(GdalAdapter, AReplacedDemIsStretchedOverItsOwnValues)
{
    // Regression, audit IO-13: the persisted statistics were trusted on the
    // next open, so a DEM replaced under the same name was stretched by the
    // OLD one's range. 300 and 400 against a stale 0-255 both clamp to white.
    const TempFile file("replaced.tif");
    const TempFile replacement("replacement.tif");
    gis::RasterExportOptions options;
    options.width = 2;
    options.height = 1;
    ASSERT_TRUE(gis::GdalDataset::writeRaster(file.path(), options, {0.0, 255.0}).ok());
    {
        const auto first = gis::GdalDataset::open(file.path());
        ASSERT_TRUE(first.ok());
        ASSERT_TRUE((*first)->readImage(16).ok());
    }
    // Replaced the way a person replaces a file - copied over it - and NOT
    // through GDAL, whose Create deletes a dataset's sidecars along with it
    // and so would hide the defect.
    ASSERT_TRUE(
        gis::GdalDataset::writeRaster(replacement.path(), options, {300.0, 400.0}).ok());
    std::filesystem::copy_file(replacement.path(), file.path(),
                               std::filesystem::copy_options::overwrite_existing);
    const auto second = gis::GdalDataset::open(file.path());
    ASSERT_TRUE(second.ok());
    const auto image = (*second)->readImage(16);
    ASSERT_TRUE(image.ok());
    ASSERT_EQ(image->rgba.size(), 8u);
    // Its own range is 300 to 400: the lower pixel is black, the upper white.
    EXPECT_EQ(image->rgba[0], 0) << "stretched by a range that is not this file's";
    EXPECT_EQ(image->rgba[4], 255);
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
