// SURFACE EDIT, MERGE, PASTE, CLIP and OFFSET through the command interpreter
// (surface_edit_verbs.hpp, docs/surface_editing.md): the drawing's surfaces
// edited as one undo step each, the replies, and every refusal.
#include <gtest/gtest.h>

#include <memory>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/terrain/surface_edit_verbs.hpp"
#include "katana/commands/entity_commands.hpp"
#include "surface_edit_support.hpp"

using namespace katana::cad;
using namespace katana::cad::testing;
using katana::core::ErrorCode;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
namespace cmd = katana::commands;

namespace {

struct SurfaceEditVerbs : ::testing::Test {
    Document document;
    CommandInterpreter interpreter{document};

    std::string reply(const std::string& line)
    {
        auto result = interpreter.run(line);
        EXPECT_TRUE(result.ok()) << line << ": " << (result.ok() ? "" : result.error().describe());
        return result.ok() ? *result : std::string();
    }

    ErrorCode refusal(const std::string& line)
    {
        auto result = interpreter.run(line);
        EXPECT_FALSE(result.ok()) << line << " was taken: " << (result.ok() ? *result : "");
        return result.ok() ? ErrorCode::Internal : result.error().code;
    }

    EntityId polyline(std::vector<Point2> vertices, bool closed, const std::string& layer = "0")
    {
        Polyline2 line;
        line.vertices = std::move(vertices);
        line.closed = closed;
        EXPECT_TRUE(document.execute(cmd::createPolyline(line, {layer, "", {}})).ok());
        const auto created = document.lastCreatedEntities();
        return created.empty() ? 0 : created.front();
    }
};

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

TEST_F(SurfaceEditVerbs, AddingAPointIsOneUndoStepAndSaysWhatChanged)
{
    putSurface(document, "ground", squareSurface());
    const TinSurface before = surfaceOf(document, "ground");
    const std::string text = reply("SURFACE EDIT ground ADD POINT 5,5,10");
    EXPECT_TRUE(contains(text, "surface name=ground operation=add_point triangles_before=2 "
                               "triangles_after=4 vertices_before=4 vertices_after=5 "
                               "points_added=1"))
        << text;
    EXPECT_TRUE(contains(text, "changed=yes")) << text;
    EXPECT_EQ(surfaceOf(document, "ground").triangleCount(), 4u);
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(5, 5), 10.0);

    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_EQ(surfaceOf(document, "ground"), before);
    ASSERT_TRUE(interpreter.run("REDO").ok());
    EXPECT_EQ(surfaceOf(document, "ground").triangleCount(), 4u);
}

TEST_F(SurfaceEditVerbs, APreviewWorksTheEditOutAndChangesNothing)
{
    putSurface(document, "ground", squareSurface());
    const auto generation = document.surfacesGeneration();
    const std::string text = reply("SURFACE EDIT ground ADD POINT 5,5,10 PREVIEW");
    EXPECT_TRUE(contains(text, "triangles_after=4")) << text;
    EXPECT_TRUE(contains(text, "preview=yes")) << text;
    EXPECT_EQ(surfaceOf(document, "ground").triangleCount(), 2u);
    EXPECT_EQ(document.surfacesGeneration(), generation);
}

TEST_F(SurfaceEditVerbs, AnEditThatChangesNothingIsNoStep)
{
    // Unit cells have no side longer than sqrt 2 < 1.5: nothing to delete.
    putSurface(document, "ground", flatGridSurface(4, 1.0));
    const auto generation = document.surfacesGeneration();
    const std::string text = reply("SURFACE EDIT ground DELETE LONG 1.5");
    EXPECT_TRUE(contains(text, "changed=no")) << text;
    EXPECT_EQ(document.surfacesGeneration(), generation);
}

TEST_F(SurfaceEditVerbs, FlipTakesTheEdgeNearestThePoint)
{
    // Corners 1 and 3 at 1: after the flip the centre lies on the diagonal
    // between them, at 1.
    putSurface(document, "ground", squareSurface(0, 1, 0, 1));
    reply("SURFACE EDIT ground FLIP 5,5");
    EXPECT_NEAR(*surfaceOf(document, "ground").elevationAt(5, 5), 1.0, 1e-12);
}

