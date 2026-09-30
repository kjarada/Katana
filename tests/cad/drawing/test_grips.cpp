// Grips (include/katana/cad/drawing/grips.hpp): what each geometry kind
// offers and what dragging its grips does, headless - the plan view only
// draws them and forwards the drag.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/cad/document.hpp"
#include "katana/cad/drawing/grips.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::CurvePolyline2;
using katana::geometry::Ellipse2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Spline2;
using katana::geometry::Vec2;
using katana::math::kHalfPi;
using katana::math::kPi;

namespace {

Entity entityOf(katana::entity::Geometry geometry, EntityId id = 7)
{
    Entity entity;
    entity.id = id;
    entity.geometry = std::move(geometry);
    return entity;
}

Grip gripOf(const Entity& entity, GripKind kind, std::size_t index = 0)
{
    for (const Grip& grip : gripsOf(entity)) {
        if (grip.kind == kind && grip.index == index) {
            return grip;
        }
    }
    ADD_FAILURE() << "no " << toString(kind) << " grip " << index;
    return Grip{};
}

Entity dragged(const Entity& entity, GripKind kind, std::size_t index, const Point2& to,
               bool insert = false)
{
    GripDrag drag;
    drag.grabbed = gripOf(entity, kind, index);
    drag.target = to;
    drag.insertVertex = insert;
    auto out = applyGripDrag(entity, drag);
    EXPECT_TRUE(out.ok()) << (out.ok() ? "" : out.error().message);
    return out.ok() ? *out : entity;
}

void expectNear(const Point2& a, const Point2& b, double tolerance = 1e-9)
{
    EXPECT_NEAR(a.x, b.x, tolerance);
    EXPECT_NEAR(a.y, b.y, tolerance);
}

EntityId add(Document& document, katana::entity::Geometry geometry)
{
    Entity entity;
    entity.geometry = std::move(geometry);
    EXPECT_TRUE(document.execute(katana::commands::createEntities({entity})).ok());
    return document.lastCreatedEntities().front();
}

} // namespace

// ---- enumeration per kind --------------------------------------------------------------

TEST(Grips, EachKindOffersItsHandles)
{
    EXPECT_EQ(gripsOf(entityOf(katana::entity::PointGeometry{Point2(1, 2)})).size(), 1u);
    EXPECT_EQ(gripsOf(entityOf(Segment2{Point2(0, 0), Point2(4, 0)})).size(), 3u);
    EXPECT_EQ(gripsOf(entityOf(Arc2{Point2(0, 0), 2.0, 0.0, kHalfPi})).size(), 4u);
    EXPECT_EQ(gripsOf(entityOf(Circle2{Point2(0, 0), 2.0})).size(), 5u);
    // A closed square: four vertices and four segment middles.
    const auto square = gripsOf(entityOf(Polyline2{
        {Point2(0, 0), Point2(4, 0), Point2(4, 4), Point2(0, 4)}, true}));
    ASSERT_EQ(square.size(), 8u);
    EXPECT_EQ(square[0].kind, GripKind::Vertex);
    EXPECT_EQ(square[4].kind, GripKind::SegmentMid);
    expectNear(square[7].position, Point2(0, 2)); // the closing segment's middle
    // A curve polyline's arc segment has its middle ON the arc.
    CurvePolyline2 bent = CurvePolyline2::fromPoints({Point2(0, 0), Point2(2, 0)});
    bent.vertices[0].bulge = 1.0;
    const auto arcGrips = gripsOf(entityOf(bent));
    ASSERT_EQ(arcGrips.size(), 3u);
    expectNear(arcGrips[2].position, Point2(1, -1));
    // A full ellipse: centre and four axis ends; an arc of it adds its ends.
    Ellipse2 ellipse;
    ellipse.majorAxis = Vec2(4, 0);
    ellipse.ratio = 0.5;
    EXPECT_EQ(gripsOf(entityOf(ellipse)).size(), 5u);
    ellipse.sweep = kHalfPi;
    EXPECT_EQ(gripsOf(entityOf(ellipse)).size(), 1u + 2u + 2u);
    // A fit spline by its fit points.
    const auto spline = *Spline2::throughPoints({Point2(0, 0), Point2(2, 3), Point2(5, 0)}, 3);
    const auto splineGrips = gripsOf(entityOf(spline));
    ASSERT_EQ(splineGrips.size(), 3u);
    EXPECT_EQ(splineGrips[1].kind, GripKind::FitPoint);
    EXPECT_TRUE(gripsOf(entityOf(katana::entity::DimensionGeometry{Point2(0, 0), Point2(1, 0), 1.0, ""}))
                    .empty())
        << "annotation is the annotation system's to give handles to";
}

