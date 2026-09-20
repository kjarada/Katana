// Import and export (PLAN.MD Phase 20).
//
// These tests write real files through GDAL and read them back, because the
// whole point of this layer is the boundary with a third-party library: a mock
// would test only our own assumptions about how GDAL behaves, which is exactly
// the thing that needs checking.

#include <gtest/gtest.h>

#include <cmath>
#include <filesystem>
#include <fstream>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/reference_data.hpp"

using namespace katana::interop;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::kPi;
using katana::math::kTwoPi;

namespace {

// A directory of its own per test, removed afterwards, so the suite can run in
// parallel with itself and leaves nothing behind.
class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-interop-" + name))
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

Model modelWith(std::vector<Entity> entities)
{
    Model model;
    for (Entity& entity : entities) {
        const auto id = model.entities.add(std::move(entity));
        EXPECT_TRUE(id.ok()) << id.error().describe();
    }
    return model;
}

Entity closedSquare(double size)
{
    Polyline2 polyline;
    polyline.vertices = {Point2(0, 0), Point2(size, 0), Point2(size, size), Point2(0, size)};
    polyline.closed = true;
    Entity entity;
    entity.geometry = polyline;
    return entity;
}

} // namespace

// ---- routing ------------------------------------------------------------------------------

TEST(InteropRouting, ExtensionsAreClassified)
{
    EXPECT_EQ(kindForPath("parcels.shp"), SourceKind::Vector);
    EXPECT_EQ(kindForPath("parcels.GeoJSON"), SourceKind::Vector); // case insensitive
    EXPECT_EQ(kindForPath("site.gpkg"), SourceKind::Vector);
    EXPECT_EQ(kindForPath("ortho.tif"), SourceKind::Raster);
    EXPECT_EQ(kindForPath("terrain.asc"), SourceKind::Raster);
    EXPECT_EQ(kindForPath("scan.las"), SourceKind::PointCloud);
    EXPECT_EQ(kindForPath("scan.laz"), SourceKind::PointCloud);
    EXPECT_EQ(kindForPath("drawing.dwg"), SourceKind::Unknown);
    EXPECT_EQ(kindForPath("noextension"), SourceKind::Unknown);

    // GDAL and PDAL both claim .ply. In a survey tool it is a point cloud, and
    // the routing must be deterministic rather than depending on list order.
    EXPECT_EQ(kindForPath("mesh.ply"), SourceKind::PointCloud);
}

TEST(InteropRouting, MissingFileIsNotFoundRatherThanACrash)
{
    const auto vector = importVector("no-such-file.geojson");
    ASSERT_FALSE(vector.ok());
    EXPECT_EQ(vector.error().code, ErrorCode::NotFound);

    const auto raster = importRaster("no-such-file.tif");
    ASSERT_FALSE(raster.ok());
    EXPECT_EQ(raster.error().code, ErrorCode::NotFound);

    const auto cloud = importPointCloud("no-such-file.las");
    ASSERT_FALSE(cloud.ok());
    EXPECT_EQ(cloud.error().code, ErrorCode::NotFound);
}

// ---- reference data -------------------------------------------------------------------------

TEST(InteropReferenceData, GeotransformIsAppliedThroughAllFourCorners)
{
    RasterOverlay raster;
    raster.width = 100;
    raster.height = 50;
    // North-up: origin at the TOP-left and a negative y step, which is how every
    // georeferenced image is stored. Taking only two corners would produce an
    // inverted, empty box.
    raster.geotransform = {1000.0, 2.0, 0.0, 5000.0, 0.0, -2.0};
    raster.hasGeotransform = true;

    const auto bounds = raster.worldBounds();
    ASSERT_FALSE(bounds.empty());
    EXPECT_DOUBLE_EQ(bounds.min.x, 1000.0);
    EXPECT_DOUBLE_EQ(bounds.max.x, 1200.0);
    EXPECT_DOUBLE_EQ(bounds.min.y, 4900.0); // 5000 - 50*2
    EXPECT_DOUBLE_EQ(bounds.max.y, 5000.0);

    // A rotated geotransform: the bounding box must cover the rotated rectangle,
    // which a two-corner implementation cannot do.
    RasterOverlay rotated = raster;
    rotated.geotransform = {0.0, 0.0, 1.0, 0.0, 1.0, 0.0}; // swaps the axes
    const auto rotatedBounds = rotated.worldBounds();
    EXPECT_DOUBLE_EQ(rotatedBounds.min.x, 0.0);
    EXPECT_DOUBLE_EQ(rotatedBounds.max.x, 50.0);
    EXPECT_DOUBLE_EQ(rotatedBounds.max.y, 100.0);
}

