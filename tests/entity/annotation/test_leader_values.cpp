// Smart leaders in the entity layer (docs/annotation.md, "Smart leaders"):
// the Along and Inside anchors, the values a leader's note reads off the
// entity its tip is on, the leader's template check, validation, and the
// version-3 geometry blob and JSON that store them.
//
// Expected values are worked out by hand in the comments beside them.

#include <gtest/gtest.h>

#include <bit>
#include <cmath>
#include <limits>
#include <string>

#include "katana/entity/anchor.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/geometry_blob.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/serialization.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::entity;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::kPi;

namespace {

Entity entityOf(EntityId id, Geometry geometry)
{
    Entity entity;
    entity.id = id;
    entity.geometry = std::move(geometry);
    return entity;
}

AnchorRef along(EntityId id, double parameter, std::uint32_t index = 0)
{
    return AnchorRef{id, AnchorPoint::Along, index, parameter};
}

// The value `name` in its own format, or "absent".
std::string said(const LabelValues& values, const std::string& name)
{
    const auto found = values.find(name);
    return found == values.end() ? std::string("absent") : formatValue(found->second);
}

double number(const LabelValues& values, const std::string& name)
{
    return values.at(name).number;
}

// A model holding `entities`, added in order (ids 1, 2, ...).
Model modelWith(std::vector<Entity> entities)
{
    Model model;
    for (Entity& entity : entities) {
        EXPECT_TRUE(model.entities.add(std::move(entity)).ok());
    }
    return model;
}

LeaderGeometry smartLeader(EntityId target, AnchorRef ref, Point2 tip, std::string templateText)
{
    ref.entity = target;
    LeaderGeometry leader;
    leader.vertices = {tip, tip + katana::geometry::Vec2(5, 5)};
    leader.tipRef = ref;
    leader.text = std::move(templateText);
    leader.fields = true;
    return leader;
}

} // namespace

// ---- anchors on and inside entities ------------------------------------------------------

TEST(SmartAnchors, AlongIsAFractionOfWhatItNames)
{
    const Entity line = entityOf(1, Segment2{Point2(0, 0), Point2(40, 0)});
    EXPECT_EQ(*resolveAnchor(line, along(1, 0.25)), Point2(10, 0));
    EXPECT_EQ(*resolveAnchor(line, along(1, 1.5)), Point2(40, 0)) << "clamped to the end";

    // A quarter turn from east, radius 10: halfway is at 45 degrees.
    const Entity arc = entityOf(2, Arc2{Point2(0, 0), 10.0, 0.0, 0.5 * kPi});
    const Point2 half = *resolveAnchor(arc, along(2, 0.5));
    EXPECT_NEAR(half.x, 10.0 * std::cos(0.25 * kPi), 1e-12);
    EXPECT_NEAR(half.y, 10.0 * std::sin(0.25 * kPi), 1e-12);

    // A quarter of a turn counter-clockwise from east is north.
    const Entity circle = entityOf(3, Circle2{Point2(0, 0), 5.0});
    const Point2 north = *resolveAnchor(circle, along(3, 0.25));
    EXPECT_NEAR(north.x, 0.0, 1e-12);
    EXPECT_NEAR(north.y, 5.0, 1e-12);

    // Segment 1 of (0,0)-(30,0)-(30,40), halfway: (30, 20).
    const Entity pipe =
        entityOf(4, Polyline2{{Point2(0, 0), Point2(30, 0), Point2(30, 40)}, false});
    EXPECT_EQ(*resolveAnchor(pipe, along(4, 0.5, 1)), Point2(30, 20));
    EXPECT_FALSE(resolveAnchor(pipe, along(4, 0.5, 2))) << "an open polyline of 3 has 2 segments";
    EXPECT_FALSE(resolveAnchor(entityOf(5, PointGeometry{Point2(1, 1)}), along(5, 0.5)));
}

