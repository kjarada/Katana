// Vector fidelity (docs/interop.md, "Fidelity"): what IMPORT and EXPORT used
// to lose without a word - a lot's hole, a field's type, a KML's geometry, a
// CSV's geometry, a MapInfo table's last centimetre, a curve, a TIN, GDAL's
// own warnings - and each driver of the export table read back after writing.
//
// Every expected value is worked by hand or taken from outside Katana: the
// lon/lat of an MGA point from PROJ's own cs2cs, areas from the shoelace
// formula on the corners written into the test.

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <map>
#include <sstream>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "katana/entity/model.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"
#include "katana/gis/reproject.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "session.hpp"

namespace {

namespace gis = katana::gis;
namespace gp = katana::gis::processing;
namespace interop = katana::interop;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::Model;
using katana::entity::PropertyValue;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

// The MGA zone 56 of GDA94: projected, metres, what a survey in Sydney is in.
constexpr const char* kMga56 = "EPSG:28356";

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-fidelity-" + name))
    {
        std::error_code ignored;
        std::filesystem::remove_all(path_, ignored);
        std::filesystem::create_directories(path_, ignored);
    }
    ~TempDir()
    {
        std::error_code ignored;
        std::filesystem::remove_all(path_, ignored);
    }
    TempDir(const TempDir&) = delete;
    TempDir& operator=(const TempDir&) = delete;
    [[nodiscard]] std::filesystem::path file(const std::string& name) const { return path_ / name; }

  private:
    std::filesystem::path path_;
};

void writeText(const std::filesystem::path& path, const std::string& text)
{
    std::ofstream out(path, std::ios::binary);
    out << text;
}

