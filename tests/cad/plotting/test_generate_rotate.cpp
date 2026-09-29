// GENERATE fit rotate=on (sheet_verbs.hpp, docs/plotting.md "Sheets on the
// command line"): the plan of the drawing, or of area=, turned to fill the
// sheet as the sheet editor's "Rotate the drawing to fill the sheet" turns it
// (smartLayoutRotated), so the Generate dialog's choice has a verb an agent
// can type. One undoable step; a refusal changes nothing.

#include <gtest/gtest.h>

#include <cmath>
#include <numbers>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plotting/arrange.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "katana/cad/selection.hpp"
#include "katana/core/text.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Vec2;
using namespace katana::cad::plotting;

namespace {

constexpr double kDegree = std::numbers::pi / 180.0;

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> "
                                << (reply.ok() ? std::string{} : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    katana::core::Error refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was accepted:\n" << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
    std::size_t steps() const { return document.history().undoCount(); }
    const SheetSet& set() const { return document.sheetSet(); }
};

// The corners of a `length` x `width` strip along `turn`, centred on `centre`.
std::vector<Point2> stripCorners(Point2 centre, double length, double width, double turn)
{
    std::vector<Point2> corners;
    for (const Vec2 corner : {Vec2(-length / 2, -width / 2), Vec2(length / 2, -width / 2),
                              Vec2(length / 2, width / 2), Vec2(-length / 2, width / 2)}) {
        corners.push_back(centre + corner.rotated(turn));
    }
    return corners;
}

std::string pointText(const Point2& point)
{
    return katana::core::formatExactReal(point.x) + "," + katana::core::formatExactReal(point.y);
}

// The strip drawn as its four sides, typed.
void drawStrip(Session& session, const std::vector<Point2>& corners)
{
    for (std::size_t i = 0; i < corners.size(); ++i) {
        session.ok("LINE " + pointText(corners[i]) + " " +
                   pointText(corners[(i + 1) % corners.size()]));
    }
}

const Viewport& plan(const Sheet& sheet)
{
    for (const Viewport& viewport : sheet.viewports) {
        if (viewport.kind == ViewportKind::Plan) {
            return viewport;
        }
    }
    ADD_FAILURE() << "the sheet has no plan";
    return sheet.viewports.front();
}

// A 450 x 20 m strip along 45 degrees, alone on an A3 sheet, whose tiling
// area is 385 x 250 mm (tilingArea of the built-in frame). Square to the
// paper it spans 332 m each way, which needs 1 : 1328 of the 250 mm, so
// 1 : 2000 on the ladder. Turned, the ladder's 1 : 1250 holds 481 x 312.5 m;
// the least turn that brings the strip's far corner (225, 10) from its centre
// within 156.25 m of the middle line is 45 degrees less the corner's
// direction, atan2(225, 10), plus the angle whose cosine is 156.25 over the
// corner's distance - the calculation the editor's own test makes
// (tests/qt_widgets/plotting/test_sheet_arrange.cpp).
const double kTurn =
    45.0 * kDegree - std::atan2(225.0, 10.0) + std::acos(156.25 / std::hypot(225.0, 10.0));

} // namespace

