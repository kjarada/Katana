// Import and export (PLAN.MD Phase 20).
//
// These tests write real files through GDAL and read them back, because the
// whole point of this layer is the boundary with a third-party library: a mock
// would test only our own assumptions about how GDAL behaves, which is exactly
// the thing that needs checking.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <optional>
#include <string>
#include <vector>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/reference_data.hpp"

using namespace katana::interop;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Box2;
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

TEST(InteropExport, ADrawingCanBeWrittenToDxfAndKeepsItsLayers)
{
    // DXF has a fixed set of fields and refuses any other, and the exporter
    // used to treat "could not create field katana_id" as fatal - so the one
    // format a CAD program cannot do without could not be written at all.
    // Found by running the bundled CLI, not by a test, because every export
    // test used a format that accepts arbitrary fields.
    const TempDir dir("dxf");
    const auto path = dir.file("drawing.dxf");

    Entity kerb = closedSquare(20.0);
    kerb.layer = "KERB";
    Entity line;
    line.geometry = Segment2{Point2(0, 0), Point2(30, 40)};
    line.layer = "BOUNDARY";
    Model model = modelWith({});
    for (const char* name : {"KERB", "BOUNDARY"}) {
        katana::entity::Layer layer;
        layer.name = name;
        ASSERT_TRUE(model.layers.add(layer));
    }
    // add(), not insert(): add assigns the id, insert expects one already.
    ASSERT_TRUE(model.entities.add(kerb).ok());
    ASSERT_TRUE(model.entities.add(line).ok());

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(written->featuresWritten, 2u);

    // What DXF cannot hold is SAID, not silently dropped (PLAN.MD section 36).
    bool warned = false;
    for (const std::string& warning : written->warnings) {
        warned = warned || warning.find("fixed set of fields") != std::string::npos;
    }
    EXPECT_TRUE(warned) << "the loss of ids and properties went unreported";

    // And what it CAN hold survives: geometry, and the layer each entity is
    // on - our `layer` attribute lands in DXF's own Layer field.
    // No option needed: the importer's default layer attribute matches DXF's
    // `Layer` field, so the CAD layers come back rather than one "entities".
    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 2u);
    bool sawKerb = false;
    bool sawBoundary = false;
    for (const Entity& entity : read->entities) {
        if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry)) {
            EXPECT_NEAR(std::abs(polyline->area()), 400.0, 1e-9);
            sawKerb = sawKerb || entity.layer == "KERB";
        } else if (const auto* segment = std::get_if<Segment2>(&entity.geometry)) {
            EXPECT_NEAR(segment->length(), 50.0, 1e-9);
            sawBoundary = sawBoundary || entity.layer == "BOUNDARY";
        }
    }
    EXPECT_TRUE(sawKerb) << "the square lost its layer";
    EXPECT_TRUE(sawBoundary) << "the line lost its layer";
    // Both layers are reported as needed, and nothing asks for an "entities"
    // layer that no entity is on.
    EXPECT_NE(std::find(read->layersNeeded.begin(), read->layersNeeded.end(), "KERB"),
              read->layersNeeded.end());
    EXPECT_NE(std::find(read->layersNeeded.begin(), read->layersNeeded.end(), "BOUNDARY"),
              read->layersNeeded.end());
    EXPECT_EQ(std::find(read->layersNeeded.begin(), read->layersNeeded.end(), "entities"),
              read->layersNeeded.end());

    // An explicit target layer still wins: "put everything here" means that.
    VectorImportOptions everythingHere;
    everythingHere.targetLayer = "IMPORTED";
    const auto flattened = importVector(path, everythingHere);
    ASSERT_TRUE(flattened.ok());
    for (const Entity& entity : flattened->entities) {
        EXPECT_EQ(entity.layer, "IMPORTED");
    }
}