TEST(Grips, GripAtFindsTheNearestWithinTheAperture)
{
    const auto grips = gripsOf(entityOf(Segment2{Point2(0, 0), Point2(4, 0)}));
    const auto found = gripAt(grips, Point2(3.9, 0.05), 0.2);
    ASSERT_TRUE(found.has_value());
    EXPECT_EQ(found->kind, GripKind::End);
    EXPECT_EQ(found->index, 1u);
    EXPECT_FALSE(gripAt(grips, Point2(3.0, 1.0), 0.2).has_value());
}

// ---- dragging per kind -------------------------------------------------------------------

TEST(GripDrags, AVertexGripMovesTheVertexAndKeepsTheArcs)
{
    CurvePolyline2 p = CurvePolyline2::fromPoints({Point2(0, 0), Point2(4, 0), Point2(8, 0)});
    p.vertices[0].bulge = 0.5;
    p.vertices[1].height = 12.0;
    const Entity out = dragged(entityOf(p), GripKind::Vertex, 1, Point2(4, 3));
    const auto& moved = std::get<CurvePolyline2>(out.geometry);
    EXPECT_EQ(moved.vertices[1].position, Point2(4, 3));
    EXPECT_EQ(moved.vertices[0].bulge, 0.5);
    EXPECT_EQ(moved.vertices[1].height, 12.0);
}

TEST(GripDrags, AStraightPolylineStaysAPolylineWithItsHeights)
{
    Entity e = entityOf(Polyline2{{Point2(0, 0), Point2(4, 0), Point2(8, 0)}, false});
    katana::entity::setHeights(e.properties, {1.0, 2.0, 3.0});
    const Entity out = dragged(e, GripKind::Vertex, 2, Point2(8, 5));
    ASSERT_TRUE(std::holds_alternative<Polyline2>(out.geometry));
    EXPECT_EQ(std::get<Polyline2>(out.geometry).vertices[2], Point2(8, 5));
    EXPECT_EQ(katana::entity::heightsOf(out.properties, 3)[2], 3.0);
}

TEST(GripDrags, ASegmentMiddleStretchesOrInsertsAVertex)
{
    const Entity e = entityOf(Polyline2{{Point2(0, 0), Point2(4, 0), Point2(4, 4)}, false});
    // Drag: the whole first segment moves up by 1 (both its vertices).
    const Entity stretched = dragged(e, GripKind::SegmentMid, 0, Point2(2, 1));
    const auto& s = std::get<Polyline2>(stretched.geometry);
    EXPECT_EQ(s.vertices[0], Point2(0, 1));
    EXPECT_EQ(s.vertices[1], Point2(4, 1));
    EXPECT_EQ(s.vertices[2], Point2(4, 4));
    // Ctrl-drag: a new vertex where the middle was dragged to.
    const Entity inserted = dragged(e, GripKind::SegmentMid, 0, Point2(2, -1), true);
    const auto& i = std::get<Polyline2>(inserted.geometry);
    ASSERT_EQ(i.vertices.size(), 4u);
    EXPECT_EQ(i.vertices[1], Point2(2, -1));
}

TEST(GripDrags, AnArcSegmentsMiddleReshapesTheArcThroughTheTarget)
{
    CurvePolyline2 p = CurvePolyline2::fromPoints({Point2(0, 0), Point2(2, 0)});
    p.vertices[0].bulge = 1.0; // through (1,-1)
    const Entity out = dragged(entityOf(p), GripKind::SegmentMid, 0, Point2(1, -0.5));
    const auto& moved = std::get<CurvePolyline2>(out.geometry);
    EXPECT_EQ(moved.vertices[0].position, Point2(0, 0));
    EXPECT_EQ(moved.vertices[1].position, Point2(2, 0));
    const auto arc = std::get<Arc2>(moved.segment(0));
    expectNear(arc.midpoint(), Point2(1, -0.5));
    // Dragged back onto the chord it is refused, not made into nonsense.
    GripDrag flat;
    flat.grabbed = gripOf(entityOf(p), GripKind::SegmentMid, 0);
    flat.target = Point2(1, 0);
    EXPECT_FALSE(applyGripDrag(entityOf(p), flat).ok());
}

TEST(GripDrags, SeveralHotGripsMoveTogether)
{
    const Entity e = entityOf(Polyline2{
        {Point2(0, 0), Point2(4, 0), Point2(8, 0), Point2(12, 0)}, false});
    GripDrag drag;
    drag.grabbed = gripOf(e, GripKind::Vertex, 1);
    drag.alsoHot = {gripOf(e, GripKind::Vertex, 2), gripOf(e, GripKind::SegmentMid, 1)};
    drag.target = Point2(4, 2);
    const auto out = applyGripDrag(e, drag);
    ASSERT_TRUE(out.ok());
    const auto& moved = std::get<Polyline2>(out->geometry);
    // Each vertex moves once, however many hot grips name it.
    EXPECT_EQ(moved.vertices[1], Point2(4, 2));
    EXPECT_EQ(moved.vertices[2], Point2(8, 2));
    EXPECT_EQ(moved.vertices[3], Point2(12, 0));
}

