// The one reader of the drawing's polygons and lines that the surface edits
// and the volumes share (cad/terrain/drawing_shapes.hpp): what each kind of
// entity gives, typed corners, the words, and what a scope takes. Every
// corner, height, area and count asserted is worked by hand beside it.

#include <gtest/gtest.h>

#include <cmath>
#include <initializer_list>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/terrain/drawing_shapes.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/geometry/curves2d.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
namespace geometry = katana::geometry;
namespace cmd = katana::commands;

namespace {

using Heights = std::vector<std::optional<double>>;

constexpr double kPi = katana::math::kPi;
// The chord tolerance the rule names: 1 mm of sagitta.
constexpr double kSagitta = geometry::kCurveChordTolerance;

Entity entityOf(katana::entity::Geometry shape, Heights heights = {})
{
    Entity entity;
    entity.id = 7;
    entity.geometry = std::move(shape);
    if (!heights.empty()) {
        katana::entity::setHeights(entity.properties, heights);
    }
    return entity;
}

Polyline2 polyline(std::vector<Point2> vertices, bool closed)
{
    return Polyline2{std::move(vertices), closed};
}

// The square [0, 10]^2 counter-clockwise from the origin.
std::vector<Point2> square()
{
    return {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)};
}

// Twice the signed area by the shoelace, halved.
double area(const Polyline2& ring)
{
    double twice = 0.0;
    const auto& v = ring.vertices;
    for (std::size_t i = 0; i < v.size(); ++i) {
        const Point2& a = v[i];
        const Point2& b = v[(i + 1) % v.size()];
        twice += a.x * b.y - b.x * a.y;
    }
    return 0.5 * twice;
}

std::vector<std::string> wordsOf(const std::string& line)
{
    std::vector<std::string> words;
    std::size_t start = 0;
    while (start < line.size()) {
        const std::size_t blank = line.find(' ', start);
        const std::size_t end = blank == std::string::npos ? line.size() : blank;
        if (end > start) {
            words.push_back(line.substr(start, end - start));
        }
        start = end + 1;
    }
    return words;
}

} // namespace

// ---- what an entity gives as a polygon -----------------------------------------------------

TEST(DrawingShapes, AClosedPolylineIsAPolygonOfItsVerticesAtTheirHeights)
{
    const ShapeOf of = shapeOf(entityOf(polyline(square(), true), {1.0, 2.0, std::nullopt, 4.0}),
                               ShapeKind::Polygon);
    ASSERT_TRUE(of.shape.has_value());
    EXPECT_EQ(of.shape->outline.vertices, square());
    EXPECT_TRUE(of.shape->outline.closed);
    EXPECT_EQ(of.shape->heights, (Heights{1.0, 2.0, std::nullopt, 4.0}));
    EXPECT_EQ(of.shape->entity, std::optional<EntityId>(7));
    EXPECT_FALSE(of.shape->chorded);
}

TEST(DrawingShapes, APolylineDrawnBackToItsStartIsAPolygonWithoutTheRepeatedCorner)
{
    // Open, but its fifth vertex is its first: the square, four corners, the
    // fifth height going with the fifth vertex.
    std::vector<Point2> drawn = square();
    drawn.push_back(Point2(0, 0));
    const ShapeOf open =
        shapeOf(entityOf(polyline(drawn, false), {1.0, 2.0, 3.0, 4.0, 1.0}), ShapeKind::Polygon);
    ASSERT_TRUE(open.shape.has_value());
    EXPECT_EQ(open.shape->outline.vertices, square());
    EXPECT_TRUE(open.shape->outline.closed);
    EXPECT_EQ(open.shape->heights, (Heights{1.0, 2.0, 3.0, 4.0}));
    // Closed and repeating it as well: the same four.
    const ShapeOf closed = shapeOf(entityOf(polyline(drawn, true)), ShapeKind::Polygon);
    ASSERT_TRUE(closed.shape.has_value());
    EXPECT_EQ(closed.shape->outline.vertices, square());
    EXPECT_EQ(closed.shape->heights, Heights(4));
}

