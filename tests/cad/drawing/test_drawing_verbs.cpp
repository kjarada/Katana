// The drawing system's verbs of the command line (src/katana_cad/drawing/
// drawing_verbs.cpp, docs/drawing.md "The command line"): each verb driven
// as an agent drives it, its reply read back as key=value records, and each
// edit taken back by ONE undo.

#include <gtest/gtest.h>

#include <cmath>
#include <sstream>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/drawing/construction.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::geometry::CurvePolyline2;
using katana::geometry::Point2;

namespace {

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string ok(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << " -> " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    ErrorCode fails(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " unexpectedly succeeded: " << (reply.ok() ? *reply : "");
        return reply.ok() ? ErrorCode::Internal : reply.error().code;
    }
    // The polyline an id names, read the way the verbs read it.
    CurvePolyline2 polyline(EntityId id) const
    {
        const auto* entity = document.model().entities.find(id);
        EXPECT_NE(entity, nullptr);
        return entity != nullptr ? readPolyline(*entity).value_or(CurvePolyline2{}) : CurvePolyline2{};
    }
    EntityType type(EntityId id) const { return document.model().entities.find(id)->type(); }
};

std::vector<std::string> lines(const std::string& text)
{
    std::vector<std::string> out;
    std::istringstream in(text);
    for (std::string line; std::getline(in, line);) {
        out.push_back(line);
    }
    return out;
}

// The value of `key` in a key=value record, or "" when it has none.
std::string value(const std::string& record, const std::string& key)
{
    std::istringstream in(record);
    for (std::string token; in >> token;) {
        if (token.rfind(key + "=", 0) == 0) {
            return token.substr(key.size() + 1);
        }
    }
    return {};
}

EntityId idOf(const std::string& record)
{
    return static_cast<EntityId>(std::stoull(value(record, "id")));
}

// A three-vertex polyline east then north, made through the verb.
EntityId corner(Session& s)
{
    return idOf(s.ok("PLINE 0,0,100 10,0,101 10,10"));
}

} // namespace

// ---- routing -------------------------------------------------------------------------------

TEST(DrawingVerbs, APlainPlineRepliesWithItsIdAndOpenStillOpensAProject)
{
    Session s;
    const std::string reply = s.ok("PLINE 0,0 10,0 10,10 C");
    EXPECT_EQ(value(reply, "kind"), "polyline");
    EXPECT_EQ(value(reply, "closed"), "yes");
    EXPECT_EQ(s.type(idOf(reply)), EntityType::Polyline);
    // OPEN with a directory is the file verb's (and fails: there is none).
    EXPECT_EQ(s.fails("OPEN /no/such/katana/project"), ErrorCode::NotFound);
}

TEST(DrawingVerbs, HelpListsTheDrawingVerbs)
{
    const std::string help = CommandInterpreter::helpText();
    for (const char* verb : {"VERTEX LIST", "WEED", "DENSIFY", "STRAIGHTEN", "STARTVERTEX",
                             "VERTEXZ", "PLINE3D", "SPLINE", "ELLIPSE", "XLINE", "RAY", "DLINE",
                             "ORTHO", "POLAR", "SNAP", "LOCK", "ANGLES"}) {
        EXPECT_NE(help.find(verb), std::string::npos) << verb;
    }
}

// ---- VERTEX ----------------------------------------------------------------------------------

TEST(DrawingVerbs, VertexListGivesASummaryAndARecordPerVertex)
{
    Session s;
    const EntityId id = corner(s);
    const auto reply = lines(s.ok("VERTEX LIST " + std::to_string(id)));
    ASSERT_EQ(reply.size(), 4u);
    EXPECT_EQ(value(reply[0], "kind"), "polyline");
    EXPECT_EQ(value(reply[0], "vertices"), "3");
    EXPECT_EQ(value(reply[0], "heights"), "2");
    EXPECT_EQ(value(reply[0], "length"), "20");
    EXPECT_EQ(value(reply[1], "index"), "0");
    EXPECT_EQ(value(reply[1], "z"), "100");
    EXPECT_EQ(value(reply[1], "bearing"), "90") << "east is a whole-circle bearing of 90";
    EXPECT_EQ(value(reply[2], "bearing"), "0");
    EXPECT_EQ(value(reply[2], "distance"), "10");
    EXPECT_EQ(value(reply[3], "z"), "none") << "no height is not zero";
    EXPECT_EQ(value(reply[3], "bearing"), "none");
    EXPECT_EQ(s.ok("VERTEX LIST #" + std::to_string(id)), s.ok("VERTEX LIST " + std::to_string(id)));
}

