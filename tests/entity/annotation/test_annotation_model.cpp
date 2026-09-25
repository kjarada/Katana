// The annotation additions to the entity layer (docs/annotation.md): the
// three tables, the text block arithmetic, the Label and Leader kinds, the
// annotation members of Text and Dimension, anchors and label values.
//
// Expected values are worked out by hand in the comments beside them.

#include <gtest/gtest.h>

#include <cmath>
#include <string>

#include "katana/entity/anchor.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/geometry_blob.hpp"
#include "katana/entity/label_values.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/serialization.hpp"
#include "katana/entity/text_block.hpp"
#include "katana/math/mat3.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::entity;
using katana::core::ErrorCode;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::kDegToRad;
using katana::math::kPi;

namespace {

Entity entityOf(EntityId id, Geometry geometry, std::string layer = "0")
{
    Entity entity;
    entity.id = id;
    entity.geometry = std::move(geometry);
    entity.layer = std::move(layer);
    return entity;
}

} // namespace

// ---- the one conversion and the tables -----------------------------------------------

TEST(AnnotationScale, PaperMillimetresBecomeModelUnitsAtTheScale)
{
    // 2.5 mm at 1:200 is 0.5 m on the ground; at 1:1000, 2.5 m; at 1:2000, 5 m.
    EXPECT_DOUBLE_EQ(annotationModelSize(2.5, 200.0), 0.5);
    EXPECT_DOUBLE_EQ(annotationModelSize(2.5, 1000.0), 2.5);
    EXPECT_DOUBLE_EQ(annotationModelSize(2.5, 2000.0), 5.0);
    EXPECT_DOUBLE_EQ(kDefaultAnnotationScale, 1000.0)
        << "the scale at which a 2.5 mm text is TextGeometry's default 2.5 units";
}

TEST(TextStyles, StandardIsBuiltInAndTheRulesAreEachSaid)
{
    TextStyleDatabase styles;
    ASSERT_TRUE(styles.contains(kDefaultTextStyleName));
    EXPECT_FALSE(styles.remove(kDefaultTextStyleName).ok());

    TextStyle road;
    road.name = "Road";
    road.paperHeight = 3.5;
    road.widthFactor = 0.8;
    road.oblique = 15.0 * kDegToRad;
    EXPECT_TRUE(styles.add(road).ok());

    TextStyle bad = road;
    bad.name = "flat";
    bad.widthFactor = 0.0;
    EXPECT_FALSE(validate(bad).ok()) << "a width factor of 0 draws nothing";
    bad.widthFactor = 1.0;
    bad.oblique = 85.0 * kDegToRad;
    EXPECT_FALSE(validate(bad).ok()) << "85 degrees of slant is a line";
    bad.oblique = 0.0;
    bad.lineSpacing = 0.1;
    EXPECT_FALSE(validate(bad).ok()) << "below MTEXT's 0.25";
    bad.lineSpacing = 1.0;
    bad.paperHeight = -1.0;
    EXPECT_FALSE(validate(bad).ok());
}

TEST(LabelStyles, TheTemplateIsCheckedOnTheWayIn)
{
    LabelStyle style;
    style.name = "Bearing";
    style.kind = LabelKind::Segment;
    style.text = "{bearing:dms} {distance:.3f}";
    EXPECT_TRUE(validate(style).ok());
    style.text = "{area}";
    const auto refused = validate(style);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().context.find("Bearing"), std::string::npos) << "names the style";
    style.text = "{bearing}";
    style.kind = LabelKind::Chainage;
    EXPECT_FALSE(validate(style).ok()) << "a chainage label has no bearing";
    style.text = "{chainage:ch}";
    style.interval = 0.0;
    EXPECT_FALSE(validate(style).ok()) << "a chainage interval must be positive";
}

TEST(LabelRules, NeedAStyleAndAKnownType)
{
    LabelRule rule;
    rule.name = "boundaries";
    EXPECT_FALSE(validate(rule).ok()) << "no style";
    rule.labelStyle = "Bearing";
    EXPECT_TRUE(validate(rule).ok());
    rule.entityType = "Spline";
    EXPECT_FALSE(validate(rule).ok());
    rule.entityType = "Polyline";
    EXPECT_TRUE(validate(rule).ok());
}