TEST(DrawingShapes, AnOpenPolylineALineAnArcAndAnEllipticalArcBoundNothing)
{
    EXPECT_EQ(shapeOf(entityOf(polyline(square(), false)), ShapeKind::Polygon).why,
              NotAShape::Open);
    EXPECT_EQ(shapeOf(entityOf(geometry::Segment2{Point2(0, 0), Point2(5, 5)}), ShapeKind::Polygon)
                  .why,
              NotAShape::Open);
    const geometry::Arc2 halfTurn{Point2(0, 0), 5.0, 0.0, kPi};
    EXPECT_EQ(shapeOf(entityOf(halfTurn), ShapeKind::Polygon).why, NotAShape::Open);
    geometry::Ellipse2 half;
    half.majorAxis = {20.0, 0.0};
    half.ratio = 0.5;
    half.sweep = kPi;
    const ShapeOf ellipticalArc = shapeOf(entityOf(half), ShapeKind::Polygon);
    EXPECT_FALSE(ellipticalArc.shape.has_value());
    EXPECT_EQ(ellipticalArc.why, NotAShape::Open);
    EXPECT_EQ(shapeOf(entityOf(geometry::CurvePolyline2::fromPoints(square(), false)),
                      ShapeKind::Polygon)
                  .why,
              NotAShape::Open);
}

TEST(DrawingShapes, ACurvePolylineIsChordedWithinAMillimetreAndItsArcPointsTakeTheirHeightAlongIt)
{
    // The half-disc over [0, 10] on the x axis: straight from A (0,0) at 1
    // to B (10,0) at 3, then the bulge-1 arc (a half turn, counter-
    // clockwise) from B back to A through (5,5). On the arc a point at angle
    // t from B (about the centre (5,0)) is t / pi of the way to A, so at
    // 3 + (1 - 3) t / pi.
    geometry::CurvePolyline2 curve;
    curve.vertices = {{Point2(0, 0), 0.0, 1.0}, {Point2(10, 0), 1.0, 3.0}};
    curve.closed = true;
    const ShapeOf of = shapeOf(entityOf(curve), ShapeKind::Polygon);
    ASSERT_TRUE(of.shape.has_value());
    const DrawingShape& shape = *of.shape;
    EXPECT_TRUE(shape.chorded);
    EXPECT_TRUE(shape.outline.closed);
    ASSERT_GT(shape.outline.vertices.size(), 3u);
    ASSERT_EQ(shape.heights.size(), shape.outline.vertices.size());
    EXPECT_EQ(shape.outline.vertices[0], Point2(0, 0));
    EXPECT_EQ(shape.heights[0], std::optional<double>(1.0));
    EXPECT_EQ(shape.outline.vertices[1], Point2(10, 0));
    EXPECT_EQ(shape.heights[1], std::optional<double>(3.0));
    EXPECT_FALSE(shape.outline.vertices.back() == Point2(0, 0)); // A is not repeated
    const Point2 centre(5, 0);
    for (std::size_t i = 2; i < shape.outline.vertices.size(); ++i) {
        const Point2& p = shape.outline.vertices[i];
        EXPECT_NEAR(p.distanceTo(centre), 5.0, 1e-9) << i;
        EXPECT_GT(p.y, 0.0) << i;
        const double turned = std::atan2(p.y, p.x - 5.0);
        ASSERT_TRUE(shape.heights[i].has_value()) << i;
        EXPECT_NEAR(*shape.heights[i], 3.0 - 2.0 * turned / kPi, 1e-9) << i;
        // The chord into this point: its middle no more than 1 mm inside.
        const Point2& q = shape.outline.vertices[i - 1];
        const Point2 middle((p.x + q.x) / 2, (p.y + q.y) / 2);
        EXPECT_LE(5.0 - middle.distanceTo(centre), kSagitta + 1e-12) << i;
    }
    // Inscribed chords lose area, at most a chord's length times its
    // sagitta each: the half-disc 25 pi / 2 less at most 5 pi x 1 mm.
    const double halfDisc = 25.0 * kPi / 2.0;
    EXPECT_LE(area(shape.outline), halfDisc);
    EXPECT_GE(area(shape.outline), halfDisc - 5.0 * kPi * kSagitta);
}