TEST(DrawingVerbs, VertexInsertSplitsTheNearestSegmentOrTheOneNamed)
{
    Session s;
    const EntityId id = corner(s);
    const std::string reply = s.ok("VERTEX INSERT " + std::to_string(id) + " 10,5");
    EXPECT_EQ(value(reply, "inserted"), "2");
    EXPECT_EQ(value(reply, "vertices"), "4");
    EXPECT_EQ(s.polyline(id).vertices[2].position, Point2(10, 5));

    s.ok("VERTEX INSERT " + std::to_string(id) + " 5,-1,100.5 after=0");
    const auto p = s.polyline(id);
    ASSERT_EQ(p.vertices.size(), 5u);
    EXPECT_EQ(p.vertices[1].position, Point2(5, -1));
    EXPECT_DOUBLE_EQ(*p.vertices[1].height, 100.5) << "a typed z is the new vertex's height";

    EXPECT_EQ(s.fails("VERTEX INSERT " + std::to_string(id) + " 1,1 after=9"),
              ErrorCode::InvalidArgument);
    EXPECT_EQ(s.fails("VERTEX INSERT " + std::to_string(id) + " 1,1 before=1"),
              ErrorCode::InvalidArgument)
        << "an unknown option is refused, not ignored";
}

TEST(DrawingVerbs, VertexInsertAtAPointOfThePolylineTakesThatSegmentAndAHeight)
{
    Session s;
    // A polyline that doubles back, (0,0) (10,0) (0,0): the middle of
    // segment 1, (5,0), lies on segment 0 too, which the nearest segment
    // would take. "#id.s1" names segment 1 itself: the vertex after it.
    const EntityId back = idOf(s.ok("PLINE 0,0 10,0 0,0"));
    const std::string b = std::to_string(back);
    EXPECT_EQ(value(s.ok("VERTEX INSERT " + b + " #" + b + ".s1"), "inserted"), "2");
    EXPECT_EQ(s.polyline(back).positions(),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(5, 0), Point2(0, 0)}));

    // "#id@x,y,z": on the line nearest x,y, with z its height.
    const EntityId id = corner(s);
    const std::string c = std::to_string(id);
    EXPECT_EQ(value(s.ok("VERTEX INSERT " + c + " #" + c + "@4,0.3,101.5"), "inserted"), "1");
    const auto on = s.polyline(id).vertices[1];
    EXPECT_NEAR(on.position.x, 4.0, 1.0e-12);
    EXPECT_NEAR(on.position.y, 0.0, 1.0e-12);
    ASSERT_TRUE(on.height.has_value());
    EXPECT_DOUBLE_EQ(*on.height, 101.5);

    // A segment it has not got: refused, and in words about the polyline's
    // point, not a dimension's.
    const auto noSegment = s.interpreter.run("VERTEX INSERT " + c + " #" + c + ".s9");
    ASSERT_FALSE(noSegment.ok());
    EXPECT_EQ(noSegment.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(noSegment.error().describe().find("dimension"), std::string::npos)
        << noSegment.error().describe();
    // An entity there is not.
    EXPECT_EQ(s.fails("VERTEX INSERT " + c + " #999@1,1"), ErrorCode::NotFound);
    // A point on a vertex already: refused, the polyline as it was.
    const std::size_t count = s.polyline(id).vertices.size();
    (void)s.fails("VERTEX INSERT " + c + " #" + c + "@10,0");
    EXPECT_EQ(s.polyline(id).vertices.size(), count);
    // A point of ANOTHER entity is that entity's: the end (3,2) of this
    // line is nearest (3,5), and goes into the corner's nearest segment, 0.
    const EntityId other = idOf(s.ok("PLINE 3,-2 3,2"));
    EXPECT_EQ(value(s.ok("VERTEX INSERT " + c + " #" + std::to_string(other) + "@3,5"),
                    "inserted"),
              "1");
    EXPECT_EQ(s.polyline(id).vertices[1].position, Point2(3, 2));
}