TEST_F(SurfaceEditVerbs, AHoleFromAClosedPolylineByIdRemovesItsArea)
{
    // A 10 x 10 grid of unit cells less [3,6] x [3,7]: 100 - 12 = 88.
    putSurface(document, "ground", flatGridSurface(10, 1.0, 4.0));
    const EntityId id =
        polyline({Point2(3, 3), Point2(6, 3), Point2(6, 7), Point2(3, 7)}, true);
    reply("SURFACE EDIT ground HOLE #" + std::to_string(id));
    EXPECT_NEAR(surfaceOf(document, "ground").planArea(), 88.0, 1e-9);
}

TEST_F(SurfaceEditVerbs, FromWithNoScopeWordIsTheSelectionAsTheScopeGrammarHasIt)
{
    // The same hole, the polyline taken from the selection: 88.
    putSurface(document, "ground", flatGridSurface(10, 1.0, 4.0));
    const EntityId id =
        polyline({Point2(3, 3), Point2(6, 3), Point2(6, 7), Point2(3, 7)}, true);
    document.selection().set({id});
    const std::string text = reply("SURFACE EDIT ground HOLE FROM");
    EXPECT_TRUE(text.starts_with("scope=selection matched=1 polygons=1 skipped=0\n")) << text;
    EXPECT_NEAR(surfaceOf(document, "ground").planArea(), 88.0, 1e-9);
}

TEST_F(SurfaceEditVerbs, ATypedPolygonClosedOnItsFirstCornerIsThatPolygon)
{
    // [3,6] x [3,7] typed back to its first corner: the same 12 out, 88 left.
    putSurface(document, "ground", flatGridSurface(10, 1.0, 4.0));
    reply("SURFACE EDIT ground HOLE 3,3 6,3 6,7 3,7 3,3");
    EXPECT_NEAR(surfaceOf(document, "ground").planArea(), 88.0, 1e-9);
}

TEST_F(SurfaceEditVerbs, ABoundaryFromAScopeSaysWhatTheScopeTook)
{
    // The pyramid clipped to [2,8]^2: 36. The scope takes the polyline and a
    // line, which is no polygon and is counted as skipped.
    putSurface(document, "ground", pyramidSurface());
    polyline({Point2(2, 2), Point2(8, 2), Point2(8, 8), Point2(2, 8)}, true);
    ASSERT_TRUE(document.execute(cmd::createLine(Point2(20, 20), Point2(30, 30))).ok());
    const std::string text = reply("SURFACE EDIT ground BOUNDARY FROM DRAWING");
    EXPECT_TRUE(text.starts_with("scope=drawing matched=2 polygons=1 skipped=1\n")) << text;
    EXPECT_NEAR(surfaceOf(document, "ground").planArea(), 36.0, 1e-9);
}

TEST_F(SurfaceEditVerbs, ADrapedBreaklineTypedInCountsItsCrossings)
{
    // y = 2.5 crosses the spokes from (0,0) and (10,0) to the apex.
    putSurface(document, "ground", pyramidSurface());
    const std::string text = reply("SURFACE EDIT ground BREAKLINE 0,2.5 10,2.5");
    EXPECT_TRUE(contains(text, "crossings=2")) << text;
    EXPECT_NEAR(*surfaceOf(document, "ground").elevationAt(2.5, 2.5), 5.0, 1e-12);
}

TEST_F(SurfaceEditVerbs, RaiseInsideATypedPolygonMovesOnlyTheVerticesInside)
{
    putSurface(document, "ground", flatGridSurface(4, 1.0));
    const std::string text =
        reply("SURFACE EDIT ground RAISE 1.5 INSIDE 0.5,0.5 2.5,0.5 2.5,2.5 0.5,2.5");
    EXPECT_TRUE(contains(text, "moved=4")) << text;
    EXPECT_TRUE(contains(text, "largest_change=1.5")) << text;
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(1, 1), 1.5);
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(3, 3), 0.0);
}

