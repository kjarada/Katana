#include <gtest/gtest.h>

#include <cmath>
#include <filesystem>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/cad/view_transform.hpp"
#include "support/property.hpp"

using namespace katana::cad;
namespace fs = std::filesystem;
namespace cmd = katana::commands;
using katana::core::ErrorCode;
using katana::entity::EntityType;
using katana::entity::Layer;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
using katana::math::kHalfPi;
using katana::math::nearlyEqual;

namespace {

EntityId mustCreate(Document& document, cmd::CommandPtr command)
{
    const auto status = document.execute(std::move(command));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    const auto created = document.lastCreatedEntities();
    return created.empty() ? 0 : created.front();
}

} // namespace

// ---- ViewTransform -------------------------------------------------------------

TEST(CadViewTransform, MapsModelSpaceToPixelsWithYFlipped)
{
    ViewTransform view;
    view.resize(800, 600);
    view.center = Point2(100, 50);
    view.scale = 2.0;

    EXPECT_EQ(view.worldToScreen(Point2(100, 50)), Point2(400, 300)); // centre
    EXPECT_EQ(view.worldToScreen(Point2(110, 60)), Point2(420, 280)); // +y is up on screen
    EXPECT_EQ(view.screenToWorld(Point2(420, 280)), Point2(110, 60));
    EXPECT_DOUBLE_EQ(view.pixelsToWorld(10.0), 5.0);

    const Box2 visible = view.visibleWorldBounds();
    EXPECT_EQ(visible.min, Point2(-100, -100));
    EXPECT_EQ(visible.max, Point2(300, 200));
}

TEST(CadViewTransform, PanningFollowsTheCursor)
{
    ViewTransform view;
    view.resize(800, 600);
    view.scale = 4.0;
    const Point2 grabbed = view.screenToWorld(Point2(200, 200));
    view.panByPixels(40, -20); // drag right and up
    EXPECT_TRUE(nearlyEqual(view.screenToWorld(Point2(240, 180)), grabbed));
}

TEST(CadViewTransform, ZoomKeepsThePointUnderTheCursorFixed)
{
    katana::test::Random random;
    ViewTransform view;
    view.resize(1280, 720);
    view.center = Point2(500000, 5000000); // survey magnitudes
    view.scale = 3.0;
    // Model coordinates near 5e6 are only representable to one ulp (~9.3e-10),
    // which is ulp * scale pixels on screen. A few such roundings are the best
    // any implementation can do, so the bound scales with the zoom level.
    const double ulp = std::nextafter(5000000.0, 1e7) - 5000000.0;
    for (int i = 0; i < 200; ++i) {
        const Point2 cursor(random.real(0, 1280), random.real(0, 720));
        const Point2 anchor = view.screenToWorld(cursor);
        view.zoomAt(cursor, random.real(0.5, 2.0));
        const Point2 after = view.worldToScreen(anchor);
        const double bound = 1e-9 + 8.0 * ulp * view.scale;
        EXPECT_NEAR(after.x, cursor.x, bound);
        EXPECT_NEAR(after.y, cursor.y, bound);
    }
}

TEST(CadViewTransform, ZoomIsClampedAndRejectsNonsenseFactors)
{
    ViewTransform view;
    view.resize(800, 600);
    for (int i = 0; i < 200; ++i) {
        view.zoomAt(Point2(400, 300), 10.0);
    }
    EXPECT_DOUBLE_EQ(view.scale, ViewTransform::kMaximumScale);
    for (int i = 0; i < 400; ++i) {
        view.zoomAt(Point2(400, 300), 0.1);
    }
    EXPECT_DOUBLE_EQ(view.scale, ViewTransform::kMinimumScale);

    const double before = view.scale;
    view.zoomAt(Point2(0, 0), 0.0);
    view.zoomAt(Point2(0, 0), -3.0);
    view.zoomAt(Point2(0, 0), std::nan(""));
    EXPECT_DOUBLE_EQ(view.scale, before);
}

TEST(CadViewTransform, FitShowsTheWholeDrawing)
{
    ViewTransform view;
    view.resize(1000, 500);
    view.fit(Box2(Point2(0, 0), Point2(100, 100)), 0.0);
    EXPECT_EQ(view.center, Point2(50, 50));
    EXPECT_DOUBLE_EQ(view.scale, 5.0); // limited by the viewport height
    EXPECT_TRUE(view.visibleWorldBounds().contains(Box2(Point2(0, 0), Point2(100, 100))));

    view.fit(Box2(Point2(0, 0), Point2(100, 100)), 0.1);
    EXPECT_DOUBLE_EQ(view.scale, 4.0); // 10% margin on each side

    view.fit(Box2{}, 0.05); // empty drawing
    EXPECT_EQ(view.center, Point2(0, 0));
    EXPECT_DOUBLE_EQ(view.scale, 1.0);

    view.scale = 7.0;
    Box2 single;
    single.expand(Point2(3, 4));
    view.fit(single); // a lone point: centre on it, keep the zoom
    EXPECT_EQ(view.center, Point2(3, 4));
    EXPECT_DOUBLE_EQ(view.scale, 7.0);

    // A box that HAS extent but is far too small to fill the window at maximum
    // zoom. Fitting it clamps to kMaximumScale; it must not be mistaken for the
    // lone point above and leave the zoom alone, or Zoom Extents on a tiny
    // drawing would leave it invisible.
    view.scale = 7.0;
    const Box2 tiny(Point2(10.0, 10.0), Point2(10.000001, 10.000001));
    view.fit(tiny, 0.0);
    EXPECT_DOUBLE_EQ(view.scale, ViewTransform::kMaximumScale);
    EXPECT_TRUE(view.visibleWorldBounds().contains(tiny));
}

TEST(CadViewTransform, GridSpacingFollowsA125Sequence)
{
    EXPECT_DOUBLE_EQ(gridSpacing(1.0, 12.0), 20.0);   // needs >= 12 units
    EXPECT_DOUBLE_EQ(gridSpacing(12.0, 12.0), 1.0);   // needs >= 1 unit
    EXPECT_DOUBLE_EQ(gridSpacing(10.0, 12.0), 2.0);   // needs >= 1.2
    EXPECT_DOUBLE_EQ(gridSpacing(4.0, 12.0), 5.0);    // needs >= 3
    EXPECT_NEAR(gridSpacing(1000.0, 12.0), 0.02, 1e-15); // needs >= 0.012
    EXPECT_DOUBLE_EQ(gridSpacing(0.0, 12.0), 1.0);    // defensive default
    for (double scale = 1e-4; scale < 1e4; scale *= 1.7) {
        EXPECT_GE(gridSpacing(scale, 12.0) * scale, 12.0 * (1.0 - 1e-12)) << scale;
        EXPECT_LE(gridSpacing(scale, 12.0) * scale, 30.0 * (1.0 + 1e-12)) << scale;
    }
}