TEST(DrawingVerbs, StraightenAndGradeTakeAClosedPolylinesShorterSideAsTheWindowDoes)
{
    // The hexagon (0,0) (10,0) (20,0) (20,10) (10,10) (0,10): from vertex 3
    // to vertex 1, forward is 4, 5 and 0 and the other way only 2. The
    // window's picks 3 then 1 take out vertex 2 alone; the verb walked
    // forward and took out three. side=other is the window's O.
    Session s;
    const EntityId h = idOf(s.ok("PLINE 0,0 10,0 20,0 20,10 10,10 0,10 CLOSE"));
    const std::string id = std::to_string(h);
    EXPECT_EQ(value(s.ok("STRAIGHTEN " + id + " 3 1"), "vertices"), "5");
    EXPECT_EQ(s.polyline(h).positions(), (std::vector<Point2>{Point2(0, 0), Point2(10, 0),
                                                              Point2(20, 10), Point2(10, 10),
                                                              Point2(0, 10)}));
    ASSERT_TRUE(s.document.undo());
    EXPECT_EQ(value(s.ok("STRAIGHTEN " + id + " 3 1 side=other"), "vertices"), "3");
    EXPECT_EQ(s.polyline(h).positions(),
              (std::vector<Point2>{Point2(10, 0), Point2(20, 0), Point2(20, 10)}));
    EXPECT_EQ(s.fails("STRAIGHTEN " + id + " 0 1 side=sideways"), ErrorCode::InvalidArgument);
    // An open polyline has one way between two vertices.
    const EntityId open = idOf(s.ok("PLINE 0,0 10,0 20,0"));
    EXPECT_EQ(s.fails("STRAIGHTEN " + std::to_string(open) + " 0 2 side=other"),
              ErrorCode::InvalidArgument);

    // Grade on the same hexagon with heights: 0 at vertex 1, 10 at vertex 3.
    // The short way, 1 to 3, puts vertex 2 halfway by length, at 5. The
    // other way, 3 to 1 through 4, 5 and 0, 40 long in four steps of 10:
    // 7.5, 5 and 2.5.
    const EntityId z = idOf(s.ok("PLINE3D 0,0,0 10,0,0 20,0,0 20,10,10 10,10,0 0,10,0 CLOSE"));
    const std::string zid = std::to_string(z);
    s.ok("VERTEXZ " + zid + " GRADE 3 1");
    auto graded = s.polyline(z);
    EXPECT_DOUBLE_EQ(*graded.vertices[2].height, 5.0);
    EXPECT_DOUBLE_EQ(*graded.vertices[4].height, 0.0) << "the long way is left alone";
    ASSERT_TRUE(s.document.undo());
    s.ok("VERTEXZ " + zid + " GRADE 3 1 side=other");
    graded = s.polyline(z);
    EXPECT_DOUBLE_EQ(*graded.vertices[4].height, 7.5);
    EXPECT_DOUBLE_EQ(*graded.vertices[5].height, 5.0);
    EXPECT_DOUBLE_EQ(*graded.vertices[0].height, 2.5);
    EXPECT_DOUBLE_EQ(*graded.vertices[2].height, 0.0) << "the short way is left alone";
}

