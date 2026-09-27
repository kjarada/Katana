// The draw system's curves - the curve polyline (arcs by bulge, heights in
// its vertices), the ellipse and the spline - through everything the GDAL
// side reads the drawing by (docs/geoprocessing.md "Bindings", docs/terrain.md
// "Surfaces on every front end" and "Sampling and drape", docs/interop.md
// "Export options"): the one entity-to-feature conversion, the surface built
// from the drawing, the drape, a zone, a clip boundary, a dissolve and EXPORT.
//
// Every expected value is worked by hand beside it. Two bounds recur:
//
//   - A chorded curve's area. Every chord point lies on the curve, so on a
//     convex curve the chorded ring lies inside it and its area is SMALLER,
//     by the sum of the segments between each chord and its arc. A segment
//     of chord c whose arc strays at most s from it has area at most c x s
//     (it lies inside the c by s rectangle over the chord); the chords are
//     each shorter than their arcs and s is at most the chord tolerance, so
//     the loss is at most (the curve's length) x (the tolerance).
//   - The default chord tolerance is DrawingDatasetOptions::curveTolerance,
//     1 mm (the export's).

#include <cmath>
#include <cstdint>
#include <map>
#include <numbers>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/geo/surface_input.hpp"
#include "katana/geometry/curves2d.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"
#include "vector_fixture.hpp"

namespace {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::CurvePolyline2;
using katana::geometry::CurveVertex;
using katana::geometry::Ellipse2;
using katana::geometry::Point2;
using katana::geometry::Spline2;
using katana::gis::GeometryKind;
using katana::gis::GeoPoint;

const std::string kPlane = std::string(KATANA_GEO_TEST_DATA) + "/plane.asc";
constexpr double kTolerance = 0.001; // DrawingDatasetOptions::curveTolerance
constexpr double kPi = std::numbers::pi;

class DrawingCurves : public katana::geo_test::VectorFixture {
  protected:
    const gp::FeatureTable* table(const igeo::DrawingDataset& dataset, const std::string& name)
    {
        for (const gp::FeatureTable& each : dataset.set.tables) {
            if (each.name == name) {
                return &each;
            }
        }
        return nullptr;
    }