std::string fileText(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

bool warned(const std::vector<std::string>& warnings, const std::string& fragment)
{
    return std::ranges::any_of(warnings, [&](const std::string& warning) {
        return warning.find(fragment) != std::string::npos;
    });
}

Model modelWith(std::vector<Entity> entities)
{
    Model model;
    for (Entity& entity : entities) {
        if (!model.layers.contains(entity.layer)) {
            katana::entity::Layer layer;
            layer.name = entity.layer;
            EXPECT_TRUE(model.layers.add(layer).ok()) << entity.layer;
        }
        EXPECT_TRUE(model.entities.add(std::move(entity)).ok());
    }
    return model;
}

// What an import gave, in a model, as the application puts it there.
Model modelOf(const interop::VectorImportResult& imported)
{
    Model model;
    for (const std::string& name : imported.layersNeeded) {
        katana::entity::Layer layer;
        layer.name = name;
        EXPECT_TRUE(model.layers.add(layer).ok()) << name;
    }
    for (const Entity& entity : imported.entities) {
        EXPECT_TRUE(model.entities.add(entity).ok());
    }
    return model;
}

Entity square(double x, double y, double side)
{
    Polyline2 ring;
    ring.vertices = {{x, y}, {x + side, y}, {x + side, y + side}, {x, y + side}};
    ring.closed = true;
    Entity entity;
    entity.geometry = ring;
    return entity;
}

double ringArea(const std::vector<gis::GeoPoint>& ring)
{
    double twice = 0.0;
    for (std::size_t i = 0; i < ring.size(); ++i) {
        const gis::GeoPoint& a = ring[i];
        const gis::GeoPoint& b = ring[(i + 1) % ring.size()];
        twice += a.x * b.y - b.x * a.y;
    }
    return std::abs(twice) / 2.0;
}

// The file's first layer as the one read gives it.
gp::FeatureTable readBack(const std::filesystem::path& path)
{
    auto dataset = gis::GdalDataset::open(path);
    EXPECT_TRUE(dataset.ok()) << (dataset.ok() ? "" : dataset.error().describe());
    if (!dataset.ok()) {
        return {};
    }
    auto table = (*dataset)->readTable(0);
    EXPECT_TRUE(table.ok()) << (table.ok() ? "" : table.error().describe());
    return table.ok() ? *table : gp::FeatureTable{};
}

std::size_t fieldIndex(const gp::FeatureTable& table, const std::string& name)
{
    for (std::size_t f = 0; f < table.fields.size(); ++f) {
        if (table.fields[f].name == name) {
            return f;
        }
    }
    ADD_FAILURE() << "no field " << name;
    return table.fields.size();
}

const PropertyValue* property(const Entity& entity, const std::string& key)
{
    const auto found = entity.properties.find(key);
    return found == entity.properties.end() ? nullptr : &found->second;
}

// ---- a hole is a hole ------------------------------------------------------------------------

TEST(VectorFidelity, ALotWithAHoleRoundTripsThroughGpkgWith9600SquareMetres)
{
    // The defect: a 100 m lot with a 20 m hole, imported and exported, came
    // back as two polygons summing to 10400 m2. By hand: 100 x 100 - 20 x 20
    // = 9600.
    const TempDir dir("hole");
    gis::VectorFeature lot;
    lot.geometry.kind = gis::GeometryKind::Polygon;
    lot.geometry.parts = {{{330000, 6250000, 0}, {330100, 6250000, 0}, {330100, 6250100, 0},
                           {330000, 6250100, 0}, {330000, 6250000, 0}},
                          {{330040, 6250040, 0}, {330060, 6250040, 0}, {330060, 6250060, 0},
                           {330040, 6250060, 0}, {330040, 6250040, 0}}};
    lot.attributes.emplace("lot", "7");
    gis::VectorExportOptions write;
    write.projectionWkt = kMga56;
    ASSERT_TRUE(gis::GdalDataset::writeVector(dir.file("lot.gpkg"), {lot}, write).ok());

    const auto imported = interop::importVector(dir.file("lot.gpkg"));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_EQ(imported->entities.size(), 2u) << "a ring each: the model has no polygon with holes";
    const Model model = modelOf(*imported);

    interop::VectorExportOptions options;
    options.projectionWkt = kMga56;
    const auto exported = interop::exportVector(model, dir.file("back.gpkg"), options);
    ASSERT_TRUE(exported.ok()) << exported.error().describe();
    EXPECT_EQ(exported->featuresWritten, 1u) << "one lot, not a lot and a solid hole";

    const gp::FeatureTable table = readBack(dir.file("back.gpkg"));
    ASSERT_EQ(table.features.size(), 1u);
    const auto& rings = table.features.front().parts.front().parts;
    ASSERT_EQ(rings.size(), 2u);
    EXPECT_NEAR(ringArea(rings[0]) - ringArea(rings[1]), 9600.0, 1e-6);
}

// ---- field types -------------------------------------------------------------------------------

TEST(VectorFidelity, RealAndIntegerPropertiesExportTyped)
{
    // An area of 8400.5 went out as the text "8400.5", and SUM() over it in
    // any GIS was then no sum at all.
    const TempDir dir("typed-out");
    Entity parcel = square(0, 0, 10);
    parcel.properties["area_m2"] = 8400.5;
    parcel.properties["lot"] = std::int64_t{7};
    parcel.properties["sealed"] = true;
    parcel.properties["owner"] = std::string("Smith");
    const Model model = modelWith({parcel});
    ASSERT_TRUE(interop::exportVector(model, dir.file("typed.gpkg")).ok());

    const gp::FeatureTable table = readBack(dir.file("typed.gpkg"));
    ASSERT_EQ(table.features.size(), 1u);
    const auto typeOf = [&](const std::string& name) {
        return table.fields[fieldIndex(table, name)].type;
    };
    EXPECT_EQ(typeOf("katana_id"), gp::FieldType::Integer64);
    EXPECT_EQ(typeOf("layer"), gp::FieldType::String);
    EXPECT_EQ(typeOf("area_m2"), gp::FieldType::Real);
    EXPECT_EQ(typeOf("lot"), gp::FieldType::Integer64);
    EXPECT_EQ(typeOf("sealed"), gp::FieldType::Boolean);
    EXPECT_EQ(typeOf("owner"), gp::FieldType::String);
    const auto& values = table.features.front().values;
    EXPECT_EQ(std::get<double>(values[fieldIndex(table, "area_m2")]), 8400.5);
    EXPECT_EQ(std::get<std::int64_t>(values[fieldIndex(table, "lot")]), 7);
    EXPECT_EQ(std::get<bool>(values[fieldIndex(table, "sealed")]), true);
}

TEST(VectorFidelity, TypedFieldsImportTyped)
{
    const TempDir dir("typed-in");
    writeText(dir.file("parcels.geojson"), R"({"type": "FeatureCollection", "features": [
        {"type": "Feature",
         "properties": {"lot": 7, "area": 8400.5, "sealed": true, "owner": "Smith",
                        "surveyed": "2024-05-01", "missing": null},
         "geometry": {"type": "Point", "coordinates": [1, 2]}}]})");
    const auto imported = interop::importVector(dir.file("parcels.geojson"));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_EQ(imported->entities.size(), 1u);
    const Entity& entity = imported->entities.front();
    ASSERT_NE(property(entity, "lot"), nullptr);
    EXPECT_EQ(*property(entity, "lot"), PropertyValue(std::int64_t{7}));
    EXPECT_EQ(*property(entity, "area"), PropertyValue(8400.5));
    EXPECT_EQ(*property(entity, "sealed"), PropertyValue(true));
    EXPECT_EQ(*property(entity, "owner"), PropertyValue(std::string("Smith")));
    // A date is ISO 8601 text, whatever GDAL typed it as.
    EXPECT_EQ(*property(entity, "surveyed"), PropertyValue(std::string("2024-05-01")));
    EXPECT_EQ(property(entity, "missing"), nullptr) << "absent is no property, not an empty one";
}