TEST(DrawingVerbs, VertexDeleteRemovesSeveralAtOnceAsOneUndoStep)
{
    Session s;
    const EntityId id = idOf(s.ok("PLINE 0,0 1,0 2,0 3,0 4,0 ARC 5,1"));
    EXPECT_EQ(value(s.ok("VERTEX DELETE " + std::to_string(id) + " 1 3"), "vertices"), "4");
    ASSERT_TRUE(s.document.undo());
    EXPECT_EQ(s.polyline(id).vertices.size(), 6u) << "one undo takes back the whole delete";
    EXPECT_EQ(s.fails("VERTEX DELETE " + std::to_string(id) + " 6"), ErrorCode::InvalidArgument);
}

TEST(DrawingVerbs, VertexMoveTakesAbsoluteRelativeAndPolarPointsFromTheVertex)
{
    Session s;
    const EntityId id = corner(s);
    const std::string idText = std::to_string(id);
    auto reply = lines(s.ok("VERTEX MOVE " + idText + " 2 12,12"));
    ASSERT_EQ(reply.size(), 2u);
    EXPECT_EQ(value(reply[1], "x"), "12");
    s.ok("VERTEX MOVE " + idText + " 2 @1,-2");
    EXPECT_EQ(s.polyline(id).vertices[2].position, Point2(13, 10)) << "@ is from the vertex";
    s.ok("VERTEX MOVE " + idText + " 1 @0,0,0.5");
    EXPECT_DOUBLE_EQ(*s.polyline(id).vertices[1].height, 101.5) << "a relative dz changes the height";
    s.ok("VERTEX MOVE " + idText + " 0 0,0,99");
    EXPECT_DOUBLE_EQ(*s.polyline(id).vertices[0].height, 99.0);
    EXPECT_EQ(s.fails("VERTEX MOVE " + idText + " 2 @0,0,1"), ErrorCode::InvalidArgument)
        << "no height for a dz to change";
}

