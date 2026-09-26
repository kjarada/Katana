// The leader edits the LEADER verbs and the window's Leaders manager share
// (cad/annotation/leader_edit.hpp, docs/annotation.md "Smart leaders"): each
// checked against the model and made ONE command, or refused in the words
// the command line replies with.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"

using namespace katana::cad;
using namespace katana::entity;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
namespace ann = katana::cad::annotation;
namespace cmd = katana::commands;

namespace {

EntityId add(Document& document, Geometry geometry, PropertyMap properties = {})
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.properties = std::move(properties);
    EXPECT_TRUE(document.execute(cmd::createEntities({std::move(entity)})).ok());
    return document.lastCreatedEntities().front();
}

const LeaderGeometry& leaderOf(const Document& document, EntityId id)
{
    return std::get<LeaderGeometry>(document.model().entities.find(id)->geometry);
}

ann::LeaderChange templateChange(std::string text)
{
    ann::LeaderChange change;
    change.note = ann::LeaderNote{ann::LeaderNote::Kind::Template, std::move(text)};
    return change;
}

} // namespace

TEST(LeaderEdit, AnUnchangedEditIsNoCommandAndAChangeIsOne)
{
    Document document;
    const EntityId pit = add(document, PointGeometry{Point2(0, 0)}, {{"invert", 10.5}});
    auto made = ann::newLeader(
        document.model(),
        {ann::AnchoredPoint{Point2(0, 0), AnchorRef{pit}}, ann::AnchoredPoint{Point2(5, 5), {}}},
        templateChange("IL {prop.invert:.2f}"), false);
    ASSERT_TRUE(made.ok()) << made.error().describe();
    const EntityId leader = add(document, *made);
    EXPECT_EQ(ann::leaderSays(document.model(), leaderOf(document, leader)), "IL 10.50");
    EXPECT_EQ(ann::noteOf(leaderOf(document, leader)),
              (ann::LeaderNote{ann::LeaderNote::Kind::Template, "IL {prop.invert:.2f}"}));

    auto same =
        ann::changeLeaders(document.model(), {leader}, templateChange("IL {prop.invert:.2f}"));
    ASSERT_TRUE(same.ok());
    EXPECT_EQ(*same, nullptr) << "the same template again changes nothing";

    ann::LeaderChange look;
    look.arrow = ArrowHead::Open;
    look.paperHeight = 3.5;
    auto changed = ann::changeLeaders(document.model(), {leader, leader}, look);
    ASSERT_TRUE(changed.ok());
    ASSERT_NE(*changed, nullptr);
    const std::size_t steps = document.history().undoCount();
    ASSERT_TRUE(document.execute(std::move(*changed)).ok());
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(leaderOf(document, leader).arrow, ArrowHead::Open);
    EXPECT_EQ(leaderOf(document, leader).paperHeight, 3.5);
    EXPECT_TRUE(leaderOf(document, leader).fields) << "the note is left as it was";

    EXPECT_FALSE(ann::changeLeaders(document.model(), {pit}, look).ok()) << "a point is no leader";
    EXPECT_FALSE(ann::changeLeaders(document.model(), {}, look).ok());
}