TEST_F(SurfaceEditVerbs, LowerSetLevelAndScaleChangeLevelsAsTheyNameIt)
{
    putSurface(document, "ground", pyramidSurface(10.0));
    reply("SURFACE EDIT ground LOWER 2");
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(5, 5), 8.0);
    reply("SURFACE EDIT ground SCALE 2 base=-2");
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(5, 5), 18.0); // -2 + 2 (8 + 2)
    reply("SURFACE EDIT ground SETLEVEL 3");
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(5, 5), 3.0);
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(0, 0), 3.0);
}

TEST_F(SurfaceEditVerbs, SmoothTakesItsPassesAndWeight)
{
    // The apex's neighbours are the four corners at 0, held on the rim.
    putSurface(document, "ground", pyramidSurface(10.0));
    reply("SURFACE EDIT ground SMOOTH passes=1 weight=1");
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(5, 5), 0.0);
}

TEST_F(SurfaceEditVerbs, ThinWithToleranceZeroRemovesThePointsThatChangeNothing)
{
    // tin_edit's case: a plane with one vertex raised; (1,3) and (3,1) go.
    putSurface(document, "ground", gridSurface(4, 1.0, [](double x, double y) {
                   return 0.5 * x + 0.25 * y + ((x == 2.0 && y == 2.0) ? 1.0 : 0.0);
               }));
    const std::string text = reply("SURFACE EDIT ground THIN 0");
    EXPECT_TRUE(contains(text, "points_removed=2")) << text;
}

TEST_F(SurfaceEditVerbs, DeletePointAndTrianglesTakeTheirPlaces)
{
    putSurface(document, "ground", pyramidSurface());
    reply("SURFACE EDIT ground DELETE POINT 5.1,5.1 reach=0.5");
    EXPECT_EQ(surfaceOf(document, "ground").triangleCount(), 2u);
    putSurface(document, "cells", flatGridSurface(4, 1.0));
    reply("SURFACE EDIT cells DELETE TRIANGLE 0.6,0.2");
    EXPECT_EQ(surfaceOf(document, "cells").triangleCount(), 31u);
    reply("SURFACE EDIT cells DELETE TRIANGLES OUTSIDE 0,0 2,0 2,2 0,2");
    EXPECT_NEAR(surfaceOf(document, "cells").planArea(), 3.5, 1e-12);
}

TEST_F(SurfaceEditVerbs, PasteMergeClipAndOffsetMakeSurfacesByName)
{
    putSurface(document, "ground", flatGridSurface(10, 10.0, 10.0));
    putSurface(document, "pad", flatGridSurface(2, 10.0, 15.0, 40.0, 40.0));

    reply("SURFACE PASTE pad INTO ground NAME combined");
    const auto* combined = document.surfaces().find("combined");
    ASSERT_NE(combined, nullptr);
    EXPECT_EQ(combined->source, "paste of pad into ground");
    EXPECT_DOUBLE_EQ(*combined->surface->elevationAt(50, 50), 15.0);
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(50, 50), 10.0);

    reply("SURFACE OFFSET ground 2 NAME raised");
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "raised").elevationAt(50, 50), 12.0);

    reply("SURFACE CLIP ground 0,0 20,0 20,20 0,20 NAME corner");
    EXPECT_NEAR(surfaceOf(document, "corner").planArea(), 400.0, 1e-9);

    reply("SURFACE MERGE ground pad NAME merged");
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "merged").elevationAt(50, 50), 15.0);

    // In place, the base keeps its name and its place.
    const std::size_t count = document.surfaces().all().size();
    reply("SURFACE PASTE pad INTO ground");
    EXPECT_EQ(document.surfaces().all().size(), count);
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "ground").elevationAt(50, 50), 15.0);
}