TEST(DrawingShapes, ACurvePolylineWithoutArcsIsItsVerticesExactlyAndNotChorded)
{
    const auto curve = geometry::CurvePolyline2::fromPolyline(polyline(square(), true),
                                                              {5.0, std::nullopt, 7.0, 8.0});
    const ShapeOf of = shapeOf(entityOf(curve), ShapeKind::Polygon);
    ASSERT_TRUE(of.shape.has_value());
    EXPECT_EQ(of.shape->outline.vertices, square());
    EXPECT_EQ(of.shape->heights, (Heights{5.0, std::nullopt, 7.0, 8.0}));
    EXPECT_FALSE(of.shape->chorded);
    // Open and back at its start: the same square.
    std::vector<Point2> drawn = square();
    drawn.push_back(Point2(0, 0));
    const ShapeOf back =
        shapeOf(entityOf(geometry::CurvePolyline2::fromPoints(drawn, false)), ShapeKind::Polygon);
    ASSERT_TRUE(back.shape.has_value());
    EXPECT_EQ(back.shape->outline.vertices, square());
    EXPECT_TRUE(back.shape->outline.closed);
}

TEST(DrawingShapes, ACircleAndAFullEllipseAreChordedAtTheEntitysOneElevation)
{
    // A circle of radius 3 at 12.5: every corner on it, every chord's middle
    // within 1 mm of it, every corner at 12.5.
    const Point2 centre(5, 5);
    Entity circle = entityOf(geometry::Circle2{centre, 3.0});
    circle.properties.insert_or_assign(std::string(katana::entity::kElevationProperty), 12.5);
    const ShapeOf ring = shapeOf(circle, ShapeKind::Polygon);
    ASSERT_TRUE(ring.shape.has_value());
    const auto& corners = ring.shape->outline.vertices;
    ASSERT_GE(corners.size(), 3u);
    EXPECT_TRUE(ring.shape->chorded);
    EXPECT_TRUE(ring.shape->outline.closed);
    EXPECT_GT(corners.front().distanceTo(corners.back()), 1e-3); // no repeat
    for (std::size_t i = 0; i < corners.size(); ++i) {
        const Point2& p = corners[i];
        const Point2& q = corners[(i + 1) % corners.size()];
        EXPECT_NEAR(p.distanceTo(centre), 3.0, 1e-9) << i;
        const Point2 middle((p.x + q.x) / 2, (p.y + q.y) / 2);
        EXPECT_LE(3.0 - middle.distanceTo(centre), kSagitta + 1e-12) << i;
    }
    EXPECT_EQ(ring.shape->heights, Heights(corners.size(), 12.5));

    // The full ellipse of semi-axes 20 and 10 about the origin, no height:
    // its corners on (x/20)^2 + (y/10)^2 = 1, and its area pi 20 10 less at
    // most its perimeter (Ramanujan: pi (90 - sqrt 3500) = 96.9) times the
    // 1 mm a chord may stand off it - 0.097; 0.15 allows a chord's farthest
    // point to be off its middle.
    geometry::Ellipse2 whole;
    whole.majorAxis = {20.0, 0.0};
    whole.ratio = 0.5;
    const ShapeOf ellipse = shapeOf(entityOf(whole), ShapeKind::Polygon);
    ASSERT_TRUE(ellipse.shape.has_value());
    EXPECT_TRUE(ellipse.shape->chorded);
    for (const Point2& p : ellipse.shape->outline.vertices) {
        EXPECT_NEAR((p.x / 20) * (p.x / 20) + (p.y / 10) * (p.y / 10), 1.0, 1e-9);
    }
    const auto& first = ellipse.shape->outline.vertices.front();
    EXPECT_GT(first.distanceTo(ellipse.shape->outline.vertices.back()), 1e-3);
    EXPECT_LE(std::abs(area(ellipse.shape->outline)), 200.0 * kPi);
    EXPECT_GE(std::abs(area(ellipse.shape->outline)), 200.0 * kPi - 0.15);
    EXPECT_EQ(ellipse.shape->heights, Heights(ellipse.shape->outline.vertices.size()));
}