// ---- KML holds longitude and latitude -------------------------------------------------------

TEST(VectorFidelity, KmlOfAProjectedDrawingIsReprojectedToLonLat)
{
    // GDAL's KML writer, handed MGA coordinates, wrote a placemark without
    // its geometry and succeeded. The expected lon/lat is PROJ's own, from
    // `echo 330000 6250000 | cs2cs -f %.12f EPSG:28356 EPSG:4326`:
    // -33.876653622997 151.161906845856 (latitude first, as EPSG orders it).
    const TempDir dir("kml");
    Entity peg;
    peg.geometry = katana::entity::PointGeometry{Point2(330000.0, 6250000.0)};
    peg.properties["code"] = std::string("PEG");
    const Model model = modelWith({peg});
    interop::VectorExportOptions options;
    options.projectionWkt = kMga56;
    const auto exported = interop::exportVector(model, dir.file("pegs.kml"), options);
    ASSERT_TRUE(exported.ok()) << exported.error().describe();
    EXPECT_TRUE(warned(exported->warnings, "longitude and latitude"))
        << "the conversion is said, not done silently";

    const gp::FeatureTable table = readBack(dir.file("pegs.kml"));
    ASSERT_EQ(table.features.size(), 1u);
    ASSERT_EQ(table.features.front().parts.size(), 1u) << "the placemark has its geometry";
    const gis::GeoPoint& at = table.features.front().parts.front().parts.front().front();
    // 1e-8 degrees is about a millimetre; cs2cs printed twelve places.
    EXPECT_NEAR(at.x, 151.161906845856, 1e-8);
    EXPECT_NEAR(at.y, -33.876653622997, 1e-8);
}

TEST(VectorFidelity, KmlWithoutAProjectCrsIsRefusedNotWrittenEmpty)
{
    const TempDir dir("kml-no-crs");
    Entity peg;
    peg.geometry = katana::entity::PointGeometry{Point2(330000.0, 6250000.0)};
    const Model model = modelWith({peg});
    const auto exported = interop::exportVector(model, dir.file("pegs.kml"));
    ASSERT_FALSE(exported.ok());
    EXPECT_EQ(exported.error().code, ErrorCode::InvalidCRS);
    EXPECT_NE(exported.error().message.find("CRS SET"), std::string::npos)
        << "the refusal says what to do: " << exported.error().message;
    EXPECT_FALSE(std::filesystem::exists(dir.file("pegs.kml"))) << "nothing is written";
}