TEST(LeaderEdit, WhatCannotBeMadeIsRefusedNotSkipped)
{
    Document document;
    const EntityId a = add(document, PointGeometry{Point2(0, 0)});
    const EntityId b = add(document, PointGeometry{Point2(10, 0)}, {{"invert", 9.0}});
    ann::LeadersForOptions options;
    options.change = templateChange("{invert}"); // no such value: every leader would be wrong
    auto refused = ann::leadersFor(document.model(), {a, b}, options, 1000.0, {});
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().context.find("no value 'invert'"), std::string::npos)
        << refused.error().describe();

    // A note one target cannot say is that target skipped.
    options.change = templateChange("IL {prop.invert}");
    auto made = ann::leadersFor(document.model(), {a, b}, options, 1000.0, {});
    ASSERT_TRUE(made.ok()) << made.error().describe();
    EXPECT_EQ(made->made, 1u);
    EXPECT_EQ(made->skipped, 1u);
    EXPECT_NE(made->firstSkip.find("no prop.invert"), std::string::npos) << made->firstSkip;

    ann::LeaderChange missingStyle;
    missingStyle.textStyle = "Nowhere";
    EXPECT_EQ(ann::checkLeaderChange(document.model(), missingStyle).error().code,
              katana::core::ErrorCode::NotFound);

    // 0 mm is a size (the style's height, no landing); below it is none.
    ann::LeaderChange none;
    none.paperHeight = 0.0;
    none.landing = 0.0;
    EXPECT_TRUE(ann::checkLeaderChange(document.model(), none).ok());
    for (const double bad : {-0.5, std::numeric_limits<double>::quiet_NaN(),
                             std::numeric_limits<double>::infinity()}) {
        ann::LeaderChange landing;
        landing.landing = bad;
        EXPECT_EQ(ann::checkLeaderChange(document.model(), landing).error().code,
                  katana::core::ErrorCode::InvalidArgument)
            << bad;
        ann::LeaderChange arrow;
        arrow.arrowSize = bad;
        EXPECT_FALSE(ann::checkLeaderChange(document.model(), arrow).ok()) << bad;
        ann::LeaderChange paper;
        paper.paperHeight = bad;
        EXPECT_FALSE(ann::checkLeaderChange(document.model(), paper).ok()) << bad;
    }
}

TEST(LeaderEdit, APlaceForALeaderIsOnTheEntityOrThereIsNone)
{
    Document document;
    const EntityId point = add(document, PointGeometry{Point2(3, 4)});
    const EntityId line = add(document, Segment2{Point2(0, 0), Point2(10, 0)});
    DimensionGeometry measured;
    measured.start = Point2(0, 0);
    measured.end = Point2(10, 0);
    const EntityId dimension = add(document, measured);
    const auto onPoint =
        ann::leaderPlaceOn(*document.model().entities.find(point), 0.25 * katana::math::kPi);
    ASSERT_TRUE(onPoint);
    EXPECT_EQ(onPoint->point, Point2(3, 4));
    EXPECT_EQ(onPoint->ref.entity, point);
    // A line's middle: half way along it.
    const auto onLine =
        ann::leaderPlaceOn(*document.model().entities.find(line), 0.25 * katana::math::kPi);
    ASSERT_TRUE(onLine);
    EXPECT_EQ(onLine->point, Point2(5, 0));
    EXPECT_EQ(onLine->ref.point, AnchorPoint::Along);
    EXPECT_EQ(onLine->ref.parameter, 0.5);
    EXPECT_FALSE(
        ann::leaderPlaceOn(*document.model().entities.find(dimension), 0.25 * katana::math::kPi))
        << "a dimension is annotation, not something to point at";

    // Halfway along an open polyline: (0,0)-(30,0)-(30,40) is 70 long, so 35
    // along is 5 into segment 1 (40 long), a fraction 5/40 = 0.125: (30,5).
    const EntityId pipe = add(document, Polyline2{{Point2(0, 0), Point2(30, 0), Point2(30, 40)}});
    const auto onPipe = ann::leaderPlaceOn(*document.model().entities.find(pipe), 0.0);
    ASSERT_TRUE(onPipe);
    EXPECT_EQ(onPipe->point, Point2(30, 5));
    EXPECT_EQ(onPipe->ref.index, 1u);
    EXPECT_EQ(onPipe->ref.parameter, 0.125);
    // An arc's middle: a quarter circle of radius 10 from 0 to 90 degrees is
    // at 45 degrees, half way along it.
    const EntityId bend = add(document, Arc2{Point2(0, 0), 10.0, 0.0, 0.5 * katana::math::kPi});
    const auto onBend = ann::leaderPlaceOn(*document.model().entities.find(bend), 0.0);
    ASSERT_TRUE(onBend);
    EXPECT_EQ(onBend->ref.parameter, 0.5);
    EXPECT_NEAR(onBend->point.x, 10.0 * std::cos(0.25 * katana::math::kPi), 1e-12);
    EXPECT_NEAR(onBend->point.y, 10.0 * std::sin(0.25 * katana::math::kPi), 1e-12);
}