TEST(SmartAnchors, InsideIsWhereAnAreaLabelGoes)
{
    const Entity lot =
        entityOf(1, Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true});
    EXPECT_EQ(*resolveAnchor(lot, AnchorRef{1, AnchorPoint::Inside}), Point2(5, 5));
    // An L of two 2-wide legs: its centroid, (29/9, 29/9) by the two rectangles'
    // moments, is in the notch, and the point given must still be inside.
    const Polyline2 ell{
        {Point2(0, 0), Point2(10, 0), Point2(10, 2), Point2(2, 2), Point2(2, 10), Point2(0, 10)},
        true};
    ASSERT_FALSE(ell.contains(*ell.centroid())) << "the premise: the centroid is in the notch";
    // The horizontal y = 29/9 crosses the L only at x = 0 and x = 2: one run
    // inside, whose middle is x = 1.
    const Point2 inside = insidePoint(ell);
    EXPECT_TRUE(ell.contains(inside));
    EXPECT_EQ(inside.x, 1.0);
    EXPECT_NEAR(inside.y, 29.0 / 9.0, 1e-12);
    EXPECT_EQ(*resolveAnchor(entityOf(6, ell), AnchorRef{6, AnchorPoint::Inside}), inside);
    // Two vertices closed are no figure; three in a row have no area, and
    // their inside point is still one on them, the same each time.
    EXPECT_FALSE(resolveAnchor(entityOf(7, Polyline2{{Point2(0, 0), Point2(10, 0)}, true}),
                               AnchorRef{7, AnchorPoint::Inside}));
    const Polyline2 flat{{Point2(0, 0), Point2(5, 0), Point2(10, 0)}, true};
    const Point2 onFlat = insidePoint(flat);
    EXPECT_EQ(onFlat.y, 0.0);
    EXPECT_GE(onFlat.x, 0.0);
    EXPECT_LE(onFlat.x, 10.0);
    EXPECT_EQ(insidePoint(flat), onFlat);
    EXPECT_EQ(
        *resolveAnchor(entityOf(2, Circle2{Point2(3, 4), 2.0}), AnchorRef{2, AnchorPoint::Inside}),
        Point2(3, 4));
    const Entity open =
        entityOf(3, Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10)}, false});
    EXPECT_FALSE(resolveAnchor(open, AnchorRef{3, AnchorPoint::Inside}))
        << "an open line has no inside";
}

TEST(SmartAnchors, TheNearestPlaceIsNamedSoItFollows)
{
    const auto line =
        nearestAnchor(entityOf(7, Segment2{Point2(0, 0), Point2(40, 0)}), Point2(10, 3));
    ASSERT_TRUE(line);
    EXPECT_EQ(line->entity, 7u);
    EXPECT_EQ(line->point, AnchorPoint::Along);
    EXPECT_DOUBLE_EQ(line->parameter, 0.25);

    // (1,1) is nearer segment 0 of the pipe, (29,20) nearer segment 1 at 20/40.
    const Entity pipe =
        entityOf(4, Polyline2{{Point2(0, 0), Point2(30, 0), Point2(30, 40)}, false});
    EXPECT_EQ(nearestAnchor(pipe, Point2(1, 1))->index, 0u);
    const auto onSecond = nearestAnchor(pipe, Point2(29, 20));
    EXPECT_EQ(onSecond->index, 1u);
    EXPECT_DOUBLE_EQ(onSecond->parameter, 0.5);
    // Exactly at the shared vertex both segments are as near: the first wins.
    const auto corner = nearestAnchor(pipe, Point2(30, 0));
    EXPECT_EQ(corner->index, 0u);
    EXPECT_DOUBLE_EQ(corner->parameter, 1.0);
    // A repeated vertex has no "along"; the segment after it is used.
    const Entity repeated =
        entityOf(5, Polyline2{{Point2(0, 0), Point2(0, 0), Point2(10, 0)}, false});
    EXPECT_EQ(nearestAnchor(repeated, Point2(0, 0))->index, 1u);

    // West of a circle's centre is half a turn; the turn is [0, 1).
    const auto circle = nearestAnchor(entityOf(3, Circle2{Point2(0, 0), 5.0}), Point2(-9, 0));
    EXPECT_DOUBLE_EQ(circle->parameter, 0.5);
    const auto east = nearestAnchor(entityOf(3, Circle2{Point2(0, 0), 5.0}), Point2(9, -1e-300));
    EXPECT_LT(east->parameter, 1.0);

    // At a circle's centre every direction is as near: atan2(+0, +0) is +0
    // (IEEE 754, C Annex F), the turn's start.
    EXPECT_EQ(nearestAnchor(entityOf(3, Circle2{Point2(2, 3), 5.0}), Point2(2, 3))->parameter, 0.0);

    EXPECT_EQ(nearestAnchor(entityOf(8, PointGeometry{Point2(1, 1)}), Point2(9, 9))->point,
              AnchorPoint::Position);
    // A polyline with no piece of any length, one of one vertex, a label and
    // a leader offer no place.
    EXPECT_FALSE(nearestAnchor(
        entityOf(10, Polyline2{{Point2(1, 1), Point2(1, 1), Point2(1, 1)}, true}), Point2(0, 0)));
    EXPECT_FALSE(nearestAnchor(entityOf(11, Polyline2{{Point2(1, 1)}, false}), Point2(0, 0)));
    EXPECT_FALSE(nearestAnchor(entityOf(12, LabelGeometry{}), Point2(0, 0)));
    EXPECT_FALSE(nearestAnchor(
        entityOf(13, LeaderGeometry{.vertices = {Point2(0, 0), Point2(5, 5)}}), Point2(0, 0)));
    EXPECT_FALSE(nearestAnchor(entityOf(9, DimensionGeometry{Point2(0, 0), Point2(10, 0), 2.0, ""}),
                               Point2(1, 1)));
    EXPECT_EQ(describe(along(1, 0.25, 3)), "along 3 0.25");
    EXPECT_EQ(describe(AnchorRef{1, AnchorPoint::Inside}), "inside");
    EXPECT_EQ(*anchorPointFromString("Along"), AnchorPoint::Along);
    EXPECT_EQ(*anchorPointFromString("inside"), AnchorPoint::Inside);
}