TEST_F(SurfaceEditVerbs, EveryRefusalSaysWhatWasWrongAndChangesNothing)
{
    putSurface(document, "ground", squareSurface());
    putSurface(document, "pad", flatGridSurface(1, 4.0, 2.0, 3.0, 3.0));
    const auto generation = document.surfacesGeneration();
    EXPECT_EQ(refusal("SURFACE EDIT nowhere ADD POINT 1,1,1"), ErrorCode::NotFound);
    EXPECT_EQ(refusal("SURFACE EDIT ground EXPLODE"), ErrorCode::ParseFailure);
    EXPECT_EQ(refusal("SURFACE EDIT ground ADD POINT 1,1"), ErrorCode::InvalidArgument);
    EXPECT_EQ(refusal("SURFACE EDIT ground ADD POINT 1,1,1 extra"), ErrorCode::ParseFailure);
    EXPECT_EQ(refusal("SURFACE EDIT ground HOLE #999"), ErrorCode::NotFound);
    EXPECT_EQ(refusal("SURFACE EDIT ground DELETE LONG -1"), ErrorCode::InvalidArgument);
    EXPECT_EQ(refusal("SURFACE EDIT ground SMOOTH passes=0"), ErrorCode::InvalidArgument);
    EXPECT_EQ(refusal("SURFACE EDIT ground SMOOTH sideways=1"), ErrorCode::ParseFailure);
    EXPECT_EQ(refusal("SURFACE EDIT ground FLIP 50,50"), ErrorCode::InvalidArgument);
    EXPECT_EQ(refusal("SURFACE EDIT ground FLIP 5,0"), ErrorCode::InvalidGeometry); // the rim
    EXPECT_EQ(refusal("SURFACE MERGE ground pad"), ErrorCode::ParseFailure);
    EXPECT_EQ(refusal("SURFACE MERGE ground pad NAME pad"), ErrorCode::AlreadyExists);
    EXPECT_EQ(refusal("SURFACE PASTE pad ground"), ErrorCode::ParseFailure);
    EXPECT_EQ(refusal("SURFACE OFFSET ground 1"), ErrorCode::ParseFailure);
    EXPECT_EQ(refusal("SURFACE CLIP ground 20,20 30,20 30,30 NAME away"), ErrorCode::InvalidState);
    EXPECT_EQ(document.surfacesGeneration(), generation);
}

TEST_F(SurfaceEditVerbs, AnEntityOfTheWrongKindIsRefused)
{
    putSurface(document, "ground", squareSurface());
    ASSERT_TRUE(document.execute(cmd::createPoint(Point2(3, 3))).ok());
    const EntityId point = document.lastCreatedEntities().front();
    EXPECT_EQ(refusal("SURFACE EDIT ground HOLE #" + std::to_string(point)),
              ErrorCode::InvalidArgument);
    // Two polygons where BOUNDARY takes one.
    polyline({Point2(1, 1), Point2(4, 1), Point2(4, 4)}, true);
    polyline({Point2(6, 6), Point2(9, 6), Point2(9, 9)}, true);
    EXPECT_EQ(refusal("SURFACE EDIT ground BOUNDARY FROM DRAWING"), ErrorCode::InvalidArgument);
}