TEST(DrawingShapes, FewerThanThreeCornersIsTooFewAndWhatIsNoOutlineIsSaidSo)
{
    EXPECT_EQ(shapeOf(entityOf(polyline({Point2(0, 0), Point2(5, 0)}, true)), ShapeKind::Polygon)
                  .why,
              NotAShape::TooFew);
    // Drawn back to its start on three vertices: two corners.
    EXPECT_EQ(shapeOf(entityOf(polyline({Point2(0, 0), Point2(5, 0), Point2(0, 0)}, false)),
                      ShapeKind::Polygon)
                  .why,
              NotAShape::TooFew);
    const katana::entity::PointGeometry point{Point2(1, 1)};
    EXPECT_EQ(shapeOf(entityOf(point), ShapeKind::Polygon).why, NotAShape::NoOutline);
    katana::entity::TextGeometry text;
    text.text = "lot 4";
    EXPECT_EQ(shapeOf(entityOf(text), ShapeKind::Polygon).why, NotAShape::NoOutline);
    auto spline = geometry::Spline2::fromControlPoints(square(), 3);
    ASSERT_TRUE(spline.ok());
    EXPECT_EQ(shapeOf(entityOf(*spline), ShapeKind::Polygon).why, NotAShape::NoOutline);
    EXPECT_EQ(shapeOf(entityOf(*spline), ShapeKind::Line).why, NotAShape::NoOutline);
}

// ---- what an entity gives as a line -----------------------------------------------------------

TEST(DrawingShapes, AsALineEachKindGivesItsVerticesInOrderAndAClosedOneWithoutTheRepeat)
{
    const ShapeOf segment = shapeOf(
        entityOf(geometry::Segment2{Point2(0, 0), Point2(3, 4)}, {1.0, 2.0}), ShapeKind::Line);
    ASSERT_TRUE(segment.shape.has_value());
    EXPECT_EQ(segment.shape->outline.vertices, (std::vector<Point2>{Point2(0, 0), Point2(3, 4)}));
    EXPECT_FALSE(segment.shape->outline.closed);
    EXPECT_EQ(segment.shape->heights, (Heights{1.0, 2.0}));

    // An open polyline back at its start stays as drawn: a line may end where
    // it began.
    std::vector<Point2> drawn = square();
    drawn.push_back(Point2(0, 0));
    const ShapeOf open = shapeOf(entityOf(polyline(drawn, false)), ShapeKind::Line);
    ASSERT_TRUE(open.shape.has_value());
    EXPECT_EQ(open.shape->outline.vertices, drawn);
    EXPECT_FALSE(open.shape->outline.closed);
    // Closed and repeating its first vertex: closed, the square's four.
    const ShapeOf closed = shapeOf(entityOf(polyline(drawn, true)), ShapeKind::Line);
    ASSERT_TRUE(closed.shape.has_value());
    EXPECT_EQ(closed.shape->outline.vertices, square());
    EXPECT_TRUE(closed.shape->outline.closed);

    // A curve polyline with an arc: chorded, from its first vertex to its
    // last.
    geometry::CurvePolyline2 curve;
    curve.vertices = {{Point2(0, 0), 1.0, 1.0}, {Point2(10, 0), 0.0, 3.0}};
    const ShapeOf arc = shapeOf(entityOf(curve), ShapeKind::Line);
    ASSERT_TRUE(arc.shape.has_value());
    EXPECT_TRUE(arc.shape->chorded);
    EXPECT_GT(arc.shape->outline.vertices.size(), 2u);
    EXPECT_EQ(arc.shape->outline.vertices.front(), Point2(0, 0));
    EXPECT_EQ(arc.shape->outline.vertices.back(), Point2(10, 0));
    EXPECT_EQ(arc.shape->heights.back(), std::optional<double>(3.0));

    EXPECT_EQ(shapeOf(entityOf(geometry::Circle2{Point2(0, 0), 2.0}), ShapeKind::Line).why,
              NotAShape::NoOutline);
    EXPECT_EQ(shapeOf(entityOf(geometry::Arc2{Point2(0, 0), 5.0, 0.0, kPi}), ShapeKind::Line).why,
              NotAShape::NoOutline);
    EXPECT_EQ(shapeOf(entityOf(polyline({Point2(1, 1)}, false)), ShapeKind::Line).why,
              NotAShape::TooFew);
}

