// The dimension and leader tools' options and their associative points:
// the Leader's Arrow, Callout, Style, Paper, SIze and Landing, the Balloon
// tool with its Number, Style and Paper, the Linear
// Dimension's Rotated option and its dimension line between the origins'
// levels, the Ordinate's Datum, and points snapped to an entity's end,
// middle, centre or vertex, which the annotation made from them then
// follows. Each tool is held to the verb line that makes the same thing
// (LEADER, BALLOON, DIM LINEAR, DIM ORDINATE), typed into a second drawing
// made the same way; every other expected value is worked out by hand
// beside it.

#include <cmath>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"
#include "tool_driver.hpp"

namespace {

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::SnapMode;
using katana::cad::SnapResult;
using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::AnchorPoint;
using katana::entity::AnchorRef;
using katana::entity::ArrowHead;
using katana::entity::CalloutShape;
using katana::entity::DimensionGeometry;
using katana::entity::DimensionKind;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;
using Outcome = katana::cad::ToolStep::Outcome;

void run(Document& document, const std::string& line)
{
    CommandInterpreter interpreter(document);
    const auto reply = interpreter.run(line);
    ASSERT_TRUE(reply.ok()) << line << "\n  -> " << reply.error().describe();
}

template <class Geometry>
std::vector<Entity> entitiesOf(Document& document)
{
    std::vector<Entity> out;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<Geometry>(entity.geometry)) {
            out.push_back(entity);
        }
    });
    return out;
}

template <class Geometry>
Geometry newest(Document& document)
{
    const auto all = entitiesOf<Geometry>(document);
    EXPECT_FALSE(all.empty());
    return all.empty() ? Geometry{} : std::get<Geometry>(all.back().geometry);
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// The one entity of each drawing of kind Geometry is the same entity: its
// geometry, layer, style and colour.
template <class Geometry>
void expectSame(Document& drawn, Document& typed)
{
    const auto made = entitiesOf<Geometry>(drawn);
    const auto expected = entitiesOf<Geometry>(typed);
    ASSERT_EQ(made.size(), expected.size());
    for (std::size_t i = 0; i < made.size(); ++i) {
        EXPECT_EQ(made[i].geometry, expected[i].geometry) << i;
        EXPECT_EQ(made[i].layer, expected[i].layer) << i;
        EXPECT_EQ(made[i].style, expected[i].style) << i;
        EXPECT_EQ(made[i].color, expected[i].color) << i;
    }
}

SnapResult snapped(Point2 point, SnapMode mode, EntityId entity)
{
    SnapResult snap;
    snap.point = point;
    snap.mode = mode;
    snap.entity = entity;
    return snap;
}

} // namespace

// ---- which point of an entity a snap names ----------------------------------------------

TEST(SnapAnchor, AnEndAMiddleACentreOrAVertexIsNamedAndNothingElseIs)
{
    Document document;
    run(document, "LINE 0,0 10,0");                 // 1
    run(document, "CIRCLE 20,0 5");                 // 2
    run(document, "PLINE 0,10 10,10 10,20");        // 3
    run(document, "POINT 30,30");                   // 4
    const auto& model = document.model();
    const auto name = [&](Point2 at, SnapMode mode, EntityId id) {
        return katana::cad::snapAnchor(model, snapped(at, mode, id));
    };
    EXPECT_EQ(name({0, 0}, SnapMode::Endpoint, 1), (AnchorRef{1, AnchorPoint::Start, 0}));
    EXPECT_EQ(name({10, 0}, SnapMode::Endpoint, 1), (AnchorRef{1, AnchorPoint::End, 0}));
    EXPECT_EQ(name({5, 0}, SnapMode::Midpoint, 1), (AnchorRef{1, AnchorPoint::Mid, 0}));
    EXPECT_EQ(name({20, 0}, SnapMode::Center, 2), (AnchorRef{2, AnchorPoint::Centre, 0}));
    // A polyline's corner by its vertex, the middle of its second segment
    // (10,10 -> 10,20) by that segment.
    EXPECT_EQ(name({10, 10}, SnapMode::Endpoint, 3), (AnchorRef{3, AnchorPoint::Vertex, 1}));
    EXPECT_EQ(name({10, 15}, SnapMode::Midpoint, 3), (AnchorRef{3, AnchorPoint::SegmentMid, 1}));
    EXPECT_EQ(name({30, 30}, SnapMode::Endpoint, 4), (AnchorRef{4, AnchorPoint::Position, 0}));
    // Somewhere along a line, where two lines cross, the grid: no point of
    // an entity that it could follow.
    EXPECT_FALSE(name({3, 0}, SnapMode::Nearest, 1));
    EXPECT_FALSE(name({0, 0}, SnapMode::Intersection, 1));
    EXPECT_FALSE(name({0, 0}, SnapMode::Grid, 0));
    // A snap that names a point the entity does not have there, or an
    // entity that is gone.
    EXPECT_FALSE(name({4, 0}, SnapMode::Endpoint, 1));
    EXPECT_FALSE(name({0, 0}, SnapMode::Endpoint, 99));
}