namespace {

// One entity of a DXF file's ENTITIES section: its type, and the group 70
// flags where it has them (bit 1 of an LWPOLYLINE's is "closed").
struct DxfEntity {
    std::string type;
    int flags = 0;
};

// Read by hand rather than through GDAL: the question is what a CAD program
// finds in the file, and GDAL's reader would answer with its own reading of it.
std::vector<DxfEntity> dxfEntities(const std::filesystem::path& path)
{
    std::ifstream in(path);
    std::vector<std::string> lines;
    for (std::string line; std::getline(in, line);) {
        while (!line.empty() && (line.back() == '\r' || line.back() == ' ')) {
            line.pop_back();
        }
        const auto first = line.find_first_not_of(' ');
        lines.push_back(first == std::string::npos ? std::string{} : line.substr(first));
    }
    std::vector<DxfEntity> entities;
    bool inEntities = false;
    for (std::size_t i = 0; i + 1 < lines.size(); i += 2) {
        const std::string& code = lines[i];
        const std::string& value = lines[i + 1];
        if (code == "2" && value == "ENTITIES") {
            inEntities = true;
        } else if (inEntities && code == "0" && value == "ENDSEC") {
            break;
        } else if (inEntities && code == "0") {
            entities.push_back({value, 0});
        } else if (inEntities && code == "70" && !entities.empty()) {
            entities.back().flags = std::stoi(value);
        }
    }
    return entities;
}

} // namespace

TEST(InteropExport, ClosedPolylinesAndCirclesGoToDxfAsClosedPolylinesNotSolidHatches)
{
    // GDAL's DXF writer turns a polygon into a HATCH with a SOLID fill by
    // default, and the exporter hands it closed polylines and circles as
    // polygons - so every parcel reached a CAD program as a filled shape.
    const TempDir dir("dxf-closed");
    const auto path = dir.file("parcels.dxf");

    Entity parcel = closedSquare(20.0);
    Entity tree;
    tree.geometry = katana::geometry::Circle2{Point2(50, 50), 5.0};
    Entity fence;
    Polyline2 fenceLine;
    fenceLine.vertices = {Point2(0, 30), Point2(10, 35), Point2(20, 30)};
    fence.geometry = fenceLine;
    Model model = modelWith({parcel, tree, fence});

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(written->featuresWritten, 3u);

    const auto entities = dxfEntities(path);
    ASSERT_EQ(entities.size(), 3u);
    int closed = 0;
    int open = 0;
    for (const DxfEntity& entity : entities) {
        EXPECT_NE(entity.type, "HATCH") << "a closed shape was written as a filled solid";
        EXPECT_EQ(entity.type, "LWPOLYLINE");
        ((entity.flags & 1) != 0 ? closed : open) += 1;
    }
    EXPECT_EQ(closed, 2) << "the parcel and the tree are closed";
    EXPECT_EQ(open, 1) << "the fence is not";

    // And back: the parcel is a closed square again, its four corners and no
    // repeat of the first. By hand: the ring written is (0,0) (20,0) (20,20)
    // (0,20) (0,0), and the repeat is what the closed flag stands for.
    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    int squares = 0;
    for (const Entity& entity : read->entities) {
        const auto* polyline = std::get_if<Polyline2>(&entity.geometry);
        if (polyline != nullptr && polyline->vertices.size() == 4) {
            ++squares;
            EXPECT_TRUE(polyline->closed);
            EXPECT_EQ(std::abs(polyline->area()), 400.0);
        }
    }
    EXPECT_EQ(squares, 1);
}

TEST(InteropImport, ADxfCircleIsImportedAsAClosedPolylineOnTheCircle)
{
    // GDAL reads a CIRCLE as a line string that repeats its first point; taken
    // as it came, that was an OPEN polyline with a seam where it began.
    const TempDir dir("dxf-circle");
    const auto path = dir.file("tree.dxf");
    {
        std::ofstream out(path);
        out << "  0\nSECTION\n  2\nENTITIES\n"
               "  0\nCIRCLE\n  8\nTREES\n 10\n5.0\n 20\n7.0\n 30\n0.0\n 40\n0.6\n"
               "  0\nENDSEC\n  0\nEOF\n";
    }

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 1u);
    const auto* polyline = std::get_if<Polyline2>(&read->entities.front().geometry);
    ASSERT_NE(polyline, nullptr);
    EXPECT_TRUE(polyline->closed);
    ASSERT_GE(polyline->vertices.size(), 3u);
    EXPECT_NE(polyline->vertices.front(), polyline->vertices.back())
        << "the closed flag stands for the repeat, which is not kept as well";
    for (const Point2& vertex : polyline->vertices) {
        EXPECT_NEAR(std::hypot(vertex.x - 5.0, vertex.y - 7.0), 0.6, 1e-9);
    }
}