TEST(SmartAnchors, AFractionThatIsNoNumberReadsAsTheStart)
{
    // A stored reference is never refused for its fraction: one that is not
    // a number, or infinite either way, is 0, the start (anchor.hpp).
    const Entity line = entityOf(1, Segment2{Point2(2, 3), Point2(42, 3)});
    for (const double bad : {std::nan(""), std::numeric_limits<double>::infinity(),
                             -std::numeric_limits<double>::infinity()}) {
        EXPECT_EQ(*resolveAnchor(line, along(1, bad)), Point2(2, 3)) << bad;
    }
    EXPECT_EQ(*resolveAnchor(line, along(1, 7.0)), Point2(42, 3)) << "a finite one is clamped";
}

TEST(SmartAnchors, PlacesFarFromTheOriginAreTheSameAsNearIt)
{
    // At a national grid's magnitudes (an easting of 334 km, a northing of
    // 6250 km) every coordinate here is an integer, exact in a double, and
    // so are the answers: a quarter of 40 is 10, the square's middle 5 in.
    const Entity line = entityOf(1, Segment2{Point2(334000, 6250000), Point2(334040, 6250000)});
    EXPECT_EQ(*resolveAnchor(line, along(1, 0.25)), Point2(334010, 6250000));
    EXPECT_EQ(nearestAnchor(line, Point2(334010, 6250003))->parameter, 0.25);
    const Entity lot = entityOf(2, Polyline2{{Point2(334000, 6250000), Point2(334010, 6250000),
                                              Point2(334010, 6250010), Point2(334000, 6250010)},
                                             true});
    EXPECT_EQ(*resolveAnchor(lot, AnchorRef{2, AnchorPoint::Inside}), Point2(334005, 6250005));
    const LabelValues values = anchorValues(line, along(1, 0.25), Point2(334010, 6250000));
    EXPECT_EQ(said(values, "chainage"), "10.000");
}

TEST(LeaderValues, ATipOnARepeatedVertexHasNoBearingOrGradeButKeepsItsChainage)
{
    // (0,0) -> (30,0) -> (30,0) -> (30,40) at RL 100, 103, 103, 111. Vertex 1
    // starts segment 2, which has no length: no direction, so no bearing and
    // no grade (absent is not zero); distance 0, dz 103 - 103 = 0, chainage
    // 30, RL 103; the whole line 30 + 0 + 40 = 70.
    Entity pipe =
        entityOf(4, Polyline2{{Point2(0, 0), Point2(30, 0), Point2(30, 0), Point2(30, 40)}, false});
    setHeights(pipe.properties, {100.0, 103.0, 103.0, 111.0});
    const LabelValues values =
        anchorValues(pipe, AnchorRef{4, AnchorPoint::Vertex, 1}, Point2(30, 0));
    EXPECT_EQ(said(values, "segment"), "2");
    EXPECT_EQ(said(values, "bearing"), "absent");
    EXPECT_EQ(said(values, "grade"), "absent");
    EXPECT_EQ(said(values, "distance"), "0.000");
    EXPECT_EQ(said(values, "dz"), "0.000");
    EXPECT_EQ(said(values, "chainage"), "30.000");
    EXPECT_EQ(said(values, "z"), "103.000");
    EXPECT_EQ(said(values, "length"), "70.000");

    // A fraction that is not a number is its segment's start: segment 3
    // from (30,0), due north, chainage 30 + 0 = 30.
    const LabelValues nan = anchorValues(pipe, along(4, std::nan(""), 2), Point2(30, 0));
    EXPECT_EQ(said(nan, "segment"), "3");
    EXPECT_EQ(said(nan, "chainage"), "30.000");
    EXPECT_EQ(said(nan, "x"), "30.000");
    EXPECT_EQ(said(nan, "bearing"), "0°00'00\"");
}