TEST(GripDrags, LinesArcsAndCircles)
{
    const Entity line = entityOf(Segment2{Point2(0, 0), Point2(4, 0)});
    EXPECT_EQ(std::get<Segment2>(dragged(line, GripKind::End, 1, Point2(4, 3)).geometry).end,
              Point2(4, 3));
    const auto whole = std::get<Segment2>(dragged(line, GripKind::Mid, 0, Point2(2, 1)).geometry);
    EXPECT_EQ(whole.start, Point2(0, 1));
    EXPECT_EQ(whole.end, Point2(4, 1));

    const Entity arc = entityOf(Arc2{Point2(0, 0), 2.0, 0.0, kPi});
    const auto moved = std::get<Arc2>(dragged(arc, GripKind::Centre, 0, Point2(1, 1)).geometry);
    expectNear(moved.center, Point2(1, 1));
    EXPECT_NEAR(moved.radius, 2.0, 1e-12);
    // Its middle dragged up to (0, 3): the arc through (2,0), (0,3), (-2,0).
    const auto reshaped = std::get<Arc2>(dragged(arc, GripKind::Mid, 0, Point2(0, 3)).geometry);
    expectNear(reshaped.midpoint(), Point2(0, 3));
    expectNear(reshaped.startPoint(), Point2(2, 0));

    const Entity circle = entityOf(Circle2{Point2(0, 0), 2.0});
    const auto bigger =
        std::get<Circle2>(dragged(circle, GripKind::Quadrant, 1, Point2(0, 5)).geometry);
    EXPECT_NEAR(bigger.radius, 5.0, 1e-12);
    GripDrag collapse;
    collapse.grabbed = gripOf(circle, GripKind::Quadrant, 0);
    collapse.target = Point2(0, 0);
    EXPECT_FALSE(applyGripDrag(circle, collapse).ok()) << "a circle of no radius";
}

TEST(GripDrags, EllipsesAndSplines)
{
    Ellipse2 ellipse;
    ellipse.majorAxis = Vec2(4, 0);
    ellipse.ratio = 0.5;
    const Entity e = entityOf(ellipse);
    const auto longer = std::get<Ellipse2>(dragged(e, GripKind::Quadrant, 0, Point2(8, 0)).geometry);
    EXPECT_NEAR(longer.majorRadius(), 8.0, 1e-12);
    EXPECT_NEAR(longer.minorRadius(), 2.0, 1e-12);
    const auto fatter = std::get<Ellipse2>(dragged(e, GripKind::Quadrant, 1, Point2(0, 3)).geometry);
    EXPECT_NEAR(fatter.minorRadius(), 3.0, 1e-12);
    // Past the major axis on a full ellipse, the axes swap.
    const auto swapped = std::get<Ellipse2>(dragged(e, GripKind::Quadrant, 1, Point2(0, 6)).geometry);
    EXPECT_NEAR(swapped.majorRadius(), 6.0, 1e-12);
    EXPECT_NEAR(swapped.minorRadius(), 4.0, 1e-12);

    const auto spline = *Spline2::throughPoints({Point2(0, 0), Point2(2, 3), Point2(5, 0)}, 3);
    const auto moved = std::get<Spline2>(
        dragged(entityOf(spline), GripKind::FitPoint, 1, Point2(2, 5)).geometry);
    EXPECT_EQ(moved.fitPoints[1], Point2(2, 5));
    EXPECT_LT(moved.distanceTo(Point2(2, 5)), 1e-3) << "re-solved through the moved point";
}

TEST(GripDrags, TextAndPointMoveByTheirInsertionGrip)
{
    const Entity text = entityOf(katana::entity::TextGeometry{Point2(1, 1), "A", 2.5, 0.0});
    EXPECT_EQ(std::get<katana::entity::TextGeometry>(
                  dragged(text, GripKind::Insertion, 0, Point2(5, 6)).geometry)
                  .position,
              Point2(5, 6));
}

// ---- as commands: one undo step ------------------------------------------------------------