TEST(InteropImport, OnlyALineThatEndsWhereItBeganIsClosed)
{
    // The other side of the rule above: a line that ends elsewhere stays
    // open, and a line there and back (three points, ends equal) is not a
    // ring at all.
    const TempDir dir("geojson-open");
    const auto path = dir.file("lines.geojson");
    {
        std::ofstream out(path);
        out << R"({"type":"FeatureCollection","features":[
{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[0,0],[10,0],[10,5]]}},
{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[0,0],[10,0],[0,0]]}},
{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[0,0],[10,0],[10,5],[0,0]]}}
]})";
    }
    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 3u);
    const auto* open = std::get_if<Polyline2>(&read->entities[0].geometry);
    const auto* thereAndBack = std::get_if<Polyline2>(&read->entities[1].geometry);
    const auto* triangle = std::get_if<Polyline2>(&read->entities[2].geometry);
    ASSERT_NE(open, nullptr);
    ASSERT_NE(thereAndBack, nullptr);
    ASSERT_NE(triangle, nullptr);
    EXPECT_FALSE(open->closed);
    EXPECT_FALSE(thereAndBack->closed);
    EXPECT_EQ(thereAndBack->vertices.size(), 3u);
    EXPECT_TRUE(triangle->closed);
    ASSERT_EQ(triangle->vertices.size(), 3u);
    // By hand: half of base 10 times height 5.
    EXPECT_EQ(std::abs(triangle->area()), 25.0);
}

TEST(InteropExport, AGeoPackageOfThousandsOfEntitiesHoldsEveryOne)
{
    // Written in one transaction now rather than one per feature; the count
    // read back is what shows the transaction was committed, not rolled back
    // or left open.
    const TempDir dir("gpkg-many");
    const auto path = dir.file("points.gpkg");
    std::vector<Entity> entities;
    for (int i = 0; i < 5000; ++i) {
        Entity entity;
        entity.geometry = katana::entity::PointGeometry{Point2(i, 2.0 * i)};
        entities.push_back(std::move(entity));
    }
    const Model model = modelWith(std::move(entities));

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(written->featuresWritten, 5000u);

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 5000u);
    const auto* last = std::get_if<katana::entity::PointGeometry>(&read->entities.back().geometry);
    ASSERT_NE(last, nullptr);
    EXPECT_EQ(last->position, Point2(4999.0, 9998.0));
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

// ---- placement advice (PLAN.MD Phase 20) ---------------------------------------

TEST(InteropPlacement, DataFarFromTheDrawingIsReportedRatherThanMergedSilently)
{
    // The real case that prompted this: a DXF in a projected CRS at
    // (255440, 7410850) imported into a drawing that sits near the origin. The
    // merge succeeds, and at a zoom showing both the original drawing is a dot
    // smaller than a pixel - with nothing saying so.
    const Box2 drawing(Point2(0.0, 0.0), Point2(160.0, 100.0));
    const Box2 survey(Point2(255440.07, 7410850.76), Point2(257712.57, 7412122.21));

    const auto advice = katana::interop::advisePlacement(drawing, survey);
    EXPECT_TRUE(advice.farApart);
    EXPECT_GT(advice.separation, 7.0e6);
    EXPECT_FALSE(advice.message.empty());

    // The suggested shift brings the incoming data's corner onto the drawing's.
    EXPECT_NEAR(advice.suggestedShift.x, 255440.07, 1e-6);
    EXPECT_NEAR(advice.suggestedShift.y, 7410850.76, 1e-6);
}