TEST(SmartAnchors, OnlyAlongHasAFractionAndItIsFromZeroToOne)
{
    LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(5, 5)}};
    leader.tipRef = along(1, 0.5);
    EXPECT_TRUE(validate(leader).ok());
    leader.tipRef = along(1, 1.0000001);
    EXPECT_FALSE(validate(leader).ok());
    leader.tipRef = along(1, std::nan(""));
    EXPECT_FALSE(validate(leader).ok());
    leader.tipRef = AnchorRef{1, AnchorPoint::End, 0, 0.5};
    EXPECT_FALSE(validate(leader).ok()) << "an end has no fraction";
    leader.tipRef = AnchorRef{1, static_cast<AnchorPoint>(9)};
    EXPECT_FALSE(validate(leader).ok());

    DimensionGeometry dimension{Point2(0, 0), Point2(10, 0), 2.0, ""};
    dimension.startRef = along(1, 2.0);
    EXPECT_FALSE(validate(dimension).ok()) << "a dimension's anchors obey the same rule";
}

// ---- what a leader can say --------------------------------------------------------------

TEST(LeaderValues, TheTemplateIsCheckedAgainstTheLeadersValues)
{
    EXPECT_TRUE(checkLeaderTemplate("IL {prop.invert:.3f}\n{length:.2f} m {bearing:dms}").ok());
    EXPECT_TRUE(checkLeaderTemplate("{chainage:ch} {area:ha:.4f} {z:.3f} {type:upper}").ok());
    const auto unknown = checkLeaderTemplate("{invert}");
    ASSERT_FALSE(unknown.ok());
    EXPECT_NE(unknown.error().context.find("a leader has no value 'invert'"), std::string::npos)
        << unknown.error().describe();
    EXPECT_FALSE(checkLeaderTemplate("{layer:.2f}").ok()) << "a number format on text";
    EXPECT_FALSE(checkLeaderTemplate("{bearing:.2f}").ok()) << "an angle needs deg first";
    EXPECT_FALSE(checkLeaderTemplate("{prop.x:dms}").ok()) << "dms applies to no property";
    EXPECT_FALSE(checkLeaderTemplate("{length").ok()) << "braces must balance";
    // Every name listed has a quantity, and nothing else does.
    for (const std::string_view name : leaderValueNames()) {
        EXPECT_TRUE(leaderValueQuantity(name)) << name;
    }
    EXPECT_FALSE(leaderValueQuantity("prop.x"));
    EXPECT_EQ(templateFields("{a} {b:.1f}\n{a}{{c}}"), (std::vector<std::string>{"a", "b"}));
}

TEST(LeaderValues, APointSaysItsLevelOnlyWhenItHasOne)
{
    Entity pit = entityOf(4, PointGeometry{Point2(100, 200)});
    pit.layer = "PITS";
    pit.properties["elevation"] = 12.25;
    pit.properties["invert"] = 10.5;
    pit.properties["pitcode"] = std::string("SMH");
    const std::string codes[] = {"pitcode"};
    const LabelValues values =
        anchorValues(pit, AnchorRef{4, AnchorPoint::Position}, Point2(100, 200), codes);
    EXPECT_EQ(said(values, "type"), "Point");
    EXPECT_EQ(said(values, "id"), "4");
    EXPECT_EQ(said(values, "layer"), "PITS");
    EXPECT_EQ(said(values, "code"), "SMH");
    EXPECT_EQ(said(values, "x"), "100.000");
    EXPECT_EQ(said(values, "northing"), "200.000");
    EXPECT_EQ(said(values, "rl"), "12.250");
    EXPECT_EQ(said(values, "prop.invert"), "10.500");
    EXPECT_EQ(said(values, "bearing"), "absent");

    pit.properties.erase("elevation");
    EXPECT_EQ(said(anchorValues(pit, AnchorRef{4}, Point2(100, 200)), "z"), "absent")
        << "absent is not zero";
}