// ---- typed corners -----------------------------------------------------------------------------

TEST(DrawingShapes, ATypedCornerIsTwoOrThreeFiniteNumbers)
{
    const auto flat = parseTypedCorner("1,2");
    ASSERT_TRUE(flat.has_value());
    EXPECT_EQ(flat->at, Point2(1, 2));
    EXPECT_FALSE(flat->z.has_value());
    const auto levelled = parseTypedCorner("1,2,-3.5");
    ASSERT_TRUE(levelled.has_value());
    EXPECT_EQ(levelled->z, std::optional<double>(-3.5));
    // A quoted word may carry blanks round its numbers.
    const auto spaced = parseTypedCorner("1.5e1, -2");
    ASSERT_TRUE(spaced.has_value());
    EXPECT_EQ(spaced->at, Point2(15, -2));
    for (const char* word : {"", "1", "1,2,3,4", "a,b", "1,inf", "1,nan", "1,2,", ",1,2", "1;2"}) {
        EXPECT_FALSE(parseTypedCorner(word).has_value()) << word;
    }
}

TEST(DrawingShapes, TypedCornersMakeAPolygonWithAndWithoutHeights)
{
    const std::vector<TypedCorner> flat = {{Point2(0, 0), std::nullopt},
                                           {Point2(4, 0), std::nullopt},
                                           {Point2(4, 3), std::nullopt}};
    const ShapeOf triangle = shapeOf(flat, ShapeKind::Polygon);
    ASSERT_TRUE(triangle.shape.has_value());
    EXPECT_FALSE(triangle.shape->entity.has_value());
    EXPECT_TRUE(triangle.shape->outline.closed);
    EXPECT_EQ(triangle.shape->outline.vertices.size(), 3u);
    EXPECT_EQ(triangle.shape->heights, Heights(3));
    EXPECT_DOUBLE_EQ(area(triangle.shape->outline), 6.0); // 4 x 3 / 2

    const std::vector<TypedCorner> levelled = {
        {Point2(0, 0), 1.0}, {Point2(4, 0), std::nullopt}, {Point2(4, 3), 3.0}};
    const ShapeOf withHeights = shapeOf(levelled, ShapeKind::Polygon);
    ASSERT_TRUE(withHeights.shape.has_value());
    EXPECT_EQ(withHeights.shape->heights, (Heights{1.0, std::nullopt, 3.0}));

    // Typed back to the first corner: the same three.
    std::vector<TypedCorner> closedByHand = flat;
    closedByHand.push_back({Point2(0, 0), std::nullopt});
    const ShapeOf back = shapeOf(closedByHand, ShapeKind::Polygon);
    ASSERT_TRUE(back.shape.has_value());
    EXPECT_EQ(back.shape->outline.vertices, triangle.shape->outline.vertices);

    // Two corners, and three of which the last is the first, are no polygon.
    using Corners = std::vector<TypedCorner>;
    EXPECT_EQ(shapeOf(Corners{flat[0], flat[1]}, ShapeKind::Polygon).why, NotAShape::TooFew);
    EXPECT_EQ(shapeOf(Corners{flat[0], flat[1], flat[0]}, ShapeKind::Polygon).why,
              NotAShape::TooFew);
    EXPECT_EQ(shapeOf(Corners{}, ShapeKind::Polygon).why, NotAShape::TooFew);

    // As a line: two vertices are one, open; one is none.
    const ShapeOf line = shapeOf(Corners{flat[0], flat[1]}, ShapeKind::Line);
    ASSERT_TRUE(line.shape.has_value());
    EXPECT_FALSE(line.shape->outline.closed);
    EXPECT_EQ(shapeOf(Corners{flat[0]}, ShapeKind::Line).why, NotAShape::TooFew);
}

// ---- the words ---------------------------------------------------------------------------------