TEST(LeaderEdit, APieceWithinTheGeometricToleranceHasNoAlongForLeaderFor)
{
    // (0,0)-(1,0)-(1+5e-8,0)-(2+5e-8,0): the half, 1 + 2.5e-8, falls in the
    // 5e-8 piece, below math::tolerance::kGeometric (1e-6) - no direction,
    // so no along, as for nearestAnchor. The tip goes to the next piece's
    // start instead, within 5e-8 of the half.
    Document document;
    const double tiny = 5e-8;
    const EntityId line =
        add(document,
            Polyline2{{Point2(0, 0), Point2(1, 0), Point2(1 + tiny, 0), Point2(2 + tiny, 0)}});
    const auto place = ann::leaderPlaceOn(*document.model().entities.find(line), 0.0);
    ASSERT_TRUE(place);
    EXPECT_EQ(place->ref.index, 2u);
    EXPECT_EQ(place->ref.parameter, 0.0);
    // Nothing of any length - which the model will not hold, but a caller
    // may still hand over - is no place at all.
    Entity dot;
    dot.id = 99;
    dot.geometry = Polyline2{{Point2(1, 1), Point2(1, 1)}};
    EXPECT_FALSE(ann::leaderPlaceOn(dot, 0.0));
}

TEST(LeaderEdit, LeadersForRefusesNoTargetsAndALengthAngleOrScaleThatIsNoSize)
{
    Document document;
    const EntityId a = add(document, PointGeometry{Point2(0, 0)});
    ann::LeadersForOptions options;
    const auto code = [&](std::vector<EntityId> ids, const ann::LeadersForOptions& with,
                          double scale) {
        const auto made = ann::leadersFor(document.model(), std::move(ids), with, scale, {});
        // No code when it was made (ErrorCode{} is InvalidArgument itself).
        return made.ok() ? std::optional<katana::core::ErrorCode>{} : made.error().code;
    };
    EXPECT_EQ(code({}, options, 1000.0), katana::core::ErrorCode::InvalidArgument);
    for (const double bad : {0.0, -1.0, std::numeric_limits<double>::infinity(),
                             std::numeric_limits<double>::quiet_NaN()}) {
        ann::LeadersForOptions length = options;
        length.length = bad;
        EXPECT_EQ(code({a}, length, 1000.0), katana::core::ErrorCode::InvalidArgument) << bad;
    }
    for (const double bad :
         {std::numeric_limits<double>::quiet_NaN(), std::numeric_limits<double>::infinity()}) {
        ann::LeadersForOptions angle = options;
        angle.angle = bad;
        EXPECT_EQ(code({a}, angle, 1000.0), katana::core::ErrorCode::InvalidArgument) << bad;
    }
    EXPECT_EQ(code({a}, options, 0.0), katana::core::ErrorCode::InvalidArgument);
    EXPECT_EQ(code({a}, options, -1.0), katana::core::ErrorCode::InvalidArgument);
    EXPECT_EQ(code({a, 999999}, options, 1000.0), katana::core::ErrorCode::NotFound);
    EXPECT_EQ(code({a}, options, 1000.0), std::nullopt) << "the premise: it can";
}