TEST(InteropPlacement, DataAlongsideTheDrawingIsNotReported)
{
    const Box2 drawing(Point2(0.0, 0.0), Point2(160.0, 100.0));

    // Overlapping.
    EXPECT_FALSE(
        katana::interop::advisePlacement(drawing, Box2(Point2(50.0, 20.0), Point2(200.0, 150.0)))
            .farApart);
    // Adjacent, and of a comparable size: a neighbouring sheet, not a different
    // coordinate system.
    EXPECT_FALSE(
        katana::interop::advisePlacement(drawing, Box2(Point2(200.0, 0.0), Point2(400.0, 100.0)))
            .farApart);
    // Ten times the size but in the same place - a site within a suburb. Still
    // not "far": the criterion is whether one becomes invisible, and at 10x
    // both are still legible.
    EXPECT_FALSE(
        katana::interop::advisePlacement(drawing, Box2(Point2(-500.0, -500.0),
                                                       Point2(1000.0, 1000.0)))
            .farApart);
}

TEST(InteropPlacement, ImportingIntoAnEmptyDrawingIsNeverFarApart)
{
    // Nothing to be far FROM. Warning here would fire on every first import,
    // which is the normal way to start a job from survey data.
    const Box2 survey(Point2(255440.07, 7410850.76), Point2(257712.57, 7412122.21));
    EXPECT_FALSE(katana::interop::advisePlacement(Box2{}, survey).farApart);
    EXPECT_FALSE(katana::interop::advisePlacement(survey, Box2{}).farApart);
    EXPECT_TRUE(katana::interop::advisePlacement(Box2{}, Box2{}).message.empty());
}

TEST(InteropPlacement, TheCriterionIsVisibilityNotADistanceInMetres)
{
    // A fixed distance threshold would be wrong for a site plan and wrong again
    // for a national grid. THE SAME 20 km gap is far apart for two 100 m
    // drawings and not for two 50 km ones, and the arithmetic says why:
    //
    //   100 m boxes, 20 km apart: each diagonal 141 m, combined view 20 100 m,
    //                             so each is 0.70% of the view - a dot.
    //   50 km boxes, 20 km apart: each diagonal 70 711 m, combined 130 000 m,
    //                             so each is 54% of the view - plainly visible.
    //
    // The threshold is 1% of the combined diagonal. (An earlier version of this
    // test used a 5 km gap for the small pair, which is 2.8% of the view -
    // small, but not invisible, so the implementation was right to say no.)
    const double gap = 20000.0;

    const Box2 smallA(Point2(0.0, 0.0), Point2(100.0, 100.0));
    const Box2 smallB(Point2(gap, 0.0), Point2(gap + 100.0, 100.0));
    EXPECT_TRUE(katana::interop::advisePlacement(smallA, smallB).farApart);

    const Box2 largeA(Point2(0.0, 0.0), Point2(50000.0, 50000.0));
    const Box2 largeB(Point2(gap + 50000.0, 0.0), Point2(gap + 100000.0, 50000.0));
    EXPECT_FALSE(katana::interop::advisePlacement(largeA, largeB).farApart);
}

TEST(InteropPlacement, ADrawingSmallButStillVisibleIsNotReported)
{
    // The boundary from the other side: two 100 m drawings 5 km apart occupy
    // 2.8% of a view showing both. Small, legible, and not worth interrupting
    // the user over - a warning that fires on ordinary adjacent sheets would be
    // ignored and would then be ignored when it mattered.
    const Box2 a(Point2(0.0, 0.0), Point2(100.0, 100.0));
    const Box2 b(Point2(5000.0, 0.0), Point2(5100.0, 100.0));
    EXPECT_FALSE(katana::interop::advisePlacement(a, b).farApart);
}