TEST(LeaderValues, ALineSaysItsBearingAndTheLevelAndChainageAtTheTip)
{
    // East 40 m, from RL 10 to RL 20: grade 10 / 40 = 25 %, and a quarter of
    // the way along is chainage 10 at RL 10 + 0.25 x 10 = 12.5.
    Entity line = entityOf(1, Segment2{Point2(0, 0), Point2(40, 0)});
    setHeights(line.properties, {10.0, 20.0});
    const LabelValues values = anchorValues(line, along(1, 0.25), Point2(10, 0));
    EXPECT_EQ(said(values, "bearing"), "90°00'00\"");
    EXPECT_EQ(said(values, "distance"), "40.000");
    EXPECT_EQ(said(values, "length"), "40.000");
    EXPECT_EQ(said(values, "chainage"), "10.000");
    EXPECT_EQ(said(values, "z"), "12.500");
    EXPECT_EQ(said(values, "dz"), "10.000");
    EXPECT_EQ(said(values, "grade"), "25.000");
    EXPECT_EQ(said(values, "segment"), "1");
    EXPECT_EQ(said(values, "x"), "10.000");

    // Only the start has a level: the tip at the start has it, the middle not.
    Entity half = entityOf(2, Segment2{Point2(0, 0), Point2(40, 0)});
    setHeights(half.properties, {10.0, std::nullopt});
    EXPECT_EQ(said(anchorValues(half, AnchorRef{2, AnchorPoint::Start}, Point2(0, 0)), "z"),
              "10.000");
    const LabelValues middle = anchorValues(half, AnchorRef{2, AnchorPoint::Mid}, Point2(20, 0));
    EXPECT_EQ(said(middle, "z"), "absent");
    EXPECT_EQ(said(middle, "grade"), "absent");
    EXPECT_EQ(said(middle, "chainage"), "20.000");
}

TEST(LeaderValues, AnArcSaysItsCurveDataAndHowFarAlongTheTipIs)
{
    // A quarter circle of radius 10 from east to north: length 5 pi, chord
    // 10 root 2, tangent 10 tan 45 = 10, the chord from (10,0) to (0,10)
    // bearing 315; halfway along is chainage 2.5 pi.
    const Entity arc = entityOf(3, Arc2{Point2(0, 0), 10.0, 0.0, 0.5 * kPi});
    const LabelValues values = anchorValues(arc, along(3, 0.5), Point2(7, 7));
    EXPECT_DOUBLE_EQ(number(values, "radius"), 10.0);
    EXPECT_DOUBLE_EQ(number(values, "diameter"), 20.0);
    EXPECT_NEAR(number(values, "length"), 5.0 * kPi, 1e-12);
    EXPECT_NEAR(number(values, "chainage"), 2.5 * kPi, 1e-12);
    EXPECT_NEAR(number(values, "chord"), 10.0 * std::sqrt(2.0), 1e-12);
    EXPECT_NEAR(number(values, "tangent"), 10.0, 1e-12);
    EXPECT_EQ(said(values, "delta"), "90°00'00\"");
    EXPECT_EQ(said(values, "bearing"), "315°00'00\"");
    // At the centre the tip is on no part of the curve: no chainage.
    EXPECT_EQ(said(anchorValues(arc, AnchorRef{3, AnchorPoint::Centre}, Point2(0, 0)), "chainage"),
              "absent");
}

TEST(LeaderValues, ACircleSaysItsSizeAndArea)
{
    const LabelValues values =
        anchorValues(entityOf(3, Circle2{Point2(0, 0), 5.0}), along(3, 0.25), Point2(0, 5));
    EXPECT_DOUBLE_EQ(number(values, "radius"), 5.0);
    EXPECT_DOUBLE_EQ(number(values, "diameter"), 10.0);
    EXPECT_NEAR(number(values, "area"), 25.0 * kPi, 1e-12);
    EXPECT_NEAR(number(values, "perimeter"), 10.0 * kPi, 1e-12);
    EXPECT_NEAR(number(values, "length"), 10.0 * kPi, 1e-12);
    EXPECT_NEAR(number(values, "y"), 5.0, 1e-12) << "the tip, a quarter turn round";
    EXPECT_EQ(said(values, "chainage"), "absent") << "a circle has no start to count from";
}