    static std::string quoted(const std::string& path) { return "\"" + path + "\""; }
};

// The 10 x 10 square from (0,0), anticlockwise, its east side (from (10,0) to
// (10,10)) an arc of a quarter turn bulging out of it. A positive bulge turns
// anticlockwise, which going north puts the arc to the east - outside.
// The arc: chord 10, included angle theta = pi/2, bulge tan(theta/4) =
// tan(pi/8); radius 10 / (2 sin(pi/4)) = 5 sqrt 2, so r^2 = 50; the segment
// between chord and arc has area r^2 / 2 (theta - sin theta) = 25 (pi/2 - 1).
// The whole area is 100 + 25 (pi/2 - 1) = 114.2699...; the arc's length is
// r theta = 5 sqrt 2 x pi / 2 = 11.107.
CurvePolyline2 bulgedSquare()
{
    CurvePolyline2 shape;
    shape.vertices = {CurveVertex{{0, 0}, 0.0, std::nullopt},
                      CurveVertex{{10, 0}, std::tan(kPi / 8.0), std::nullopt},
                      CurveVertex{{10, 10}, 0.0, std::nullopt},
                      CurveVertex{{0, 10}, 0.0, std::nullopt}};
    shape.closed = true;
    return shape;
}

// From (0,0) at height 10 to (10,0) at height 20 by an anticlockwise
// semicircle (bulge 1): centre (5,0), radius 5, through (5,-5). A point at
// angle phi about the centre (phi from pi to 2 pi, anticlockwise from +x) is
// (phi - pi) / pi of the way along the arc by length, so its height by the
// geometry's rule (linear by length along the segment) is
// 10 + 10 (phi - pi) / pi.
CurvePolyline2 slopingSemicircle(std::optional<double> endHeight = 20.0)
{
    CurvePolyline2 shape;
    shape.vertices = {CurveVertex{{0, 0}, 1.0, 10.0}, CurveVertex{{10, 0}, 0.0, endHeight}};
    return shape;
}

double heightOnSemicircle(double x, double y)
{
    // atan2 gives (-pi, pi]: below the diameter is (-pi, 0], and the end
    // (10,0) is 0 - each a turn short of the angle anticlockwise from the
    // start's pi. The start itself is pi (or -pi, read at y = -0).
    double phi = std::atan2(y, x - 5.0);
    if (phi < kPi / 2.0) {
        phi += 2.0 * kPi;
    }
    return 10.0 + 10.0 * (phi - kPi) / kPi;
}

double ringArea(const std::vector<GeoPoint>& ring)
{
    double twice = 0.0;
    for (std::size_t i = 0; i < ring.size(); ++i) {
        const GeoPoint& a = ring[i];
        const GeoPoint& b = ring[(i + 1) % ring.size()];
        twice += a.x * b.y - b.x * a.y;
    }
    return twice / 2.0;
}

// The ellipse centred (20,15) with semi-axes 15 (along x) and 10: inside
// plane.asc (0..40 by 0..30). Its area is pi a b = 150 pi = 471.2389; its
// perimeter, by Ramanujan's second approximation (error below 1e-6 at this
// eccentricity), pi (3 (a + b) - sqrt((3a + b)(a + 3b))) = pi (75 -
// sqrt(55 x 45)) = 79.327.
Ellipse2 zoneEllipse()
{
    Ellipse2 ellipse;
    ellipse.center = {20, 15};
    ellipse.majorAxis = {15, 0};
    ellipse.ratio = 10.0 / 15.0;
    return ellipse;
}
constexpr double kEllipseArea = 150.0 * kPi;
const double kEllipsePerimeter = kPi * (75.0 - std::sqrt(55.0 * 45.0));

// ---- the one conversion ---------------------------------------------------------------------

TEST_F(DrawingCurves, AClosedCurvePolylineIsAnAreaShortOfItsArcsSegmentByAtMostTheChordError)
{
    const EntityId id = add(bulgedSquare());
    auto dataset = igeo::drawingDataset(document.model(), {id});
    ASSERT_TRUE(dataset.ok()) << dataset.error().describe();
    ASSERT_EQ(dataset->set.tables.size(), 1u);
    const gp::FeatureTable* areas = table(*dataset, "polygons");
    ASSERT_NE(areas, nullptr);
    ASSERT_EQ(areas->features.size(), 1u);
    const katana::gis::VectorGeometry& ring = areas->features.front().parts.front();
    EXPECT_EQ(ring.kind, GeometryKind::Polygon);
    EXPECT_FALSE(ring.hasZ); // no vertex has a height
    ASSERT_EQ(ring.parts.size(), 1u);
    const auto& points = ring.parts.front();
    // More than the four vertices: the arc is chords. The ring is not closed
    // by a repeat, as every area's ring here.
    EXPECT_GT(points.size(), 4u);
    EXPECT_FALSE(points.front().x == points.back().x && points.front().y == points.back().y);
    const double exact = 100.0 + 25.0 * (kPi / 2.0 - 1.0);
    const double arcLength = 5.0 * std::sqrt(2.0) * kPi / 2.0;
    const double area = ringArea(points);
    EXPECT_GT(area, 0.0); // anticlockwise, as drawn
    EXPECT_LE(area, exact + 1e-9);
    EXPECT_GE(area, exact - arcLength * kTolerance);
    EXPECT_EQ(dataset->stats.polygons, 1u);
    EXPECT_EQ(std::get<std::string>(areas->features.front().values[4]), "curvepolyline");
}

TEST_F(DrawingCurves, ASlopingArcsChordPointsTakeTheHeightLinearByLengthAlongIt)
{
    const EntityId id = add(slopingSemicircle());
    auto dataset = igeo::drawingDataset(document.model(), {id});
    ASSERT_TRUE(dataset.ok()) << dataset.error().describe();
    const gp::FeatureTable* lines = table(*dataset, "lines");
    ASSERT_NE(lines, nullptr);
    ASSERT_EQ(lines->features.size(), 1u);
    const katana::gis::VectorGeometry& line = lines->features.front().parts.front();
    EXPECT_EQ(line.kind, GeometryKind::LineString);
    ASSERT_TRUE(line.hasZ);
    EXPECT_TRUE(lines->hasZ);
    const auto& points = line.parts.front();
    ASSERT_GT(points.size(), 2u);
    EXPECT_DOUBLE_EQ(points.front().z, 10.0);
    EXPECT_DOUBLE_EQ(points.back().z, 20.0);
    for (const GeoPoint& p : points) {
        // On the circle, below its diameter, at the height worked out above.
        // The trigonometry is exact to a few ulps of the coordinates: 1e-9.
        EXPECT_NEAR(std::hypot(p.x - 5.0, p.y), 5.0, 1e-9);
        EXPECT_LE(p.y, 1e-9);
        EXPECT_NEAR(p.z, heightOnSemicircle(p.x, p.y), 1e-9) << p.x << "," << p.y;
    }
}

TEST_F(DrawingCurves, AnArcWithAnEndNotSurveyedGoesInPlanNotAtZero)
{
    const EntityId id = add(slopingSemicircle(std::nullopt));
    auto dataset = igeo::drawingDataset(document.model(), {id});
    ASSERT_TRUE(dataset.ok());
    const gp::FeatureTable* lines = table(*dataset, "lines");
    ASSERT_NE(lines, nullptr);
    EXPECT_FALSE(lines->features.front().parts.front().hasZ);
    ASSERT_FALSE(dataset->stats.warnings.empty());
    EXPECT_NE(dataset->stats.warnings.front().find("height at every vertex"), std::string::npos);
    // And an algorithm that needs heights is not handed it.
    igeo::DrawingDatasetOptions heighted;
    heighted.requireHeights = true;
    auto gridded = igeo::drawingDataset(document.model(), {id}, heighted);
    ASSERT_TRUE(gridded.ok());
    EXPECT_EQ(gridded->stats.skipped["heightless"], 1u);
}

TEST_F(DrawingCurves, AWholeEllipseIsAnAreaAndAnArcOfOneALineBothInPlan)
{
    const EntityId whole = add(zoneEllipse());
    Ellipse2 half = zoneEllipse();
    half.sweep = kPi;
    const EntityId arc = add(half);
    auto dataset = igeo::drawingDataset(document.model(), {whole, arc});
    ASSERT_TRUE(dataset.ok());
    const gp::FeatureTable* areas = table(*dataset, "polygons");
    const gp::FeatureTable* lines = table(*dataset, "lines");
    ASSERT_NE(areas, nullptr);
    ASSERT_NE(lines, nullptr);
    ASSERT_EQ(areas->features.size(), 1u);
    ASSERT_EQ(lines->features.size(), 1u);
    const auto& ring = areas->features.front().parts.front();
    EXPECT_FALSE(ring.hasZ);
    const double area = ringArea(ring.parts.front());
    EXPECT_LE(area, kEllipseArea + 1e-9);
    EXPECT_GE(area, kEllipseArea - kEllipsePerimeter * kTolerance);
    // The half from t = 0 to pi: (35,15) round the top to (5,15).
    const auto& path = lines->features.front().parts.front();
    EXPECT_EQ(path.kind, GeometryKind::LineString);
    EXPECT_NEAR(path.parts.front().front().x, 35.0, 1e-9);
    EXPECT_NEAR(path.parts.front().back().x, 5.0, 1e-9);
    EXPECT_NEAR(path.parts.front().back().y, 15.0, 1e-9);
}

TEST_F(DrawingCurves, AClosedSplineIsAnAreaAndAnOpenOneALine)
{
    auto closed = Spline2::throughPoints({{0, 0}, {10, 0}, {10, 10}, {0, 10}, {0, 0}});
    ASSERT_TRUE(closed.ok()) << closed.error().describe();
    auto open = Spline2::throughPoints({{0, 20}, {5, 25}, {10, 20}});
    ASSERT_TRUE(open.ok());
    const EntityId shut = add(*closed);
    const EntityId path = add(*open);
    auto dataset = igeo::drawingDataset(document.model(), {shut, path});
    ASSERT_TRUE(dataset.ok());
    const gp::FeatureTable* areas = table(*dataset, "polygons");
    const gp::FeatureTable* lines = table(*dataset, "lines");
    ASSERT_NE(areas, nullptr);
    ASSERT_NE(lines, nullptr);
    EXPECT_EQ(areas->features.size(), 1u);
    EXPECT_EQ(lines->features.size(), 1u);
    // The open one runs from its first fit point to its last.
    const auto& points = lines->features.front().parts.front().parts.front();
    EXPECT_NEAR(points.front().x, 0.0, 1e-9);
    EXPECT_NEAR(points.back().x, 10.0, 1e-9);
    EXPECT_NEAR(points.back().y, 20.0, 1e-9);
}

// ---- the surface from the drawing -----------------------------------------------------------

TEST_F(DrawingCurves, ASurfaceTakesACurvePolylineAsABreaklineThroughItsChordsAtTheirHeights)
{
    const EntityId string = add(slopingSemicircle());
    const EntityId ellipse = add(zoneEllipse());
    const katana::cad::geo::SurfaceInput input =
        katana::cad::geo::surfaceInput(document.model(), {string, ellipse});
    EXPECT_EQ(input.used, 1u);
    EXPECT_EQ(input.heightlessVertices, 0u);
    EXPECT_EQ(input.skipped.at("ellipse"), 1u);
    ASSERT_EQ(input.input.breaklines.size(), 1u);
    const auto& vertices = input.input.breaklines.front().vertices;
    EXPECT_FALSE(input.input.breaklines.front().closed);
    // The arc as chords (more than its two ends), within a millimetre
    // (geometry::kCurveChordTolerance), each at its height by length.
    ASSERT_GT(vertices.size(), 2u);
    EXPECT_EQ(input.input.points.size(), vertices.size());
    for (const katana::geometry::Point3& p : vertices) {
        EXPECT_NEAR(std::hypot(p.x - 5.0, p.y), 5.0, 1e-9);
        EXPECT_NEAR(p.z, heightOnSemicircle(p.x, p.y), 1e-9);
    }
}

TEST_F(DrawingCurves, ASurfaceBreaksACurvePolylineWhereAnEndHasNoHeight)
{
    // The arc's far end is not surveyed: every chord point after the first
    // has no height (none is invented), so only the first vertex is a point
    // and there is no breakline of two.
    const EntityId string = add(slopingSemicircle(std::nullopt));
    const katana::cad::geo::SurfaceInput input =
        katana::cad::geo::surfaceInput(document.model(), {string});
    EXPECT_TRUE(input.input.breaklines.empty());
    ASSERT_EQ(input.input.points.size(), 1u);
    EXPECT_DOUBLE_EQ(input.input.points.front().z, 10.0);
    EXPECT_GT(input.heightlessVertices, 1u);
}

TEST_F(DrawingCurves, AClosedCurvePolylineIsAClosedBreaklineWithNoRepeatedPoint)
{
    // Two vertices at height 5, each bulge 1: two semicircles, a circle of
    // radius 5 about (5,0), level.
    CurvePolyline2 circle;
    circle.vertices = {CurveVertex{{0, 0}, 1.0, 5.0}, CurveVertex{{10, 0}, 1.0, 5.0}};
    circle.closed = true;
    const EntityId id = add(circle);
    const auto input = katana::cad::geo::surfaceInput(document.model(), {id});
    ASSERT_EQ(input.input.breaklines.size(), 1u);
    const auto& ring = input.input.breaklines.front();
    EXPECT_TRUE(ring.closed);
    ASSERT_GT(ring.vertices.size(), 3u);
    EXPECT_FALSE(ring.vertices.front().x == ring.vertices.back().x &&
                 ring.vertices.front().y == ring.vertices.back().y);
    for (const katana::geometry::Point3& p : ring.vertices) {
        EXPECT_DOUBLE_EQ(p.z, 5.0);
    }
}

// ---- the drape ------------------------------------------------------------------------------

TEST_F(DrawingCurves, DrapeGivesACurvePolylinesVerticesTheirHeightsInItsGeometryAsOneStep)
{
    // plane.asc is z = 100 + 0.05 x at the cell centres; bilinear sampling of
    // a plane is the plane (tests/geo/test_drape_and_sample.cpp): 5.5 ->
    // 100.275, 20.25 -> 101.0125, 35 -> 101.75, each within 1e-5 (Float32).
    CurvePolyline2 drawn;
    drawn.vertices = {CurveVertex{{5.5, 5}, 0.2, std::nullopt},
                      CurveVertex{{20.25, 10}, 0.0, std::nullopt},
                      CurveVertex{{35, 25}, 0.0, std::nullopt}};
    const EntityId id = add(drawn);
    const std::size_t before = document.history().undoCount();
    const std::string reply = ok("DRAPE FILE " + quoted(kPlane) + " DRAWING");
    // One step, or the undo at the end would take the drawing of it away.
    ASSERT_EQ(document.history().undoCount(), before + 1) << reply;
    EXPECT_NE(reply.find("entities=1 vertices=3 off=0"), std::string::npos) << reply;
    const Entity* draped = document.model().entities.find(id);
    ASSERT_NE(draped, nullptr);
    const auto* curve = std::get_if<CurvePolyline2>(&draped->geometry);
    ASSERT_NE(curve, nullptr) << "the drape kept the arcs";
    ASSERT_EQ(curve->vertices.size(), 3u);
    EXPECT_NEAR(curve->vertices[0].height.value_or(0.0), 100.275, 1e-5);
    EXPECT_NEAR(curve->vertices[1].height.value_or(0.0), 101.0125, 1e-5);
    EXPECT_NEAR(curve->vertices[2].height.value_or(0.0), 101.75, 1e-5);
    EXPECT_DOUBLE_EQ(curve->vertices[0].bulge, 0.2);
    EXPECT_EQ(curve->vertices[0].position, Point2(5.5, 5));
    // The heights are the geometry's: no elevation property is written.
    EXPECT_FALSE(draped->properties.contains(std::string(katana::entity::kElevationsProperty)));
    // A second drape on the same ground changes nothing.
    const std::string again = ok("DRAPE FILE " + quoted(kPlane) + " DRAWING");
    EXPECT_EQ(document.history().undoCount(), before + 1) << again;
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(std::get<CurvePolyline2>(document.model().entities.find(id)->geometry), drawn);
}

TEST_F(DrawingCurves, DrapeLeavesAnEllipseAndASplineAndCountsThem)
{
    (void)add(zoneEllipse());
    auto spline = Spline2::throughPoints({{5, 5}, {10, 8}, {15, 5}});
    ASSERT_TRUE(spline.ok());
    (void)add(*spline);
    const std::string reply = ok("DRAPE FILE " + quoted(kPlane) + " DRAWING PREVIEW");
    EXPECT_NE(reply.find("skipped.ellipse=1"), std::string::npos) << reply;
    EXPECT_NE(reply.find("skipped.spline=1"), std::string::npos) << reply;
}

// ---- zones, boundaries, dissolve ------------------------------------------------------------

TEST_F(DrawingCurves, AnEllipseZonesCountIsItsAreaInCellsWithinTheChordError)
{
    // plane.asc's cells are 1 m, so a fractional count is the zone's area:
    // the chorded ellipse's, between pi a b - (perimeter x tolerance) and
    // pi a b (see the top of the file): 471.2389 - 0.0793 .. 471.2389. The
    // coverage fractions may be single precision (exactextract keeps them
    // as float): 6e-8 relative over 471 cells is 2.8e-5; 1e-4 allows that.
    const EntityId zone = add(zoneEllipse(), "zones");
    const std::string reply =
        ok("RASTER ZONAL FILE " + quoted(kPlane) + " LAYERS zones stats=count");
    const Entity* entity = document.model().entities.find(zone);
    ASSERT_NE(entity, nullptr);
    const auto found = entity->properties.find("zone_count");
    ASSERT_NE(found, entity->properties.end()) << reply;
    const double count = std::holds_alternative<double>(found->second)
                             ? std::get<double>(found->second)
                             : static_cast<double>(std::get<std::int64_t>(found->second));
    EXPECT_LE(count, kEllipseArea + 1e-4);
    EXPECT_GE(count, kEllipseArea - kEllipsePerimeter * kTolerance - 1e-4);
}

TEST_F(DrawingCurves, ALineClippedByAnEllipseKeepsTheMajorAxisLength)
{
    // The line y = 15 from x = 0 to 40 crosses the ellipse where its major
    // axis ends, (5,15) and (35,15): 30 m inside. There the ellipse's tangent
    // is vertical, so a chord cutting the corner lies at most the chord
    // tolerance inside it along x: the clipped length is 30 less at most
    // 2 mm at the two ends.
    (void)line(0, 15, 40, 15, "pipes");
    (void)add(zoneEllipse(), "zones");
    const std::string reply = ok("GIS CLIP LAYERS pipes BY LAYERS zones");
    const auto pieces = on("gis/clip");
    ASSERT_EQ(pieces.size(), 1u) << reply;
    double length = 0.0;
    if (const auto* segment = std::get_if<katana::geometry::Segment2>(&pieces.front().geometry)) {
        length = segment->length();
    } else if (const auto* poly =
                   std::get_if<katana::geometry::Polyline2>(&pieces.front().geometry)) {
        length = poly->length();
    }
    EXPECT_LE(length, 30.0 + 1e-9);
    EXPECT_GE(length, 30.0 - 2.0 * kTolerance);
}

TEST_F(DrawingCurves, DissolveReplaceDeletesTheEllipsesItMerged)
{
    // Two whole ellipses and a closed curve polyline are three areas, and
    // REPLACE deletes all three with the merged area made - the dissolve's
    // own list of what it merged is the conversion's areas table.
    (void)add(zoneEllipse(), "lots");
    Ellipse2 second = zoneEllipse();
    second.center = {30, 15};
    (void)add(second, "lots");
    (void)add(bulgedSquare(), "lots");
    const std::string reply = ok("GIS DISSOLVE LAYERS lots REPLACE");
    EXPECT_EQ(record(reply, "output")->get("deleted"), "3") << reply;
    EXPECT_TRUE(on("lots").empty());
}

// ---- EXPORT ---------------------------------------------------------------------------------

TEST_F(DrawingCurves, ExportWritesEachCurveKindAndInfoReadsItBack)
{
    (void)add(bulgedSquare(), "curves");
    (void)add(slopingSemicircle(), "strings");
    (void)add(zoneEllipse(), "ellipses");
    auto spline = Spline2::throughPoints({{0, 20}, {5, 25}, {10, 20}});
    ASSERT_TRUE(spline.ok());
    (void)add(*spline, "splines");
    const std::string out = (scratch_ / "curves.gpkg").generic_string();
    const std::string exported = ok("EXPORT " + quoted(out) + " split=layer");
    const std::string info = ok("INFO " + quoted(out));
    std::map<std::string, katana::app::geo::Record> layers;
    for (const katana::app::geo::Record& each : katana::app::geo::parseRecords(info)) {
        if (each.kind == "layer") {
            layers.emplace(each.get("name").value_or(""), each);
        }
    }
    ASSERT_EQ(layers.size(), 4u) << exported << "\n" << info;
    for (const auto& [name, layer] : layers) {
        EXPECT_EQ(layer.get("features"), "1") << name;
    }
    // A closed curve polyline and a whole ellipse are areas, in plan; the 3D
    // string is a line with Z; the open spline a line in plan. INFO names a
    // layer's geometry by its ISO SQL/MM well-known-text type (ISO 13249-3,
    // as OGC Simple Features 1.2.1 does): Polygon, LineString, LineStringZ.
    EXPECT_EQ(layers.at("curves").get("geometry"), "Polygon") << info;
    EXPECT_EQ(layers.at("ellipses").get("geometry"), "Polygon") << info;
    EXPECT_EQ(layers.at("strings").get("geometry"), "LineStringZ") << info;
    EXPECT_EQ(layers.at("splines").get("geometry"), "LineString") << info;
}

} // namespace