TEST(InteropPlacement, SmallDataPlacedInOrBesideTheDrawingIsNotFarApart)
{
    // Audit IO-02. Correctly placed data that is merely small used to be "far
    // apart" with a separation of 0, and the default button moved it to the
    // drawing's corner. Each case worked by hand against a 2 km site, diagonal
    // 2 828.4 m:
    const Box2 site(Point2(0.0, 0.0), Point2(2000.0, 2000.0));

    // One control point in the middle: no extent, so never lost by its size;
    // the site is the whole combined view, 100% of it.
    const Box2 onePoint(Point2(500.0, 500.0), Point2(500.0, 500.0));
    const auto point = katana::interop::advisePlacement(site, onePoint);
    EXPECT_FALSE(point.farApart);
    EXPECT_TRUE(point.message.empty());
    EXPECT_EQ(point.suggestedShift.x, 0.0);
    EXPECT_EQ(point.suggestedShift.y, 0.0);

    // A 10 m detail inside: 14.1 m, 0.5% of the view - but also 0.5% of the
    // site alone, so it would be as small wherever it sat. Either way round.
    const Box2 detail(Point2(1000.0, 1000.0), Point2(1010.0, 1010.0));
    EXPECT_FALSE(katana::interop::advisePlacement(site, detail).farApart);
    EXPECT_FALSE(katana::interop::advisePlacement(detail, site).farApart);

    // The corner case the margin is for: a 20.5 m shed 79.5 m off the site's
    // corner. Combined view (0,0)-(2100,2100), diagonal 2 969.8 m; the shed's
    // diagonal 29.0 m is 0.98% of it, under the line, and 1.025% of the site's,
    // over it - lost "by placement" to a margin of 1x, though showing both grows
    // the view by only 5%. Against the 2% margin (56.6 m) it is not.
    const Box2 shed(Point2(2079.5, 2079.5), Point2(2100.0, 2100.0));
    EXPECT_FALSE(katana::interop::advisePlacement(site, shed).farApart);
}

TEST(InteropPlacement, OnePointFarAwayStillReportsTheDrawingItWouldHide)
{
    // The case the advice exists for must survive IO-02's fix even when the
    // import is a single point: a control point in MGA coordinates imported
    // into a drawing near the origin. The point has no size to lose, but the
    // DRAWING, diagonal 188.7 m, is 0.0025% of a 7.4e6 m view. The shift is the
    // same corner-to-corner one as for any far import.
    const Box2 drawing(Point2(0.0, 0.0), Point2(160.0, 100.0));
    const Box2 point(Point2(255440.07, 7410850.76), Point2(255440.07, 7410850.76));
    const auto advice = katana::interop::advisePlacement(drawing, point);
    EXPECT_TRUE(advice.farApart);
    EXPECT_FALSE(advice.message.empty());
    EXPECT_NEAR(advice.suggestedShift.x, 255440.07, 1e-6);
    EXPECT_NEAR(advice.suggestedShift.y, 7410850.76, 1e-6);
}

TEST(InteropPlacement, TwoLonePointsAreNeverLostHoweverFarApart)
{
    // The degenerate case: neither part has an extent, so both draw as markers
    // at any zoom and neither can become invisible. Nothing to warn about.
    const Box2 here(Point2(0.0, 0.0), Point2(0.0, 0.0));
    const Box2 there(Point2(255440.07, 7410850.76), Point2(255440.07, 7410850.76));
    EXPECT_FALSE(katana::interop::advisePlacement(here, there).farApart);
}

// ---- heights (audit IO-01) ------------------------------------------------------------------
//
// A vector file's Z is a height, and Katana keeps heights in the `elevation` /
// `elevations` properties every other part of it reads (entity.hpp). Import
// used to drop Z and export to write Z = 0 - so a 3D DXF became a drawing on
// the datum, and a surveyed point went out at sea level.

namespace {

using katana::entity::heightsOf;
using katana::entity::kElevationProperty;
using katana::entity::kElevationsProperty;

Entity heightedPoint(double x, double y, std::optional<double> z)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{Point2(x, y)};
    katana::entity::setHeights(entity.properties, {z});
    return entity;
}

// The imported entity that came from model entity `id`, found by the
// katana_id attribute the export writes.
const Entity* byKatanaId(const VectorImportResult& read, katana::entity::EntityId id)
{
    for (const Entity& entity : read.entities) {
        const auto found = entity.properties.find("katana_id");
        if (found != entity.properties.end() &&
            std::get<std::string>(found->second) == std::to_string(id)) {
            return &entity;
        }
    }
    return nullptr;
}

bool hasHeightProperty(const Entity& entity)
{
    return entity.properties.contains(kElevationProperty) ||
           entity.properties.contains(kElevationsProperty);
}

std::string fileText(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

void writeText(const std::filesystem::path& path, const std::string& text)
{
    std::ofstream out(path, std::ios::binary);
    out << text;
}

bool warned(const std::vector<std::string>& warnings, const std::string& fragment)
{
    return std::any_of(warnings.begin(), warnings.end(), [&](const std::string& warning) {
        return warning.find(fragment) != std::string::npos;
    });
}

} // namespace