TEST(LeaderEdit, AligningRefusesWhatCannotBeAColumnAndIsNoCommandWhenNothingMoves)
{
    Document document;
    const EntityId pit = add(document, PointGeometry{Point2(0, 0)});
    const EntityId a =
        add(document, LeaderGeometry{.vertices = {Point2(0, 0), Point2(20, 30)}, .text = "A"});
    const EntityId b =
        add(document, LeaderGeometry{.vertices = {Point2(5, 0), Point2(20, 10)}, .text = "B"});
    const auto code = [&](std::vector<EntityId> ids, std::optional<double> x,
                          std::optional<double> spacing) {
        const auto aligned = ann::alignLeaders(document.model(), std::move(ids), x, spacing);
        return aligned.ok() ? std::optional<katana::core::ErrorCode>{} : aligned.error().code;
    };
    EXPECT_EQ(code({}, std::nullopt, std::nullopt), katana::core::ErrorCode::InvalidArgument);
    EXPECT_EQ(code({pit}, std::nullopt, std::nullopt), katana::core::ErrorCode::InvalidArgument);
    for (const double bad : {0.0, -1.0, std::numeric_limits<double>::infinity(),
                             std::numeric_limits<double>::quiet_NaN()}) {
        EXPECT_EQ(code({a, b}, std::nullopt, bad), katana::core::ErrorCode::InvalidArgument) << bad;
    }
    for (const double bad :
         {std::numeric_limits<double>::quiet_NaN(), std::numeric_limits<double>::infinity()}) {
        EXPECT_EQ(code({a, b}, bad, std::nullopt), katana::core::ErrorCode::InvalidArgument) << bad;
    }
    // Finite, yet the lowest of three notes would be stacked past any
    // number: 30 - 2 x DBL_MAX overflows to minus infinity.
    const EntityId c =
        add(document, LeaderGeometry{.vertices = {Point2(9, 0), Point2(20, 20)}, .text = "C"});
    EXPECT_EQ(code({a, b, c}, std::nullopt, std::numeric_limits<double>::max()),
              katana::core::ErrorCode::InvalidArgument);
    // Both already hang at x = 20 and x= keeps each one's height: no command.
    const auto still = ann::alignLeaders(document.model(), {a, b}, 20.0, std::nullopt);
    ASSERT_TRUE(still.ok());
    EXPECT_EQ(still->command, nullptr);
    EXPECT_EQ(still->count, 2u);
}

TEST(LeaderEdit, ABalloonAtTheLargestNumberRefusesTheNextRatherThanWrapping)
{
    constexpr long long largest = std::numeric_limits<long long>::max();
    Document document;
    const EntityId a = add(document, PointGeometry{Point2(0, 0)});
    const EntityId b = add(document, PointGeometry{Point2(10, 0)});
    const std::vector<ann::AnchoredPoint> points{ann::AnchoredPoint{Point2(0, 0), AnchorRef{a}},
                                                 ann::AnchoredPoint{Point2(5, 5), {}}};
    ann::LeadersForOptions balloons;
    balloons.balloon = true;

    // One short of the largest: one more fits, two do not.
    add(document, LeaderGeometry{.vertices = {Point2(50, 0), Point2(55, 5)},
                                 .text = std::to_string(largest - 1),
                                 .callout = CalloutShape::Circle});
    EXPECT_EQ(*ann::nextBalloonNumber(document.model()), largest);
    auto one = ann::leadersFor(document.model(), {a}, balloons, 1000.0, {});
    ASSERT_TRUE(one.ok()) << one.error().describe();
    EXPECT_FALSE(ann::leadersFor(document.model(), {a, b}, balloons, 1000.0, {}).ok());
    ASSERT_TRUE(document.execute(std::move(one->command)).ok());
    EXPECT_EQ(leaderOf(document, document.lastCreatedEntities().front()).text,
              std::to_string(largest));

    // At the largest there is no next: refused, not wrapped to a negative.
    EXPECT_FALSE(ann::nextBalloonNumber(document.model()).ok());
    EXPECT_FALSE(ann::newLeader(document.model(), points, {}, true).ok());
    EXPECT_FALSE(ann::leadersFor(document.model(), {a}, balloons, 1000.0, {}).ok());
    // A balloon given its note is not numbered, so is made.
    ann::LeaderChange noted;
    noted.note = ann::LeaderNote{ann::LeaderNote::Kind::Text, "A"};
    EXPECT_TRUE(ann::newLeader(document.model(), points, noted, true).ok());
    // Numbered again from 1, there is a next once more.
    auto renumbering = ann::renumberBalloons(document.model(), 1, ann::BalloonOrder::Id);
    ASSERT_TRUE(document.execute(std::move(renumbering.command)).ok());
    EXPECT_EQ(*ann::nextBalloonNumber(document.model()), 3);
}