TEST(DrawingVerbs, VertexSetEditsTheColumnsTheVerticesPanelShows)
{
    Session s;
    const EntityId id = corner(s);
    const std::string idText = std::to_string(id);
    s.ok("VERTEX SET " + idText + " 1 x=11 z=none");
    auto p = s.polyline(id);
    EXPECT_EQ(p.vertices[1].position, Point2(11, 0));
    EXPECT_FALSE(p.vertices[1].height.has_value());
    // A bearing and a distance move the NEXT vertex, as a traverse is edited.
    s.ok("VERTEX SET " + idText + " 0 bearing=90 distance=20");
    p = s.polyline(id);
    EXPECT_NEAR(p.vertices[1].position.x, 20.0, 1e-9);
    EXPECT_NEAR(p.vertices[1].position.y, 0.0, 1e-9);
    const std::string arc = s.ok("VERTEX SET " + idText + " 1 bulge=0.5");
    EXPECT_EQ(value(arc, "kind"), "curvepolyline") << "a bulge makes it a curve polyline";
    EXPECT_EQ(s.type(id), EntityType::CurvePolyline);
    ASSERT_TRUE(s.document.undo());
    EXPECT_EQ(s.type(id), EntityType::Polyline);
    EXPECT_EQ(s.fails("VERTEX SET " + idText + " 0 colour=red"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.fails("VERTEX SET " + idText + " 2 distance=4"), ErrorCode::InvalidArgument)
        << "the last vertex of an open polyline starts no segment";
}

TEST(DrawingVerbs, VertexVerbsRefuseWhatIsNotAPolyline)
{
    Session s;
    s.ok("LINE 0,0 1,1");
    const EntityId line = s.document.lastCreatedEntities().front();
    EXPECT_EQ(s.fails("VERTEX LIST " + std::to_string(line)), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.fails("VERTEX LIST 999"), ErrorCode::NotFound);
    EXPECT_EQ(s.fails("VERTEX FROB 1"), ErrorCode::ParseFailure);
}

// ---- whole-polyline edits ------------------------------------------------------------------

TEST(DrawingVerbs, WeedSimplifiesWithinTheToleranceAndSaysHowManyWereThere)
{
    Session s;
    const EntityId id = idOf(s.ok("PLINE 0,0 1,0.001 2,0 3,0.5 4,0"));
    const std::string reply = s.ok("WEED " + std::to_string(id) + " tolerance=0.01");
    EXPECT_EQ(value(reply, "before"), "5");
    EXPECT_EQ(value(reply, "vertices"), "4");
    EXPECT_EQ(s.fails("WEED " + std::to_string(id)), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.fails("WEED " + std::to_string(id) + " tolerance=0"), ErrorCode::InvalidArgument);
}

TEST(DrawingVerbs, WeedAndDensifyActOnTheSelectionAsOneStep)
{
    Session s;
    const EntityId a = idOf(s.ok("PLINE 0,0 10,0 10,10 LINE"));
    const EntityId b = idOf(s.ok("PLINE 0,20 10,20 L"));
    s.ok("SELECT ALL");
    const auto reply = lines(s.ok("DENSIFY SELECTION interval=2.5"));
    ASSERT_EQ(reply.size(), 2u);
    EXPECT_EQ(s.polyline(a).vertices.size(), 9u);
    EXPECT_EQ(s.polyline(b).vertices.size(), 5u);
    ASSERT_TRUE(s.document.undo());
    EXPECT_EQ(s.polyline(a).vertices.size(), 3u);
    EXPECT_EQ(s.polyline(b).vertices.size(), 2u);
}

TEST(DrawingVerbs, DensifyChordsArcsWhenGivenAChordTolerance)
{
    Session s;
    const EntityId id = idOf(s.ok("PLINE 0,0 10,0 ARC 20,10"));
    EXPECT_EQ(s.type(id), EntityType::CurvePolyline);
    s.ok("DENSIFY " + std::to_string(id) + " chord=0.01");
    EXPECT_EQ(s.type(id), EntityType::Polyline) << "chorded, it is straight";
    EXPECT_GT(s.polyline(id).vertices.size(), 10u);
}

TEST(DrawingVerbs, CloseAndOpenOnIdsTheSelectionOrNothing)
{
    Session s;
    const EntityId id = corner(s);
    EXPECT_EQ(value(s.ok("CLOSE " + std::to_string(id)), "closed"), "yes");
    EXPECT_EQ(value(s.ok("OPEN #" + std::to_string(id)), "closed"), "no");
    s.ok("SELECT " + std::to_string(id));
    EXPECT_EQ(value(s.ok("CLOSE"), "closed"), "yes");
    EXPECT_EQ(value(s.ok("OPEN"), "closed"), "no");
    EXPECT_EQ(value(s.ok("OPEN SELECTION"), "closed"), "no") << "opening an open one leaves it";
    s.ok("SELECT NONE");
    EXPECT_EQ(s.fails("CLOSE"), ErrorCode::InvalidState);
}

TEST(DrawingVerbs, StraightenStartVertexAndTheHeightVerbs)
{
    Session s;
    const EntityId id = idOf(s.ok("PLINE3D 0,0,10 1,1 2,-1 3,0,13"));
    const std::string idText = std::to_string(id);
    EXPECT_EQ(value(s.ok("VERTEXZ " + idText + " INTERPOLATE"), "heights"), "4");
    auto p = s.polyline(id);
    EXPECT_GT(*p.vertices[1].height, 10.0);
    EXPECT_LT(*p.vertices[2].height, 13.0);
    s.ok("VERTEXZ " + idText + " 1 none");
    EXPECT_FALSE(s.polyline(id).vertices[1].height.has_value());
    s.ok("VERTEXZ " + idText + " 1 50");
    s.ok("VERTEXZ " + idText + " GRADE 0 3");
    p = s.polyline(id);
    EXPECT_LT(*p.vertices[1].height, 13.0) << "graded back between the ends";
    EXPECT_EQ(value(s.ok("STRAIGHTEN " + idText + " 0 3"), "vertices"), "2");

    const EntityId square = idOf(s.ok("PLINE 0,0 1,0 1,1 0,1 CLOSE"));
    s.ok("STARTVERTEX " + std::to_string(square) + " 2");
    EXPECT_EQ(s.polyline(square).vertices.front().position, Point2(1, 1));
    EXPECT_EQ(s.fails("STARTVERTEX " + std::to_string(square) + " 7"), ErrorCode::InvalidArgument);
}

// ---- the draw verbs ---------------------------------------------------------------------------

TEST(DrawingVerbs, PlineArcDrawsTangentArcsAndLineGoesBackToStraight)
{
    Session s;
    const std::string reply = s.ok("PLINE 0,0 10,0 ARC 20,10 LINE 20,20");
    EXPECT_EQ(value(reply, "kind"), "curvepolyline");
    EXPECT_EQ(value(reply, "arcs"), "yes");
    const auto p = s.polyline(idOf(reply));
    ASSERT_EQ(p.vertices.size(), 4u);
    EXPECT_DOUBLE_EQ(p.vertices[0].bulge, 0.0);
    // Leaving east, ending north at 20,10: a counter-clockwise quarter
    // circle, bulge tan(90/4).
    EXPECT_NEAR(p.vertices[1].bulge, std::tan(katana::math::kPi / 8.0), 1e-12);
    EXPECT_DOUBLE_EQ(p.vertices[2].bulge, 0.0);
    ASSERT_TRUE(s.document.undo());
    EXPECT_EQ(s.document.model().entities.size(), 0u);
}

TEST(DrawingVerbs, PlineClosedInArcModeClosesWithATangentArc)
{
    Session s;
    const auto p = s.polyline(idOf(s.ok("PLINE 0,0 10,0 ARC 10,10 CLOSE")));
    EXPECT_TRUE(p.closed);
    EXPECT_NE(p.vertices.back().bulge, 0.0);
}

TEST(DrawingVerbs, Pline3dGivesEachPointItsHeightOrTheDefault)
{
    Session s;
    const std::string reply = s.ok("PLINE3D 0,0,5 10,0 10,10,7 z=6");
    EXPECT_EQ(value(reply, "kind"), "polyline") << "a straight 3D string stays a plain polyline";
    const auto p = s.polyline(idOf(reply));
    EXPECT_DOUBLE_EQ(*p.vertices[0].height, 5.0);
    EXPECT_DOUBLE_EQ(*p.vertices[1].height, 6.0);
    EXPECT_DOUBLE_EQ(*p.vertices[2].height, 7.0);
    EXPECT_EQ(s.fails("PLINE3D 0,0 0,0"), ErrorCode::InvalidGeometry);
}

TEST(DrawingVerbs, SplineThroughPointsOrByControlPoints)
{
    Session s;
    const std::string fit = s.ok("SPLINE 0,0 5,5 10,0 15,5");
    EXPECT_EQ(value(fit, "kind"), "spline");
    const auto* entity = s.document.model().entities.find(idOf(fit));
    const auto& spline = std::get<katana::geometry::Spline2>(entity->geometry);
    EXPECT_EQ(spline.fitPoints.size(), 4u);
    const std::string control = s.ok("SPL 0,0 5,5 10,0 control=on degree=2");
    const auto& byControl = std::get<katana::geometry::Spline2>(
        s.document.model().entities.find(idOf(control))->geometry);
    EXPECT_EQ(byControl.degree, 2);
    EXPECT_TRUE(byControl.fitPoints.empty());
    EXPECT_EQ(s.fails("SPLINE 0,0 1,1 degree=11"), ErrorCode::InvalidArgument);
}

TEST(DrawingVerbs, EllipseByCentreAxisAndMinorOrRatioAndAsAnArc)
{
    Session s;
    const std::string full = s.ok("ELLIPSE 0,0 10,0 minor=4");
    const auto& e = std::get<katana::geometry::Ellipse2>(
        s.document.model().entities.find(idOf(full))->geometry);
    EXPECT_DOUBLE_EQ(e.majorRadius(), 10.0);
    EXPECT_DOUBLE_EQ(e.minorRadius(), 4.0);
    EXPECT_TRUE(e.isFull());
    const std::string arc = s.ok("EL 0,0 0,10 ratio=0.5 start=0 end=90");
    const auto& a = std::get<katana::geometry::Ellipse2>(
        s.document.model().entities.find(idOf(arc))->geometry);
    EXPECT_NEAR(a.sweep, katana::math::kPi / 2.0, 1e-12);
    EXPECT_EQ(s.fails("ELLIPSE 0,0 10,0"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.fails("ELLIPSE 0,0 10,0 minor=4 start=10"), ErrorCode::InvalidArgument);
}

TEST(DrawingVerbs, XlineAndRayGoOnTheConstructionLayerWithItInOneStep)
{
    Session s;
    const auto reply = lines(s.ok("XLINE 0,0 1,1 angle=90"));
    ASSERT_EQ(reply.size(), 2u);
    EXPECT_EQ(value(reply[0], "layer"), std::string(kConstructionLayer));
    EXPECT_NE(s.document.model().layers.find(std::string(kConstructionLayer)), nullptr);
    const auto& line = std::get<katana::geometry::Segment2>(
        s.document.model().entities.find(idOf(reply[0]))->geometry);
    EXPECT_NEAR(line.length(), 2.0 * kConstructionReach, 1e-6);
    ASSERT_TRUE(s.document.undo());
    EXPECT_EQ(s.document.model().layers.find(std::string(kConstructionLayer)), nullptr)
        << "the layer made for the lines goes with them";

    const auto ray = lines(s.ok("RAY 5,5 5,6"));
    ASSERT_EQ(ray.size(), 1u);
    const auto& r = std::get<katana::geometry::Segment2>(
        s.document.model().entities.find(idOf(ray[0]))->geometry);
    EXPECT_EQ(r.start, Point2(5, 5));
    EXPECT_NEAR(r.end.y, 5.0 + kConstructionReach, 1e-6);
    EXPECT_EQ(s.fails("RAY 5,5 5,5"), ErrorCode::InvalidGeometry);
}

TEST(DrawingVerbs, XlineAnglesFollowTheAngleConvention)
{
    Session s;
    s.ok("ANGLES bearing");
    const auto reply = lines(s.ok("XLINE 0,0 angle=0"));
    const auto& line = std::get<katana::geometry::Segment2>(
        s.document.model().entities.find(idOf(reply[0]))->geometry);
    EXPECT_NEAR(line.start.x, 0.0, 1e-6) << "a bearing of 0 is north: the line runs up the y axis";
    const auto quadrant = lines(s.ok("XLINE 0,0 angle=N90dE"));
    const auto& east = std::get<katana::geometry::Segment2>(
        s.document.model().entities.find(idOf(quadrant[0]))->geometry);
    EXPECT_NEAR(east.start.y, 0.0, 1e-6);
}

TEST(DrawingVerbs, DlineDrawsBothSidesOfThePath)
{
    Session s;
    const auto reply = lines(s.ok("DLINE 0,0 10,0 10,10 width=2"));
    ASSERT_EQ(reply.size(), 2u);
    const auto left = s.polyline(idOf(reply[0]));
    const auto right = s.polyline(idOf(reply[1]));
    EXPECT_NEAR(left.vertices.front().position.y, 1.0, 1e-9);
    EXPECT_NEAR(right.vertices.front().position.y, -1.0, 1e-9);
    ASSERT_TRUE(s.document.undo());
    EXPECT_EQ(s.document.model().entities.size(), 0u) << "both sides are one step";
    EXPECT_EQ(s.fails("DLINE 0,0 10,0"), ErrorCode::InvalidArgument);
}

// ---- drafting settings --------------------------------------------------------------------------

TEST(DrawingVerbs, DraftingVerbsSetTheSharedSettingsAndReplyWithThemAll)
{
    Session s;
    int changes = 0;
    const auto listener = s.document.addListener([&changes] { ++changes; });
    std::string reply = s.ok("ORTHO on");
    EXPECT_TRUE(s.document.drafting().ortho);
    EXPECT_EQ(value(reply, "ortho"), "on");
    EXPECT_EQ(value(reply, "polar"), "off");
    EXPECT_GE(changes, 1) << "the views hear of it";
    reply = s.ok("POLAR on increment=22.5");
    EXPECT_NEAR(s.document.drafting().polarIncrement, 22.5 * katana::math::kDegToRad, 1e-12);
    EXPECT_EQ(value(reply, "increment"), "22.5");
    s.ok("TRACKING on");
    EXPECT_TRUE(s.document.drafting().objectTracking);
    s.ok("ANGLES bearing");
    EXPECT_EQ(s.document.drafting().angles, AngleConvention::Bearing);
    reply = s.ok("LOCK angle=90 length=12.5");
    ASSERT_TRUE(s.document.drafting().angleLock.has_value());
    EXPECT_NEAR(*s.document.drafting().angleLock, 0.0, 1e-12) << "a bearing of 90 is east";
    EXPECT_EQ(value(reply, "lengthlock"), "12.5");
    s.ok("LOCK OFF");
    EXPECT_FALSE(s.document.drafting().angleLock.has_value());
    EXPECT_FALSE(s.document.drafting().lengthLock.has_value());
    EXPECT_EQ(s.fails("ORTHO sideways"), ErrorCode::ParseFailure);
    EXPECT_EQ(s.fails("POLAR increment=400"), ErrorCode::InvalidArgument);
    EXPECT_EQ(s.ok("DRAFTING"), s.ok("ORTHO"));
}

TEST(DrawingVerbs, SnapSetsAddsAndRemovesModesByName)
{
    Session s;
    std::string reply = s.ok("SNAP modes=endpoint,quadrant,node");
    EXPECT_EQ(s.document.drafting().snapModes,
              SnapMode::Endpoint | SnapMode::Quadrant | SnapMode::Node);
    EXPECT_EQ(value(reply, "modes"), "endpoint,quadrant,node");
    s.ok("SNAP add=apparentintersection,extension remove=node");
    EXPECT_TRUE(hasMode(s.document.drafting().snapModes, SnapMode::ApparentIntersection));
    EXPECT_TRUE(hasMode(s.document.drafting().snapModes, SnapMode::Extension));
    EXPECT_FALSE(hasMode(s.document.drafting().snapModes, SnapMode::Node));
    s.ok("OSNAP off");
    EXPECT_FALSE(s.document.drafting().snapEnabled);
    reply = s.ok("SNAP on modes=all");
    EXPECT_EQ(s.document.drafting().snapModes, kAllSnapModes);
    EXPECT_EQ(s.fails("SNAP modes=endpoint,sideways"), ErrorCode::ParseFailure);
}

TEST(DrawingVerbs, TheDraftingRecordReadsBackAsTheSameSettings)
{
    // Every mode's name is one word (a blank would end the value for any
    // key=value reader), and SNAP modes= takes the record's own list.
    Session s;
    const std::string all = value(s.ok("SNAP on modes=all"), "modes");
    EXPECT_EQ(all.find(' '), std::string::npos) << all;
    EXPECT_NE(all.find("apparentintersection"), std::string::npos) << all;
    const SnapModes written = s.document.drafting().snapModes;
    s.ok("SNAP modes=none");
    s.ok("SNAP modes=" + all);
    EXPECT_EQ(s.document.drafting().snapModes, written) << "the record did not read back";
    // The default increment is 15 degrees, held in radians; said as 15.
    Session fresh;
    EXPECT_EQ(value(fresh.ok("DRAFTING"), "increment"), "15");
}

TEST(DrawingVerbs, DrawVerbPointsFollowTheAngleConventionToo)
{
    Session s;
    s.ok("ANGLES bearing");
    const auto p = s.polyline(idOf(s.ok("PLINE3D 0,0,1 @10<90")));
    EXPECT_NEAR(p.vertices[1].position.x, 10.0, 1e-9) << "a bearing of 90 is east";
    EXPECT_NEAR(p.vertices[1].position.y, 0.0, 1e-9);
}