TEST(InteropHeights, HeightsGoIntoTheGeometryAndComeBackExactly)
{
    // Heights exact in binary except 0.1, which only has to round-trip.
    const TempDir dir("heights-geojson");
    const auto path = dir.file("heights.geojson");

    Entity string;
    Polyline2 polyline;
    polyline.vertices = {Point2(0, 0), Point2(10, 0), Point2(10, 10)};
    string.geometry = polyline;
    katana::entity::setHeights(string.properties, {10.25, 11.5, 0.1});
    Entity pad = closedSquare(20.0);
    katana::entity::setHeights(pad.properties, {7.0, 7.0, 7.0, 7.0});
    const Model model = modelWith({heightedPoint(5, 7, 32.5), string, pad,
                                   heightedPoint(1, 1, std::nullopt)});

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(fileText(path).find("\"elevation"), std::string::npos)
        << "a height written into the geometry is not repeated as an attribute";

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 4u);

    const Entity* point = byKatanaId(*read, 1);
    ASSERT_NE(point, nullptr);
    EXPECT_EQ(heightsOf(point->properties, 1)[0], std::optional<double>(32.5));

    const Entity* back = byKatanaId(*read, 2);
    ASSERT_NE(back, nullptr);
    const auto heights = heightsOf(back->properties, 3);
    EXPECT_EQ(heights[0], std::optional<double>(10.25));
    EXPECT_EQ(heights[1], std::optional<double>(11.5));
    EXPECT_EQ(heights[2], std::optional<double>(0.1));

    const Entity* square = byKatanaId(*read, 3);
    ASSERT_NE(square, nullptr);
    for (const auto& z : heightsOf(square->properties, 4)) {
        EXPECT_EQ(z, std::optional<double>(7.0));
    }

    const Entity* flat = byKatanaId(*read, 4);
    ASSERT_NE(flat, nullptr);
    EXPECT_FALSE(hasHeightProperty(*flat)) << "no height went out and none came back";
}

TEST(InteropHeights, ARealZeroIsAHeightAndAMissingZIsNone)
{
    // The distinction the old import could not make: [5, 7, 0.0] is surveyed
    // at the datum, [1, 1] has no height at all.
    const TempDir dir("heights-zero");
    const auto path = dir.file("mixed.geojson");
    writeText(path, R"({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"id": "datum"},
         "geometry": {"type": "Point", "coordinates": [5.0, 7.0, 0.0]}},
        {"type": "Feature", "properties": {"id": "plan"},
         "geometry": {"type": "Point", "coordinates": [1.0, 1.0]}}]})");

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 2u);
    for (const Entity& entity : read->entities) {
        const auto& id = std::get<std::string>(entity.properties.at("id"));
        if (id == "datum") {
            EXPECT_EQ(heightsOf(entity.properties, 1)[0], std::optional<double>(0.0));
        } else {
            EXPECT_FALSE(hasHeightProperty(entity)) << "a 2D point must not arrive at 0";
        }
    }
}

TEST(InteropHeights, AVerticalStepLosesAHeightAndSaysSo)
{
    // (0,0) at 1 then 5 is one vertex in plan: the model has no zero-length
    // segment, so the second is dropped - and with it a real height.
    const TempDir dir("heights-step");
    const auto path = dir.file("step.geojson");
    writeText(path, R"({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {},
         "geometry": {"type": "LineString",
                      "coordinates": [[0, 0, 1], [0, 0, 5], [10, 0, 6]]}}]})");

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 1u);
    const auto heights = heightsOf(read->entities.front().properties, 2);
    EXPECT_EQ(heights[0], std::optional<double>(1.0)) << "the first of the two is kept";
    EXPECT_EQ(heights[1], std::optional<double>(6.0));
    EXPECT_TRUE(warned(read->warnings, "1 vertices were dropped")) << "the lost height is reported";
}