TEST(LeaderValues, APolylineSaysTheSegmentTheTipIsOnAndItsWholeLength)
{
    // (0,0) -> (30,0) -> (30,40) at RL 100, 103, 111. Halfway up segment 2
    // (index 1): bearing 0 (north), 40 long, dz 8, grade 8 / 40 = 20 %,
    // chainage 30 + 20 = 50, RL 103 + 4 = 107; the whole line 70 long.
    Entity pipe = entityOf(4, Polyline2{{Point2(0, 0), Point2(30, 0), Point2(30, 40)}, false});
    setHeights(pipe.properties, {100.0, 103.0, 111.0});
    const LabelValues values = anchorValues(pipe, along(4, 0.5, 1), Point2(30, 20));
    EXPECT_EQ(said(values, "segment"), "2");
    EXPECT_EQ(said(values, "bearing"), "0°00'00\"");
    EXPECT_EQ(said(values, "distance"), "40.000");
    EXPECT_EQ(said(values, "dz"), "8.000");
    EXPECT_EQ(said(values, "grade"), "20.000");
    EXPECT_EQ(said(values, "chainage"), "50.000");
    EXPECT_EQ(said(values, "z"), "107.000");
    EXPECT_EQ(said(values, "length"), "70.000") << "the whole line's, not the segment's";
    EXPECT_EQ(said(values, "vertices"), "3");
    EXPECT_EQ(said(values, "area"), "absent") << "an open line has no area";

    // The end of an open line is the end of its last segment.
    const LabelValues end = anchorValues(pipe, AnchorRef{4, AnchorPoint::End}, Point2(30, 40));
    EXPECT_EQ(said(end, "chainage"), "70.000");
    EXPECT_EQ(said(end, "z"), "111.000");
    EXPECT_EQ(said(end, "segment"), "2");

    // A vertex since removed: the place nearest the tip stands for it.
    const LabelValues stale =
        anchorValues(pipe, AnchorRef{4, AnchorPoint::Vertex, 9}, Point2(31, 10));
    EXPECT_EQ(said(stale, "chainage"), "40.000");
    EXPECT_EQ(said(stale, "x"), "30.000");
}

TEST(LeaderValues, InsideALotSaysItsAreaAndALevelOnlyWhenItIsFlat)
{
    Entity lot =
        entityOf(5, Polyline2{{Point2(0, 0), Point2(20, 0), Point2(20, 10), Point2(0, 10)}, true});
    lot.properties["lot"] = std::int64_t{12};
    const LabelValues values = anchorValues(lot, AnchorRef{5, AnchorPoint::Inside}, Point2(0, 0));
    EXPECT_EQ(said(values, "area"), "200.0");
    EXPECT_EQ(said(values, "perimeter"), "60.000");
    EXPECT_EQ(said(values, "x"), "10.000");
    EXPECT_EQ(said(values, "y"), "5.000");
    EXPECT_EQ(said(values, "prop.lot"), "12");
    EXPECT_EQ(said(values, "bearing"), "absent") << "inside is on no segment";
    EXPECT_EQ(said(values, "z"), "absent");
    lot.properties["elevation"] = 31.5;
    EXPECT_EQ(said(anchorValues(lot, AnchorRef{5, AnchorPoint::Inside}, Point2(0, 0)), "rl"),
              "31.500");
}

TEST(LeaderValues, ATextAndADimensionSayWhatTheyAre)
{
    const LabelValues text = anchorValues(
        entityOf(6, TextGeometry{Point2(0, 0), "EX. KERB", 2.5, 0.0}), AnchorRef{6}, Point2(0, 0));
    EXPECT_EQ(said(text, "text"), "EX. KERB");
    const LabelValues aligned =
        anchorValues(entityOf(7, DimensionGeometry{Point2(0, 0), Point2(12, 5), 2.0, ""}),
                     AnchorRef{7}, Point2(0, 0));
    EXPECT_EQ(said(aligned, "measurement"), "13.000");
    EXPECT_EQ(said(aligned, "angle"), "absent");
    DimensionGeometry angular;
    angular.kind = DimensionKind::Angular;
    angular.start = Point2(10, 0);
    angular.end = Point2(0, 10);
    angular.vertex = Point2(0, 0);
    const LabelValues angle = anchorValues(entityOf(8, angular), AnchorRef{8}, Point2(0, 0));
    EXPECT_EQ(said(angle, "angle"), "90°00'00\"");
}