TEST(LeaderEdit, ATipPutOnALotGoesInsideItWhenItIsInside)
{
    Document document;
    // A 10 x 10 lot: inside, the tip points into it - its middle (5,5);
    // outside, it goes to the nearest place on it, (10,5) half way up the
    // east side (segment 1).
    const EntityId lot = add(
        document, Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true});
    const Entity& outline = *document.model().entities.find(lot);
    const auto in = ann::tipPlaceOn(outline, Point2(3, 4));
    ASSERT_TRUE(in);
    EXPECT_EQ(in->ref.point, AnchorPoint::Inside);
    EXPECT_EQ(in->point, Point2(5, 5));
    const auto out = ann::tipPlaceOn(outline, Point2(14, 5));
    ASSERT_TRUE(out);
    EXPECT_EQ(out->ref.point, AnchorPoint::Along);
    EXPECT_EQ(out->ref.index, 1u);
    EXPECT_EQ(out->point, Point2(10, 5));
    // A circle's inside is its centre; an open line has none.
    const EntityId ring = add(document, Circle2{Point2(20, 20), 4.0});
    EXPECT_EQ(ann::tipPlaceOn(*document.model().entities.find(ring), Point2(21, 21))->point,
              Point2(20, 20));
    const EntityId open = add(document, Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10)}});
    EXPECT_EQ(ann::tipPlaceOn(*document.model().entities.find(open), Point2(8, 2))->ref.point,
              AnchorPoint::Along);
    // A dimension offers no place.
    DimensionGeometry measured;
    measured.start = Point2(0, 0);
    measured.end = Point2(10, 0);
    const EntityId dimension = add(document, measured);
    EXPECT_FALSE(ann::tipPlaceOn(*document.model().entities.find(dimension), Point2(5, 1)));
}

TEST(LeaderEdit, ALabelStyleLendsItsLookUnlessTheChangeGivesOne)
{
    Document document;
    TextStyle notes;
    notes.name = "Notes";
    ASSERT_TRUE(document.execute(cmd::createTextStyle(notes)).ok());
    LabelStyle invert;
    invert.name = "Invert";
    invert.kind = LabelKind::Point;
    invert.text = "{prop.invert}";
    invert.textStyle = "Notes";
    invert.paperHeight = 3.0;
    ASSERT_TRUE(document.execute(cmd::createLabelStyle(invert)).ok());
    const EntityId pit = add(document, PointGeometry{Point2(0, 0)}, {{"invert", 9.5}});
    LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(5, 5)}, .tipRef = AnchorRef{pit}};
    ann::LeaderChange change;
    change.note = ann::LeaderNote{ann::LeaderNote::Kind::LabelStyle, "Invert"};
    ASSERT_TRUE(ann::applyLeaderChange(document.model(), change, leader).ok());
    EXPECT_EQ(leader.style, "Notes");
    EXPECT_EQ(leader.paperHeight, 3.0);

    LeaderGeometry own{.vertices = {Point2(0, 0), Point2(5, 5)}, .tipRef = AnchorRef{pit}};
    change.textStyle = "";
    change.paperHeight = 2.0;
    ASSERT_TRUE(ann::applyLeaderChange(document.model(), change, own).ok());
    EXPECT_EQ(own.style, "") << "given: the default face";
    EXPECT_EQ(own.paperHeight, 2.0);
}