TEST(Globs, MatchStarsAndQuestionMarksWithoutCase)
{
    EXPECT_TRUE(globMatches("", "anything"));
    EXPECT_TRUE(globMatches("EP*", "ep12"));
    EXPECT_TRUE(globMatches("*BDY*", "lot-bdy-3"));
    EXPECT_TRUE(globMatches("T?", "TB"));
    EXPECT_FALSE(globMatches("T?", "TBX"));
    EXPECT_TRUE(globMatches("survey/*", "survey/points"));
    EXPECT_FALSE(globMatches("survey/*", "design/survey"));
    EXPECT_TRUE(globMatches("a*b*c", "aXXbYYc"));
    EXPECT_FALSE(globMatches("a*b*c", "aXXbYY"));
}

TEST(LabelValues, TheCodeIsTheFirstOfTheNamesGivenThatTheTargetCarries)
{
    Model model;
    Entity point = entityOf(1, PointGeometry{});
    point.metadata["feature"] = std::string("TREE");
    ASSERT_TRUE(model.entities.insert(point).ok());
    LabelStyle style;
    style.name = "code";
    style.kind = LabelKind::Point;
    style.text = "{code}";
    const LabelGeometry label{.target = 1, .style = "code"};
    EXPECT_EQ(formatLabel(style.text, labelPieces(model, label, style)[0].values), "")
        << "no names given: no code";
    const std::vector<std::string> names = {"code", "feature"};
    EXPECT_EQ(formatLabel(style.text, labelPieces(model, label, style, names)[0].values), "TREE")
        << "found in the metadata";
    point.properties["code"] = std::string("EP");
    ASSERT_TRUE(model.entities.replace(point).ok());
    EXPECT_EQ(formatLabel(style.text, labelPieces(model, label, style, names)[0].values), "EP")
        << "the first name wins";
}

// ---- text blocks -----------------------------------------------------------------------

TEST(TextBlock, TheNinePointsPlaceTheBlock)
{
    // Two lines at height 2, pitch 5/3 x 2 = 10/3: the block is 2 + 10/3 =
    // 16/3 tall, from the top of the first line to the last baseline. 10 wide.
    const double h = 2.0;
    const double block = h + kLinePitch * h;
    const auto bl = textBlockExtent(10.0, 2, h, 1.0, TextJustify::BottomLeft);
    EXPECT_DOUBLE_EQ(bl.left, 0.0);
    EXPECT_DOUBLE_EQ(bl.bottom, 0.0);
    EXPECT_DOUBLE_EQ(bl.top, block);
    const auto mc = textBlockExtent(10.0, 2, h, 1.0, TextJustify::MiddleCentre);
    EXPECT_DOUBLE_EQ(mc.left, -5.0);
    EXPECT_DOUBLE_EQ(mc.right, 5.0);
    EXPECT_DOUBLE_EQ(mc.bottom, -0.5 * block);
    const auto tr = textBlockExtent(10.0, 2, h, 1.0, TextJustify::TopRight);
    EXPECT_DOUBLE_EQ(tr.right, 0.0);
    EXPECT_DOUBLE_EQ(tr.top, 0.0);
    // The first line's baseline is a height below the top; a narrower second
    // line is centred in a centred block.
    const Point2 first = lineOrigin(mc, 0, 10.0, h, 1.0, TextJustify::MiddleCentre);
    EXPECT_DOUBLE_EQ(first.y, mc.top - h);
    const Point2 second = lineOrigin(mc, 1, 4.0, h, 1.0, TextJustify::MiddleCentre);
    EXPECT_DOUBLE_EQ(second.x, -2.0);
    EXPECT_DOUBLE_EQ(second.y, mc.bottom);
}

TEST(TextBlock, AOneLineBaselineLeftTextHasTheBoxItAlwaysHad)
{
    // The box before justification existed: the baseline from the position,
    // 0.6 h per character long, and h up. "ABCD" at 2.5: 6 x 2.5.
    TextGeometry text{Point2(10, 20), "ABCD", 2.5, 0.0};
    const auto box = boundingBox(text);
    EXPECT_DOUBLE_EQ(box.min.x, 10.0);
    EXPECT_DOUBLE_EQ(box.min.y, 20.0);
    EXPECT_DOUBLE_EQ(box.max.x, 16.0);
    EXPECT_DOUBLE_EQ(box.max.y, 22.5);
    EXPECT_DOUBLE_EQ(distanceTo(text, Point2(13, 20)), 0.0) << "picked by its baseline";
}