// ---- selection & picking --------------------------------------------------------

TEST(CadSelection, SetOperationsAndPruning)
{
    Document document;
    const EntityId a = mustCreate(document, cmd::createPoint(Point2(0, 0)));
    const EntityId b = mustCreate(document, cmd::createPoint(Point2(1, 1)));

    SelectionSet& selection = document.selection();
    selection.set({b, a, b});
    EXPECT_EQ(selection.ids(), (std::vector<EntityId>{a, b})); // ordered, de-duplicated
    selection.toggle(a);
    EXPECT_FALSE(selection.contains(a));
    selection.toggle(a);
    EXPECT_TRUE(selection.contains(a));

    ASSERT_TRUE(document.execute(cmd::deleteEntities({a})).ok());
    EXPECT_EQ(selection.ids(), (std::vector<EntityId>{b})); // deleted entities leave the selection
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(selection.ids(), (std::vector<EntityId>{b})); // and do not sneak back in
}

TEST(CadPicking, PicksTheNearestEntityWithinTolerance)
{
    Document document;
    const EntityId line = mustCreate(document, cmd::createLine(Point2(0, 0), Point2(10, 0)));
    const EntityId circle = mustCreate(document, cmd::createCircle(Point2(5, 5), 2.0));
    const auto& model = document.model();

    EXPECT_EQ(pickEntity(model, Point2(5, 0.2), 0.5), line);
    EXPECT_EQ(pickEntity(model, Point2(5, 2.9), 0.5), circle); // near the circumference
    EXPECT_FALSE(pickEntity(model, Point2(5, 5), 0.5).has_value()); // the inside is not the curve
    EXPECT_FALSE(pickEntity(model, Point2(50, 50), 0.5).has_value());

    SelectionFilter circlesOnly;
    circlesOnly.types.insert(EntityType::Circle);
    EXPECT_FALSE(pickEntity(model, Point2(5, 0.2), 0.5, circlesOnly).has_value());
}

TEST(CadPicking, CoincidentEntitiesPickTheOneDrawnLast)
{
    Document document;
    mustCreate(document, cmd::createLine(Point2(0, 0), Point2(10, 0)));
    const EntityId onTop = mustCreate(document, cmd::createLine(Point2(0, 0), Point2(10, 0)));
    EXPECT_EQ(pickEntity(document.model(), Point2(5, 0), 0.5), onTop);
}

TEST(CadPicking, HiddenAndLockedEntitiesCannotBePicked)
{
    Document document;
    ASSERT_TRUE(document.execute(cmd::createLayer(Layer{"Hidden"})).ok());
    ASSERT_TRUE(document.execute(cmd::createLayer(Layer{"Locked"})).ok());
    mustCreate(document, cmd::createLine(Point2(0, 0), Point2(10, 0), {"Hidden", "", {}}));
    mustCreate(document, cmd::createLine(Point2(0, 5), Point2(10, 5), {"Locked", "", {}}));
    const EntityId invisible = mustCreate(document, cmd::createLine(Point2(0, 9), Point2(10, 9)));

    Layer hidden = *document.model().layers.find("Hidden");
    hidden.visible = false;
    Layer locked = *document.model().layers.find("Locked");
    locked.locked = true;
    ASSERT_TRUE(document.execute(cmd::updateLayer(hidden)).ok());
    ASSERT_TRUE(document.execute(cmd::updateLayer(locked)).ok());
    ASSERT_TRUE(document.execute(cmd::setEntityVisible({invisible}, false)).ok());

    const auto& model = document.model();
    EXPECT_FALSE(pickEntity(model, Point2(5, 0), 0.5).has_value());
    EXPECT_FALSE(pickEntity(model, Point2(5, 5), 0.5).has_value());
    EXPECT_FALSE(pickEntity(model, Point2(5, 9), 0.5).has_value());
    EXPECT_TRUE(pickInBox(model, Box2(Point2(-1, -1), Point2(11, 11)), BoxSelectionMode::Crossing)
                    .empty());
}

TEST(CadPicking, WindowNeedsFullContainmentCrossingOnlyContact)
{
    Document document;
    const EntityId inside = mustCreate(document, cmd::createLine(Point2(1, 1), Point2(4, 4)));
    const EntityId crossing = mustCreate(document, cmd::createLine(Point2(3, 3), Point2(20, 3)));
    const EntityId bigCircle = mustCreate(document, cmd::createCircle(Point2(2.5, 2.5), 50.0));
    const EntityId cutsCorner = mustCreate(document, cmd::createCircle(Point2(0, 0), 1.0));
    mustCreate(document, cmd::createLine(Point2(30, 30), Point2(40, 40))); // far away
    const auto& model = document.model();
    const Box2 box(Point2(0, 0), Point2(5, 5));

    EXPECT_EQ(pickInBox(model, box, BoxSelectionMode::Window), (std::vector<EntityId>{inside}));
    // The big circle's bounding box overlaps the selection box, but its curve
    // never comes near it: crossing selection must not grab it.
    EXPECT_EQ(pickInBox(model, box, BoxSelectionMode::Crossing),
              (std::vector<EntityId>{inside, crossing, cutsCorner}));
    (void)bigCircle;
    EXPECT_TRUE(pickInBox(model, Box2{}, BoxSelectionMode::Crossing).empty());
}

// ---- snapping ---------------------------------------------------------------------

namespace {

SnapRequest request(const Point2& cursor, SnapModes modes, double aperture = 0.5)
{
    SnapRequest r;
    r.cursor = cursor;
    r.modes = modes;
    r.aperture = aperture;
    return r;
}

} // namespace

TEST(CadSnapping, EndpointMidpointCenterIntersection)
{
    Document document;
    mustCreate(document, cmd::createLine(Point2(0, 0), Point2(10, 0)));
    mustCreate(document, cmd::createLine(Point2(4, -5), Point2(4, 5)));
    mustCreate(document, cmd::createCircle(Point2(20, 20), 3.0));
    const auto& model = document.model();

    const auto endpoint = snap(model, request(Point2(9.8, 0.2), kAllSnapModes));
    ASSERT_TRUE(endpoint.has_value());
    EXPECT_EQ(endpoint->mode, SnapMode::Endpoint);
    EXPECT_EQ(endpoint->point, Point2(10, 0));

    const auto midpoint = snap(model, request(Point2(5.2, 0.1), kDefaultSnapModes));
    ASSERT_TRUE(midpoint.has_value());
    EXPECT_EQ(midpoint->mode, SnapMode::Midpoint);
    EXPECT_EQ(midpoint->point, Point2(5, 0));

    const auto intersection = snap(model, request(Point2(4.1, 0.1), kDefaultSnapModes));
    ASSERT_TRUE(intersection.has_value());
    EXPECT_EQ(intersection->mode, SnapMode::Intersection);
    EXPECT_TRUE(nearlyEqual(intersection->point, Point2(4, 0)));

    // Hovering the circumference offers the centre.
    const auto center = snap(model, request(Point2(23.1, 20), kDefaultSnapModes));
    ASSERT_TRUE(center.has_value());
    EXPECT_EQ(center->mode, SnapMode::Center);
    EXPECT_EQ(center->point, Point2(20, 20));

    EXPECT_FALSE(snap(model, request(Point2(50, 50), kDefaultSnapModes)).has_value());
}