TEST(VectorFidelity, AKmlKeepsItsHeightsAndInventsNone)
{
    // KML's default altitude mode puts a coordinate on the ground whatever
    // its altitude, so the export says "absolute" for a heighted feature, and
    // the import takes an altitude as a height only when it says so.
    // GDAL's KMZ writer (LIBKML) gives a 2D coordinate an altitude of 0,
    // which only the mode keeps from reading as a height.
    const TempDir dir("kml-heights");
    Entity surveyed;
    surveyed.geometry = katana::entity::PointGeometry{Point2(330000.0, 6250000.0)};
    katana::entity::setHeights(surveyed.properties, {32.5});
    Entity planOnly;
    planOnly.geometry = katana::entity::PointGeometry{Point2(330010.0, 6250000.0)};
    const Model model = modelWith({surveyed, planOnly});
    for (const auto& [file, driver] : {std::pair<std::string, std::string>{"heights.kml", ""},
                                       std::pair<std::string, std::string>{"heights.kmz", "LIBKML"}}) {
        SCOPED_TRACE(file);
        interop::VectorExportOptions options;
        options.projectionWkt = kMga56;
        options.driver = driver;
        const auto exported = interop::exportVector(model, dir.file(file), options);
        ASSERT_TRUE(exported.ok()) << exported.error().describe();

        const auto imported = interop::importVector(dir.file(file));
        ASSERT_TRUE(imported.ok()) << imported.error().describe();
        ASSERT_EQ(imported->entities.size(), 2u);
        std::size_t heighted = 0;
        for (const Entity& entity : imported->entities) {
            const auto z = katana::entity::heightsOf(entity.properties, 1)[0];
            if (z) {
                EXPECT_EQ(*z, 32.5);
                ++heighted;
            }
        }
        EXPECT_EQ(heighted, 1u) << "the plan-only point must not come back at 0";
    }
}

// ---- CSV carries its geometry ----------------------------------------------------------------

TEST(VectorFidelity, CsvExportCarriesWktGeometry)
{
    // CSV used to be written as katana_id,layer and nothing else - reported as
    // exported. A mix of kinds goes as WKT, typed by the .csvt beside it.
    const TempDir dir("csv");
    Entity kerb;
    kerb.geometry = Segment2{Point2(0, 0), Point2(30, 40)};
    kerb.properties["width"] = 0.15;
    Entity lot = square(100, 100, 20);
    Entity peg;
    peg.geometry = katana::entity::PointGeometry{Point2(5, 7)};
    const Model model = modelWith({kerb, lot, peg});
    const auto exported = interop::exportVector(model, dir.file("mixed.csv"));
    ASSERT_TRUE(exported.ok()) << exported.error().describe();
    const std::string text = fileText(dir.file("mixed.csv"));
    EXPECT_NE(text.find("LINESTRING (0 0,30 40)"), std::string::npos) << text;
    EXPECT_NE(text.find("POLYGON ((100 100,120 100,120 120,100 120,100 100))"), std::string::npos)
        << text;
    EXPECT_TRUE(std::filesystem::exists(dir.file("mixed.csvt")));

    const auto imported = interop::importVector(dir.file("mixed.csv"));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_EQ(imported->entities.size(), 3u);
    for (const Entity& entity : imported->entities) {
        EXPECT_EQ(property(entity, "WKT"), nullptr) << "the geometry column is not a property";
        if (const auto* line = std::get_if<Segment2>(&entity.geometry)) {
            EXPECT_EQ(line->length(), 50.0); // 3-4-5, times 10
            EXPECT_EQ(*property(entity, "width"), PropertyValue(0.15));
        } else if (const auto* ring = std::get_if<Polyline2>(&entity.geometry)) {
            EXPECT_TRUE(ring->closed);
            EXPECT_EQ(ring->area(), 400.0);
        }
    }
}

TEST(VectorFidelity, ACsvOfSurveyedPointsHasXYAndZColumnsAndKeepsItsHeights)
{
    const TempDir dir("csv-points");
    Entity a;
    a.geometry = katana::entity::PointGeometry{Point2(330000.125, 6250000.5)};
    katana::entity::setHeights(a.properties, {32.25});
    Entity b;
    b.geometry = katana::entity::PointGeometry{Point2(330001.125, 6250000.5)};
    katana::entity::setHeights(b.properties, {33.0});
    const Model model = modelWith({a, b});
    ASSERT_TRUE(interop::exportVector(model, dir.file("points.csv")).ok());
    const std::string text = fileText(dir.file("points.csv"));
    EXPECT_EQ(text.rfind("X,Y,Z,", 0), 0u) << text;

    const auto imported = interop::importVector(dir.file("points.csv"));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_EQ(imported->entities.size(), 2u);
    const auto* first = std::get_if<katana::entity::PointGeometry>(&imported->entities[0].geometry);
    ASSERT_NE(first, nullptr);
    EXPECT_EQ(first->position, Point2(330000.125, 6250000.5));
    EXPECT_EQ(katana::entity::heightsOf(imported->entities[0].properties, 1)[0],
              std::optional<double>(32.25));
    EXPECT_EQ(property(imported->entities[0], "X"), nullptr);
    EXPECT_EQ(property(imported->entities[0], "Z"), nullptr);
}