TEST_F(SurfaceEditVerbs, AScopeThatTakesNothingIsReportedAndNoStepIsMade)
{
    // The drawing holds one POINT: no polygon, no line, no point with a level.
    // CLAUDE.md 1.1: a scope that takes nothing is reported, not an error - the
    // scope record says what it took, the surface record says changed=no and
    // why nothing ran, and the session goes on. (OUTSIDE of no polygon is not
    // the whole surface, and an INSIDE that took nothing is not "no INSIDE".)
    putSurface(document, "ground", flatGridSurface(4, 1.0));
    ASSERT_TRUE(document.execute(cmd::createPoint(Point2(3, 3))).ok());
    const auto generation = document.surfacesGeneration();
    const TinSurface before = surfaceOf(document, "ground");
    struct Case {
        const char* line;
        const char* why;
    };
    for (const Case& test : {Case{"DELETE TRIANGLES INSIDE FROM DRAWING", "the scope took no polygon"},
                             Case{"DELETE TRIANGLES OUTSIDE FROM DRAWING", "the scope took no polygon"},
                             Case{"DELETE POINTS INSIDE FROM DRAWING", "the scope took no polygon"},
                             Case{"BREAKLINE FROM DRAWING", "the scope took no line"},
                             Case{"BOUNDARY FROM DRAWING", "the scope took no polygon"},
                             Case{"HOLE FROM DRAWING", "the scope took no polygon"},
                             Case{"RAISE 1 INSIDE FROM DRAWING", "the scope took no polygon"},
                             Case{"SMOOTH INSIDE FROM DRAWING", "the scope took no polygon"},
                             Case{"ADD POINTS FROM DRAWING", "the scope took no point with a level"}}) {
        SCOPED_TRACE(test.line);
        const std::string text = reply(std::string("SURFACE EDIT ground ") + test.line);
        EXPECT_TRUE(contains(text, "scope=drawing matched=1")) << text;
        EXPECT_TRUE(contains(text, "changed=no ran=no reason=\"" + std::string(test.why) + "\"")) << text;
        EXPECT_EQ(document.surfacesGeneration(), generation);
        EXPECT_EQ(surfaceOf(document, "ground"), before);
    }
}

TEST_F(SurfaceEditVerbs, AClipOfAScopeThatTakesNothingMakesNoSurfaceAndSaysSo)
{
    putSurface(document, "ground", flatGridSurface(4, 1.0));
    ASSERT_TRUE(document.execute(cmd::createPoint(Point2(3, 3))).ok());
    const std::string text = reply("SURFACE CLIP ground FROM DRAWING NAME part");
    EXPECT_TRUE(contains(text, "surface name=part operation=clip changed=no ran=no reason=\"the scope took no polygon\""))
        << text;
    EXPECT_EQ(document.surfaces().find("part"), nullptr);
}

TEST_F(SurfaceEditVerbs, SeveralBreaklinesCrossingAtDifferentLevelsAreRefusedInEitherOrder)
{
    // (5,5) is 12 on the first and 8 on the second. Added one after the other
    // the first named won there and the second bent to it.
    putSurface(document, "ground", squareSurface(10, 10, 10, 10));
    reply("PLINE3D 1,5,12 9,5,12");
    const EntityId along = document.lastCreatedEntities().front();
    reply("PLINE3D 5,1,8 5,9,8");
    const EntityId across = document.lastCreatedEntities().front();
    const auto generation = document.surfacesGeneration();
    const std::string a = "#" + std::to_string(along);
    const std::string c = "#" + std::to_string(across);
    EXPECT_EQ(refusal("SURFACE EDIT ground BREAKLINE " + a + " " + c), ErrorCode::InvalidGeometry);
    EXPECT_EQ(refusal("SURFACE EDIT ground BREAKLINE " + c + " " + a), ErrorCode::InvalidGeometry);
    EXPECT_EQ(refusal("SURFACE EDIT ground BREAKLINE FROM DRAWING"), ErrorCode::InvalidGeometry);
    EXPECT_EQ(document.surfacesGeneration(), generation);
}

TEST_F(SurfaceEditVerbs, SeveralBreaklinesThatAgreeWhereTheyCrossAreOneEdit)
{
    putSurface(document, "ground", squareSurface(10, 10, 10, 10));
    reply("PLINE3D 1,5,12 9,5,12");
    reply("PLINE3D 5,1,12 5,9,12");
    const std::string text = reply("SURFACE EDIT ground BREAKLINE FROM DRAWING");
    EXPECT_TRUE(contains(text, "scope=drawing matched=2 lines=2 skipped=0\n")) << text;
    EXPECT_TRUE(contains(text, "crossings=1")) << text;
    EXPECT_NEAR(*surfaceOf(document, "ground").elevationAt(5, 5), 12.0, 1e-9);
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_EQ(surfaceOf(document, "ground").triangleCount(), 2u) << "one undo step for both lines";
}