TEST(InteropHeights, AStringThatComesBackOverItsStartAtAnotherHeightStaysOpenWithEveryHeight)
{
    // A ramp that climbs round a 20 m square and ends one level above where
    // it began: its ends meet in plan only. Closing it would drop the last
    // vertex, and height 14 with it, for a ring the string never was.
    const TempDir dir("heights-ramp");
    const auto path = dir.file("ramp.geojson");
    writeText(path, R"({"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"id": "ramp"},
         "geometry": {"type": "LineString",
                      "coordinates": [[0, 0, 10], [20, 0, 11], [20, 20, 12],
                                      [0, 20, 13], [0, 0, 14]]}},
        {"type": "Feature", "properties": {"id": "pad"},
         "geometry": {"type": "LineString",
                      "coordinates": [[0, 0, 7], [20, 0, 7], [20, 20, 7],
                                      [0, 20, 7], [0, 0, 7]]}}]})");

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 2u);
    for (const Entity& entity : read->entities) {
        const auto* polyline = std::get_if<Polyline2>(&entity.geometry);
        ASSERT_NE(polyline, nullptr);
        if (std::get<std::string>(entity.properties.at("id")) == "ramp") {
            EXPECT_FALSE(polyline->closed) << "its ends are 4 m apart in height";
            ASSERT_EQ(polyline->vertices.size(), 5u);
            EXPECT_EQ(polyline->vertices.back(), Point2(0, 0));
            // By hand: the file's five heights, one per vertex, in order.
            const auto heights = heightsOf(entity.properties, 5);
            for (std::size_t i = 0; i < 5; ++i) {
                EXPECT_EQ(heights[i], std::optional<double>(10.0 + static_cast<double>(i)));
            }
        } else {
            // The same square with its ends at one height IS a ring - how a
            // closed 3D polyline comes out of a DXF - so it still closes,
            // with its four corners.
            EXPECT_TRUE(polyline->closed);
            ASSERT_EQ(polyline->vertices.size(), 4u);
            for (const auto& z : heightsOf(entity.properties, 4)) {
                EXPECT_EQ(z, std::optional<double>(7.0));
            }
        }
    }
    EXPECT_FALSE(warned(read->warnings, "vertices were dropped")) << "no height was lost";
}

TEST(InteropHeights, AnOpen3DStringThatReturnsOverItsStartInPlanComesBackOpen)
{
    // Katana's own open string, round-tripped: the export writes its five
    // vertices with their heights, and the import must not read the equal
    // ends in plan as a ring.
    const TempDir dir("heights-ramp-trip");
    const auto path = dir.file("ramp.geojson");
    Entity ramp;
    Polyline2 polyline;
    polyline.vertices = {Point2(0, 0), Point2(20, 0), Point2(20, 20), Point2(0, 20), Point2(0, 0)};
    ramp.geometry = polyline;
    katana::entity::setHeights(ramp.properties, {10.0, 11.0, 12.0, 13.0, 14.0});
    const Model model = modelWith({ramp});

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 1u);
    const auto* back = std::get_if<Polyline2>(&read->entities.front().geometry);
    ASSERT_NE(back, nullptr);
    EXPECT_FALSE(back->closed);
    EXPECT_EQ(back->vertices, polyline.vertices);
    const auto heights = heightsOf(read->entities.front().properties, 5);
    EXPECT_EQ(heights.back(), std::optional<double>(14.0)) << "the top of the ramp is kept";
}

TEST(InteropHeights, AStringHeightedAtOnlySomeVerticesGoesInPlanWithItsHeightsKept)
{
    // A 3D geometry needs a height at every vertex; writing 0 for the missing
    // one would be a height nobody measured. So the string goes in plan and
    // the list travels as an attribute, and reading it back restores it.
    const TempDir dir("heights-partial");
    const auto path = dir.file("partial.geojson");

    Entity string;
    Polyline2 polyline;
    polyline.vertices = {Point2(0, 0), Point2(10, 0), Point2(10, 10)};
    string.geometry = polyline;
    katana::entity::setHeights(string.properties, {1.0, std::nullopt, 3.0});
    const Model model = modelWith({string});

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_TRUE(warned(written->warnings, "only some of their vertices"));

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 1u);
    const auto heights = heightsOf(read->entities.front().properties, 3);
    EXPECT_EQ(heights[0], std::optional<double>(1.0));
    EXPECT_FALSE(heights[1].has_value());
    EXPECT_EQ(heights[2], std::optional<double>(3.0));
}

