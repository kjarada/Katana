// What a leader reads beyond the entity its tip is on (docs/annotation.md,
// "What a leader can say"): the drawing's surfaces and alignments, through a
// LeaderContext (entity/leader_values.hpp), and the +.Nf step that writes a
// difference with its sign.
//
// The context here is a fake made of planes and a straight line, so every
// expected value is worked out by hand in the comment beside it:
//
//   ground           z = 10 + 0.1 x + 0.2 y over the square 0..100 x 0..100,
//                    nothing outside it
//   Existing Ground  z = 5 over the same square (a name with a blank)
//   MC01             the x axis from (0, 0) to (100, 0): station = x, offset
//                    = y (north is LEFT looking east, along increasing
//                    station), and a design profile 20 + 0.01 x from station
//                    0 to 80 only
//
// The cad suite reads a real TIN from a Document (test_leader_presets.cpp).

#include <gtest/gtest.h>

#include <cmath>
#include <optional>
#include <string>
#include <vector>

#include "katana/entity/anchor.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/entity/model.hpp"

using namespace katana::entity;
using katana::geometry::Point2;

namespace {

bool inSquare(const Point2& at)
{
    return at.x >= 0.0 && at.x <= 100.0 && at.y >= 0.0 && at.y <= 100.0;
}

LeaderContext fakeContext()
{
    LeaderContext context;
    context.surfaces = {"ground", "Existing Ground"};
    context.alignments = {"MC01"};
    context.surfaceLevel = [](std::string_view name, const Point2& at) -> std::optional<double> {
        if (!inSquare(at)) {
            return std::nullopt;
        }
        if (name == "ground") {
            return 10.0 + 0.1 * at.x + 0.2 * at.y;
        }
        if (name == "Existing Ground") {
            return 5.0;
        }
        return std::nullopt;
    };
    context.alignmentPlace = [](std::string_view name,
                                const Point2& at) -> std::optional<AlignmentPlace> {
        if (name != "MC01" || at.x < 0.0 || at.x > 100.0) {
            return std::nullopt;
        }
        AlignmentPlace place{at.x, at.y, std::nullopt};
        if (at.x <= 80.0) {
            place.profile = 20.0 + 0.01 * at.x;
        }
        return place;
    };
    return context;
}

// A model holding one point at `at`, with a level when `z` is given; its id.
struct OnePoint {
    Model model;
    EntityId id = 0;
};
OnePoint pointAt(Point2 at, std::optional<double> z)
{
    OnePoint made;
    Entity point;
    point.geometry = PointGeometry{at};
    if (z) {
        point.properties["elevation"] = *z;
    }
    EXPECT_TRUE(made.model.entities.add(std::move(point)).ok());
    made.id = made.model.entities.ids().front();
    return made;
}

LeaderGeometry smartLeader(const OnePoint& on, std::string templateText)
{
    LeaderGeometry leader;
    const Point2 at = std::get<PointGeometry>(on.model.entities.find(on.id)->geometry).position;
    leader.vertices = {at, at + katana::geometry::Vec2(5, 5)};
    leader.tipRef = AnchorRef{on.id, AnchorPoint::Position};
    leader.text = std::move(templateText);
    leader.fields = true;
    return leader;
}

std::string said(const LabelValues& values, const std::string& name)
{
    const auto found = values.find(name);
    return found == values.end() ? std::string("absent") : formatValue(found->second);
}

std::string refusalOf(const OnePoint& on, const std::string& templateText,
                      const LeaderContext& context)
{
    const auto status =
        checkLeaderSaysSomething(on.model, smartLeader(on, templateText), {}, context);
    EXPECT_FALSE(status.ok()) << templateText << " was not refused";
    return status.ok() ? std::string() : status.error().message;
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

TEST(LeaderContextValues, ASurfaceIsReadUnderTheTipAndTheTargetsHeightAboveItIsPositive)
{
    // (30, 40) at RL 25: ground there is 10 + 3 + 8 = 21, so the point is 4
    // above it - a fill of 4, and no cut.
    const OnePoint on = pointAt(Point2(30, 40), 25.0);
    const LeaderContext context = fakeContext();
    const LeaderGeometry leader = smartLeader(
        on, "GL {surface.ground:.3f}\n{dsurface.ground:+.3f}\nCUT {cut.ground:.2f}\nFILL "
            "{fill.ground:.2f}");
    EXPECT_EQ(leaderNote(on.model, leader, {}, context), "GL 21.000\n+4.000\nFILL 4.00");
}

TEST(LeaderContextValues, CutAndFillSayOnlyTheOneThatHoldsAndOnTheSurfaceIsAFillOfNothing)
{
    const LeaderContext context = fakeContext();
    const std::string cutFill = "CUT {cut.ground:.2f}\nFILL {fill.ground:.2f}";
    // RL 18 under ground 21: 3 below, a cut of 3; its difference -3.
    const OnePoint below = pointAt(Point2(30, 40), 18.0);
    EXPECT_EQ(leaderNote(below.model, smartLeader(below, cutFill), {}, context), "CUT 3.00");
    EXPECT_EQ(leaderNote(below.model, smartLeader(below, "{dsurface.ground:+.3f}"), {}, context),
              "-3.000");
    // RL 21 exactly on ground 21: neither above nor below, said as a fill
    // of nothing, and a difference of nothing has no sign.
    const OnePoint on = pointAt(Point2(30, 40), 21.0);
    EXPECT_EQ(leaderNote(on.model, smartLeader(on, cutFill), {}, context), "FILL 0.00");
    EXPECT_EQ(leaderNote(on.model, smartLeader(on, "{dsurface.ground:+.3f}"), {}, context),
              "0.000");
}

TEST(LeaderContextValues, ANameWithABlankIsWrittenInTheTemplateAsItIs)
{
    // Existing Ground is 5 everywhere in the square: RL 25 is 20 above.
    const OnePoint on = pointAt(Point2(30, 40), 25.0);
    EXPECT_EQ(leaderNote(on.model,
                         smartLeader(on, "{surface.Existing Ground:.1f} {dsurface.Existing "
                                         "Ground:+.1f}"),
                         {}, fakeContext()),
              "5.0 +20.0");
    EXPECT_TRUE(checkLeaderTemplate("{dsurface.Existing Ground:+.3f}").ok());
}

TEST(LeaderContextValues, AStationIsReadAlongTheAlignmentWithItsOffsetAndTheProfile)
{
    // (30, -4) at RL 25 on MC01: station 30, 4 to the right (south of an
    // eastward line), design level 20 + 0.3 = 20.3, so 4.7 above the design.
    const OnePoint on = pointAt(Point2(30, -4), 25.0);
    EXPECT_EQ(leaderNote(on.model,
                         smartLeader(on, "CH {station.MC01:ch}\nOS {offset.MC01:+.3f}\nDL "
                                         "{profile.MC01:.3f}\n{dprofile.MC01:+.3f}"),
                         {}, fakeContext()),
              "CH 0+030.000\nOS -4.000\nDL 20.300\n+4.700");
}

TEST(LeaderContextValues, WhatCannotBeReadDropsItsLineAndARefusalSaysWhy)
{
    const LeaderContext context = fakeContext();
    // Off the surface: (150, 40) is outside the square.
    const OnePoint off = pointAt(Point2(150, 40), 25.0);
    EXPECT_TRUE(contains(refusalOf(off, "{dsurface.ground:+.3f}", context),
                         "dsurface.ground (the tip is off surface ground)"));
    // No level on the point: the ground under it is still read, the
    // difference is not.
    const OnePoint flat = pointAt(Point2(30, 40), std::nullopt);
    EXPECT_EQ(leaderNote(flat.model, smartLeader(flat, "{surface.ground:.3f}\n{dsurface.ground}"),
                         {}, context),
              "21.000");
    EXPECT_TRUE(contains(refusalOf(flat, "{fill.ground}", context), "the target has no level"));
    // A surface the drawing has not, and an alignment.
    const OnePoint on = pointAt(Point2(30, 40), 25.0);
    EXPECT_TRUE(
        contains(refusalOf(on, "{surface.grnd}", context), "the drawing has no surface grnd"));
    EXPECT_TRUE(
        contains(refusalOf(on, "{station.MC02}", context), "the drawing has no alignment MC02"));
    // The cut of a point above the surface.
    EXPECT_TRUE(
        contains(refusalOf(on, "{cut.ground}", context), "the target is not below surface ground"));
    // Beyond the alignment's end, and past its profile's.
    const OnePoint beyond = pointAt(Point2(120, 5), 25.0);
    EXPECT_TRUE(contains(refusalOf(beyond, "{station.MC01}", context),
                         "the tip is beyond the ends of alignment MC01"));
    const OnePoint late = pointAt(Point2(90, 0), 25.0);
    EXPECT_TRUE(contains(refusalOf(late, "{profile.MC01}", context),
                         "alignment MC01 has no design profile at station 90.000"));
}

TEST(LeaderContextValues, WithNoContextASurfacesValueIsAbsentNeverZero)
{
    const OnePoint on = pointAt(Point2(30, 40), 25.0);
    const LeaderGeometry leader = smartLeader(on, "RL {rl:.3f}\n{dsurface.ground:+.3f}");
    EXPECT_EQ(leaderNote(on.model, leader), "RL 25.000");
    const auto values = *leaderValues(on.model, leader);
    EXPECT_EQ(said(values, "dsurface.ground"), "absent");
    EXPECT_EQ(whyAbsent("dsurface.ground", values, Point2(30, 40), LeaderContext{}),
              "no surfaces were given to read here");
}

TEST(LeaderContextValues, AnchorValuesListEveryValueOfEachSurfaceAndAlignmentThatHolds)
{
    const OnePoint on = pointAt(Point2(30, 40), 25.0);
    const Entity& point = *on.model.entities.find(on.id);
    const LabelValues values =
        anchorValues(point, AnchorRef{on.id, AnchorPoint::Position}, Point2(30, 40), {},
                     fakeContext());
    EXPECT_EQ(said(values, "surface.ground"), "21.000");
    EXPECT_EQ(said(values, "dsurface.ground"), "4.000");
    EXPECT_EQ(said(values, "fill.ground"), "4.000");
    EXPECT_EQ(said(values, "cut.ground"), "absent") << "above the surface: no cut";
    EXPECT_EQ(said(values, "surface.Existing Ground"), "5.000");
    EXPECT_EQ(said(values, "dsurface.Existing Ground"), "20.000");
    // Station 30 printed as a chainage's own (a plain 3-decimal number),
    // offset 40 left, design 20.3, 4.7 above it.
    EXPECT_EQ(said(values, "station.MC01"), "30.000");
    EXPECT_EQ(said(values, "offset.MC01"), "40.000");
    EXPECT_EQ(said(values, "profile.MC01"), "20.300");
    EXPECT_EQ(said(values, "dprofile.MC01"), "4.700");
    // Without a context, none of them.
    const LabelValues bare =
        anchorValues(point, AnchorRef{on.id, AnchorPoint::Position}, Point2(30, 40));
    EXPECT_EQ(said(bare, "surface.ground"), "absent");
    EXPECT_EQ(said(bare, "rl"), "25.000");
}

TEST(LeaderContextValues, TheTemplateCheckKnowsEveryKindWhateverTheNameAndItsQuantity)
{
    for (const std::string_view kind : leaderContextKinds()) {
        EXPECT_TRUE(leaderValueQuantity(std::string(kind) + ".anything").has_value()) << kind;
        EXPECT_FALSE(leaderValueQuantity(std::string(kind) + ".").has_value()) << kind;
    }
    EXPECT_EQ(leaderContextKinds().size(), 8U);
    EXPECT_EQ(leaderValueQuantity("station.MC01"), LabelQuantity::Chainage);
    EXPECT_EQ(leaderValueQuantity("offset.MC01"), LabelQuantity::Length);
    EXPECT_EQ(leaderValueQuantity("dsurface.ground"), LabelQuantity::Number);
    EXPECT_FALSE(leaderValueQuantity("ground.surface").has_value());
    EXPECT_TRUE(checkLeaderTemplate("CH {station.MC01:ch}").ok()) << "a station is a chainage";
    EXPECT_FALSE(checkLeaderTemplate("{offset.MC01:ch}").ok()) << "an offset is not";
    EXPECT_FALSE(checkLeaderTemplate("{surface.ground:dms}").ok()) << "a level is no angle";
    EXPECT_FALSE(checkLeaderTemplate("{surface.:.3f}").ok()) << "a kind with no name";
    const auto parsed = contextValueName("dsurface.Existing Ground");
    ASSERT_TRUE(parsed.has_value());
    EXPECT_EQ(parsed->kind, "dsurface");
    EXPECT_EQ(parsed->name, "Existing Ground");
    EXPECT_FALSE(contextValueName("prop.invert").has_value());
}

TEST(LeaderContextValues, APlusFormatWritesTheSignOfAValueAboveNothingOnly)
{
    const auto with = [](double dz) {
        LabelValues values;
        values.insert_or_assign("dz", LabelValue::of(LabelQuantity::Number, dz));
        return formatLabel("{dz:+.3f}", values);
    };
    EXPECT_EQ(with(0.25), "+0.250");
    EXPECT_EQ(with(-0.25), "-0.250");
    // Both round to nothing at three decimals, and nothing has no sign.
    EXPECT_EQ(with(0.0004), "0.000");
    EXPECT_EQ(with(-0.0004), "0.000");
    // Rounded half away from zero first (label_text.hpp): 2.5 is +3.
    LabelValues half;
    half.insert_or_assign("dz", LabelValue::of(LabelQuantity::Number, 2.5));
    EXPECT_EQ(formatLabel("{dz:+.0f}", half), "+3");
    EXPECT_TRUE(checkLabelTemplate("{dz:+.3f}", LabelKind::Segment).ok());
    EXPECT_FALSE(checkLabelTemplate("{layer:+.3f}", LabelKind::Segment).ok())
        << "a text has no sign to write";
    EXPECT_FALSE(checkLabelTemplate("{dz:+.3f:m}", LabelKind::Segment).ok())
        << "a number format is the last step";
}

TEST(LeaderContextValues, APointOnTheSurfaceWithinTheToleranceIsAFillOfNothingNotACutOfTheRoundingError)
{
    // The surface's level is interpolated, so a point exactly on it can come
    // out a few bits either side. Here the surface is one ulp above the
    // point's 21 at (30, 40): 21 + 3.55e-15, so level - ground is -3.55e-15,
    // and the exact comparison called it a cut (and the leader that asked for
    // a fill was refused). Within the geometric tolerance (1e-7) is on it.
    const auto contextAt = [](double ground) {
        LeaderContext context = fakeContext();
        context.surfaceLevel = [ground](std::string_view name, const Point2& at) -> std::optional<double> {
            return name == "ground" && inSquare(at) ? std::optional<double>(ground) : std::nullopt;
        };
        return context;
    };
    const OnePoint on = pointAt(Point2(30, 40), 21.0);
    const std::string cutFill = "CUT {cut.ground:.2f}\nFILL {fill.ground:.2f}";
    const double aUlpAbove = std::nextafter(21.0, 100.0);
    ASSERT_GT(aUlpAbove, 21.0);
    ASSERT_LT(aUlpAbove - 21.0, 1.0e-14);
    EXPECT_EQ(leaderNote(on.model, smartLeader(on, cutFill), {}, contextAt(aUlpAbove)), "FILL 0.00");
    // And the other way round, a ulp below: a fill, as it always was.
    EXPECT_EQ(leaderNote(on.model, smartLeader(on, cutFill), {}, contextAt(std::nextafter(21.0, 0.0))),
              "FILL 0.00");
    // A fill-only note on such a point is accepted, not refused as "below".
    EXPECT_TRUE(checkLeaderSaysSomething(on.model, smartLeader(on, "FILL {fill.ground:.3f}"), {},
                                         contextAt(aUlpAbove))
                    .ok());
    // Inside the tolerance (5e-8 of the 1e-7) is on the surface; outside it
    // (1e-6) is a real cut of a micrometre, said as one - and not a fill.
    EXPECT_EQ(leaderNote(on.model, smartLeader(on, cutFill), {}, contextAt(21.0 + 5.0e-8)), "FILL 0.00");
    EXPECT_EQ(leaderNote(on.model, smartLeader(on, cutFill), {}, contextAt(21.0 + 1.0e-6)), "CUT 0.00");
    EXPECT_FALSE(checkLeaderSaysSomething(on.model, smartLeader(on, "FILL {fill.ground:.3f}"), {},
                                          contextAt(21.0 + 1.0e-6))
                     .ok());
}