TEST(DrawingShapes, TheWordsAreIdsCornersOrAScopeAndTheReadingStopsWhereTheyEnd)
{
    auto words = wordsOf("#3 #12 EACH");
    std::size_t at = 0;
    auto read = readShapeWords(words, at);
    ASSERT_TRUE(read.ok());
    EXPECT_EQ(read->ids, (std::vector<EntityId>{3, 12}));
    EXPECT_EQ(at, 2u);

    words = wordsOf("1,2 3,4,5 bulk=2");
    at = 0;
    read = readShapeWords(words, at);
    ASSERT_TRUE(read.ok());
    ASSERT_EQ(read->corners.size(), 2u);
    EXPECT_EQ(read->corners[1].z, std::optional<double>(5.0));
    EXPECT_EQ(at, 2u);

    words = wordsOf("FROM LAYERS a,b ONLY WHERE TYPE=polyline EACH");
    at = 0;
    read = readShapeWords(words, at);
    ASSERT_TRUE(read.ok());
    ASSERT_TRUE(read->scope.has_value());
    EXPECT_EQ(read->scope->source, ScopeSource::Layers);
    EXPECT_EQ(read->scope->layers, (std::vector<std::string>{"a", "b"}));
    EXPECT_FALSE(read->scope->sublayers);
    EXPECT_FALSE(read->scope->filter.types.empty());
    EXPECT_EQ(words[at], "EACH");

    // FROM with no scope word is the selection, as the scope grammar has it.
    for (const char* line : {"FROM", "FROM EACH"}) {
        words = wordsOf(line);
        at = 0;
        read = readShapeWords(words, at);
        ASSERT_TRUE(read.ok()) << line;
        ASSERT_TRUE(read->scope.has_value()) << line;
        EXPECT_EQ(read->scope->source, ScopeSource::Selection) << line;
        EXPECT_EQ(at, 1u) << line;
    }

    // No word, or a word that is none of the three: nothing read.
    words = wordsOf("EACH");
    at = 1;
    EXPECT_TRUE(readShapeWords(words, at)->empty());
    at = 0;
    EXPECT_TRUE(readShapeWords(words, at)->empty());
    EXPECT_EQ(at, 0u);
}

TEST(DrawingShapes, ABadIdIsGivenBackForTheVerbToRefuseAndAScopesRefusalIsTheParsers)
{
    for (const char* line : {"#5 #x", "#5 #0", "#5 #-3", "#5 #"}) {
        const auto words = wordsOf(line);
        std::size_t at = 0;
        const auto read = readShapeWords(words, at);
        ASSERT_TRUE(read.ok()) << line;
        EXPECT_EQ(read->ids, (std::vector<EntityId>{5})) << line;
        EXPECT_EQ(read->badId, std::optional<std::string>(words[1])) << line;
        EXPECT_EQ(at, 1u) << line;
    }
    const auto twoScopes = wordsOf("FROM DRAWING SELECTION");
    std::size_t at = 0;
    const auto refused = readShapeWords(twoScopes, at);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument); // scope_verbs: one scope
    const auto badArea = wordsOf("FROM AREA 1,2,3");
    at = 0;
    const auto area3 = readShapeWords(badArea, at);
    ASSERT_FALSE(area3.ok());
    EXPECT_EQ(area3.error().code, ErrorCode::ParseFailure);
}

// ---- what the words name in a drawing -----------------------------------------------------------

namespace {

struct DrawingShapesInADrawing : ::testing::Test {
    Document document;

    EntityId add(katana::entity::Geometry shape, Heights heights = {})
    {
        Entity entity = entityOf(std::move(shape), std::move(heights));
        EXPECT_TRUE(document.execute(cmd::createEntities({std::move(entity)})).ok());
        const auto created = document.lastCreatedEntities();
        return created.empty() ? 0 : created.front();
    }

    katana::core::Result<DrawingShapes> resolved(const std::string& line, ShapeKind kind,
                                                 const ScopeViewProvider& views = {})
    {
        const auto words = wordsOf(line);
        std::size_t at = 0;
        auto read = readShapeWords(words, at);
        if (!read) {
            return read.error();
        }
        EXPECT_EQ(at, words.size()) << line;
        return resolveShapes(document, *read, kind, views);
    }

    static std::string ids(std::initializer_list<EntityId> list)
    {
        std::string text;
        for (const EntityId id : list) {
            text += (text.empty() ? "#" : " #") + std::to_string(id);
        }
        return text;
    }
};

} // namespace