TEST(TextBlock, CharactersAreCountedNotBytes)
{
    EXPECT_EQ(characterCount("Café"), 4u);
    EXPECT_EQ(characterCount("36\u00B052'"), 6u) << "3, 6, the degree sign, 5, 2 and the minute mark";
    ASSERT_EQ(textLines("a\nbc\n").size(), 3u) << "a trailing break is a trailing empty line";
    EXPECT_EQ(textLines("").size(), 1u);
}

// ---- the new kinds and members ---------------------------------------------------------

TEST(AnnotationGeometry, DimensionKindsMeasureWhatTheySay)
{
    DimensionGeometry linear;
    linear.kind = DimensionKind::Linear;
    linear.start = Point2(0, 0);
    linear.end = Point2(3, 4);
    EXPECT_DOUBLE_EQ(linear.measurement(), 3.0) << "horizontal: the x difference";
    linear.angle = 0.5 * kPi;
    EXPECT_NEAR(linear.measurement(), 4.0, 1e-12) << "vertical: the y difference";

    DimensionGeometry angular;
    angular.kind = DimensionKind::Angular;
    angular.vertex = Point2(0, 0);
    angular.start = Point2(1, 0);
    angular.end = Point2(0, 1);
    EXPECT_NEAR(angular.measurement(), 0.5 * kPi, 1e-15);
    std::swap(angular.start, angular.end);
    EXPECT_NEAR(angular.measurement(), 1.5 * kPi, 1e-15) << "counter-clockwise from start";

    DimensionGeometry radius;
    radius.kind = DimensionKind::Radius;
    radius.vertex = Point2(5, 5);
    radius.start = Point2(8, 9);
    EXPECT_DOUBLE_EQ(radius.measurement(), 5.0);
    radius.kind = DimensionKind::Diameter;
    EXPECT_DOUBLE_EQ(radius.measurement(), 10.0);

    DimensionGeometry ordinate;
    ordinate.kind = DimensionKind::OrdinateX;
    ordinate.vertex = Point2(100, 200);
    ordinate.start = Point2(90, 230);
    ordinate.end = Point2(90, 240);
    EXPECT_DOUBLE_EQ(ordinate.measurement(), -10.0) << "west of the datum is negative";
    ordinate.kind = DimensionKind::OrdinateY;
    EXPECT_DOUBLE_EQ(ordinate.measurement(), 30.0);
    EXPECT_TRUE(validate(ordinate).ok());
}

TEST(AnnotationGeometry, LabelsAndLeadersValidate)
{
    LabelGeometry label{.target = 3, .style = "S", .anchor = Point2(1, 2)};
    EXPECT_TRUE(validate(label).ok());
    label.alignment = "road";
    EXPECT_FALSE(validate(label).ok()) << "a target AND an alignment";
    label.alignment.clear();
    label.style.clear();
    EXPECT_FALSE(validate(label).ok()) << "no style";

    LeaderGeometry leader{.vertices = {Point2(0, 0)}};
    EXPECT_FALSE(validate(leader).ok()) << "one vertex";
    leader.vertices.push_back(Point2(0, 0));
    EXPECT_FALSE(validate(leader).ok()) << "no length";
    leader.vertices.back() = Point2(3, 4);
    EXPECT_TRUE(validate(leader).ok());
    leader.arrowSize = -1.0;
    EXPECT_FALSE(validate(leader).ok());
}

TEST(AnnotationGeometry, TransformsMoveThePointsAndKeepThePaperSizes)
{
    const auto move = katana::math::Mat3::translation(katana::geometry::Vec2(10.0, 20.0));
    LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(3, 4)}, .arrowSize = 2.5};
    const auto moved = transformed(leader, katana::math::Mat3::scaling(2.0, 2.0) * move);
    ASSERT_TRUE(moved.ok());
    const auto& out = std::get<LeaderGeometry>(*moved);
    EXPECT_EQ(out.vertices[1], Point2(26, 48));
    EXPECT_DOUBLE_EQ(out.arrowSize, 2.5) << "paper millimetres do not scale with the model";

    // A mirrored angular dimension measures the same angle: its rays swap.
    DimensionGeometry angular;
    angular.kind = DimensionKind::Angular;
    angular.vertex = Point2(0, 0);
    angular.start = Point2(1, 0);
    angular.end = Point2(0, 1);
    angular.startRef = AnchorRef{5, AnchorPoint::End, 0};
    const auto mirrored = transformed(angular, katana::math::Mat3::scaling(-1.0, 1.0));
    ASSERT_TRUE(mirrored.ok());
    const auto& m = std::get<DimensionGeometry>(*mirrored);
    EXPECT_NEAR(m.measurement(), 0.5 * kPi, 1e-12);
    EXPECT_EQ(m.endRef.entity, 5u) << "the reference goes with its point";
}