// ---- MapInfo keeps the millimetre ------------------------------------------------------------

TEST(VectorFidelity, TabExportKeepsSurveyCoordinatesToTheMillimetre)
{
    // The defect: LINE 330000,6250000 330100,6250100 to a .tab read back as
    // 330099.99 6250099.99. MapInfo keeps coordinates as integers across its
    // bounds, and the default bounds make that step a centimetre; the data's
    // own extent makes it far below 1e-4.
    const TempDir dir("tab");
    Entity line;
    line.geometry = Segment2{Point2(330000.0, 6250000.0), Point2(330100.0, 6250100.0)};
    Entity other;
    other.geometry = Segment2{Point2(330000.125, 6250000.0), Point2(330100.456, 6250100.789)};
    const Model model = modelWith({line, other});
    ASSERT_TRUE(interop::exportVector(model, dir.file("lines.tab")).ok());

    const auto imported = interop::importVector(dir.file("lines.tab"));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_EQ(imported->entities.size(), 2u);
    const auto* back = std::get_if<Segment2>(&imported->entities[0].geometry);
    const auto* back2 = std::get_if<Segment2>(&imported->entities[1].geometry);
    ASSERT_NE(back, nullptr);
    ASSERT_NE(back2, nullptr);
    EXPECT_NEAR(back->start.x, 330000.0, 1e-4);
    EXPECT_NEAR(back->start.y, 6250000.0, 1e-4);
    EXPECT_NEAR(back->end.x, 330100.0, 1e-4);
    EXPECT_NEAR(back->end.y, 6250100.0, 1e-4);
    EXPECT_NEAR(back2->start.x, 330000.125, 1e-4);
    EXPECT_NEAR(back2->end.x, 330100.456, 1e-4);
    EXPECT_NEAR(back2->end.y, 6250100.789, 1e-4);
}

// ---- curves and faces ----------------------------------------------------------------------------