// ---- Leader options ----------------------------------------------------------------------

TEST(LeaderOptions, TheArrowCalloutStyleAndPaperAreTheLeaderVerbsOwn)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    EXPECT_EQ(driver.tool().prompt(),
              "Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing]");
    (void)driver.type("A");
    EXPECT_EQ(driver.tool().prompt(), "Enter arrowhead [Closed/Open/Tick/Dot/None] <Closed>");
    EXPECT_EQ(driver.type("zigzag").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("o").outcome, Outcome::Continue);
    (void)driver.type("C");
    EXPECT_EQ(driver.tool().prompt(), "Enter callout [None/Box/Circle] <None>");
    (void)driver.type("Box");
    (void)driver.type("S");
    EXPECT_EQ(driver.type("Nowhere").outcome, Outcome::Rejected) << "no such text style";
    (void)driver.type("Standard");
    (void)driver.type("P");
    EXPECT_EQ(driver.tool().prompt(), "Enter the note's height on paper, mm <2.5>");
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected);
    (void)driver.type("3.5");
    EXPECT_EQ(driver.tool().prompt(),
              "Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing] (Open "
              "arrow, Box callout, style Standard, 3.5 mm)");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    (void)driver.enter();
    (void)driver.type("AB");
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);

    // The Standard dimension style's arrow, 2.5 model units at 1 : 1000, is
    // 2.5 mm; the last segment rises 36.9 degrees, more than 15, so the
    // landing is one arrowhead long.
    Document typed;
    run(typed, "LEADER 0,0 8,6 text=AB arrow=open callout=box style=Standard paper=3.5 "
               "arrowsize=2.5 landing=2.5");
    expectSame<LeaderGeometry>(driver.document(), typed);

    // Only that leader: the next takes the dimension style's again.
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    (void)driver.enter();
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    const LeaderGeometry next = newest<LeaderGeometry>(driver.document());
    EXPECT_EQ(next.arrow, ArrowHead::ClosedFilled);
    EXPECT_EQ(next.callout, CalloutShape::None);
    EXPECT_EQ(next.paperHeight, 2.5);
    EXPECT_TRUE(next.style.empty());
}

TEST(LeaderOptions, EnterAtAnOptionKeepsWhatItHasAndUndoLeavesIt)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.type("A");
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    (void)driver.type("A");
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(),
              "Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing]");
    EXPECT_EQ(driver.type("-2").outcome, Outcome::Rejected) << "not an option, not a point";
    EXPECT_EQ(driver.executed(), 0);
}