TEST(LeaderEdit, AttachingNeedsAnEntityAndTheValuesAreRowsInOrder)
{
    Document document;
    const EntityId line = add(document, Segment2{Point2(0, 0), Point2(40, 0)}, {{"size", 150.0}});
    const EntityId leader =
        add(document, LeaderGeometry{.vertices = {Point2(9, 9), Point2(20, 20)}, .text = "X"});
    EXPECT_FALSE(
        ann::attachLeader(document.model(), leader, ann::AnchoredPoint{Point2(1, 1), {}}).ok())
        << "a plain point is nothing to attach to";
    auto attach = ann::attachLeader(
        document.model(), leader,
        ann::AnchoredPoint{Point2(10, 0), AnchorRef{line, AnchorPoint::Along, 0, 0.25}});
    ASSERT_TRUE(attach.ok());
    ASSERT_TRUE(document.execute(std::move(*attach)).ok());
    EXPECT_EQ(leaderOf(document, leader).vertices.front(), Point2(10, 0));

    const auto values = ann::leaderTargetValues(document.model(), leaderOf(document, leader));
    ASSERT_TRUE(values);
    const auto rows = ann::leaderValueRows(*values);
    ASSERT_FALSE(rows.empty());
    EXPECT_EQ(rows.front().name, "id") << "leaderValueNames order";
    EXPECT_EQ(rows.back().name, "prop.size") << "then the properties";
    EXPECT_EQ(rows.back().text, "150.000");

    auto property =
        ann::setLeaderTargetProperty(document.model(), leader, "size", PropertyValue{200.0});
    ASSERT_TRUE(property.ok());
    ASSERT_TRUE(document.execute(std::move(*property)).ok());
    EXPECT_EQ(std::get<double>(document.model().entities.find(line)->properties.at("size")), 200.0);
    EXPECT_FALSE(
        ann::setLeaderTargetProperty(document.model(), leader, "", PropertyValue{1.0}).ok());
}

TEST(LeaderEdit, ANumberedBalloonIsACircleSoItsNumberIsCounted)
{
    Document document;
    const EntityId a = add(document, PointGeometry{Point2(0, 0)});
    const EntityId b = add(document, PointGeometry{Point2(10, 0)});
    const std::vector<ann::AnchoredPoint> points{ann::AnchoredPoint{Point2(0, 0), AnchorRef{a}},
                                                 ann::AnchoredPoint{Point2(5, 5), {}}};
    ann::LeaderChange box;
    box.callout = CalloutShape::Box;
    const auto refused = ann::newLeader(document.model(), points, box, true);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("circle"), std::string::npos);
    ann::LeadersForOptions options;
    options.balloon = true;
    options.change = box;
    EXPECT_FALSE(ann::leadersFor(document.model(), {a, b}, options, 1000.0, {}).ok());

    // Given a note, a balloon is not numbered, and may be any shape.
    box.note = ann::LeaderNote{ann::LeaderNote::Kind::Text, "A"};
    const auto noted = ann::newLeader(document.model(), points, box, true);
    ASSERT_TRUE(noted.ok()) << noted.error().describe();
    EXPECT_EQ(noted->callout, CalloutShape::Box);
    // A circle numbered, and the next one numbered on from it.
    const auto first = ann::newLeader(document.model(), points, {}, true);
    ASSERT_TRUE(first.ok());
    EXPECT_EQ(first->text, "1");
    add(document, *first);
    EXPECT_EQ(*ann::nextBalloonNumber(document.model()), 2);
}

TEST(LeaderEdit, AttachingATipWhereItIsIsNoCommand)
{
    Document document;
    const EntityId line = add(document, Segment2{Point2(0, 0), Point2(40, 0)});
    const EntityId leader =
        add(document, LeaderGeometry{.vertices = {Point2(9, 9), Point2(20, 20)}, .text = "X"});
    const ann::AnchoredPoint place{Point2(10, 0), AnchorRef{line, AnchorPoint::Along, 0, 0.25}};
    auto attach = ann::attachLeader(document.model(), leader, place);
    ASSERT_TRUE(attach.ok());
    ASSERT_NE(*attach, nullptr);
    ASSERT_TRUE(document.execute(std::move(*attach)).ok());
    auto again = ann::attachLeader(document.model(), leader, place);
    ASSERT_TRUE(again.ok());
    EXPECT_EQ(*again, nullptr) << "on that place already";
}