TEST(InteropReferenceData, IdsAreNeverReused)
{
    ReferenceData data;
    RasterOverlay raster;
    raster.width = raster.height = 4;
    const ReferenceId first = data.add(raster);
    const ReferenceId second = data.add(raster);
    EXPECT_NE(first, second);

    ASSERT_TRUE(data.remove(first));
    const ReferenceId third = data.add(raster);
    EXPECT_NE(third, first) << "a removed id must never come back";
    EXPECT_NE(third, second);

    data.clear();
    EXPECT_TRUE(data.empty());
    EXPECT_NE(data.add(raster), third) << "clear() must not reset the id counter either";
}

TEST(InteropReferenceData, VisibleBoundsIgnoresHiddenLayers)
{
    ReferenceData data;
    RasterOverlay visible;
    visible.width = visible.height = 10;
    visible.geotransform = {0.0, 1.0, 0.0, 10.0, 0.0, -1.0};
    visible.hasGeotransform = true;
    data.add(visible);

    RasterOverlay hidden = visible;
    hidden.visible = false;
    hidden.geotransform = {1000.0, 1.0, 0.0, 1000.0, 0.0, -1.0};
    data.add(hidden);

    const auto bounds = data.visibleBounds();
    ASSERT_FALSE(bounds.empty());
    EXPECT_DOUBLE_EQ(bounds.max.x, 10.0) << "a hidden layer must not drag the extents out to it";
}

TEST(InteropReferenceData, ColorRampHandlesADegenerateRange)
{
    katana::pointcloud::PointCloudPoint point;
    point.z = 42.0;
    // A perfectly flat surface: every point has the same value, so the range is
    // zero and the naive normalisation would divide by it.
    const Rgb flat = colorForPoint(point, PointColorMode::Elevation, 42.0, 42.0);
    EXPECT_TRUE(std::isfinite(static_cast<double>(flat.r)));
    const Rgb same = colorForPoint(point, PointColorMode::Elevation, 42.0, 42.0);
    EXPECT_EQ(flat.r, same.r);
    EXPECT_EQ(flat.g, same.g);
    EXPECT_EQ(flat.b, same.b);

    // Ground and building are standard classes and must not collide.
    EXPECT_NE(classificationColor(2).r, classificationColor(6).r);
    // An unassigned class gets neutral grey rather than an invented meaning.
    const Rgb unknown = classificationColor(200);
    EXPECT_EQ(unknown.r, unknown.g);
    EXPECT_EQ(unknown.g, unknown.b);

    // SourceColor falls back to the ramp when the file has no colour, rather
    // than painting everything black.
    point.hasColor = false;
    const Rgb fallback = colorForPoint(point, PointColorMode::SourceColor, 0.0, 100.0);
    const Rgb ramp = colorForPoint(point, PointColorMode::Elevation, 0.0, 100.0);
    EXPECT_EQ(fallback.r, ramp.r);
    EXPECT_EQ(fallback.b, ramp.b);
}

// ---- export ---------------------------------------------------------------------------------

TEST(InteropExport, UnknownExtensionIsRejectedByName)
{
    const Model model = modelWith({closedSquare(10.0)});
    const auto result = exportVector(model, "out.xyzzy");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::Unsupported);
    EXPECT_NE(result.error().message.find("xyzzy"), std::string::npos)
        << "the error should name the extension: " << result.error().message;
}