TEST(CadSnapping, ExactSnapsBeatNearestAndNearestBeatsGrid)
{
    Document document;
    mustCreate(document, cmd::createLine(Point2(0, 0), Point2(10, 0)));
    const auto& model = document.model();

    SnapRequest r = request(Point2(2.3, 0.2), kAllSnapModes);
    r.gridSpacing = 1.0;
    const auto nearest = snap(model, r); // no endpoint or midpoint within 0.5
    ASSERT_TRUE(nearest.has_value());
    EXPECT_EQ(nearest->mode, SnapMode::Nearest);
    EXPECT_TRUE(nearlyEqual(nearest->point, Point2(2.3, 0)));

    r.cursor = Point2(0.3, 0.2); // now the endpoint is in range too
    EXPECT_EQ(snap(model, r)->mode, SnapMode::Endpoint);

    r.cursor = Point2(2.3, 7.4); // nothing nearby: fall through to the grid
    const auto grid = snap(model, r);
    ASSERT_TRUE(grid.has_value());
    EXPECT_EQ(grid->mode, SnapMode::Grid);
    EXPECT_EQ(grid->point, Point2(2, 7));

    r.modes = SnapMode::Endpoint | SnapMode::Midpoint; // grid not enabled
    EXPECT_FALSE(snap(model, r).has_value());
}

TEST(CadSnapping, PerpendicularAndTangentNeedAStartPoint)
{
    Document document;
    mustCreate(document, cmd::createLine(Point2(0, 0), Point2(10, 0)));
    mustCreate(document, cmd::createCircle(Point2(20, 0), 5.0));
    const auto& model = document.model();

    SnapRequest r = request(Point2(3.1, 0.1), SnapMode::Perpendicular | SnapMode::Tangent);
    EXPECT_FALSE(snap(model, r).has_value()); // no `from`: these modes cannot apply

    r.from = Point2(3, 8);
    const auto foot = snap(model, r);
    ASSERT_TRUE(foot.has_value());
    EXPECT_EQ(foot->mode, SnapMode::Perpendicular);
    EXPECT_TRUE(nearlyEqual(foot->point, Point2(3, 0)));

    // From (20, 13): tangent points satisfy |CT| = 5 and CT . FT = 0.
    r.from = Point2(20, 13);
    const double tangentY = 25.0 / 13.0; // r^2 / d above the centre
    const double tangentX = std::sqrt(25.0 - tangentY * tangentY);
    r.cursor = Point2(20 + tangentX + 0.1, tangentY);
    const auto tangent = snap(model, r);
    ASSERT_TRUE(tangent.has_value());
    EXPECT_EQ(tangent->mode, SnapMode::Tangent);
    EXPECT_TRUE(nearlyEqual(tangent->point, Point2(20 + tangentX, tangentY), 1e-9));
    const Vec2 radius = tangent->point - Point2(20, 0);
    const Vec2 tangentLine = tangent->point - *r.from;
    EXPECT_NEAR(radius.dot(tangentLine), 0.0, 1e-9);

    r.from = Point2(21, 0); // inside the circle: no tangent exists
    r.modes = static_cast<SnapModes>(SnapMode::Tangent);
    EXPECT_FALSE(snap(model, r).has_value());
}

TEST(CadSnapping, ArcSnapsRespectTheSweepAndFindTheOffExtentCentre)
{
    Document document;
    // A shallow arc high on a big circle: its centre is far outside its own extents.
    mustCreate(document, cmd::createArc(Arc2{Point2(0, 0), 100.0, kHalfPi - 0.05, 0.1}));
    const auto& model = document.model();

    const auto center = snap(model, request(Point2(0.1, 0.1), kDefaultSnapModes));
    ASSERT_TRUE(center.has_value());
    EXPECT_EQ(center->mode, SnapMode::Center);

    const auto mid = snap(model, request(Point2(0.1, 100.1), kDefaultSnapModes));
    ASSERT_TRUE(mid.has_value());
    EXPECT_EQ(mid->mode, SnapMode::Midpoint);
    EXPECT_TRUE(nearlyEqual(mid->point, Point2(0, 100), 1e-9));

    // The perpendicular foot on the far side of the circle is not on the arc.
    SnapRequest r = request(Point2(0, -100), static_cast<SnapModes>(SnapMode::Perpendicular));
    r.from = Point2(0, 50);
    EXPECT_FALSE(snap(model, r).has_value());
}

// ---- Document -----------------------------------------------------------------------

TEST(CadDocument, CurrentLayerDrivesNewEntitiesAndSurvivesUndo)
{
    Document document;
    ASSERT_TRUE(document.execute(cmd::createLayer(Layer{"Survey"})).ok());
    ASSERT_TRUE(document.setCurrentLayer("Survey").ok());
    EXPECT_EQ(document.currentAttributes().layer, "Survey");
    EXPECT_EQ(document.setCurrentLayer("Missing").error().code, ErrorCode::NotFound);

    ASSERT_TRUE(document.undo().ok()); // the layer no longer exists
    EXPECT_EQ(document.currentLayer(), "0");
}

TEST(CadDocument, NotifiesListenersOfModelSelectionAndLayerChanges)
{
    Document document;
    int notifications = 0;
    document.addListener([&] { ++notifications; });

    mustCreate(document, cmd::createPoint(Point2(0, 0)));
    EXPECT_EQ(notifications, 1);
    document.selection().set({1});
    document.notifySelectionChanged();
    EXPECT_EQ(notifications, 2);
    EXPECT_FALSE(document.execute(cmd::createCircle(Point2(0, 0), -1)).ok());
    EXPECT_EQ(notifications, 2); // failed commands change nothing
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(notifications, 3);
}