TEST(AnnotationGeometry, JsonAndBlobRoundTripEveryNewMember)
{
    TextGeometry text{Point2(1, 2), "two\nlines", 1.25, 0.3};
    text.style = "Road";
    text.paperHeight = 3.5;
    text.justify = TextJustify::MiddleCentre;
    DimensionGeometry dimension;
    dimension.kind = DimensionKind::Angular;
    dimension.start = Point2(10, 0);
    dimension.end = Point2(0, 10);
    dimension.offset = 4.0;
    dimension.vertexRef = AnchorRef{9, AnchorPoint::SegmentMid, 2};
    LabelGeometry label{.target = 4, .part = 2, .style = "Bearing", .anchor = Point2(5, 6),
                        .position = Point2(7, 8), .textOverride = "X", .rule = "bdy"};
    LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(3, 4), Point2(6, 4)},
                          .text = "PIT 12\nIL 10.5",
                          .arrow = ArrowHead::Dot,
                          .callout = CalloutShape::Box,
                          .style = "Road",
                          .paperHeight = 3.0,
                          .arrowSize = 2.0,
                          .landing = 0.0,
                          .tipRef = AnchorRef{3, AnchorPoint::Vertex, 7}};
    for (const Geometry& geometry : {Geometry{text}, Geometry{dimension}, Geometry{label},
                                     Geometry{leader}}) {
        const auto json = geometryToJson(geometry);
        ASSERT_TRUE(json.ok());
        const auto back = geometryFromJson(*json);
        ASSERT_TRUE(back.ok()) << *json;
        EXPECT_EQ(*back, geometry) << *json;

        const auto blob = geometryToBlob(geometry);
        ASSERT_TRUE(blob.ok());
        EXPECT_EQ(static_cast<std::uint8_t>((*blob)[0]), kBlobVersionAnnotation);
        const auto fromBlob = geometryFromBlob(*blob);
        ASSERT_TRUE(fromBlob.ok()) << fromBlob.error().describe();
        EXPECT_EQ(*fromBlob, geometry);
    }
}

TEST(AnnotationGeometry, AnOldTextOrDimensionIsStoredExactlyAsBefore)
{
    // Byte for byte the version-1 layout: 2 header bytes, the point (16),
    // height and rotation (16), and the string (4 + 4 bytes).
    const TextGeometry text{Point2(1, 2), "ABCD", 2.5, 0.0};
    const auto blob = geometryToBlob(text);
    ASSERT_TRUE(blob.ok());
    EXPECT_EQ(static_cast<std::uint8_t>((*blob)[0]), kBlobVersion);
    EXPECT_EQ(blob->size(), 2u + 16u + 16u + 4u + 4u);
    const auto json = geometryToJson(text);
    ASSERT_TRUE(json.ok());
    EXPECT_EQ(json->find("justify"), std::string::npos) << "no new member in the old JSON";

    const DimensionGeometry dimension{Point2(0, 0), Point2(10, 0), 2.0, ""};
    const auto dimensionBlob = geometryToBlob(dimension);
    ASSERT_TRUE(dimensionBlob.ok());
    EXPECT_EQ(static_cast<std::uint8_t>((*dimensionBlob)[0]), kBlobVersion);
}

TEST(AnnotationGeometry, AVersionTwoBlobCutShortIsRefused)
{
    LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(3, 4)}, .text = "A"};
    auto blob = geometryToBlob(leader);
    ASSERT_TRUE(blob.ok());
    blob->pop_back();
    EXPECT_FALSE(geometryFromBlob(*blob).ok());
    // A label in a version-1 blob cannot have been written by anything.
    auto label = geometryToBlob(LabelGeometry{.target = 1, .style = "S"});
    ASSERT_TRUE(label.ok());
    (*label)[0] = std::byte{kBlobVersion};
    EXPECT_FALSE(geometryFromBlob(*label).ok());
}

// ---- anchors ----------------------------------------------------------------------------