TEST(LeaderValues, TheNoteIsReadOffTheTargetAndALineWithNothingToSayIsDropped)
{
    Entity pit = entityOf(0, PointGeometry{Point2(10, 10)});
    pit.properties["invert"] = 10.25;
    Model model = modelWith({pit});
    const EntityId id = model.entities.ids().front();

    LeaderGeometry leader =
        smartLeader(id, AnchorRef{}, Point2(10, 10), "PIT {id}\nIL {prop.invert:.2f}\nRL {rl:.3f}");
    EXPECT_EQ(leaderNote(model, leader), "PIT " + std::to_string(id) + "\nIL 10.25")
        << "no RL: that line is dropped";
    EXPECT_EQ(missingValues(leader.text, *leaderValues(model, leader)),
              (std::vector<std::string>{"rl"}));

    // A plain leader's text is its text, braces and all.
    LeaderGeometry plain = leader;
    plain.fields = false;
    EXPECT_EQ(leaderNote(model, plain), plain.text);

    // A label style's template, read live.
    LabelStyle style;
    style.name = "Invert";
    style.kind = LabelKind::Point;
    style.text = "INV {prop.invert:.3f}";
    ASSERT_TRUE(model.labelStyles.add(style).ok());
    LeaderGeometry styled = leader;
    styled.fields = false;
    styled.text.clear();
    styled.labelStyle = "Invert";
    EXPECT_TRUE(validate(styled).ok());
    EXPECT_EQ(leaderNote(model, styled), "INV 10.250");
    styled.labelStyle = "Gone";
    EXPECT_EQ(leaderNote(model, styled), "") << "a missing style says nothing";
    EXPECT_FALSE(checkLeaderSaysSomething(model, styled).ok());
}

TEST(LeaderValues, ALeaderThatWouldSayNothingIsRefusedNamingWhatIsMissing)
{
    Model model = modelWith({entityOf(0, PointGeometry{Point2(0, 0)})});
    const EntityId id = model.entities.ids().front();
    const LeaderGeometry leader = smartLeader(id, AnchorRef{}, Point2(0, 0), "IL {prop.invrt}");
    const auto refused = checkLeaderSaysSomething(model, leader);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("no prop.invrt"), std::string::npos)
        << refused.error().describe();
    EXPECT_NE(refused.error().context.find("type=Point"), std::string::npos);

    LeaderGeometry nowhere = leader;
    nowhere.tipRef = AnchorRef{};
    EXPECT_FALSE(checkLeaderSaysSomething(model, nowhere).ok());
    LeaderGeometry gone = leader;
    gone.tipRef.entity = 999;
    EXPECT_EQ(checkLeaderSaysSomething(model, gone).error().code,
              katana::core::ErrorCode::NotFound);
    LeaderGeometry plain{.vertices = {Point2(0, 0), Point2(1, 1)}, .text = "{nothing}"};
    EXPECT_TRUE(checkLeaderSaysSomething(model, plain).ok()) << "a plain note is what it is";
}

// ---- validation and storage ---------------------------------------------------------------

TEST(SmartLeaders, ValidationKeepsOneTemplateAndATipToReadFrom)
{
    LeaderGeometry leader = smartLeader(3, AnchorRef{}, Point2(0, 0), "{length}");
    EXPECT_TRUE(validate(leader).ok());

    LeaderGeometry noTip = leader;
    noTip.tipRef = AnchorRef{};
    EXPECT_FALSE(validate(noTip).ok()) << "a smart leader reads the entity its tip is on";

    LeaderGeometry both = leader;
    both.labelStyle = "Invert";
    EXPECT_FALSE(validate(both).ok()) << "its own template or its style's, not both";

    LeaderGeometry styledWithText = leader;
    styledWithText.fields = false;
    styledWithText.labelStyle = "Invert";
    EXPECT_FALSE(validate(styledWithText).ok()) << "a styled leader has no text of its own";

    LeaderGeometry badTemplate = leader;
    badTemplate.text = "{invert}";
    const auto refused = validate(badTemplate);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::InvalidGeometry);

    LeaderGeometry empty = leader;
    empty.text.clear();
    EXPECT_FALSE(validate(empty).ok());
}