TEST(CadDocument, SaveReopenAndModifiedFlag)
{
    const fs::path directory = fs::temp_directory_path() / "katana-cad-tests" / "document.katana";
    fs::remove_all(directory.parent_path());
    {
        Document document;
        EXPECT_FALSE(document.isModified());
        EXPECT_EQ(document.save().error().code, ErrorCode::InvalidState); // nowhere to save yet

        ASSERT_TRUE(document.execute(cmd::createLayer(Layer{"Roads"})).ok());
        mustCreate(document, cmd::createLine(Point2(0, 0), Point2(10, 10), {"Roads", "", {}}));
        EXPECT_TRUE(document.isModified());

        const auto saved = document.saveAs(directory);
        ASSERT_TRUE(saved.ok()) << saved.error().describe();
        EXPECT_FALSE(document.isModified());
        EXPECT_EQ(document.metadata().name, "document");
        EXPECT_EQ(document.saveAs(directory).error().code, ErrorCode::AlreadyExists);

        mustCreate(document, cmd::createCircle(Point2(5, 5), 2.0));
        EXPECT_TRUE(document.isModified());
        ASSERT_TRUE(document.save().ok());
        EXPECT_FALSE(document.isModified());
        ASSERT_TRUE(document.undo().ok());
        EXPECT_TRUE(document.isModified()); // differs from what is on disk
    }
    {
        Document reopened;
        mustCreate(reopened, cmd::createPoint(Point2(99, 99))); // replaced by open()
        const auto opened = reopened.open(directory);
        ASSERT_TRUE(opened.ok()) << opened.error().describe();
        EXPECT_EQ(reopened.model().entities.size(), 2u);
        EXPECT_TRUE(reopened.model().layers.contains("Roads"));
        EXPECT_FALSE(reopened.isModified());
        EXPECT_FALSE(reopened.history().canUndo()); // history does not cross documents

        // Editing keeps working after a reload (the stack was rebuilt).
        const EntityId id = mustCreate(reopened, cmd::createPoint(Point2(1, 1)));
        EXPECT_EQ(id, 3u); // ids continue after the saved ones
        ASSERT_TRUE(reopened.undo().ok());

        EXPECT_FALSE(reopened.open(directory.parent_path() / "missing.katana").ok());
        EXPECT_EQ(reopened.model().entities.size(), 2u); // a failed open changes nothing

        reopened.newDocument();
        EXPECT_TRUE(reopened.model().entities.empty());
        EXPECT_FALSE(reopened.hasProject());
    }
    fs::remove_all(directory.parent_path());
}

// ---- CommandInterpreter ---------------------------------------------------------------

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
    // A missing entity is reported as a test failure, never a null dereference.
    const katana::entity::Entity& entity(EntityId id) const
    {
        static const katana::entity::Entity missing{};
        const katana::entity::Entity* found = document.model().entities.find(id);
        EXPECT_NE(found, nullptr) << "no entity with id " << id;
        return found != nullptr ? *found : missing;
    }
};

} // namespace

TEST(CadInterpreter, DrawsWithAbsoluteRelativeAndPolarPoints)
{
    Session s;
    s.ok("line 0,0 10,0");
    EXPECT_EQ(std::get<Segment2>(s.entity(1).geometry), (Segment2{Point2(0, 0), Point2(10, 0)}));

    s.ok("L 10,0 @0,5"); // alias + relative to the last point
    EXPECT_EQ(std::get<Segment2>(s.entity(2).geometry).end, Point2(10, 5));

    s.ok("LINE 0,0 @10<90"); // polar: 10 units at 90 degrees
    EXPECT_TRUE(nearlyEqual(std::get<Segment2>(s.entity(3).geometry).end, Point2(0, 10)));

    s.ok("CIRCLE 5,5 2.5");
    EXPECT_DOUBLE_EQ(std::get<Circle2>(s.entity(4).geometry).radius, 2.5);

    s.ok("ARC 1,0 0,1 -1,0");
    EXPECT_NEAR(std::get<Arc2>(s.entity(5).geometry).radius, 1.0, 1e-12);

    s.ok("PLINE 0,0 4,0 4,3 CLOSE");
    EXPECT_DOUBLE_EQ(std::get<Polyline2>(s.entity(6).geometry).area(), 6.0);

    s.ok("RECT 10,10 0,0");
    EXPECT_DOUBLE_EQ(std::get<Polyline2>(s.entity(7).geometry).area(), 100.0);

    s.ok("TEXT 1,1 2.5 \"Bench mark A\"");
    EXPECT_EQ(std::get<katana::entity::TextGeometry>(s.entity(8).geometry).text, "Bench mark A");

    s.ok("DIM 0,0 10,0 2");
    EXPECT_DOUBLE_EQ(std::get<katana::entity::DimensionGeometry>(s.entity(9).geometry).measurement(),
                     10.0);
    s.ok("POINT 3,4");
    EXPECT_EQ(s.document.model().entities.size(), 10u);
}

TEST(CadInterpreter, ChainedLineIsOneUndoStep)
{
    Session s;
    EXPECT_EQ(s.ok("LINE 0,0 10,0 10,10 0,10"), "3 lines created");
    EXPECT_EQ(s.document.model().entities.size(), 3u);
    s.ok("U");
    EXPECT_TRUE(s.document.model().entities.empty());
}

TEST(CadInterpreter, RejectsMalformedInputWithoutSideEffects)
{
    Session s;
    EXPECT_EQ(s.fails("FROBNICATE 1 2"), ErrorCode::ParseFailure);
    EXPECT_EQ(s.fails("LINE 0,0"), ErrorCode::InvalidArgument);        // usage
    EXPECT_EQ(s.fails("LINE 0,0 ten,0"), ErrorCode::ParseFailure);     // not a number
    EXPECT_EQ(s.fails("LINE 0,0 1,5x"), ErrorCode::ParseFailure);      // trailing junk
    EXPECT_EQ(s.fails("LINE 0,0 inf,0"), ErrorCode::ParseFailure);     // non-finite
    EXPECT_EQ(s.fails("CIRCLE 0,0 -5"), ErrorCode::InvalidGeometry);   // validated by the model
    EXPECT_EQ(s.fails("LINE 3,3 3,3"), ErrorCode::InvalidGeometry);    // zero length
    EXPECT_EQ(s.fails("ARC 0,0 1,1 2,2"), ErrorCode::InvalidGeometry); // collinear
    EXPECT_EQ(s.fails("TEXT 0,0 2.5 \"unterminated"), ErrorCode::ParseFailure);
    // An empty quoted argument. Title casing the type name starts at the second
    // character, so an empty string used to hand std::transform a first that is
    // past its last and write off the end of the buffer (access violation).
    EXPECT_EQ(s.fails("SELECT TYPE \"\""), ErrorCode::ParseFailure);
    EXPECT_EQ(s.fails("SELECT LAYER \"\""), ErrorCode::NotFound);
    EXPECT_EQ(s.fails("MOVE 1,1"), ErrorCode::InvalidState); // nothing selected
    EXPECT_TRUE(s.document.model().entities.empty());
    EXPECT_FALSE(s.document.history().canUndo());
    EXPECT_EQ(s.ok(""), "");
    EXPECT_EQ(s.ok("   "), "");
}