TEST_F(DrawingShapesInADrawing, IdsAreReadInTheOrderTypedAndTheFirstThatIsNoShapeRefusesTheLine)
{
    const EntityId lot = add(polyline(square(), true));
    const EntityId pond = add(geometry::Circle2{Point2(30, 30), 4.0});
    const EntityId kerb = add(geometry::Segment2{Point2(0, 0), Point2(5, 5)});

    const auto both = resolved(ids({pond, lot}), ShapeKind::Polygon);
    ASSERT_TRUE(both.ok());
    ASSERT_EQ(both->shapes.size(), 2u);
    EXPECT_EQ(both->shapes[0].entity, std::optional<EntityId>(pond));
    EXPECT_EQ(both->shapes[1].entity, std::optional<EntityId>(lot));
    EXPECT_FALSE(both->scope.has_value());
    EXPECT_FALSE(both->refused.has_value());

    const auto open = resolved(ids({lot, kerb}), ShapeKind::Polygon);
    ASSERT_TRUE(open.ok());
    ASSERT_TRUE(open->refused.has_value());
    EXPECT_EQ(open->refused->entity, std::optional<EntityId>(kerb));
    EXPECT_EQ(open->refused->why, NotAShape::Open);

    // The same line is a line.
    const auto line = resolved(ids({kerb}), ShapeKind::Line);
    ASSERT_TRUE(line.ok());
    EXPECT_FALSE(line->refused.has_value());
    EXPECT_EQ(line->shapes.size(), 1u);
}

TEST_F(DrawingShapesInADrawing, AnIdOfNoEntityIsRefusedBeforeAnyIdIsReadAsAShape)
{
    const EntityId kerb = add(geometry::Segment2{Point2(0, 0), Point2(5, 5)});
    const auto refused = resolved(ids({kerb, 999}), ShapeKind::Polygon);
    ASSERT_TRUE(refused.ok());
    ASSERT_TRUE(refused->refused.has_value());
    EXPECT_EQ(refused->refused->entity, std::optional<EntityId>(999));
    EXPECT_EQ(refused->refused->why, NotAShape::NoEntity);
    EXPECT_TRUE(refused->shapes.empty());
}

TEST_F(DrawingShapesInADrawing, TypedCornersAreOneShapeOrRefusedAsTooFew)
{
    const auto triangle = resolved("0,0,1 4,0,2 4,3,3", ShapeKind::Polygon);
    ASSERT_TRUE(triangle.ok());
    ASSERT_EQ(triangle->shapes.size(), 1u);
    EXPECT_EQ(triangle->shapes[0].heights, (Heights{1.0, 2.0, 3.0}));
    EXPECT_FALSE(triangle->shapes[0].entity.has_value());

    const auto two = resolved("0,0 4,0", ShapeKind::Polygon);
    ASSERT_TRUE(two.ok());
    ASSERT_TRUE(two->refused.has_value());
    EXPECT_FALSE(two->refused->entity.has_value());
    EXPECT_EQ(two->refused->why, NotAShape::TooFew);
    EXPECT_TRUE(two->shapes.empty());
}