TEST(Anchors, EachKindOffersItsPoints)
{
    const Entity line = entityOf(1, Segment2{Point2(0, 0), Point2(10, 0)});
    EXPECT_EQ(*resolveAnchor(line, {1, AnchorPoint::End, 0}), Point2(10, 0));
    EXPECT_EQ(*resolveAnchor(line, {1, AnchorPoint::Mid, 0}), Point2(5, 0));
    EXPECT_FALSE(resolveAnchor(line, {1, AnchorPoint::Centre, 0})) << "a line has no centre";

    const Entity lot =
        entityOf(2, Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true});
    EXPECT_EQ(*resolveAnchor(lot, {2, AnchorPoint::Vertex, 2}), Point2(10, 10));
    EXPECT_EQ(*resolveAnchor(lot, {2, AnchorPoint::SegmentMid, 3}), Point2(0, 5))
        << "the closing segment of a closed polyline is the last";
    EXPECT_FALSE(resolveAnchor(lot, {2, AnchorPoint::Vertex, 4}));

    const Entity circle = entityOf(3, Circle2{Point2(4, 4), 2.0});
    EXPECT_EQ(*resolveAnchor(circle, {3, AnchorPoint::Centre, 0}), Point2(4, 4));
    EXPECT_EQ(describe(AnchorRef{2, AnchorPoint::Vertex, 2}), "vertex 2");
}

// ---- label values -----------------------------------------------------------------------

TEST(LabelValues, APointHasItsLevelOnlyWhenItHasOne)
{
    Model model;
    Entity point = entityOf(1, PointGeometry{Point2(300000.5, 6250000.25)});
    point.properties["point"] = std::int64_t{1001};
    ASSERT_TRUE(model.entities.insert(point).ok());
    LabelStyle style;
    style.name = "pt";
    style.kind = LabelKind::Point;
    style.text = "{point}\nRL {z:.3f}\n{easting:.3f}";
    const LabelGeometry label{.target = 1, .style = "pt"};
    auto pieces = labelPieces(model, label, style);
    ASSERT_EQ(pieces.size(), 1u);
    EXPECT_EQ(formatLabel(style.text, pieces[0].values), "1001\n300000.500")
        << "no RL line: the point has no level, and absent is not zero";

    Entity levelled = point;
    levelled.properties["elevation"] = 31.25;
    ASSERT_TRUE(model.entities.replace(levelled).ok());
    pieces = labelPieces(model, label, style);
    EXPECT_EQ(formatLabel(style.text, pieces[0].values), "1001\nRL 31.250\n300000.500");
}

TEST(LabelValues, EachSegmentOfAPolylineIsAPiece)
{
    Model model;
    // A 3-4-5 triangle's two legs and hypotenuse, closed: bearings 90, 0 and
    // 180 + 36 52' 12" = 216 52' 12" from (3,4) back to the origin... in order:
    // (0,0)->(3,0) east 90, (3,0)->(3,4) north 0, (3,4)->(0,0) 216.87 deg.
    Entity lot = entityOf(1, Polyline2{{Point2(0, 0), Point2(3, 0), Point2(3, 4)}, true});
    lot.properties["elevations"] = std::string("10 10 12");
    ASSERT_TRUE(model.entities.insert(lot).ok());
    LabelStyle style;
    style.name = "bd";
    style.kind = LabelKind::Segment;
    style.text = "{bearing:dms} {distance:.3f}";
    const auto pieces = labelPieces(model, LabelGeometry{.target = 1, .style = "bd"}, style);
    ASSERT_EQ(pieces.size(), 3u);
    EXPECT_EQ(formatLabel(style.text, pieces[0].values), "90°00'00\" 3.000");
    EXPECT_EQ(formatLabel(style.text, pieces[1].values), "0°00'00\" 4.000");
    EXPECT_EQ(formatLabel(style.text, pieces[2].values), "216°52'12\" 5.000");
    // The grade of the second leg: 2 m up over 4 m is 50 per cent.
    EXPECT_EQ(formatLabel("{grade:.1f}%", pieces[1].values), "50.0%");
    EXPECT_EQ(pieces[1].anchor, Point2(3, 2)) << "the middle of the segment";

    const auto one =
        labelPieces(model, LabelGeometry{.target = 1, .part = 2, .style = "bd"}, style);
    ASSERT_EQ(one.size(), 1u);
    EXPECT_EQ(formatLabel(style.text, one[0].values), "216°52'12\" 5.000");
}