TEST(CadInterpreter, RelativePointNeedsAPreviousPoint)
{
    Session s;
    EXPECT_EQ(s.fails("LINE @1,1 5,5"), ErrorCode::InvalidState);
    EXPECT_EQ(s.fails("LINE 0,0 5<45"), ErrorCode::ParseFailure); // polar must be relative
}

TEST(CadInterpreter, SelectAndTransform)
{
    Session s;
    s.ok("LINE 0,0 10,0");
    s.ok("CIRCLE 5,5 1");
    EXPECT_EQ(s.ok("SELECT ALL"), "2 selected");
    EXPECT_EQ(s.ok("MOVE 100,200"), "2 moved");
    EXPECT_EQ(std::get<Segment2>(s.entity(1).geometry).start, Point2(100, 200));

    EXPECT_EQ(s.ok("SELECT TYPE circle"), "1 selected");
    s.ok("COPY 10,0");
    EXPECT_EQ(s.document.model().entities.size(), 3u);

    EXPECT_EQ(s.ok("SELECT 1"), "1 selected");
    s.ok("ROTATE 100,200 90");
    EXPECT_TRUE(nearlyEqual(std::get<Segment2>(s.entity(1).geometry).end, Point2(100, 210)));
    s.ok("SCALE 100,200 2");
    EXPECT_TRUE(nearlyEqual(std::get<Segment2>(s.entity(1).geometry).end, Point2(100, 220)));
    s.ok("MIRROR 0,0 0,1 KEEP");
    EXPECT_EQ(s.document.model().entities.size(), 4u);
    s.ok("ARRAY 2 2 50,50");
    EXPECT_EQ(s.document.model().entities.size(), 7u);

    s.ok("SELECT ALL");
    EXPECT_EQ(s.ok("ERASE"), "7 erased");
    EXPECT_TRUE(s.document.selection().empty());
    EXPECT_EQ(s.ok("UNDO 3"), "3 undone");
    EXPECT_EQ(s.ok("REDO 99"), "3 redone"); // stops when history runs out
    EXPECT_EQ(s.fails("SELECT 999"), ErrorCode::NotFound);
}

TEST(CadInterpreter, EditingCommands)
{
    Session s;
    s.ok("LINE 0,0 10,0");  // 1
    s.ok("LINE 3,-2 3,2");  // 2
    s.ok("LINE 7,-2 7,2");  // 3
    s.ok("TRIM 1 5,0 2 3"); // removes the middle of line 1
    EXPECT_EQ(std::get<Segment2>(s.entity(1).geometry), (Segment2{Point2(0, 0), Point2(3, 0)}));
    EXPECT_EQ(s.document.model().entities.size(), 4u);

    s.ok("LINE 20,0 24,0");       // 5
    s.ok("LINE 30,-5 30,5");      // 6
    s.ok("EXTEND 5 24,0 6");
    EXPECT_EQ(std::get<Segment2>(s.entity(5).geometry).end, Point2(30, 0));

    s.ok("OFFSET 5 2 25,9");
    EXPECT_EQ(std::get<Segment2>(s.entity(7).geometry), (Segment2{Point2(20, 2), Point2(30, 2)}));

    s.ok("LINE 50,0 40,0"); // 8
    s.ok("LINE 40,0 40,10"); // 9
    s.ok("FILLET 8 9 2");
    EXPECT_EQ(s.entity(10).type(), EntityType::Arc);
    s.ok("U");
    EXPECT_FALSE(s.document.model().entities.contains(10));
    s.ok("CHAMFER 8 9 1"); // single distance applies to both sides
    // The bevel is entity 11, not 10: the undone fillet arc retired id 10 for good.
    ASSERT_TRUE(s.document.model().entities.contains(11));
    EXPECT_EQ(std::get<Segment2>(s.entity(11).geometry), (Segment2{Point2(41, 0), Point2(40, 1)}));

    EXPECT_EQ(s.fails("FILLET 8 9 5000"), ErrorCode::InvalidGeometry);
    EXPECT_EQ(s.fails("TRIM 1 1,0 3"), ErrorCode::InvalidGeometry); // cutter 3 no longer crosses
    EXPECT_EQ(s.fails("OFFSET abc 2 0,0"), ErrorCode::ParseFailure);
}

TEST(CadInterpreter, LayersAndAttributes)
{
    Session s;
    s.ok("LAYER NEW Survey #FF0000");
    s.ok("LAYER SET Survey");
    s.ok("POINT 1,1");
    EXPECT_EQ(s.entity(1).layer, "Survey");
    EXPECT_EQ(s.document.model().layers.find("Survey")->color.toHex(), "#FF0000");

    const std::string listing = s.ok("LAYER LIST");
    EXPECT_NE(listing.find("* Survey"), std::string::npos) << listing; // current layer is starred
    EXPECT_NE(listing.find("(1 entities)"), std::string::npos) << listing;

    EXPECT_EQ(s.fails("LAYER LOCK Survey"), ErrorCode::CommandRejected); // current layer
    EXPECT_EQ(s.fails("LAYER DELETE Survey"), ErrorCode::CommandRejected); // in use
    EXPECT_EQ(s.fails("LAYER NEW Survey"), ErrorCode::AlreadyExists);
    EXPECT_EQ(s.fails("LAYER NEW Bad #GGGGGG"), ErrorCode::ParseFailure);
    EXPECT_EQ(s.fails("LAYER SET Nowhere"), ErrorCode::NotFound);

    s.ok("SELECT ALL");
    s.ok("CHLAYER 0");
    s.ok("COLOR #00FF00");
    s.ok("PROP elevation 101.25");
    s.ok("PROP code IP");
    s.ok("PROP order 2");
    s.ok("PROP verified true");
    const auto& properties = s.entity(1).properties;
    EXPECT_DOUBLE_EQ(std::get<double>(properties.at("elevation")), 101.25);
    EXPECT_EQ(std::get<std::string>(properties.at("code")), "IP");
    EXPECT_EQ(std::get<std::int64_t>(properties.at("order")), 2);
    EXPECT_TRUE(std::get<bool>(properties.at("verified")));
    EXPECT_EQ(s.entity(1).color->toHex(), "#00FF00");
    s.ok("COLOR BYLAYER");
    EXPECT_FALSE(s.entity(1).color.has_value());

    s.ok("LAYER SET 0");
    s.ok("LAYER HIDE Survey");
    EXPECT_FALSE(s.document.model().layers.find("Survey")->visible);
    s.ok("LAYER DELETE Survey"); // empty now
    const std::string info = s.ok("INFO 1");
    EXPECT_NE(info.find("elevation = 101.25"), std::string::npos) << info;
    EXPECT_NE(s.ok("LIST").find("1 entities"), std::string::npos);
}