// SIze and Landing are LEADER's arrowsize= and landing=, which the tool once
// had no way to give: only the verb could, and nothing after the leader was
// drawn. A landing given is drawn as given, on a level leader too; Auto
// gives the rule back - one arrowhead where the last segment slopes.
TEST(LeaderOptions, SizeAndLandingAreTheLeaderVerbsArrowsizeAndLanding)
{
    ToolDriver driver;
    driver.start("annotate.leader");
    (void)driver.type("S");
    EXPECT_EQ(driver.tool().prompt(), "Enter text style name <Standard>") << "S is Style's";
    (void)driver.enter();
    (void)driver.type("si");
    EXPECT_EQ(driver.tool().prompt(), "Enter the arrowhead's size on paper, mm <2.5>");
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("big").outcome, Outcome::Rejected);
    (void)driver.type("4");
    (void)driver.type("L");
    EXPECT_EQ(driver.tool().prompt(),
              "Enter the landing's length on paper, mm, 0 for none, or Auto <Auto>");
    EXPECT_EQ(driver.type("-1").outcome, Outcome::Rejected);
    (void)driver.type("5");
    EXPECT_EQ(driver.tool().prompt(),
              "Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing] "
              "(arrowhead 4 mm, landing 5 mm)");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(10.0, 0.0);
    (void)driver.enter();
    (void)driver.type("AB");
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);

    // The Standard dimension style's closed arrow; its 2.5 model-unit text
    // height is 2.5 mm at 1 : 1000.
    Document typed;
    run(typed, "LEADER 0,0 10,0 text=AB arrow=closed paper=2.5 arrowsize=4 landing=5");
    expectSame<LeaderGeometry>(driver.document(), typed);

    // Auto: the rule again. The last segment rises at atan(6/8) = 36.9
    // degrees, more than 15, so the landing is one arrowhead: the 4 given.
    (void)driver.type("SI");
    (void)driver.type("4");
    (void)driver.type("L");
    (void)driver.type("5");
    (void)driver.type("L");
    EXPECT_EQ(driver.tool().prompt(),
              "Enter the landing's length on paper, mm, 0 for none, or Auto <5>");
    (void)driver.type("auto");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    (void)driver.enter();
    (void)driver.type("CD");
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    const LeaderGeometry sloped = newest<LeaderGeometry>(driver.document());
    EXPECT_EQ(sloped.arrowSize, 4.0);
    EXPECT_EQ(sloped.landing, 4.0);
}

// ---- Balloon -------------------------------------------------------------------------------

TEST(Balloon, ItIsTheBalloonVerbsNumberedCircleCountingOnFromTheHighest)
{
    ToolDriver driver;
    run(driver.document(), "BALLOON 0,0 5,5");
    driver.start("annotate.balloon");
    EXPECT_EQ(driver.tool().prompt(), "Specify the balloon's arrow point or [Number/Style/Paper]");
    (void)driver.click(10.0, 0.0);
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected) << "a balloon needs its circle's place";
    (void)driver.click(15.0, 5.0);
    const ToolStep done = driver.enter();
    ASSERT_EQ(done.outcome, Outcome::Done);
    EXPECT_EQ(done.message, "balloon 2");

    Document typed;
    run(typed, "BALLOON 0,0 5,5");
    run(typed, "BALLOON 10,0 15,5");
    expectSame<LeaderGeometry>(driver.document(), typed);
    const LeaderGeometry balloon = newest<LeaderGeometry>(driver.document());
    EXPECT_EQ(balloon.callout, CalloutShape::Circle);
    EXPECT_EQ(balloon.text, "2");
}

TEST(Balloon, NumberGivesAnotherAndTheNextCountsOnFromTheHighestWholeNumber)
{
    ToolDriver driver;
    driver.start("annotate.balloon");
    (void)driver.type("N");
    EXPECT_EQ(driver.tool().prompt(), "Enter the balloon's number <1>");
    (void)driver.type("7");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(5.0, 5.0);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    (void)driver.click(10.0, 0.0);
    (void)driver.type("N");
    (void)driver.type("A1"); // not a whole number: counts for nothing
    (void)driver.click(15.0, 5.0);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    (void)driver.click(20.0, 0.0);
    (void)driver.click(25.0, 5.0);
    const ToolStep third = driver.enter();
    ASSERT_EQ(third.outcome, Outcome::Done);
    EXPECT_EQ(third.message, "balloon 8") << "one more than 7; A1 is not counted";
    EXPECT_EQ(driver.enter().outcome, Outcome::Done) << "Enter at the first prompt ends it";
    EXPECT_TRUE(driver.finished());
}