TEST(InteropExport, RoundTripsThroughGeoJson)
{
    const TempDir dir("roundtrip");
    const auto path = dir.file("shapes.geojson");

    Entity line;
    line.geometry = Segment2{Point2(0, 0), Point2(30, 40)};
    Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(5, 7)};
    const Model model = modelWith({closedSquare(20.0), line, point});

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(written->featuresWritten, 3u);
    ASSERT_TRUE(std::filesystem::exists(path));

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->entities.size(), 3u);

    // The square keeps its area, and the line its length, through the round trip.
    bool sawSquare = false;
    bool sawLine = false;
    bool sawPoint = false;
    for (const Entity& entity : read->entities) {
        if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry)) {
            EXPECT_TRUE(polyline->closed);
            EXPECT_NEAR(std::abs(polyline->area()), 400.0, 1e-9);
            sawSquare = true;
        } else if (const auto* segment = std::get_if<Segment2>(&entity.geometry)) {
            EXPECT_NEAR(segment->length(), 50.0, 1e-9);
            sawLine = true;
        } else if (std::get_if<katana::entity::PointGeometry>(&entity.geometry) != nullptr) {
            sawPoint = true;
        }
    }
    EXPECT_TRUE(sawSquare);
    EXPECT_TRUE(sawLine);
    EXPECT_TRUE(sawPoint);
}

TEST(InteropExport, TextAndDimensionsAreSkippedAndCounted)
{
    const TempDir dir("skipped");
    const auto path = dir.file("mixed.geojson");

    Entity text;
    katana::entity::TextGeometry label;
    label.position = Point2(1, 1);
    label.text = "PARCEL 42";
    label.height = 2.5;
    text.geometry = label;

    const Model model = modelWith({closedSquare(10.0), text});
    const auto result = exportVector(model, path);
    ASSERT_TRUE(result.ok()) << result.error().describe();

    // Skipped, never silently dropped: the count and a warning both say so.
    EXPECT_EQ(result->featuresWritten, 1u);
    EXPECT_EQ(result->entitiesSkipped, 1u);
    const bool mentionsText =
        std::any_of(result->warnings.begin(), result->warnings.end(),
                    [](const std::string& warning) {
                        return warning.find("text") != std::string::npos;
                    });
    EXPECT_TRUE(mentionsText) << "the user must be told what was left out";
}

TEST(InteropExport, MixedGeometryIntoAShapefileIsRefusedWithoutWritingAnything)
{
    const TempDir dir("mixed-shp");
    const auto path = dir.file("mixed.shp");

    Entity line;
    line.geometry = Segment2{Point2(0, 0), Point2(10, 0)};
    const Model model = modelWith({closedSquare(10.0), line});

    const auto result = exportVector(model, path);
    ASSERT_FALSE(result.ok()) << "a shapefile holds one geometry type per file";
    EXPECT_EQ(result.error().code, ErrorCode::Unsupported);

    // The refusal happens BEFORE anything is created. A half-written shapefile
    // is three or more files and is worse than no file, because it opens.
    EXPECT_FALSE(std::filesystem::exists(path));
    EXPECT_FALSE(std::filesystem::exists(dir.file("mixed.shx")));
    EXPECT_FALSE(std::filesystem::exists(dir.file("mixed.dbf")));
}

TEST(InteropExport, UniformGeometryIntoAShapefileSucceeds)
{
    const TempDir dir("uniform-shp");
    const auto path = dir.file("squares.shp");
    const Model model = modelWith({closedSquare(10.0), closedSquare(20.0)});

    const auto result = exportVector(model, path);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->featuresWritten, 2u);
    EXPECT_TRUE(std::filesystem::exists(path));
}