TEST(SmartLeaders, AVersionThreeBlobHoldsThemAndNothingElseNeedsOne)
{
    LeaderGeometry smart = smartLeader(3, along(3, 0.375, 2), Point2(1, 2), "IL {prop.invert}");
    smart.callout = CalloutShape::Box;
    LeaderGeometry styled = smartLeader(4, AnchorRef{4, AnchorPoint::Inside}, Point2(1, 2), "");
    styled.fields = false;
    styled.labelStyle = "Lot";
    LeaderGeometry plainAlong{.vertices = {Point2(0, 0), Point2(3, 4)}, .text = "A"};
    plainAlong.tipRef = along(9, 0.5);
    DimensionGeometry dimension{Point2(0, 0), Point2(10, 0), 2.0, ""};
    dimension.endRef = along(5, 1.0, 1);
    for (const Geometry& geometry :
         {Geometry{smart}, Geometry{styled}, Geometry{plainAlong}, Geometry{dimension}}) {
        const auto blob = geometryToBlob(geometry);
        ASSERT_TRUE(blob.ok());
        EXPECT_EQ(static_cast<std::uint8_t>((*blob)[0]), kBlobVersionSmartLeader);
        const auto back = geometryFromBlob(*blob);
        ASSERT_TRUE(back.ok()) << back.error().describe();
        EXPECT_EQ(*back, geometry);

        const auto json = geometryToJson(geometry);
        ASSERT_TRUE(json.ok());
        const auto fromJson = geometryFromJson(*json);
        ASSERT_TRUE(fromJson.ok()) << *json;
        EXPECT_EQ(*fromJson, geometry) << *json;
    }

    // A plain leader with an old anchor is still version 2, byte for byte:
    // header 2, vertices 4 + 32, text 4 + 1, style 4, arrow and callout 2,
    // three sizes 24, the anchor 8 + 1 + 4.
    LeaderGeometry plain{.vertices = {Point2(0, 0), Point2(3, 4)}, .text = "A"};
    plain.tipRef = AnchorRef{3, AnchorPoint::Vertex, 7};
    const auto old = geometryToBlob(plain);
    ASSERT_TRUE(old.ok());
    EXPECT_EQ(static_cast<std::uint8_t>((*old)[0]), kBlobVersionAnnotation);
    EXPECT_EQ(old->size(), 2u + 36u + 5u + 4u + 2u + 24u + 13u);
    const auto json = geometryToJson(plain);
    ASSERT_TRUE(json.ok());
    EXPECT_EQ(json->find("fields"), std::string::npos);
    EXPECT_EQ(json->find("parameter"), std::string::npos);
}

TEST(SmartLeaders, ANegativeZeroFractionKeepsItsBits)
{
    LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(3, 4)}};
    leader.tipRef = AnchorRef{2, AnchorPoint::Start, 0, -0.0};
    ASSERT_TRUE(validate(leader).ok()) << "-0 is 0";
    const auto blob = geometryToBlob(leader);
    ASSERT_TRUE(blob.ok());
    EXPECT_EQ(static_cast<std::uint8_t>((*blob)[0]), kBlobVersionSmartLeader)
        << "version 2 cannot hold the sign";
    const auto back = geometryFromBlob(*blob);
    ASSERT_TRUE(back.ok());
    EXPECT_TRUE(std::signbit(std::get<LeaderGeometry>(*back).tipRef.parameter));
}

TEST(SmartLeaders, AMalformedBlobIsRefused)
{
    // A version-2 anchor naming a point version 2 did not have.
    LeaderGeometry plain{.vertices = {Point2(0, 0), Point2(3, 4)}, .text = "A"};
    plain.tipRef = AnchorRef{3, AnchorPoint::End};
    auto v2 = geometryToBlob(plain);
    ASSERT_TRUE(v2.ok());
    ASSERT_EQ(static_cast<std::uint8_t>((*v2)[0]), kBlobVersionAnnotation);
    (*v2)[v2->size() - 5] = std::byte{static_cast<std::uint8_t>(AnchorPoint::Along)};
    EXPECT_FALSE(geometryFromBlob(*v2).ok());

    const LeaderGeometry smart = smartLeader(3, along(3, 0.5), Point2(1, 2), "{length}");
    auto v3 = geometryToBlob(smart);
    ASSERT_TRUE(v3.ok());
    auto cut = *v3;
    cut.pop_back();
    EXPECT_FALSE(geometryFromBlob(cut).ok());
    // The fields byte (before the empty label style's 4-byte length) is 0 or 1.
    auto flag = *v3;
    flag[flag.size() - 5] = std::byte{2};
    EXPECT_FALSE(geometryFromBlob(flag).ok());
    auto future = *v3;
    future[0] = std::byte{4};
    EXPECT_FALSE(geometryFromBlob(future).ok()) << "a version from a newer build";
}