TEST(CadInterpreter, SaveAndOpenRoundTripThroughTheCommandLine)
{
    const fs::path directory = fs::temp_directory_path() / "katana-cad-tests" / "cli.katana";
    fs::remove_all(directory.parent_path());
    const std::string quoted = "\"" + directory.generic_string() + "\"";
    {
        Session s;
        s.ok("RECT 0,0 30,20");
        EXPECT_EQ(s.fails("SAVE"), ErrorCode::InvalidState);
        s.ok("SAVE " + quoted);
        s.ok("CIRCLE 15,10 5");
        s.ok("SAVE");
    }
    {
        Session s;
        s.ok("OPEN " + quoted);
        EXPECT_EQ(s.document.model().entities.size(), 2u);
        s.ok("NEW");
        EXPECT_TRUE(s.document.model().entities.empty());
        EXPECT_EQ(s.fails("OPEN \"" + (directory.parent_path() / "nope").generic_string() + "\""),
                  ErrorCode::NotFound);
    }
    fs::remove_all(directory.parent_path());
}

TEST(CadInterpreter, KeepsAHistoryAndHasHelp)
{
    Session s;
    s.ok("POINT 1,1");
    (void)s.interpreter.run("NONSENSE");
    EXPECT_EQ(s.interpreter.history(), (std::vector<std::string>{"POINT 1,1", "NONSENSE"}));
    EXPECT_NE(s.ok("HELP").find("FILLET"), std::string::npos);
    EXPECT_NE(s.ok("?").find("FILLET"), std::string::npos);
}

// Regression. The interpreter remembers the last point so that "@10,10" can be
// relative to it. It cannot see the document being replaced behind its back, so
// after File > New the GUI used to resolve a relative point against the
// DISCARDED drawing's last point - silently placing geometry somewhere the user
// never indicated, with no error to show for it.
TEST(CadInterpreter, RelativePointStateIsForgottenWhenTheDocumentIsReplaced)
{
    Session s;
    s.ok("LINE 100,100 110,100"); // last point is now 110,100

    // Without a reset the next relative point would resolve against 110,100.
    s.document.newDocument();
    s.interpreter.resetPointState();

    EXPECT_EQ(s.fails("LINE @10,10 @5,0"), ErrorCode::InvalidState)
        << "a relative point with no previous point must be refused, not invented";

    // An absolute point re-establishes the reference, and relative works again.
    s.ok("LINE 0,0 @10,0");
    // Not a hard-coded id: the counter is not reset by newDocument(), because
    // ids are never reused - so the new line does NOT start again at 1.
    const auto ids = s.document.model().entities.ids();
    ASSERT_EQ(ids.size(), 1u);
    EXPECT_EQ(std::get<Segment2>(s.entity(ids.back()).geometry).end, Point2(10, 0));
}

// Snapping culls a polyline's segments against the cursor's reach before the
// pairwise intersection loop. That loop is quadratic in the candidate count, so
// a 3000-vertex surveyed string used to cost 247 ms per mouse move in Release -
// against PLAN.MD section 32's 16 ms budget - purely because the entity-level
// filter admits the whole polyline whenever any part of it is near the cursor.
//
// The cull must not change ANY result, which is what these assert. It is exact
// because every candidate is accepted only within `aperture` of the cursor and
// lies on the curve that produced it, so a segment whose bounding box misses
// the reach box cannot carry an acceptable point.
TEST(CadSnapping, CullingLongPolylinesDoesNotChangeWhatIsFound)
{
    katana::entity::Model model;
    // A long polyline crossed by a short one, intersecting at (50, 0).
    Polyline2 path;
    for (int i = 0; i <= 200; ++i) {
        path.vertices.emplace_back(static_cast<double>(i) * 0.5, 0.0);
    }
    katana::entity::Entity longString;
    longString.geometry = path;
    ASSERT_TRUE(model.entities.add(longString).ok());

    katana::entity::Entity crossing;
    crossing.geometry = Segment2{Point2(50.0, -5.0), Point2(50.0, 5.0)};
    ASSERT_TRUE(model.entities.add(crossing).ok());

    // The intersection is still found, even though it sits in the middle of a
    // polyline whose far ends are nowhere near the cursor.
    katana::cad::SnapRequest request;
    request.cursor = Point2(50.2, 0.2);
    request.aperture = 1.0;
    request.modes = static_cast<katana::cad::SnapModes>(katana::cad::SnapMode::Intersection);
    const auto hit = katana::cad::snap(model, request);
    ASSERT_TRUE(hit.has_value()) << "the cull removed a real intersection";
    EXPECT_EQ(hit->mode, katana::cad::SnapMode::Intersection);
    EXPECT_TRUE(nearlyEqual(hit->point, Point2(50.0, 0.0)));

    // Nearest reads the same culled list, so it must still land on the curve.
    request.modes = static_cast<katana::cad::SnapModes>(katana::cad::SnapMode::Nearest);
    request.cursor = Point2(20.3, 0.4);
    const auto nearest = katana::cad::snap(model, request);
    ASSERT_TRUE(nearest.has_value());
    EXPECT_EQ(nearest->mode, katana::cad::SnapMode::Nearest);
    EXPECT_NEAR(nearest->point.y, 0.0, 1e-12);
    EXPECT_NEAR(nearest->point.x, 20.3, 1e-12);

    // And a cursor genuinely far from everything still snaps to nothing.
    request.cursor = Point2(50.0, 40.0);
    request.modes = katana::cad::kDefaultSnapModes;
    EXPECT_FALSE(katana::cad::snap(model, request).has_value());
}

TEST(CadInterpreter, LinetypesCanBeDefinedListedAndAttachedToALayer)
{
    // The whole point of the verb: without it a pattern can be defined in the
    // library and never reached from the application, which is finished work in
    // the tests and unfinished work to the user.
    Session session;
    EXPECT_NE(session.ok("LINETYPE LIST").find("continuous"), std::string::npos);

    session.ok("LINETYPE NEW fence 1 -0.5");
    const std::string listed = session.ok("LINETYPE LIST");
    EXPECT_NE(listed.find("fence"), std::string::npos) << listed;
    EXPECT_NE(listed.find("period=1.5"), std::string::npos) << listed;

    session.ok("LAYER NEW boundary");
    session.ok("LAYER LTYPE boundary fence");
    ASSERT_NE(session.document.model().layers.find("boundary"), nullptr);
    EXPECT_EQ(session.document.model().layers.find("boundary")->linetype, "fence");

    // A layer still using it blocks deletion, naming the layer so it can be
    // found rather than just refusing.
    const auto refused = session.interpreter.run("LINETYPE DELETE fence");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().describe().find("boundary"), std::string::npos)
        << refused.error().describe();
}