TEST(GripCommands, AGripDragIsOneUndoStepAcrossEntities)
{
    Document document;
    const EntityId a = add(document, Polyline2{{Point2(0, 0), Point2(4, 0), Point2(8, 0)}, false});
    const EntityId b = add(document, Segment2{Point2(4, 0), Point2(4, 5)});
    const std::size_t before = document.history().undoCount();

    const auto gripsA = gripsOf(*document.model().entities.find(a));
    const auto gripsB = gripsOf(*document.model().entities.find(b));
    GripDrag drag;
    drag.grabbed = gripsA[1];  // vertex 1 at (4,0)
    drag.alsoHot = {gripsB[0]}; // the line's start, also at (4,0)
    drag.target = Point2(4, -2);
    ASSERT_TRUE(document.execute(gripDragCommand(drag)).ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(std::get<Polyline2>(document.model().entities.find(a)->geometry).vertices[1],
              Point2(4, -2));
    EXPECT_EQ(std::get<Segment2>(document.model().entities.find(b)->geometry).start, Point2(4, -2));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(std::get<Polyline2>(document.model().entities.find(a)->geometry).vertices[1],
              Point2(4, 0));
    EXPECT_EQ(std::get<Segment2>(document.model().entities.find(b)->geometry).start, Point2(4, 0));
}

TEST(GripCommands, DeleteOnHotVertexGripsRemovesThemInOneStep)
{
    Document document;
    const EntityId id = add(document, Polyline2{
        {Point2(0, 0), Point2(2, 1), Point2(4, 0), Point2(6, 1), Point2(8, 0)}, false});
    const auto grips = gripsOf(*document.model().entities.find(id));
    const std::size_t before = document.history().undoCount();
    ASSERT_TRUE(document.execute(deleteHotVertices({grips[1], grips[3]})).ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(std::get<Polyline2>(document.model().entities.find(id)->geometry).vertices.size(), 3u);
    // Segment middles are not vertices: nothing to delete is refused.
    EXPECT_FALSE(document.execute(deleteHotVertices({grips[6]})).ok());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(std::get<Polyline2>(document.model().entities.find(id)->geometry).vertices.size(), 5u);
}

TEST(GripCommands, TheSelectionsGripsSkipLockedLayersAndHonourTheLimit)
{
    Document document;
    const EntityId id = add(document, Segment2{Point2(0, 0), Point2(1, 0)});
    EXPECT_EQ(gripsOfSelection(document, {id}, kNoLayerOverrides).size(), 3u);
    EXPECT_TRUE(gripsOfSelection(document, {id}, kNoLayerOverrides, 2).empty())
        << "past the limit, none";
    const auto preview = gripPreview(
        document, GripDrag{gripsOf(*document.model().entities.find(id))[1], Point2(1, 1), {}, false});
    ASSERT_EQ(preview.size(), 1u);
    EXPECT_EQ(std::get<Segment2>(preview[0]).end, Point2(1, 1));
}

TEST(GripCommands, AViewOffersNoGripOnWhatItHidesAndTheOthersKeepTheirs)
{
    // A design line and an as-built line, both selected; a view hiding the
    // design layer - the as-built view - offers the as-built line's three
    // grips (two ends and the middle) and none of the design line's, which
    // it shows as a ghost at most. The document's rule alone offers all six.
    Document document;
    for (const char* name : {"design", "asbuilt"}) {
        katana::entity::Layer layer;
        layer.name = name;
        ASSERT_TRUE(document.execute(katana::commands::createLayer(layer)).ok());
    }
    katana::commands::EntityAttributes onDesign;
    onDesign.layer = "design";
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(10, 10), Point2(40, 30), onDesign))
                    .ok());
    const EntityId design = document.lastCreatedEntities().front();
    katana::commands::EntityAttributes onAsBuilt;
    onAsBuilt.layer = "asbuilt";
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(10, 30), Point2(40, 10), onAsBuilt))
                    .ok());
    const EntityId asBuilt = document.lastCreatedEntities().front();

    LayerOverrides asBuiltView;
    ASSERT_TRUE(asBuiltView.hide("design"));
    const auto offered = gripsOfSelection(document, {design, asBuilt}, asBuiltView);
    ASSERT_EQ(offered.size(), 3u);
    for (const Grip& grip : offered) {
        EXPECT_EQ(grip.entity, asBuilt);
    }
    // Where the two cross, (25, 20), both lines have their middles: in the
    // as-built view the grip there is the as-built line's.
    const auto atCrossing = gripAt(offered, Point2(25, 20), 0.5);
    ASSERT_TRUE(atCrossing.has_value());
    EXPECT_EQ(atCrossing->entity, asBuilt);
    // The design line alone selected, the as-built view offers nothing.
    EXPECT_TRUE(gripsOfSelection(document, {design}, asBuiltView).empty());
    EXPECT_EQ(gripsOfSelection(document, {design, asBuilt}, kNoLayerOverrides).size(), 6u);
}