TEST(InteropHeights, AShapefileOfHeightedPointsIsWritten3D)
{
    const TempDir dir("heights-shp");
    const auto path = dir.file("points.shp");
    const Model model = modelWith({heightedPoint(0, 0, 12.5), heightedPoint(5, 5, 13.75)});

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_FALSE(warned(written->warnings, "written in plan"));

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(heightsOf(byKatanaId(*read, 1)->properties, 1)[0], std::optional<double>(12.5));
    EXPECT_EQ(heightsOf(byKatanaId(*read, 2)->properties, 1)[0], std::optional<double>(13.75));
}

TEST(InteropHeights, AShapefileMixingHeightedAndHeightlessEntitiesIsWrittenInPlanAndSaysSo)
{
    // A shapefile layer is all 2D or all 3D, and a 3D one has no "no height":
    // the heightless point would be written at 0. So the layer goes in plan,
    // the heights ride as attributes, and the round trip still keeps them.
    const TempDir dir("heights-shp-mixed");
    const auto path = dir.file("mixed.shp");
    const Model model = modelWith({heightedPoint(0, 0, 32.5), heightedPoint(5, 5, std::nullopt)});

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_TRUE(warned(written->warnings, "1 of 2 entities have heights"));

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const Entity* heighted = byKatanaId(*read, 1);
    const Entity* heightless = byKatanaId(*read, 2);
    ASSERT_NE(heighted, nullptr);
    ASSERT_NE(heightless, nullptr);
    EXPECT_EQ(heightsOf(heighted->properties, 1)[0], std::optional<double>(32.5));
    EXPECT_FALSE(heightsOf(heightless->properties, 1)[0].has_value())
        << "the heightless point must not come back at 0";
}

TEST(InteropHeights, ADxfPointKeepsItsHeight)
{
    // The audit's case: a DXF carries heights only in its geometry, because
    // its fields are fixed and an "elevation" attribute has nowhere to go.
    const TempDir dir("heights-dxf");
    const auto path = dir.file("survey.dxf");
    Entity string;
    string.geometry = Segment2{Point2(255440.0, 7410850.0), Point2(255450.0, 7410860.0)};
    katana::entity::setHeights(string.properties, {32.5, 33.25});
    const Model model = modelWith({heightedPoint(255440.125, 7410850.25, 32.5), string});

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->entities.size(), 2u);
    bool sawPoint = false;
    bool sawLine = false;
    for (const Entity& entity : read->entities) {
        if (std::holds_alternative<katana::entity::PointGeometry>(entity.geometry)) {
            EXPECT_EQ(heightsOf(entity.properties, 1)[0], std::optional<double>(32.5));
            sawPoint = true;
        } else if (std::holds_alternative<Segment2>(entity.geometry)) {
            const auto heights = heightsOf(entity.properties, 2);
            EXPECT_EQ(heights[0], std::optional<double>(32.5));
            EXPECT_EQ(heights[1], std::optional<double>(33.25));
            sawLine = true;
        }
    }
    EXPECT_TRUE(sawPoint);
    EXPECT_TRUE(sawLine);
}

TEST(InteropHeights, AGeoPackageHoldsHeightedAndHeightlessEntitiesTogether)
{
    // Unlike a shapefile, each GeoPackage geometry has its own dimension, so a
    // mixed drawing keeps its heights in the geometry and nothing goes in plan.
    const TempDir dir("heights-gpkg");
    const auto path = dir.file("mixed.gpkg");
    const Model model = modelWith({heightedPoint(0, 0, 32.5), heightedPoint(5, 5, std::nullopt)});

    const auto written = exportVector(model, path);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_FALSE(warned(written->warnings, "written in plan"));

    const auto read = importVector(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(heightsOf(byKatanaId(*read, 1)->properties, 1)[0], std::optional<double>(32.5));
    EXPECT_FALSE(hasHeightProperty(*byKatanaId(*read, 2)))
        << "the heightless point must not come back at 0";
}