TEST(CadInterpreter, ALinetypeWithAnOddNumberOfLengthsIsRefusedWithAUsefulReason)
{
    // "must end with a gap" is true and unhelpful; the actual mistake is
    // forgetting the gap after the last dash.
    Session session;
    const auto refused = session.interpreter.run("LINETYPE NEW bad 1 -0.5 1");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().describe().find("pairs"), std::string::npos)
        << refused.error().describe();

    // And the DXF rules still apply underneath.
    EXPECT_EQ(session.fails("LINETYPE NEW bad -1 0.5"), ErrorCode::InvalidArgument)
        << "must not start with a gap";
    session.fails("LINETYPE DELETE continuous");
    session.fails("LAYER LTYPE 0 no-such-pattern");
}

TEST(CadInterpreter, OpenReportsHowManyEntitiesItActuallyOpened)
{
    // The count used to be built as a sibling ARGUMENT to document_.open() in
    // the same call, and C++ does not order function arguments - so it was read
    // before the open ran and every OPEN reported "(0 entities)" however large
    // the project was. Unspecified order, not undefined behaviour, and it reads
    // as perfectly correct code.
    const auto directory =
        std::filesystem::temp_directory_path() / "katana_open_count_test";
    std::error_code ignored;
    std::filesystem::remove_all(directory, ignored);

    {
        Session session;
        session.ok("LINE 0,0 10,0");
        session.ok("LINE 0,5 10,5");
        session.ok("CIRCLE 5,5 2");
        session.ok("SAVE " + directory.string());
    }

    Session reopened;
    const std::string reply = reopened.ok("OPEN " + directory.string());
    EXPECT_NE(reply.find("(3 entities)"), std::string::npos)
        << "reported: " << reply;
    EXPECT_EQ(reopened.document.model().entities.size(), 3u);

    std::filesystem::remove_all(directory, ignored);
}

TEST(CadInterpreter, DimensionStylesCanBeDefinedTunedAndAttachedToALayer)
{
    Session session;
    EXPECT_NE(session.ok("DIMSTYLE LIST").find("Standard"), std::string::npos);

    session.ok("DIMSTYLE NEW site");
    session.ok("DIMSTYLE SET site DECIMALS 1");
    session.ok("DIMSTYLE SET site SCALE 1000");
    session.ok("DIMSTYLE SET site SUFFIX mm");
    session.ok("DIMSTYLE SET site HEAD Dot");

    const auto* style = session.document.model().dimensionStyles.find("site");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->decimals, 1);
    EXPECT_DOUBLE_EQ(style->unitScale, 1000.0);
    EXPECT_EQ(style->arrowHead, katana::entity::ArrowHead::Dot);

    // LIST shows what a ten-unit dimension would READ as, which is the question
    // anyone setting a style is actually asking.
    const std::string listed = session.ok("DIMSTYLE LIST");
    EXPECT_NE(listed.find("10000.0mm"), std::string::npos) << listed;

    session.ok("LAYER NEW dims");
    session.ok("LAYER DIMSTYLE dims site");
    EXPECT_EQ(session.document.model().layers.find("dims")->dimensionStyle, "site");

    // A layer still using it blocks deletion, naming the layer.
    const auto refused = session.interpreter.run("DIMSTYLE DELETE site");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().describe().find("dims"), std::string::npos)
        << refused.error().describe();
}