TEST_F(DrawingShapesInADrawing, AScopeTakesTheShapesAndCountsWhatIsNoneByWhy)
{
    const EntityId lot = add(polyline(square(), true));                              // polygon
    const EntityId pond = add(geometry::Circle2{Point2(30, 30), 4.0});                // polygon
    geometry::Ellipse2 whole;
    whole.center = Point2(60, 0);
    whole.majorAxis = {20.0, 0.0};
    whole.ratio = 0.5;
    const EntityId basin = add(whole);                                                // polygon
    add(polyline({Point2(0, 20), Point2(10, 20), Point2(10, 30)}, false));            // open
    add(geometry::Segment2{Point2(0, 40), Point2(5, 45)});                            // open
    add(geometry::Arc2{Point2(0, 50), 5.0, 0.0, kPi});                                // open
    add(polyline({Point2(0, 60), Point2(5, 60)}, true));                              // too few
    add(katana::entity::PointGeometry{Point2(1, 1)});                                 // no outline

    const auto polygons = resolved("FROM DRAWING", ShapeKind::Polygon);
    ASSERT_TRUE(polygons.ok());
    ASSERT_TRUE(polygons->scope.has_value());
    EXPECT_EQ(polygons->scope->matched.size(), 8u);
    ASSERT_EQ(polygons->shapes.size(), 3u);
    EXPECT_EQ(polygons->shapes[0].entity, std::optional<EntityId>(lot)); // ascending ids
    EXPECT_EQ(polygons->shapes[1].entity, std::optional<EntityId>(pond));
    EXPECT_EQ(polygons->shapes[2].entity, std::optional<EntityId>(basin));
    EXPECT_EQ(polygons->skipped.open, 3u);
    EXPECT_EQ(polygons->skipped.tooFew, 1u);
    EXPECT_EQ(polygons->skipped.noOutline, 1u);
    EXPECT_EQ(polygons->skipped.total(), 5u);
    EXPECT_FALSE(polygons->refused.has_value()); // a scope refuses nothing

    // As lines: the three polylines and the line; the circle, the ellipse,
    // the arc and the point are none.
    const auto lines = resolved("FROM DRAWING", ShapeKind::Line);
    ASSERT_TRUE(lines.ok());
    EXPECT_EQ(lines->shapes.size(), 4u);
    EXPECT_EQ(lines->skipped.noOutline, 4u);
    EXPECT_EQ(lines->skipped.open + lines->skipped.tooFew, 0u);
}

TEST_F(DrawingShapesInADrawing, AScopeThatTakesNothingIsReportedAndNotRefused)
{
    add(polyline(square(), true));
    const auto nothing = resolved("FROM AREA 1000,1000,1010,1010", ShapeKind::Polygon);
    ASSERT_TRUE(nothing.ok());
    ASSERT_TRUE(nothing->scope.has_value());
    EXPECT_TRUE(nothing->scope->matched.empty());
    EXPECT_TRUE(nothing->shapes.empty());
    EXPECT_EQ(nothing->skipped.total(), 0u);
    EXPECT_FALSE(nothing->refused.has_value());
}

TEST_F(DrawingShapesInADrawing, FromWithNoScopeWordIsTheSelection)
{
    add(polyline(square(), true));
    const EntityId pond = add(geometry::Circle2{Point2(30, 30), 4.0});
    document.selection().set({pond});
    const auto selected = resolved("FROM", ShapeKind::Polygon);
    ASSERT_TRUE(selected.ok());
    ASSERT_EQ(selected->shapes.size(), 1u);
    EXPECT_EQ(selected->shapes[0].entity, std::optional<EntityId>(pond));
    EXPECT_EQ(selected->scope->resolved.words.source, ScopeSource::Selection);
}

TEST_F(DrawingShapesInADrawing, AViewScopeWithNoWindowIsTheMatchersRefusal)
{
    add(polyline(square(), true));
    const auto refused = resolved("FROM VIEW", ShapeKind::Polygon);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidState); // scope_verbs: VIEW headless
}

TEST_F(DrawingShapesInADrawing, TheEntitiesNamedAreWhatAVerbReadingPointsTakes)
{
    const EntityId lot = add(polyline(square(), true));
    const EntityId mark = add(katana::entity::PointGeometry{Point2(1, 1)});
    auto words = wordsOf("FROM DRAWING");
    std::size_t at = 0;
    const auto scope = readShapeWords(words, at);
    ASSERT_TRUE(scope.ok());
    const auto all = entitiesNamed(document, *scope, {});
    ASSERT_TRUE(all.ok());
    EXPECT_EQ(all->ids, (std::vector<EntityId>{lot, mark}));
    EXPECT_TRUE(all->scope.has_value());
    EXPECT_FALSE(all->missing.has_value());

    words = wordsOf(ids({mark, 999, lot}));
    at = 0;
    const auto typed = readShapeWords(words, at);
    ASSERT_TRUE(typed.ok());
    const auto named = entitiesNamed(document, *typed, {});
    ASSERT_TRUE(named.ok());
    EXPECT_EQ(named->missing, std::optional<EntityId>(999));
    EXPECT_EQ(named->ids, (std::vector<EntityId>{mark}));
    EXPECT_FALSE(named->scope.has_value());
}