// Style and Paper are BALLOON's style= and paper=, which the tool once had
// no way to give: a balloon in a text style, at a height on paper, made by
// the tool is the entity the verb makes. The style is found ignoring case,
// as the Leader's is.
TEST(Balloon, StyleAndPaperAreTheBalloonVerbsOwn)
{
    ToolDriver driver;
    run(driver.document(), "TEXTSTYLE NEW Notes");
    driver.start("annotate.balloon");
    (void)driver.type("S");
    EXPECT_EQ(driver.tool().prompt(), "Enter text style name <Standard>");
    EXPECT_EQ(driver.type("Nowhere").outcome, Outcome::Rejected) << "no such text style";
    (void)driver.type("notes");
    (void)driver.click(0.0, 0.0);
    (void)driver.type("P");
    EXPECT_EQ(driver.tool().prompt(), "Enter the number's height on paper, mm <the style's>");
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.click(3.0, 3.0).outcome, Outcome::Rejected) << "a height is asked for";
    (void)driver.type("3.5");
    (void)driver.click(5.0, 5.0);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);

    Document typed;
    run(typed, "TEXTSTYLE NEW Notes");
    run(typed, "BALLOON 0,0 5,5 style=Notes paper=3.5");
    expectSame<LeaderGeometry>(driver.document(), typed);
    const LeaderGeometry balloon = newest<LeaderGeometry>(driver.document());
    EXPECT_EQ(balloon.style, "Notes");
    EXPECT_EQ(balloon.paperHeight, 3.5);
}

// ---- Linear: Rotated, and a line between the levels ---------------------------------------

TEST(LinearRotated, ATypedAngleMakesTheLinearKindDimLinearMakes)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(10.0, 5.0);
    EXPECT_TRUE(contains(driver.tool().prompt(), "[Text/Horizontal/Vertical/Rotated/Undo]"))
        << driver.tool().prompt();
    (void)driver.type("R");
    EXPECT_EQ(driver.tool().prompt(), "Specify angle of dimension line or two points on it <0>");
    EXPECT_EQ(driver.type("30").outcome, Outcome::Continue);
    EXPECT_TRUE(contains(driver.tool().prompt(), "rotated 30 degrees")) << driver.tool().prompt();
    const ToolStep done = driver.click(2.0, 8.0);
    ASSERT_EQ(done.outcome, Outcome::Done) << done.message;

    Document typed;
    run(typed, "DIM LINEAR 0,0 10,5 at=2,8 angle=30");
    expectSame<DimensionGeometry>(driver.document(), typed);
    const DimensionGeometry dimension = newest<DimensionGeometry>(driver.document());
    EXPECT_EQ(dimension.kind, DimensionKind::Linear);
    // Along 30 degrees: 10 cos 30 + 5 sin 30 = 8.660254... + 2.5.
    EXPECT_NEAR(dimension.measurement(), 10.0 * std::sqrt(3.0) / 2.0 + 2.5, 1e-12);
}

TEST(LinearRotated, TwoPointsGiveTheAngleAndUndoTakesItBack)
{
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(10.0, 5.0);
    (void)driver.type("Rotated");
    (void)driver.click(1.0, 1.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify second point");
    EXPECT_EQ(driver.click(1.0, 1.0).outcome, Outcome::Rejected) << "two points, one place";
    (void)driver.click(3.0, 3.0); // 45 degrees
    EXPECT_TRUE(contains(driver.tool().prompt(), "rotated 45 degrees")) << driver.tool().prompt();
    (void)driver.undo(); // the angle
    EXPECT_FALSE(contains(driver.tool().prompt(), "rotated")) << driver.tool().prompt();
    (void)driver.type("R");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(4.0, 4.0);
    ASSERT_EQ(driver.click(-5.0, 10.0).outcome, Outcome::Done);
    const DimensionGeometry dimension = newest<DimensionGeometry>(driver.document());
    EXPECT_EQ(dimension.kind, DimensionKind::Linear);
    EXPECT_NEAR(dimension.angle, 0.25 * katana::math::kPi, 1e-15);
    // Along 45 degrees: (10 + 5) / sqrt 2.
    EXPECT_NEAR(dimension.measurement(), 15.0 / std::sqrt(2.0), 1e-12);
}

TEST(LinearRotated, ALineBetweenTheOriginsLevelsIsTheLinearKindDimHorizontalMakes)
{
    // Once refused: the aligned projection the tool stores cannot draw a
    // line between the two levels, whose extension lines leave in opposite
    // directions. A Linear-kind dimension draws each from its own origin, as
    // AutoCAD's DIMLINEAR does, so that is what is made there.
    ToolDriver driver;
    driver.start("annotate.dimlinear");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 6.0);
    (void)driver.type("H");
    const ToolStep done = driver.click(12.0, 3.0);
    ASSERT_EQ(done.outcome, Outcome::Done) << done.message;
    EXPECT_EQ(done.message, "linear dimension measuring 8");
    Document typed;
    run(typed, "DIM HORIZONTAL 0,0 8,6 at=12,3");
    expectSame<DimensionGeometry>(driver.document(), typed);
    EXPECT_EQ(newest<DimensionGeometry>(driver.document()).kind, DimensionKind::Linear);
}