TEST(InteropExport, CurveTessellationHonoursTheStatedTolerance)
{
    const TempDir dir("curves");
    const auto path = dir.file("circle.geojson");

    constexpr double kRadius = 5.0;
    constexpr double kTolerance = 0.001;
    Entity circle;
    circle.geometry = katana::geometry::Circle2{Point2(0, 0), kRadius};
    const Model model = modelWith({circle});

    VectorExportOptions options;
    options.curveTolerance = kTolerance;
    ASSERT_TRUE(exportVector(model, path, options).ok());

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 1u);
    const auto* polygon = std::get_if<Polyline2>(&read->entities.front().geometry);
    ASSERT_NE(polygon, nullptr);

    const auto sides = static_cast<double>(polygon->vertices.size());
    ASSERT_GE(sides, 3.0);

    // Every vertex lies exactly on the circle...
    for (const Point2& vertex : polygon->vertices) {
        EXPECT_NEAR(std::hypot(vertex.x, vertex.y), kRadius, 1e-9);
    }
    // ...and the deviation at the middle of a chord - the sagitta,
    // r(1 - cos(pi/n)) - is within the tolerance that was asked for. This is the
    // actual contract of curveTolerance, and asserting the vertex COUNT instead
    // would only restate the implementation's formula back at itself.
    const double sagitta = kRadius * (1.0 - std::cos(kPi / sides));
    EXPECT_LE(sagitta, kTolerance);
    // Not wastefully fine either: one side fewer would exceed the tolerance, so
    // the count is the smallest that satisfies it.
    const double coarser = kRadius * (1.0 - std::cos(kPi / (sides - 1.0)));
    EXPECT_GT(coarser, kTolerance);

    // The inscribed polygon's area follows n/2 r^2 sin(2 pi / n) exactly.
    const double expectedArea = 0.5 * sides * kRadius * kRadius * std::sin(kTwoPi / sides);
    EXPECT_NEAR(std::abs(polygon->area()), expectedArea, 1e-9);
}

TEST(InteropExport, AnEmptySelectionIsAnErrorRatherThanAnEmptyFile)
{
    const TempDir dir("empty");
    const Model model; // no entities at all
    const auto result = exportVector(model, dir.file("nothing.geojson"));
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
    EXPECT_FALSE(std::filesystem::exists(dir.file("nothing.geojson")));
}

// ---- import ---------------------------------------------------------------------------------

TEST(InteropImport, MultiGeometryIsFlattenedAndAttributesAreCopiedToEachPart)
{
    const TempDir dir("multi");
    const auto path = dir.file("multi.geojson");

    // Written by hand rather than by our own exporter: a round trip through our
    // writer could not produce a MultiPolygon, so it would never exercise the
    // flattening path that real GIS data hits constantly.
    std::ofstream out(path);
    out << R"({"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"parcel":"A1"},"geometry":{"type":"MultiPolygon",
       "coordinates":[[[[0,0],[10,0],[10,10],[0,10],[0,0]]],
                      [[[20,0],[30,0],[30,10],[20,10],[20,0]]]]}}]})";
    out.close();

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 2u) << "one entity per polygon of the multi-geometry";

    for (const Entity& entity : read->entities) {
        const auto* polygon = std::get_if<Polyline2>(&entity.geometry);
        ASSERT_NE(polygon, nullptr);
        EXPECT_TRUE(polygon->closed);
        // The closing vertex is dropped: `closed` expresses it instead, and
        // keeping it would make a zero-length final segment.
        EXPECT_EQ(polygon->vertices.size(), 4u);
        EXPECT_NEAR(std::abs(polygon->area()), 100.0, 1e-9);

        const auto parcel = entity.properties.find("parcel");
        ASSERT_NE(parcel, entity.properties.end()) << "each part keeps the feature's attributes";
        EXPECT_EQ(std::get<std::string>(parcel->second), "A1");
    }
}

TEST(InteropImport, TwoPointLinesBecomeLinesAndLongerOnesPolylines)
{
    const TempDir dir("kinds");
    const auto path = dir.file("lines.geojson");
    std::ofstream out(path);
    out << R"({"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{},"geometry":{"type":"LineString",
       "coordinates":[[0,0],[10,0]]}},
      {"type":"Feature","properties":{},"geometry":{"type":"LineString",
       "coordinates":[[0,0],[10,0],[10,10]]}}]})";
    out.close();

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 2u);
    // A two-point line is a Line, not a two-vertex polyline: a drafter expects
    // to be able to fillet it.
    EXPECT_NE(std::get_if<Segment2>(&read->entities[0].geometry), nullptr);
    EXPECT_NE(std::get_if<Polyline2>(&read->entities[1].geometry), nullptr);
}