TEST(GenerateRotate, TheFittedDrawingIsTurnedToFillTheSheetInOneStep)
{
    Session s;
    drawStrip(s, stripCorners(Point2(0.0, 0.0), 450.0, 20.0, 45.0 * kDegree));
    const std::size_t steps = s.steps();

    EXPECT_EQ(s.ok("GENERATE fit rotate=off").substr(0, 20), "generated 1 sheet: 1");
    ASSERT_EQ(s.set().sheets.size(), 1u);
    EXPECT_EQ(plan(s.set().sheets[0]).scale, 2000.0);
    EXPECT_EQ(plan(s.set().sheets[0]).rotation, 0.0);

    const std::string turned = s.ok("GENERATE fit rotate=on");
    EXPECT_EQ(turned.substr(0, 20), "generated 1 sheet: 2");
    ASSERT_EQ(s.set().sheets.size(), 2u);
    const Viewport& view = plan(s.set().sheets[1]);
    EXPECT_EQ(view.scale, 1250.0);
    EXPECT_NEAR(view.rotation, kTurn, 1e-9);
    EXPECT_NE(turned.find("kind=plan scale=1250 rect="), std::string::npos) << turned;
    EXPECT_EQ(s.steps(), steps + 2);
    EXPECT_EQ(s.document.history().undoName(), "GENERATE_SHEETS");

    // What the dialog's "Rotate" does is what the editor always did: the
    // same function over the drawing's outline.
    LayoutRequest request;
    request.planArea = katana::cad::drawnExtent(s.document.model(), {});
    auto expected = smartLayoutRotated(s.document.model(), request,
                                       drawnOutline(s.document.model(), {}));
    ASSERT_TRUE(expected.ok()) << expected.error().describe();
    SheetSet firstOnly;
    firstOnly.sheets = {s.set().sheets[0]};
    prepareForAppend(firstOnly, *expected);
    EXPECT_EQ(std::vector<Sheet>{s.set().sheets[1]}, *expected);

    ASSERT_TRUE(s.document.undo().ok());
    EXPECT_EQ(s.set().sheets.size(), 1u);
}

TEST(GenerateRotate, AnAreaIsTurnedWhenItFitsThePaperBetterThatWay)
{
    // A 100 x 300 m area on the 385 x 250 mm tiling area. Square, its 300 m
    // go up the 250 mm: 1 : 1200 or more, 1 : 1250 on the ladder. Turned by
    // t its height is 100 sin t + 300 cos t = hypot(100, 300) cos(t - a),
    // a = atan2(100, 300); at 1 : 1000 that must be 250 m or less, which it
    // first is at t = a + acos(250 / hypot(100, 300)), 56.2 degrees, where
    // its width, 305 m, is well inside the 385. 1 : 750 cannot be reached:
    // the height is 187.5 m or less only from 72.1 degrees, where the width
    // is 316 m, past the 288.75 m 1 : 750 allows. So the turn is the least
    // one that reaches 1 : 1000, not a quarter.
    Session s;
    s.ok("LINE 0,0 1,1");
    s.ok("GENERATE fit area=0,0,100,300");
    s.ok("GENERATE fit area=0,0,100,300 rotate=on");
    ASSERT_EQ(s.set().sheets.size(), 2u);
    const Viewport& square = plan(s.set().sheets[0]);
    const Viewport& turned = plan(s.set().sheets[1]);
    EXPECT_EQ(square.rotation, 0.0);
    EXPECT_EQ(square.scale, 1250.0);
    EXPECT_EQ(turned.scale, 1000.0);
    EXPECT_NEAR(std::abs(turned.rotation),
                std::atan2(100.0, 300.0) + std::acos(250.0 / std::hypot(100.0, 300.0)), 1e-9);
}

TEST(GenerateRotate, AStraightLineIsTurnedAcrossTheSheet)
{
    // One 300 m line along the x axis. Square it goes across the 385 mm:
    // 1 : 780 or more, 1 : 1000 on the ladder. Turned by t it needs
    // 300 cos t across and 300 sin t up; at 1 : 750 those are 288.75 m and
    // 187.5 m, both met from t = acos(288.75 / 300), 15.7 degrees, to 38.7.
    Session s;
    s.ok("LINE 0,0 300,0");
    s.ok("GENERATE fit");
    s.ok("GENERATE fit rotate=on");
    ASSERT_EQ(s.set().sheets.size(), 2u);
    EXPECT_EQ(plan(s.set().sheets[0]).scale, 1000.0);
    EXPECT_EQ(plan(s.set().sheets[1]).scale, 750.0);
    EXPECT_NEAR(std::abs(plan(s.set().sheets[1]).rotation), std::acos(288.75 / 300.0), 1e-9);
}