// ---- Ordinate: Datum -----------------------------------------------------------------------

TEST(OrdinateDatum, ADatumMeasuresFromItAsDimOrdinateDatumDoesAndIsKeptForTheNext)
{
    ToolDriver driver;
    driver.start("annotate.dimordinate");
    EXPECT_EQ(driver.tool().prompt(), "Specify feature location or [Datum]");
    (void)driver.type("D");
    EXPECT_EQ(driver.tool().prompt(), "Specify the datum point <0,0>");
    EXPECT_EQ(driver.type("100,200").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify feature location or [Datum] (datum 100,200)");
    (void)driver.click(112.0, 207.0);
    const ToolStep up = driver.click(112.0, 215.0);
    ASSERT_EQ(up.outcome, Outcome::Done);
    EXPECT_EQ(up.message, "X ordinate dimension measuring 12");
    Document typed;
    run(typed, "DIM ORDINATE 112,207 at=112,215 datum=100,200");
    expectSame<DimensionGeometry>(driver.document(), typed);

    // Started again, it measures from the newest ordinate's datum: the
    // drawing keeps it, not the program.
    EXPECT_EQ(driver.tool().prompt(), "Specify feature location or [Datum] (datum 100,200)");
    (void)driver.click(130.0, 210.0);
    const ToolStep across = driver.click(140.0, 210.0);
    EXPECT_EQ(across.message, "Y ordinate dimension measuring 10");
    // Enter at the datum prompt keeps it; 0,0 goes back to the origin.
    (void)driver.type("D");
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify feature location or [Datum] (datum 100,200)");
    (void)driver.type("D");
    (void)driver.type("0,0");
    EXPECT_EQ(driver.tool().prompt(), "Specify feature location or [Datum]");
}

// ---- associative points ------------------------------------------------------------------

TEST(AssociativePoints, AnAlignedDimensionSnappedToALinesEndsFollowsTheLine)
{
    ToolDriver driver;
    run(driver.document(), "LINE 0,0 10,0"); // 1
    driver.start("annotate.dimaligned");
    (void)driver.clickSnapped(0.1, 0.1);  // its start, 0.14 away
    (void)driver.clickSnapped(9.9, -0.1); // its end
    ASSERT_EQ(driver.click(5.0, 4.0).outcome, Outcome::Done);
    const DimensionGeometry made = newest<DimensionGeometry>(driver.document());
    EXPECT_EQ(made.start, Point2(0.0, 0.0));
    EXPECT_EQ(made.end, Point2(10.0, 0.0));
    EXPECT_EQ(made.startRef, (AnchorRef{1, AnchorPoint::Start, 0}));
    EXPECT_EQ(made.endRef, (AnchorRef{1, AnchorPoint::End, 0}));
    // As DIM ALIGNED #1.start #1.end at=5,4.
    Document typed;
    run(typed, "LINE 0,0 10,0");
    run(typed, "DIM ALIGNED #1.start #1.end at=5,4");
    expectSame<DimensionGeometry>(driver.document(), typed);

    // Moved 3 up, the dimension goes with it.
    run(driver.document(), "SELECT 1");
    run(driver.document(), "MOVE 0,3");
    const DimensionGeometry followed = newest<DimensionGeometry>(driver.document());
    EXPECT_EQ(followed.start, Point2(0.0, 3.0));
    EXPECT_EQ(followed.end, Point2(10.0, 3.0));
    // A click with nothing to snap to is a plain point, as before.
    driver.start("annotate.dimaligned");
    (void)driver.clickSnapped(50.0, 50.0);
    (void)driver.clickSnapped(60.0, 50.0);
    ASSERT_EQ(driver.click(55.0, 55.0).outcome, Outcome::Done);
    EXPECT_FALSE(newest<DimensionGeometry>(driver.document()).startRef.associated());
}

TEST(AssociativePoints, ASnappedLinearDimensionIsTheLinearKindAndFollows)
{
    // The aligned projection's start and end are not the origins, so it
    // could not follow them; a snapped origin makes the Linear kind, which
    // measures from the origins themselves.
    ToolDriver driver;
    run(driver.document(), "LINE 0,0 8,6"); // 1
    driver.start("annotate.dimlinear");
    (void)driver.clickSnapped(0.0, 0.1);
    (void)driver.clickSnapped(8.0, 6.1);
    ASSERT_EQ(driver.click(4.0, 10.0).outcome, Outcome::Done);
    Document typed;
    run(typed, "LINE 0,0 8,6");
    run(typed, "DIM HORIZONTAL #1.start #1.end at=4,10");
    expectSame<DimensionGeometry>(driver.document(), typed);
    run(driver.document(), "SELECT 1");
    run(driver.document(), "MOVE 2,0");
    const DimensionGeometry followed = newest<DimensionGeometry>(driver.document());
    EXPECT_EQ(followed.start, Point2(2.0, 0.0));
    EXPECT_EQ(followed.measurement(), 8.0);
}

TEST(AssociativePoints, AnOrdinateAnAngleAndALeaderTipFollowWhatTheyWereSnappedTo)
{
    ToolDriver driver;
    run(driver.document(), "LINE 0,0 10,0");  // 1
    run(driver.document(), "LINE 0,0 0,10");  // 2
    run(driver.document(), "CIRCLE 20,20 2"); // 3

    // An ordinate of the circle's centre.
    driver.start("annotate.dimordinate");
    (void)driver.clickSnapped(20.1, 19.9);
    ASSERT_EQ(driver.click(20.0, 30.0).outcome, Outcome::Done);
    EXPECT_EQ(newest<DimensionGeometry>(driver.document()).startRef,
              (AnchorRef{3, AnchorPoint::Centre, 0}));

    // An angle at the shared corner between the two lines' far ends.
    driver.start("annotate.dimangular");
    (void)driver.enter();
    (void)driver.clickSnapped(0.1, 0.1);   // line 1's start
    (void)driver.clickSnapped(9.9, 0.1);   // line 1's end
    (void)driver.clickSnapped(0.1, 9.9);   // line 2's end
    ASSERT_EQ(driver.click(3.0, 3.0).outcome, Outcome::Done);
    const DimensionGeometry angle = newest<DimensionGeometry>(driver.document());
    EXPECT_EQ(angle.vertexRef.entity, 1u);
    EXPECT_EQ(angle.startRef, (AnchorRef{1, AnchorPoint::End, 0}));
    EXPECT_EQ(angle.endRef, (AnchorRef{2, AnchorPoint::End, 0}));

    // A leader whose tip is on line 2's middle.
    driver.start("annotate.leader");
    (void)driver.clickSnapped(0.1, 5.1);
    (void)driver.click(-5.0, 8.0);
    (void)driver.enter();
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(newest<LeaderGeometry>(driver.document()).tipRef,
              (AnchorRef{2, AnchorPoint::Mid, 0}));

    // Everything moves with what it was snapped to.
    run(driver.document(), "SELECT ALL");
    run(driver.document(), "SELECT 1 2 3");
    run(driver.document(), "MOVE 100,0");
    EXPECT_EQ(newest<DimensionGeometry>(driver.document()).vertex, Point2(100.0, 0.0));
    EXPECT_EQ(newest<LeaderGeometry>(driver.document()).vertices.front(), Point2(100.0, 5.0));
}