TEST(LabelValues, AnArcHasItsCurveData)
{
    Model model;
    // Radius 10, a quarter turn: length 15.708, chord 10 sqrt 2 = 14.142,
    // tangent R tan 45 = 10.
    ASSERT_TRUE(model.entities.insert(entityOf(1, Arc2{Point2(0, 0), 10.0, 0.0, 0.5 * kPi})).ok());
    LabelStyle style;
    style.name = "arc";
    style.kind = LabelKind::Arc;
    style.text = "R{radius:.1f} L{length:.3f} C{chord:.3f} T{tangent:.3f} D{delta:dms}";
    const auto pieces = labelPieces(model, LabelGeometry{.target = 1, .style = "arc"}, style);
    ASSERT_EQ(pieces.size(), 1u);
    EXPECT_EQ(formatLabel(style.text, pieces[0].values),
              "R10.0 L15.708 C14.142 T10.000 D90°00'00\"");
}

TEST(LabelValues, AnAreaIsLabelledInsideItEvenWhenItsCentroidIsNot)
{
    Model model;
    // An L: 20 x 20 with the top-right 10 x 10 missing - 300 m2. Its centroid
    // (8.33, 8.33) is inside; a U's would not be. The U below: 30 wide, 20
    // tall, a 10 x 15 notch from the top middle - 600 - 150 = 450 m2, centroid
    // x 15, y (600 x 10 - 150 x 12.5) / 450 = 9.17: in the notch's column but
    // below it... inside. A thin C whose centroid is outside:
    const Polyline2 c{{Point2(0, 0), Point2(30, 0), Point2(30, 2), Point2(2, 2), Point2(2, 28),
                       Point2(30, 28), Point2(30, 30), Point2(0, 30)},
                      true};
    ASSERT_FALSE(c.contains(*c.centroid())) << "the premise: the C's centroid is in its mouth";
    ASSERT_TRUE(model.entities.insert(entityOf(1, c)).ok());
    LabelStyle style;
    style.name = "area";
    style.kind = LabelKind::Area;
    style.text = "{area:m2:.1f}\n{area:ha:.4f} ha";
    const auto pieces = labelPieces(model, LabelGeometry{.target = 1, .style = "area"}, style);
    ASSERT_EQ(pieces.size(), 1u);
    EXPECT_TRUE(c.contains(pieces[0].anchor));
    // 30 x 30 = 900 less the mouth 28 x 26 = 728: 172 m2.
    EXPECT_EQ(formatLabel(style.text, pieces[0].values), "172.0\n0.0172 ha");
}

TEST(LabelValues, ChainagesAreCountedNotAccumulated)
{
    Model model;
    Alignment road;
    road.name = "road";
    road.horizontal.pis = {katana::geometry::AlignmentPI{Point2(0, 0)},
                           katana::geometry::AlignmentPI{Point2(100, 0)}};
    ASSERT_TRUE(model.alignments.add(road).ok());
    LabelStyle style;
    style.name = "ch";
    style.kind = LabelKind::Chainage;
    style.text = "{chainage:ch}";
    style.interval = 20.0;
    style.tickInterval = 10.0;
    const auto pieces = labelPieces(model, LabelGeometry{.alignment = "road", .style = "ch"}, style);
    // 0, 10, ... 100: eleven marks, six of them labelled.
    ASSERT_EQ(pieces.size(), 11u);
    std::size_t labelled = 0;
    for (const auto& piece : pieces) {
        labelled += piece.tickOnly ? 0 : 1;
    }
    EXPECT_EQ(labelled, 6u);
    EXPECT_EQ(formatLabel(style.text, pieces[10].values), "0+100.000");
    EXPECT_EQ(pieces[10].anchor, Point2(100, 0));
}

TEST(LabelValues, AStyleThatCannotLabelTheTargetMakesNothing)
{
    Model model;
    ASSERT_TRUE(model.entities.insert(entityOf(1, PointGeometry{})).ok());
    LabelStyle style;
    style.name = "bd";
    style.kind = LabelKind::Segment;
    style.text = "{distance}";
    EXPECT_TRUE(labelPieces(model, LabelGeometry{.target = 1, .style = "bd"}, style).empty());
    EXPECT_TRUE(labelPieces(model, LabelGeometry{.target = 99, .style = "bd"}, style).empty())
        << "a target that is gone";
}