TEST(GenerateRotate, TheFrontEndsContentIsWhatIsTurned)
{
    // The window draws a strip of imagery the document knows nothing of; the
    // plan is turned to it, as the editor turns the plan to what it draws.
    Document document;
    CommandInterpreter interpreter(document);
    const std::vector<Point2> corners = stripCorners(Point2(5000.0, 7000.0), 450.0, 20.0, 45.0 * kDegree);
    int asked = 0;
    interpreter.setSheetContext([&] {
        ++asked;
        SheetVerbContext context;
        Box2 extent;
        for (const Point2& corner : corners) {
            extent.expand(corner);
        }
        context.drawingExtent = extent;
        context.content = [&corners](const Viewport&) { return corners; };
        return context;
    });
    const auto reply = interpreter.run("GENERATE fit rotate=on");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    ASSERT_EQ(document.sheetSet().sheets.size(), 1u);
    const Viewport& view = plan(document.sheetSet().sheets[0]);
    EXPECT_EQ(view.scale, 1250.0);
    EXPECT_NEAR(view.rotation, kTurn, 1e-9);
    EXPECT_NEAR(view.centre.x, 5000.0, 1e-6);
    EXPECT_NEAR(view.centre.y, 7000.0, 1e-6);
    EXPECT_GE(asked, 1);
}

TEST(GenerateRotate, WhatCannotBeTurnedIsRefusedAndChangesNothing)
{
    Session s;
    s.ok("ALIGN NEW ROAD 0,0 1000,0");
    const std::size_t steps = s.steps();
    // A plan along an alignment already follows it.
    EXPECT_EQ(s.refused("GENERATE fit alignment=ROAD rotate=on").message,
              "turning the drawing to fill the sheet needs a plan of an area");
    // Nor are sections after the plan turned with it.
    EXPECT_EQ(s.refused("GENERATE fit area=0,0,100,100 interval=20 rotate=on").message,
              "only a plan of an area is turned to fill the sheet; a strip already follows its "
              "alignment");
    EXPECT_EQ(s.refused("GENERATE fit rotate=sideways").message, "rotate= is on or off");
    EXPECT_EQ(s.refused("GENERATE grid scale=500 rotate=on").message,
              "GENERATE GRID takes no option rotate=; it takes paper orientation frame area "
              "scale overlap keyplan replace");
    EXPECT_TRUE(s.set().sheets.empty());
    EXPECT_EQ(s.steps(), steps);
    // An empty drawing has nothing to turn, or to lay out.
    Session empty;
    EXPECT_NE(empty.refused("GENERATE fit rotate=on").message.find("the drawing is empty"),
              std::string::npos);
}

TEST(GenerateRotate, ContentThatIsOnePointIsLeftSquare)
{
    // What the front end says the plan shows is one point: it has no size
    // to turn, so the layout is the square one over the extent.
    Document document;
    CommandInterpreter interpreter(document);
    interpreter.setSheetContext([] {
        SheetVerbContext context;
        context.drawingExtent = Box2(Point2(0.0, 0.0), Point2(300.0, 100.0));
        context.content = [](const Viewport&) { return std::vector<Point2>{Point2(5.0, 5.0)}; };
        return context;
    });
    ASSERT_TRUE(interpreter.run("GENERATE fit rotate=on").ok());
    ASSERT_TRUE(interpreter.run("GENERATE fit").ok());
    const SheetSet& set = document.sheetSet();
    ASSERT_EQ(set.sheets.size(), 2u);
    EXPECT_EQ(plan(set.sheets[0]).rotation, 0.0);
    EXPECT_EQ(plan(set.sheets[0]).scale, plan(set.sheets[1]).scale);
    EXPECT_EQ(plan(set.sheets[0]).centre, plan(set.sheets[1]).centre);
}

TEST(GenerateRotate, TheHelpNamesTheOption)
{
    EXPECT_NE(sheetVerbHelp().find("rotate=on"), std::string::npos);
}