TEST(VectorFidelity, ACircularStringImportsAsAnArc)
{
    // Written as WKT in a CSV, which GDAL reads curves from as they are. By
    // hand: the circle through (0,0), (10,10) and (20,0) has its centre at
    // (10,0) and a radius of 10; ending where it starts through (20,0) it is
    // the whole circle with that centre and radius.
    const TempDir dir("arcs");
    writeText(dir.file("curves.csv"),
              "WKT,what\n"
              "\"CIRCULARSTRING (0 0,10 10,20 0)\",arc\n"
              "\"CIRCULARSTRING (0 0,20 0,0 0)\",circle\n"
              "\"CURVEPOLYGON (CIRCULARSTRING (0 0,20 0,0 0))\",disc\n"
              "\"CIRCULARSTRING (0 0,10 10,20 0,30 -10,40 0)\",wave\n");
    const auto imported = interop::importVector(dir.file("curves.csv"));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    std::map<std::string, const Entity*> byWhat;
    for (const Entity& entity : imported->entities) {
        byWhat[std::get<std::string>(entity.properties.at("what"))] = &entity;
    }
    ASSERT_EQ(byWhat.size(), 4u);

    const auto* arc = std::get_if<katana::geometry::Arc2>(&byWhat["arc"]->geometry);
    ASSERT_NE(arc, nullptr) << "an arc, not three points of a polyline";
    EXPECT_NEAR(arc->center.x, 10.0, 1e-12);
    EXPECT_NEAR(arc->center.y, 0.0, 1e-12);
    EXPECT_NEAR(arc->radius, 10.0, 1e-12);
    EXPECT_NEAR(arc->startPoint().x, 0.0, 1e-9);
    EXPECT_NEAR(arc->endPoint().x, 20.0, 1e-9);
    EXPECT_NEAR(arc->midpoint().y, 10.0, 1e-9) << "through (10,10), the top, not the bottom";

    for (const char* whole : {"circle", "disc"}) {
        const auto* circle = std::get_if<katana::geometry::Circle2>(&byWhat[whole]->geometry);
        ASSERT_NE(circle, nullptr) << whole;
        EXPECT_EQ(circle->center, Point2(10.0, 0.0)) << whole;
        EXPECT_EQ(circle->radius, 10.0) << whole;
    }

    // Two arcs in one line: chords within the import's 1 mm, every vertex on
    // one of the two circles (centres (10,0) and (30,0), radius 10).
    const auto* wave = std::get_if<Polyline2>(&byWhat["wave"]->geometry);
    ASSERT_NE(wave, nullptr);
    ASSERT_GT(wave->vertices.size(), 5u);
    for (const Point2& vertex : wave->vertices) {
        const double left = std::abs(std::hypot(vertex.x - 10.0, vertex.y) - 10.0);
        const double right = std::abs(std::hypot(vertex.x - 30.0, vertex.y) - 10.0);
        EXPECT_LT(std::min(left, right), 1e-9) << vertex.x << "," << vertex.y;
    }
    for (std::size_t i = 0; i + 1 < wave->vertices.size(); ++i) {
        const Point2& a = wave->vertices[i];
        const Point2& b = wave->vertices[i + 1];
        const Point2 middle((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        const double centre = middle.x < 20.0 ? 10.0 : 30.0;
        EXPECT_LE(10.0 - std::hypot(middle.x - centre, middle.y), 0.001 + 1e-12);
    }
    EXPECT_TRUE(warned(imported->warnings, "made chords"));
}

TEST(VectorFidelity, ATinIsReportedAsSkipped)
{
    const TempDir dir("tin");
    writeText(dir.file("faces.csv"), "WKT,what\n"
                                     "\"TIN Z (((0 0 0,10 0 0,0 10 0,0 0 0)))\",tin\n"
                                     "\"POINT (1 2)\",peg\n");
    const auto imported = interop::importVector(dir.file("faces.csv"));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    EXPECT_EQ(imported->entities.size(), 1u);
    EXPECT_EQ(imported->featuresRead, 2u);
    EXPECT_EQ(imported->featuresSkipped, 1u);
    ASSERT_TRUE(imported->skipped.contains("tin"));
    EXPECT_EQ(imported->skipped.at("tin"), 1u);
    EXPECT_TRUE(warned(imported->warnings, "1 TIN left out")) << "said, never dropped silently";
}

// ---- GDAL's warnings -----------------------------------------------------------------------------

// std::cout and std::cerr swapped for strings while it lives.
class Captured {
  public:
    Captured() : out_(std::cout.rdbuf(outText_.rdbuf())), err_(std::cerr.rdbuf(errText_.rdbuf())) {}
    ~Captured()
    {
        std::cout.rdbuf(out_);
        std::cerr.rdbuf(err_);
    }
    Captured(const Captured&) = delete;
    Captured& operator=(const Captured&) = delete;
    [[nodiscard]] std::string out() const { return outText_.str(); }
    [[nodiscard]] std::string err() const { return errText_.str(); }

  private:
    std::ostringstream outText_;
    std::ostringstream errText_;
    std::streambuf* out_;
    std::streambuf* err_;
};

TEST(VectorFidelity, WarningsReachTheReply)
{
    // A shapefile's field names are ten characters at most, and GDAL says so
    // when it shortens one - to a handler that used to drop it.
    const TempDir dir("warnings");
    const std::string file = dir.file("pegs.shp").generic_string();
    katana::app::Session session(nullptr);
    Captured captured;
    ASSERT_TRUE(session.run("POINT 1,2"));
    ASSERT_TRUE(session.run("SELECT ALL"));
    ASSERT_TRUE(session.run("PROP SET surveyed_by_party Smith text")) << captured.err();
    const std::size_t before = captured.out().size();
    ASSERT_TRUE(session.run("EXPORT \"" + file + "\"")) << captured.err();
    const std::string reply = captured.out().substr(before);
    // GDAL's own words, as its shapefile writer says them.
    EXPECT_NE(reply.find("'surveyed_by_party' to 'surveyed_b'"), std::string::npos)
        << "GDAL's warning names the field it shortened: " << reply;
}

// ---- every driver of the export table --------------------------------------------------------

TEST(VectorFidelity, RoundTripOfEveryExportDriver)
{
    // Two lines with typed properties, written and read back by each driver:
    // the geometry to the micrometre (the lon/lat of KML to 1e-9 degrees,
    // 0.1 mm), and the fields each format can hold with their types.
    struct Case {
        const char* file;
        bool fields; // DXF has only its own
        bool typed;  // the types survive, not only the text
    };
    const std::vector<Case> cases = {{"lines.shp", true, true},   {"lines.geojson", true, true},
                                     {"lines.gpkg", true, true},  {"lines.kml", true, true},
                                     {"lines.gml", true, true},   {"lines.dxf", false, false},
                                     {"lines.csv", true, true},   {"lines.sqlite", true, true},
                                     {"lines.tab", true, true}};
    Entity kerb;
    kerb.geometry = Segment2{Point2(330000.0, 6250000.0), Point2(330030.0, 6250040.0)};
    kerb.properties["code"] = std::string("KERB");
    kerb.properties["width"] = 0.15;
    kerb.properties["count"] = std::int64_t{3};
    Entity fence;
    Polyline2 fenceLine;
    fenceLine.vertices = {Point2(330000.0, 6250100.0), Point2(330010.0, 6250105.0),
                          Point2(330020.0, 6250100.0)};
    fence.geometry = fenceLine;
    fence.properties["code"] = std::string("FENCE");
    fence.properties["width"] = 0.05;
    fence.properties["count"] = std::int64_t{12};
    const Model model = modelWith({kerb, fence});

    for (const Case& test : cases) {
        SCOPED_TRACE(test.file);
        const TempDir dir(std::string("every-") + test.file);
        interop::VectorExportOptions options;
        options.projectionWkt = kMga56;
        const auto exported = interop::exportVector(model, dir.file(test.file), options);
        ASSERT_TRUE(exported.ok()) << exported.error().describe();
        EXPECT_EQ(exported->featuresWritten, 2u);

        const auto imported = interop::importVector(dir.file(test.file));
        ASSERT_TRUE(imported.ok()) << imported.error().describe();
        ASSERT_EQ(imported->entities.size(), 2u);
        const bool lonLat = std::string(test.file).ends_with(".kml");
        for (const Entity& back : imported->entities) {
            const bool isKerb = std::holds_alternative<Segment2>(back.geometry);
            const Entity& original = isKerb ? kerb : fence;
            std::vector<Point2> expected;
            if (isKerb) {
                const auto& segment = std::get<Segment2>(original.geometry);
                expected = {segment.start, segment.end};
            } else {
                expected = std::get<Polyline2>(original.geometry).vertices;
            }
            std::vector<Point2> got;
            if (const auto* segment = std::get_if<Segment2>(&back.geometry)) {
                got = {segment->start, segment->end};
            } else if (const auto* line = std::get_if<Polyline2>(&back.geometry)) {
                got = line->vertices;
            }
            ASSERT_EQ(got.size(), expected.size());
            for (std::size_t i = 0; i < got.size(); ++i) {
                Point2 want = expected[i];
                if (lonLat) {
                    const auto moved = gis::transformPoint(want.x, want.y, kMga56, "EPSG:4326");
                    ASSERT_TRUE(moved.ok());
                    want = Point2((*moved)[0], (*moved)[1]);
                }
                const double tolerance = lonLat ? 1e-9 : 1e-6;
                EXPECT_NEAR(got[i].x, want.x, tolerance);
                EXPECT_NEAR(got[i].y, want.y, tolerance);
            }
            if (!test.fields) {
                continue;
            }
            const PropertyValue* code = property(back, "code");
            ASSERT_NE(code, nullptr);
            EXPECT_EQ(*code, original.properties.at("code"));
            const PropertyValue* width = property(back, "width");
            const PropertyValue* count = property(back, "count");
            ASSERT_NE(width, nullptr);
            ASSERT_NE(count, nullptr);
            if (test.typed) {
                ASSERT_TRUE(std::holds_alternative<double>(*width)) << katana::entity::typeName(*width);
                EXPECT_NEAR(std::get<double>(*width), std::get<double>(original.properties.at("width")),
                            1e-12);
                EXPECT_EQ(*count, original.properties.at("count"));
            }
        }
    }
}

} // namespace