TEST(CadInterpreter, DimensionStyleRejectsNonsenseWithItsOwnReason)
{
    Session session;
    session.ok("DIMSTYLE NEW site");

    EXPECT_EQ(session.fails("DIMSTYLE SET site TEXT 0"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("DIMSTYLE SET site DECIMALS 99"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("DIMSTYLE SET site HEAD Diamond"), ErrorCode::ParseFailure);
    EXPECT_EQ(session.fails("DIMSTYLE SET site NOSUCHFIELD 1"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("DIMSTYLE SET missing TEXT 1"), ErrorCode::NotFound);
    session.fails("DIMSTYLE DELETE Standard");
    session.fails("LAYER DIMSTYLE 0 no-such-style");

    // A refused change must leave the stored style alone.
    EXPECT_DOUBLE_EQ(session.document.model().dimensionStyles.find("site")->textHeight, 2.5);
}

TEST(CadInterpreter, HatchPatternsCanBeDefinedAndAttachedToALayer)
{
    Session session;
    // Only the built-in is there to begin with, so a drawing is unhatched
    // until someone says otherwise.
    EXPECT_NE(session.ok("HATCH LIST").find("none"), std::string::npos);

    session.ok("HATCH NEW brick 0 0.25");
    session.ok("HATCH SOLID concrete");
    session.ok("LAYER NEW paving");
    session.ok("LAYER HATCH paving brick");

    const auto* pattern = session.document.model().hatchPatterns.find("brick");
    ASSERT_NE(pattern, nullptr);
    ASSERT_EQ(pattern->families.size(), 1u);
    EXPECT_DOUBLE_EQ(pattern->families[0].spacing, 0.25);
    EXPECT_DOUBLE_EQ(pattern->families[0].angle, 0.0);
    EXPECT_FALSE(pattern->solid);

    ASSERT_NE(session.document.model().hatchPatterns.find("concrete"), nullptr);
    EXPECT_TRUE(session.document.model().hatchPatterns.find("concrete")->solid);
    ASSERT_NE(session.document.model().layers.find("paving"), nullptr);
    EXPECT_EQ(session.document.model().layers.find("paving")->hatchPattern, "brick");
}

TEST(CadInterpreter, HatchAnglesAreTypedInDegreesAndStoredInRadians)
{
    // Every other angle the interpreter takes is in degrees, because that is
    // what a drafter types. The model keeps one unit. 45 degrees is pi/4
    // exactly as a mathematical quantity, so this is a real conversion check
    // and not a restatement of whatever the parser produced.
    Session session;
    session.ok("HATCH NEW cross 45 0.5 135 0.5");

    const auto* pattern = session.document.model().hatchPatterns.find("cross");
    ASSERT_NE(pattern, nullptr);
    ASSERT_EQ(pattern->families.size(), 2u);
    EXPECT_NEAR(pattern->families[0].angle, std::acos(-1.0) / 4.0, 1e-12);
    EXPECT_NEAR(pattern->families[1].angle, 3.0 * std::acos(-1.0) / 4.0, 1e-12);

    // And LIST puts them back in degrees, so what is typed is what is read.
    const std::string listed = session.ok("HATCH LIST");
    EXPECT_NE(listed.find("45 deg"), std::string::npos) << listed;
    EXPECT_NE(listed.find("135 deg"), std::string::npos) << listed;
}

TEST(CadInterpreter, HatchRejectsNonsenseWithItsOwnReason)
{
    Session session;
    // An odd number of arguments means an angle with no spacing, which would
    // otherwise be read as a spacing of whatever came next.
    EXPECT_EQ(session.fails("HATCH NEW lopsided 45"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("HATCH NEW zero 0 0"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("HATCH NEW negative 0 -1"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("HATCH DELETE none"), ErrorCode::CommandRejected);
    EXPECT_TRUE(session.document.model().hatchPatterns.contains("none"));
    EXPECT_FALSE(session.document.model().hatchPatterns.contains("lopsided"));
}

TEST(CadInterpreter, DeletingAHatchPatternALayerStillUsesIsRefusedAndNamesTheLayer)
{
    // Allowing it would leave the layer naming a pattern that is gone, and the
    // drawing would quietly stop being hatched with nothing to say why.
    Session session;
    session.ok("HATCH NEW brick 0 0.25");
    session.ok("LAYER NEW paving");
    session.ok("LAYER HATCH paving brick");

    const auto refused = session.interpreter.run("HATCH DELETE brick");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().describe().find("paving"), std::string::npos)
        << refused.error().describe();
    EXPECT_TRUE(session.document.model().hatchPatterns.contains("brick"));

    // Detach it and the deletion goes through.
    session.ok("LAYER HATCH paving none");
    session.ok("HATCH DELETE brick");
    EXPECT_FALSE(session.document.model().hatchPatterns.contains("brick"));
}

TEST(CadInterpreter, CreatingAHatchPatternIsUndoable)
{
    Session session;
    session.ok("HATCH NEW brick 0 0.25");
    ASSERT_TRUE(session.document.model().hatchPatterns.contains("brick"));
    session.ok("UNDO");
    EXPECT_FALSE(session.document.model().hatchPatterns.contains("brick"));
    session.ok("REDO");
    EXPECT_TRUE(session.document.model().hatchPatterns.contains("brick"));
}

// ---- alignments -----------------------------------------------------------------------

TEST(CadInterpreter, AnAlignmentIsDefinedByItsPIsAndSolvedOnDemand)
{
    Session session;
    EXPECT_TRUE(session.ok("ALIGN LIST").empty()) << "nothing is defined in a new document";

    // Three PIs with no curves: a 200 m kinked centreline.
    session.ok("ALIGN NEW road 0,0 100,0 100,100");
    const auto* road = session.document.model().alignments.find("road");
    ASSERT_NE(road, nullptr);
    ASSERT_EQ(road->horizontal.pis.size(), 3u);
    EXPECT_NE(session.ok("ALIGN LIST").find("length 200.000"), std::string::npos)
        << session.ok("ALIGN LIST");

    // Round the corner: T = R tan(45 deg) = 50, so the length becomes
    // 100 + 50 pi / 2 = 178.540.
    session.ok("ALIGN SET road 1 50");
    EXPECT_NE(session.ok("ALIGN LIST").find("length 178.540"), std::string::npos)
        << session.ok("ALIGN LIST");

    session.ok("ALIGN START road 1000");
    EXPECT_NE(session.ok("ALIGN LIST").find("1000.000 to 1178.540"), std::string::npos)
        << session.ok("ALIGN LIST");
}

TEST(CadInterpreter, AlignmentPIsCanBeAppendedWithTheirCurveData)
{
    Session session;
    session.ok("ALIGN NEW road 0,0 400,0");
    // Extend past the corner with a 30 degree turn, R = 300, 90 m spirals.
    session.ok("ALIGN SET road 1 300 90 90");
    session.ok("ALIGN PI road 659.808,150");
    const auto* road = session.document.model().alignments.find("road");
    ASSERT_NE(road, nullptr);
    ASSERT_EQ(road->horizontal.pis.size(), 3u);
    EXPECT_DOUBLE_EQ(road->horizontal.pis[1].radius, 300.0);
    EXPECT_DOUBLE_EQ(road->horizontal.pis[1].spiralIn, 90.0);
    EXPECT_DOUBLE_EQ(road->horizontal.pis[1].spiralOut, 90.0);
}

TEST(CadInterpreter, AStationTableIncludesEveryKeyStationNotOnlyTheInterval)
{
    // A setting-out table that skipped the TS, SC, CS and ST would be useless
    // in the field: those are the points that get pegged.
    Session session;
    session.ok("ALIGN NEW road 0,0 100,0 100,100");
    session.ok("ALIGN SET road 1 50");
    const std::string table = session.ok("ALIGN STATIONS road 25");
    // TS at 50 (on the interval anyway), ST at 128.540 (not on it).
    EXPECT_NE(table.find("128.540"), std::string::npos) << table;
    EXPECT_NE(table.find("178.540"), std::string::npos) << table;
    // Tangent rows say so; arc rows give the radius.
    EXPECT_NE(table.find("straight"), std::string::npos) << table;
    EXPECT_NE(table.find("50.000"), std::string::npos) << table;
    EXPECT_EQ(session.fails("ALIGN STATIONS road 0"), ErrorCode::InvalidArgument);
}

TEST(CadInterpreter, AnAlignmentThatCannotBeBuiltIsRefusedNamingThePI)
{
    Session session;
    session.ok("ALIGN NEW road 0,0 400,0 659.808,150");
    // 500 m spirals on R = 300 use more deflection than a 30 degree corner has.
    const auto refused = session.interpreter.run("ALIGN SET road 1 300 500 500");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(refused.error().describe().find("PI 1"), std::string::npos)
        << refused.error().describe();
    // The model kept the last good definition.
    EXPECT_DOUBLE_EQ(session.document.model().alignments.find("road")->horizontal.pis[1].spiralIn,
                     0.0);

    // One PI is a malformed command, and the verb says so before the solver
    // is asked - InvalidArgument from the usage guard, not InvalidGeometry.
    // The solver's own "at least two PIs" refusal is reached through the API
    // and is tested in the geometry suite.
    EXPECT_EQ(session.fails("ALIGN NEW lonely 0,0"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("ALIGN SET road 7 50"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("ALIGN PI nosuch 1,1"), ErrorCode::NotFound);
}

TEST(CadInterpreter, AlignmentEditsAreUndoable)
{
    Session session;
    session.ok("ALIGN NEW road 0,0 100,0 100,100");
    session.ok("ALIGN SET road 1 50");
    ASSERT_DOUBLE_EQ(session.document.model().alignments.find("road")->horizontal.pis[1].radius,
                     50.0);
    session.ok("UNDO");
    EXPECT_DOUBLE_EQ(session.document.model().alignments.find("road")->horizontal.pis[1].radius,
                     0.0);
    session.ok("UNDO");
    EXPECT_EQ(session.document.model().alignments.find("road"), nullptr);
    session.ok("REDO");
    ASSERT_NE(session.document.model().alignments.find("road"), nullptr);
    session.ok("ALIGN DELETE road");
    EXPECT_EQ(session.document.model().alignments.find("road"), nullptr);
    session.ok("UNDO");
    EXPECT_NE(session.document.model().alignments.find("road"), nullptr);
}