TEST_F(SurfaceEditVerbs, ASurfaceOrALayerCalledPreviewIsANameNotThePreviewFlag)
{
    putSurface(document, "ground", flatGridSurface(10, 1.0, 4.0));
    putSurface(document, "Preview", flatGridSurface(10, 1.0, 4.0));
    // A surface called Preview as the source of an offset, a clip and a paste's
    // top, as a member of a merge, and a new surface called preview.
    reply("SURFACE OFFSET Preview 1 NAME up");
    EXPECT_DOUBLE_EQ(*surfaceOf(document, "up").elevationAt(5, 5), 5.0);
    reply("SURFACE CLIP Preview 1,1 5,1 5,5 1,5 NAME clipped");
    EXPECT_NEAR(surfaceOf(document, "clipped").planArea(), 16.0, 1e-9);
    reply("SURFACE MERGE ground Preview NAME merged");
    EXPECT_NE(document.surfaces().find("merged"), nullptr);
    reply("SURFACE OFFSET ground 3 NAME preview");
    EXPECT_NE(document.surfaces().find("preview"), nullptr);
    // A layer called preview in a FROM scope.
    reply("LAYER NEW preview");
    reply("LAYER SET preview");
    polyline({Point2(3, 3), Point2(6, 3), Point2(6, 7), Point2(3, 7)}, true, "preview");
    reply("SURFACE EDIT ground HOLE FROM LAYERS preview ONLY");
    EXPECT_NEAR(surfaceOf(document, "ground").planArea(), 88.0, 1e-9);
    // And a PREVIEW that is not a value still previews, wherever it stands.
    const std::size_t count = document.surfaces().all().size();
    const std::string text = reply("SURFACE OFFSET ground 1 NAME later PREVIEW");
    EXPECT_TRUE(contains(text, "preview=yes")) << text;
    EXPECT_EQ(document.surfaces().all().size(), count);
    const std::string early = reply("SURFACE EDIT ground PREVIEW RAISE 1");
    EXPECT_TRUE(contains(early, "preview=yes")) << early;
}

TEST_F(SurfaceEditVerbs, DeletePointJustOutsideTheRimTakesTheCornerWithinReach)
{
    putSurface(document, "ground", flatGridSurface(4, 1.0));
    // (-0.05,-0.05) is 0.07 from the corner (0,0) and off the surface.
    const std::string text = reply("SURFACE EDIT ground DELETE POINT -0.05,-0.05 reach=0.2");
    EXPECT_TRUE(contains(text, "triangles_before=32 triangles_after=31")) << text;
    EXPECT_TRUE(contains(text, "points_removed=1")) << text;
    // Far outside, nothing is within reach: off the surface, said as such.
    auto far = interpreter.run("SURFACE EDIT ground DELETE POINT -5,-5 reach=0.2");
    ASSERT_FALSE(far.ok());
    EXPECT_EQ(far.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(far.error().message, "off the surface")) << far.error().message;
    // On the surface with no vertex within reach: said so.
    auto bare = interpreter.run("SURFACE EDIT ground DELETE POINT 1.5,0.2 reach=0.01");
    ASSERT_FALSE(bare.ok());
    EXPECT_TRUE(contains(bare.error().message, "within reach")) << bare.error().message;
    auto offNoReach = interpreter.run("SURFACE EDIT ground DELETE POINT -0.05,-0.05");
    ASSERT_FALSE(offNoReach.ok());
    EXPECT_EQ(offNoReach.error().message, "the point is off the surface");
}