TEST(InteropImport, LayerNameComesFromTheSourceAndIsReported)
{
    const TempDir dir("layers");
    const auto path = dir.file("parcels.geojson");
    std::ofstream out(path);
    out << R"({"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[1,2]}}]})";
    out.close();

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->layersNeeded.size(), 1u);
    // The caller must create these before executing the create command, so they
    // have to be reported rather than assumed to exist.
    EXPECT_FALSE(read->layersNeeded.front().empty());
    EXPECT_EQ(read->entities.front().layer, read->layersNeeded.front());

    // Provenance is recorded so an imported entity can be traced to its file.
    const auto source = read->entities.front().metadata.find("source.file");
    ASSERT_NE(source, read->entities.front().metadata.end());
    EXPECT_EQ(std::get<std::string>(source->second), "parcels.geojson");
}

TEST(InteropImport, AnOriginShiftIsAppliedAndRestoredOnExport)
{
    const TempDir dir("shift");
    const auto path = dir.file("far.geojson");
    // Survey coordinates, where a double has little resolution left.
    std::ofstream out(path);
    out << R"({"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{},"geometry":{"type":"Point",
       "coordinates":[500000.25,5000000.75]}}]})";
    out.close();

    VectorImportOptions options;
    options.originShift = katana::geometry::Vec2(500000.0, 5000000.0);
    const auto read = importVector(path, options);
    ASSERT_TRUE(read.ok()) << read.error().describe();

    const auto* point = std::get_if<katana::entity::PointGeometry>(&read->entities.front().geometry);
    ASSERT_NE(point, nullptr);
    EXPECT_DOUBLE_EQ(point->position.x, 0.25);
    EXPECT_DOUBLE_EQ(point->position.y, 0.75);

    // Exporting with the same shift puts it back exactly where it came from.
    const Model model = modelWith({read->entities.front()});
    VectorExportOptions exportOptions;
    exportOptions.originShift = *options.originShift;
    const auto back = dir.file("back.geojson");
    ASSERT_TRUE(exportVector(model, back, exportOptions).ok());

    const auto reread = importVector(back);
    ASSERT_TRUE(reread.ok()) << reread.error().describe();
    const auto* restored =
        std::get_if<katana::entity::PointGeometry>(&reread->entities.front().geometry);
    ASSERT_NE(restored, nullptr);
    EXPECT_DOUBLE_EQ(restored->position.x, 500000.25);
    EXPECT_DOUBLE_EQ(restored->position.y, 5000000.75);
}

TEST(InteropImport, ImportedEntitiesPassCommandValidation)
{
    // The GUI and the CLI both feed importVector's output straight into
    // createEntities, so whatever the importer produces has to survive the
    // command layer's validation - including the metadata it attaches.
    const TempDir dir("validation");
    const auto path = dir.file("shapes.geojson");
    std::ofstream out(path);
    out << R"({"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"n":"1"},"geometry":{"type":"Polygon",
       "coordinates":[[[0,0],[10,0],[10,10],[0,10],[0,0]]]}}]})";
    out.close();

    auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();

    Model model;
    katana::commands::CommandStack stack(model);
    auto transaction = std::make_unique<katana::commands::Transaction>("IMPORT");
    for (const std::string& layer : read->layersNeeded) {
        katana::entity::Layer created;
        created.name = layer;
        transaction->add(katana::commands::createLayer(created));
    }
    transaction->add(katana::commands::createEntities(std::move(read->entities)));

    const auto status = stack.execute(std::move(transaction));
    ASSERT_TRUE(status.ok()) << status.error().describe();
    EXPECT_EQ(model.entities.size(), 1u);

    // And the whole import is a single undo step.
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(model.entities.empty());
}