TEST_F(SurfaceEditVerbs, ATypedPolygonThatClosesToTooFewCornersSaysHowManyItHas)
{
    putSurface(document, "ground", squareSurface());
    // 0,0 10,0 0,0 is two corners and the first again: "needs three corners
    // [corners=3]" contradicted itself.
    for (const char* line : {"SURFACE EDIT ground BOUNDARY 0,0 10,0 0,0",
                             "SURFACE EDIT ground HOLE 0,0 10,0 0,0 0,0"}) {
        auto result = interpreter.run(line);
        ASSERT_FALSE(result.ok()) << line;
        EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
        EXPECT_TRUE(contains(result.error().message, "three distinct corners"))
            << result.error().message;
        EXPECT_TRUE(contains(result.error().context, "corners=2 typed=")) << result.error().context;
    }
    auto flat = interpreter.run("SURFACE EDIT ground DELETE TRIANGLES INSIDE 5,5 5,5 5,5");
    ASSERT_FALSE(flat.ok());
    EXPECT_TRUE(contains(flat.error().context, "corners=1 typed=3")) << flat.error().context;
}

TEST_F(SurfaceEditVerbs, AThinThatRemovesNothingFromASurfaceNotInCanonicalOrderIsNoStep)
{
    // The square's triangles written rotated, as an archive may give them: THIN
    // rebuilt the arrays sorted, which differs, so "changed=yes" and an undo
    // step for an edit that did nothing.
    putSurface(document, "ground",
               makeSurface({katana::geometry::Point3(0, 0, 0), katana::geometry::Point3(10, 0, 0),
                            katana::geometry::Point3(10, 10, 0), katana::geometry::Point3(0, 10, 0)},
                           {{1, 2, 0}, {3, 0, 2}}));
    const auto generation = document.surfacesGeneration();
    const std::string text = reply("SURFACE EDIT ground THIN 0");
    EXPECT_TRUE(contains(text, "points_removed=0")) << text;
    EXPECT_TRUE(contains(text, "changed=no")) << text;
    EXPECT_EQ(document.surfacesGeneration(), generation);
}

TEST_F(SurfaceEditVerbs, ALevelBeyondTheMostAnEditAcceptsIsRefused)
{
    putSurface(document, "ground", squareSurface());
    const auto generation = document.surfacesGeneration();
    EXPECT_EQ(refusal("SURFACE EDIT ground ADD POINT 1e308,1,1"), ErrorCode::InvalidArgument);
    EXPECT_EQ(refusal("SURFACE EDIT ground RAISE 1e308"), ErrorCode::InvalidArgument);
    EXPECT_EQ(refusal("SURFACE EDIT ground SETLEVEL 1e70000"), ErrorCode::ParseFailure);
    EXPECT_EQ(document.surfacesGeneration(), generation);
}

TEST_F(SurfaceEditVerbs, TheToolsActOnTheChosenSurfaceElseTheFirstVisible)
{
    setSurfaceEditTarget("");
    EXPECT_EQ(activeEditSurface(document).error().code, ErrorCode::NotFound);
    putSurface(document, "hidden", squareSurface(), false);
    EXPECT_EQ(*activeEditSurface(document), "hidden"); // the first, none being visible
    putSurface(document, "ground", squareSurface());
    putSurface(document, "pad", squareSurface());
    EXPECT_EQ(*activeEditSurface(document), "ground");
    setSurfaceEditTarget("PAD");
    EXPECT_EQ(*activeEditSurface(document), "pad");
    setSurfaceEditTarget("gone");
    EXPECT_EQ(*activeEditSurface(document), "ground");
    setSurfaceEditTarget("");
    EXPECT_EQ(surfaceNameWord("my ground"), "\"my ground\"");
    EXPECT_EQ(surfaceNameWord("ground"), "ground");
}

TEST_F(SurfaceEditVerbs, HelpNamesEveryVerb)
{
    const std::string help = reply("HELP SURFACE EDIT");
    for (const char* word : {"DELETE TRIANGLE", "FLIP", "ADD POINTS", "BREAKLINE", "BOUNDARY",
                             "HOLE", "SETLEVEL", "SMOOTH", "THIN", "SURFACE MERGE",
                             "SURFACE PASTE", "SURFACE CLIP", "SURFACE OFFSET"}) {
        EXPECT_TRUE(contains(help, word)) << word;
    }
    EXPECT_TRUE(contains(reply("HELP"), "SURFACE EDIT"));
}
