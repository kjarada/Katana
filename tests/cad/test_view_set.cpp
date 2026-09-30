// The open views of the workspace (PLAN.MD 47; include/katana/cad/view_set.hpp).
//
// ViewSet is the model the dock workspace is a picture of: which views are
// open, under which never-reused id, which one is active, what each is called
// and what camera it starts with. Every expectation is worked out from the
// header's contract - ids by counting, "most recent" by replaying the clicks,
// camera directions from the definition of the standard views in
// render/camera.hpp - not read back from a run.

#include <gtest/gtest.h>

#include <cmath>
#include <cstdint>
#include <memory>
#include <set>
#include <string>
#include <vector>

#include "katana/cad/view_set.hpp"

using katana::cad::kNoView;
using katana::cad::ViewId;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::core::ErrorCode;
using katana::render::Camera;
using katana::render::Projection;
using katana::render::Vec3;

namespace {

std::vector<ViewId> idsOf(const ViewSet& set)
{
    std::vector<ViewId> ids;
    for (const ViewState* view : set.views()) {
        ids.push_back(view->id);
    }
    return ids;
}

// Where the camera looks, against a direction worked out by hand.
void expectLooking(const Camera& camera, const Vec3& expected, double tolerance)
{
    const Vec3 forward = camera.forward();
    EXPECT_NEAR(forward.x, expected.x, tolerance);
    EXPECT_NEAR(forward.y, expected.y, tolerance);
    EXPECT_NEAR(forward.z, expected.z, tolerance);
}

// An orbit no standard view produces, so "untouched" is distinguishable from
// "reset to the same place".
void orbitSomewhereOdd(Camera& camera)
{
    camera.setProjection(Projection::Perspective);
    camera.setOrientation(1.0, 0.3);
    camera.setDistance(42.0);
    camera.setTarget(katana::geometry::Point3(1.0, 2.0, 3.0));
}

void expectSameCamera(const Camera& actual, const Camera& expected)
{
    EXPECT_EQ(actual.projection(), expected.projection());
    EXPECT_EQ(actual.azimuth(), expected.azimuth());
    EXPECT_EQ(actual.elevation(), expected.elevation());
    EXPECT_EQ(actual.distance(), expected.distance());
    EXPECT_EQ(actual.target().x, expected.target().x);
    EXPECT_EQ(actual.target().y, expected.target().y);
    EXPECT_EQ(actual.target().z, expected.target().z);
}

// The directions render/camera.hpp defines. Front looks north (from -Y). An
// isometric view from the south-west looks north-east and down at the
// isometric elevation atan(1/sqrt 2), which is exactly the angle that makes
// all three components equal: (1, 1, -1) / sqrt 3.
const double kThird = 1.0 / std::sqrt(3.0);
const Vec3 kLookingNorth{0.0, 1.0, 0.0};
const Vec3 kLookingNorthEastAndDown{kThird, kThird, -kThird};
const Vec3 kLookingDown{0.0, 0.0, -1.0};
// Top cannot look exactly down: the camera clamps elevation "just inside the
// poles" so that up never becomes parallel to the view direction. The guard
// in camera.cpp is 1e-3 rad, which leaves the x-y components at sin(1e-3),
// about 1e-3; 2e-3 bounds that without depending on its exact value.
constexpr double kPoleTolerance = 2.0e-3;
// cos and sin of multiples of pi/4 in doubles: a few ulps.
constexpr double kDirectionTolerance = 1.0e-12;

} // namespace

// ---- ids and the active view ------------------------------------------------------------

TEST(ViewSet, AnEmptySetHasNoActiveView)
{
    ViewSet set;
    EXPECT_TRUE(set.empty());
    EXPECT_EQ(set.size(), 0u);
    EXPECT_EQ(set.activeId(), kNoView);
    EXPECT_EQ(set.active(), nullptr);
    EXPECT_TRUE(set.views().empty());
}

TEST(ViewSet, IdsCountUpFromOneAndAreNeverReusedAfterARemove)
{
    // A dock that outlives its view by a queued event must not find a
    // different view under the same id.
    ViewSet set;
    EXPECT_EQ(set.add(ViewKind::Plan).id, 1u);
    EXPECT_EQ(set.add(ViewKind::Model3D).id, 2u);
    ASSERT_TRUE(set.remove(2).ok());
    EXPECT_EQ(set.add(ViewKind::Model3D).id, 3u) << "2 is gone, not free";
    ASSERT_TRUE(set.remove(1).ok());
    ASSERT_TRUE(set.remove(3).ok());
    ASSERT_TRUE(set.empty());
    EXPECT_EQ(set.add(ViewKind::Plan).id, 4u) << "not even once the set is empty";
}

TEST(ViewSet, TheFirstViewAddedBecomesActiveAndLaterOnesDoNot)
{
    ViewSet set;
    const ViewId first = set.add(ViewKind::Plan).id;
    EXPECT_EQ(set.activeId(), first);
    ASSERT_NE(set.active(), nullptr);
    EXPECT_EQ(set.active()->id, first);

    set.add(ViewKind::Model3D);
    set.add(ViewKind::Section);
    EXPECT_EQ(set.activeId(), first) << "opening a view and activating it are separate";
}

TEST(ViewSet, ActivatingAnUnknownViewIsNotFoundAndChangesNothing)
{
    ViewSet set;
    set.add(ViewKind::Plan);
    const ViewId second = set.add(ViewKind::Model3D).id;
    ASSERT_TRUE(set.activate(second).ok());

    for (const ViewId unknown : {ViewId{99}, kNoView}) {
        const auto status = set.activate(unknown);
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(status.error().code, ErrorCode::NotFound);
        EXPECT_EQ(set.activeId(), second) << "no other view may be activated instead";
    }
}

TEST(ViewSet, RemovingAnUnknownViewIsNotFoundAndChangesNothing)
{
    ViewSet set;
    set.add(ViewKind::Plan);
    set.add(ViewKind::Model3D);
    for (const ViewId unknown : {ViewId{99}, kNoView}) {
        const auto status = set.remove(unknown);
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(status.error().code, ErrorCode::NotFound);
    }
    EXPECT_EQ(set.size(), 2u);
    EXPECT_EQ(set.activeId(), 1u);
}

TEST(ViewSet, RemovingTheActiveViewActivatesTheMostRecentlyActiveOfTheRest)
{
    // Clicks: 1 (active on opening), 3, 2, 4. The order of recent activity
    // is therefore 1 < 3 < 2 < 4, and each removal of the active view must
    // fall back one step down that list.
    ViewSet set;
    for (int i = 0; i < 4; ++i) {
        set.add(ViewKind::Plan);
    }
    ASSERT_TRUE(set.activate(3).ok());
    ASSERT_TRUE(set.activate(2).ok());
    ASSERT_TRUE(set.activate(4).ok());

    ASSERT_TRUE(set.remove(4).ok());
    EXPECT_EQ(set.activeId(), 2u);
    ASSERT_TRUE(set.remove(2).ok());
    EXPECT_EQ(set.activeId(), 3u);
    ASSERT_TRUE(set.remove(3).ok());
    EXPECT_EQ(set.activeId(), 1u);
    ASSERT_TRUE(set.remove(1).ok());
    EXPECT_EQ(set.activeId(), kNoView) << "never an id that is not open";
    EXPECT_EQ(set.active(), nullptr);
}

TEST(ViewSet, RemovingAViewThatIsNotActiveLeavesTheActiveViewAlone)
{
    ViewSet set;
    set.add(ViewKind::Plan);
    set.add(ViewKind::Model3D);
    set.add(ViewKind::Section);
    ASSERT_TRUE(set.activate(2).ok());
    ASSERT_TRUE(set.remove(3).ok());
    EXPECT_EQ(set.activeId(), 2u);
    ASSERT_TRUE(set.remove(1).ok());
    EXPECT_EQ(set.activeId(), 2u);
}

// ---- restore ------------------------------------------------------------------------------

TEST(ViewSet, RestoringAViewKeepsItsIdAndMovesTheCounterPastIt)
{
    ViewSet set;
    const auto restored = set.restore(10, ViewKind::Model3D);
    ASSERT_TRUE(restored.ok()) << restored.error().describe();
    ASSERT_NE(*restored, nullptr);
    EXPECT_EQ((*restored)->id, 10u);
    EXPECT_EQ((*restored)->kind, ViewKind::Model3D);
    EXPECT_EQ(set.find(10), *restored);
    EXPECT_EQ(set.activeId(), 10u) << "the first view into an empty set, like add()";

    EXPECT_EQ(set.add(ViewKind::Plan).id, 11u);
}

TEST(ViewSet, RestoringAnIdBelowTheCounterDoesNotLowerIt)
{
    ViewSet set;
    set.add(ViewKind::Plan);
    set.add(ViewKind::Plan);
    set.add(ViewKind::Plan);
    ASSERT_TRUE(set.remove(2).ok());

    const auto restored = set.restore(2, ViewKind::Section);
    ASSERT_TRUE(restored.ok()) << restored.error().describe();
    EXPECT_EQ((*restored)->id, 2u);
    EXPECT_EQ(set.add(ViewKind::Plan).id, 4u) << "3 was issued; the next is 4, not 3";
}

TEST(ViewSet, RestoringTheNoViewIdOrAnOpenIdIsRefused)
{
    ViewSet set;
    set.add(ViewKind::Plan);

    const auto zero = set.restore(kNoView, ViewKind::Plan);
    ASSERT_FALSE(zero.ok());
    EXPECT_EQ(zero.error().code, ErrorCode::InvalidArgument);

    const auto open = set.restore(1, ViewKind::Model3D);
    ASSERT_FALSE(open.ok());
    EXPECT_EQ(open.error().code, ErrorCode::AlreadyExists);
    EXPECT_EQ(set.find(1)->kind, ViewKind::Plan) << "the open view is not replaced";

    EXPECT_EQ(set.size(), 1u);
    EXPECT_EQ(set.add(ViewKind::Plan).id, 2u) << "a refused restore moves no counter";
}

// ---- take ---------------------------------------------------------------------------------

TEST(ViewSet, TakingAViewHandsOverItsStateAndForgetsIt)
{
    ViewSet set;
    set.add(ViewKind::Plan);
    ViewState& second = set.add(ViewKind::Model3D);
    second.layers.hide("survey");
    ASSERT_TRUE(set.activate(2).ok());

    std::unique_ptr<ViewState> taken = set.take(2);
    ASSERT_NE(taken, nullptr);
    EXPECT_EQ(taken.get(), &second) << "the same object, which a closing widget still holds";
    EXPECT_EQ(taken->id, 2u);
    EXPECT_TRUE(taken->layers.hidesDirectly("survey"));

    EXPECT_EQ(set.find(2), nullptr) << "unfindable at once";
    EXPECT_EQ(set.activeId(), 1u) << "and no longer active";
    EXPECT_EQ(set.size(), 1u);
    EXPECT_EQ(set.count(ViewKind::Model3D), 0u);

    EXPECT_EQ(set.take(2), nullptr) << "a second take finds nothing";
    EXPECT_EQ(set.take(99), nullptr);
    EXPECT_EQ(set.add(ViewKind::Plan).id, 3u) << "a taken id is not reused either";
}

// ---- kind and camera ------------------------------------------------------------------------

TEST(ViewSet, ANewViewStartsWithTheCameraOfItsKind)
{
    ViewSet set;
    const ViewState& plan = set.add(ViewKind::Plan);
    EXPECT_EQ(plan.camera.projection(), Projection::Orthographic);
    expectLooking(plan.camera, kLookingDown, kPoleTolerance);

    const ViewState& model = set.add(ViewKind::Model3D);
    EXPECT_EQ(model.camera.projection(), Projection::Perspective);
    expectLooking(model.camera, kLookingNorthEastAndDown, kDirectionTolerance);

    // Orthographic, because a side view is read off with a scale rule.
    const ViewState& elevation = set.add(ViewKind::Elevation);
    EXPECT_EQ(elevation.camera.projection(), Projection::Orthographic);
    expectLooking(elevation.camera, kLookingNorth, kDirectionTolerance);

    // A section draws in its own station/elevation transform and leaves the
    // camera as a default-constructed one.
    const ViewState& section = set.add(ViewKind::Section);
    expectSameCamera(section.camera, Camera{});
}

TEST(ViewSet, SettingTheKindOfAnUnknownViewIsNotFoundAndChangesNothing)
{
    ViewSet set;
    set.add(ViewKind::Plan);
    const auto status = set.setKind(7, ViewKind::Model3D);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::NotFound);
    EXPECT_EQ(set.count(ViewKind::Model3D), 0u);
}

TEST(ViewSet, ChoosingTheKindAViewAlreadyHasLeavesItsCameraAlone)
{
    // "A 3D view's orbit is not reset by choosing 3D again."
    ViewSet set;
    ViewState& view = set.add(ViewKind::Model3D);
    orbitSomewhereOdd(view.camera);
    const Camera orbited = view.camera;

    ASSERT_TRUE(set.setKind(view.id, ViewKind::Model3D).ok());
    expectSameCamera(view.camera, orbited);
    EXPECT_EQ(view.number, 1);
}

TEST(ViewSet, ChangingKindGivesTheCameraThatKindsStartingView)
{
    ViewSet set;
    ViewState& view = set.add(ViewKind::Plan);

    ASSERT_TRUE(set.setKind(view.id, ViewKind::Model3D).ok());
    EXPECT_EQ(view.kind, ViewKind::Model3D);
    EXPECT_EQ(view.camera.projection(), Projection::Perspective);
    expectLooking(view.camera, kLookingNorthEastAndDown, kDirectionTolerance);

    orbitSomewhereOdd(view.camera);
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Elevation).ok());
    EXPECT_EQ(view.camera.projection(), Projection::Orthographic);
    expectLooking(view.camera, kLookingNorth, kDirectionTolerance);

    orbitSomewhereOdd(view.camera);
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Plan).ok());
    EXPECT_EQ(view.camera.projection(), Projection::Orthographic);
    expectLooking(view.camera, kLookingDown, kPoleTolerance);
}

TEST(ViewSet, TurningAViewIntoASectionLeavesItsCameraAlone)
{
    ViewSet set;
    ViewState& view = set.add(ViewKind::Model3D);
    orbitSomewhereOdd(view.camera);
    const Camera orbited = view.camera;

    ASSERT_TRUE(set.setKind(view.id, ViewKind::Section).ok());
    EXPECT_EQ(view.kind, ViewKind::Section);
    expectSameCamera(view.camera, orbited);
}

TEST(ViewSet, AThreeDViewTurnedIntoASectionAndBackKeepsItsOrbit)
{
    // configureCamera's contract (viewport_layout.hpp): "A Section leaves the
    // camera alone, so switching back to a model view restores what it had."
    // Leaving it alone on the way in is only half of that; coming back must
    // not then reset it to the starting view of the kind it already had.
    ViewSet set;
    ViewState& view = set.add(ViewKind::Model3D);
    orbitSomewhereOdd(view.camera);
    const Camera orbited = view.camera;

    ASSERT_TRUE(set.setKind(view.id, ViewKind::Section).ok());
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Model3D).ok());
    expectSameCamera(view.camera, orbited);

    // The same for an elevation view, whose camera is a different start.
    ViewState& side = set.add(ViewKind::Elevation);
    side.camera.setOrientation(-1.2, 0.1);
    const Camera turned = side.camera;
    ASSERT_TRUE(set.setKind(side.id, ViewKind::Section).ok());
    ASSERT_TRUE(set.setKind(side.id, ViewKind::Elevation).ok());
    expectSameCamera(side.camera, turned);
}

TEST(ViewSet, ACameraPointedAfreshIsNoLongerFramedAndOneLeftAloneStillIs)
{
    // ViewState::cameraFramed: "setKind clears it whenever it points the
    // camera afresh". The 3D widget is rebuilt on every change of kind and
    // frames at its first paint unless this is set, so it must survive
    // exactly the changes that leave the camera alone - a trip through a
    // section, choosing the same kind - and no other.
    ViewSet set;
    ViewState& view = set.add(ViewKind::Model3D);
    EXPECT_FALSE(view.cameraFramed); // a new view has framed nothing yet

    view.cameraFramed = true; // what RenderViewWidget::zoomExtents records
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Model3D).ok());
    EXPECT_TRUE(view.cameraFramed);
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Section).ok());
    EXPECT_TRUE(view.cameraFramed);
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Model3D).ok());
    EXPECT_TRUE(view.cameraFramed);

    // 3D -> Elevation points the camera at the front: a frame from the 3D
    // orbit says nothing about what the elevation shows.
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Elevation).ok());
    EXPECT_FALSE(view.cameraFramed);

    // Through a section to a DIFFERENT model kind: configured, so cleared.
    view.cameraFramed = true;
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Section).ok());
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Model3D).ok());
    EXPECT_FALSE(view.cameraFramed);
}

TEST(ViewSet, ASectionTurnedIntoAModelViewStartsFromThatKindsView)
{
    // Opened as a section, the camera was never pointed anywhere, so arriving
    // at a model kind must configure it.
    ViewSet set;
    ViewState& view = set.add(ViewKind::Section);
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Model3D).ok());
    EXPECT_EQ(view.camera.projection(), Projection::Perspective);
    expectLooking(view.camera, kLookingNorthEastAndDown, kDirectionTolerance);

    // And a model view that went through a section to a DIFFERENT model kind
    // gets that kind's start, not the camera it had before.
    orbitSomewhereOdd(view.camera);
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Section).ok());
    ASSERT_TRUE(set.setKind(view.id, ViewKind::Elevation).ok());
    EXPECT_EQ(view.camera.projection(), Projection::Orthographic);
    expectLooking(view.camera, kLookingNorth, kDirectionTolerance);
}

TEST(ViewSet, ChangingKindKeepsThePlanZoomTheLayersAndTheSection)
{
    // "Everything below survives a change of kind: switching a view to 3D and
    // back keeps its plan zoom, its hidden layers and its section."
    ViewSet set;
    ViewState& view = set.add(ViewKind::Plan);
    view.plan.center = katana::geometry::Point2(512.0, -256.0);
    view.plan.scale = 0.125;
    view.planFramed = true;
    view.layers.hide("survey");
    view.hiddenReferences = {7, 9};
    view.section = katana::cad::Section{};
    view.section->length = 12.5;
    view.sectionExaggeration = 4.0;

    for (const ViewKind through : {ViewKind::Model3D, ViewKind::Section, ViewKind::Elevation}) {
        ASSERT_TRUE(set.setKind(view.id, through).ok());
        ASSERT_TRUE(set.setKind(view.id, ViewKind::Plan).ok());
        EXPECT_EQ(view.plan.center, katana::geometry::Point2(512.0, -256.0));
        EXPECT_EQ(view.plan.scale, 0.125);
        EXPECT_TRUE(view.planFramed);
        EXPECT_TRUE(view.layers.hidesDirectly("survey"));
        EXPECT_EQ(view.hiddenReferences, (std::set<std::uint64_t>{7, 9}));
        ASSERT_TRUE(view.section.has_value());
        EXPECT_EQ(view.section->length, 12.5);
        EXPECT_EQ(view.sectionExaggeration, 4.0);
    }
}

// ---- numbering and titles ---------------------------------------------------------------------

TEST(ViewSet, EachKindIsNumberedWithTheLowestNumberNoOpenViewOfThatKindHolds)
{
    ViewSet set;
    EXPECT_EQ(set.add(ViewKind::Plan).number, 1);
    EXPECT_EQ(set.add(ViewKind::Plan).number, 2);
    EXPECT_EQ(set.add(ViewKind::Model3D).number, 1) << "numbers count per kind";
    EXPECT_EQ(set.add(ViewKind::Plan).number, 3);

    ASSERT_TRUE(set.remove(1).ok()); // "Plan 1"
    EXPECT_EQ(set.add(ViewKind::Plan).number, 1) << "the lowest free, not a session counter";
    EXPECT_EQ(set.add(ViewKind::Plan).number, 4);
    EXPECT_EQ(set.add(ViewKind::Section).number, 1);
    EXPECT_EQ(set.find(2)->number, 2) << "closing Plan 1 does not renumber Plan 2";
}

TEST(ViewSet, ChangingKindRenumbersTheViewWithinItsNewKind)
{
    ViewSet set;
    const ViewId plan1 = set.add(ViewKind::Plan).id;
    const ViewId plan2 = set.add(ViewKind::Plan).id;
    const ViewId model1 = set.add(ViewKind::Model3D).id;

    ASSERT_TRUE(set.setKind(plan2, ViewKind::Model3D).ok());
    EXPECT_EQ(set.find(plan2)->number, 2) << "3D 1 is taken";
    EXPECT_EQ(ViewSet::title(*set.find(plan2)), "3D 2");

    ASSERT_TRUE(set.setKind(model1, ViewKind::Plan).ok());
    EXPECT_EQ(set.find(model1)->number, 2) << "Plan 1 is still open; Plan 2 became a 3D view";

    ASSERT_TRUE(set.setKind(plan1, ViewKind::Elevation).ok());
    EXPECT_EQ(ViewSet::title(*set.find(plan1)), "Elevation 1");

    // Choosing the kind it already has renumbers nothing.
    ASSERT_TRUE(set.setKind(plan2, ViewKind::Model3D).ok());
    EXPECT_EQ(set.find(plan2)->number, 2);
}

TEST(ViewSet, ATitleIsTheKindAndTheNumber)
{
    ViewState view;
    view.kind = ViewKind::Plan;
    view.number = 1;
    EXPECT_EQ(ViewSet::title(view), "Plan 1");
    view.kind = ViewKind::Model3D;
    view.number = 2;
    EXPECT_EQ(ViewSet::title(view), "3D 2");
    view.kind = ViewKind::Section;
    view.number = 1;
    EXPECT_EQ(ViewSet::title(view), "Section 1");
    view.kind = ViewKind::Elevation;
    view.number = 12;
    EXPECT_EQ(ViewSet::title(view), "Elevation 12");
}

// ---- mostRecent -------------------------------------------------------------------------------

TEST(ViewSet, MostRecentIsTheActiveViewWhenItIsOfThatKind)
{
    ViewSet set;
    set.add(ViewKind::Model3D);
    const ViewId second = set.add(ViewKind::Model3D).id;
    ASSERT_TRUE(set.activate(second).ok());
    ASSERT_NE(set.mostRecent(ViewKind::Model3D), nullptr);
    EXPECT_EQ(set.mostRecent(ViewKind::Model3D)->id, second);
}

TEST(ViewSet, MostRecentIsTheLastActiveOfThatKindAfterTheUserClickedAnotherKind)
{
    // Plot, F9 and the Standard Views must reach the 3D view the user last
    // touched even after they clicked back into the plan.
    ViewSet set;
    const ViewId plan = set.add(ViewKind::Plan).id;
    set.add(ViewKind::Model3D);
    const ViewId touched = set.add(ViewKind::Model3D).id;
    ASSERT_TRUE(set.activate(touched).ok());
    ASSERT_TRUE(set.activate(plan).ok());

    ASSERT_NE(set.mostRecent(ViewKind::Model3D), nullptr);
    EXPECT_EQ(set.mostRecent(ViewKind::Model3D)->id, touched) << "not the first-opened one";
    EXPECT_EQ(set.mostRecent(ViewKind::Plan)->id, plan);
}

TEST(ViewSet, MostRecentFallsBackToTheFirstOpenedWhenNoneOfThatKindWasEverActive)
{
    ViewSet set;
    set.add(ViewKind::Plan); // active
    const ViewId first = set.add(ViewKind::Model3D).id;
    const ViewId second = set.add(ViewKind::Model3D).id;
    const ViewId third = set.add(ViewKind::Model3D).id;

    ASSERT_NE(set.mostRecent(ViewKind::Model3D), nullptr);
    EXPECT_EQ(set.mostRecent(ViewKind::Model3D)->id, first);
    ASSERT_TRUE(set.remove(first).ok());
    EXPECT_EQ(set.mostRecent(ViewKind::Model3D)->id, second);

    // And one activation outranks every never-activated view.
    ASSERT_TRUE(set.activate(third).ok());
    ASSERT_TRUE(set.activate(1).ok());
    EXPECT_EQ(set.mostRecent(ViewKind::Model3D)->id, third);
}

TEST(ViewSet, MostRecentIsNullWhenNoViewOfThatKindIsOpen)
{
    ViewSet set;
    EXPECT_EQ(set.mostRecent(ViewKind::Plan), nullptr);
    const ViewId only = set.add(ViewKind::Model3D).id;
    EXPECT_EQ(set.mostRecent(ViewKind::Section), nullptr);
    ASSERT_TRUE(set.remove(only).ok());
    EXPECT_EQ(set.mostRecent(ViewKind::Model3D), nullptr);
}

TEST(ViewSet, MostRecentFollowsAViewThatChangedKind)
{
    ViewSet set;
    set.add(ViewKind::Plan);
    const ViewId changing = set.add(ViewKind::Model3D).id;
    ASSERT_TRUE(set.setKind(changing, ViewKind::Section).ok());
    EXPECT_EQ(set.mostRecent(ViewKind::Model3D), nullptr);
    ASSERT_NE(set.mostRecent(ViewKind::Section), nullptr);
    EXPECT_EQ(set.mostRecent(ViewKind::Section)->id, changing);
}

// ---- listing ---------------------------------------------------------------------------------

TEST(ViewSet, CountCountsTheOpenViewsOfOneKind)
{
    ViewSet set;
    EXPECT_EQ(set.count(ViewKind::Plan), 0u);
    set.add(ViewKind::Plan);
    set.add(ViewKind::Model3D);
    const ViewId plan = set.add(ViewKind::Plan).id;
    EXPECT_EQ(set.count(ViewKind::Plan), 2u);
    EXPECT_EQ(set.count(ViewKind::Model3D), 1u);
    EXPECT_EQ(set.count(ViewKind::Elevation), 0u);
    ASSERT_TRUE(set.setKind(plan, ViewKind::Elevation).ok());
    EXPECT_EQ(set.count(ViewKind::Plan), 1u);
    EXPECT_EQ(set.count(ViewKind::Elevation), 1u);
    EXPECT_EQ(set.size(), 3u);
}

TEST(ViewSet, ViewsAreListedInTheOrderTheyWereOpened)
{
    ViewSet set;
    set.add(ViewKind::Plan);
    set.add(ViewKind::Model3D);
    ASSERT_TRUE(set.restore(10, ViewKind::Section).ok());
    set.add(ViewKind::Plan);
    EXPECT_EQ(idsOf(set), (std::vector<ViewId>{1, 2, 10, 11}));

    ASSERT_TRUE(set.remove(2).ok());
    EXPECT_EQ(idsOf(set), (std::vector<ViewId>{1, 10, 11}));
    ASSERT_TRUE(set.activate(11).ok());
    EXPECT_EQ(idsOf(set), (std::vector<ViewId>{1, 10, 11})) << "activation does not reorder";
}

TEST(ViewSet, AViewsStateStaysAtOneAddressWhateverElseOpensOrCloses)
{
    // A widget holds a pointer to its view's state for as long as it lives;
    // the state must not move under it. Enough views are opened to force the
    // container to grow several times over.
    ViewSet set;
    ViewState* plan = &set.add(ViewKind::Plan);
    plan->layers.hide("survey");
    ViewState* model = &set.add(ViewKind::Model3D);

    std::vector<ViewId> others;
    for (int i = 0; i < 64; ++i) {
        others.push_back(set.add(i % 2 == 0 ? ViewKind::Section : ViewKind::Elevation).id);
    }
    for (std::size_t i = 0; i < others.size(); i += 3) {
        ASSERT_TRUE(set.remove(others[i]).ok());
    }
    ASSERT_TRUE(set.setKind(model->id, ViewKind::Elevation).ok());

    EXPECT_EQ(set.find(1), plan);
    EXPECT_EQ(set.find(2), model);
    EXPECT_TRUE(plan->layers.hidesDirectly("survey"));
    EXPECT_EQ(model->kind, ViewKind::Elevation);
}

// ---- linked views (view_link.hpp) -------------------------------------------------------
//
// The link keeps plan views showing the same centre at the same scale. Every
// number below is worked by hand from ViewTransform's definitions: a view W
// by H pixels centred on c at s pixels a unit puts screen (x, y) at world
// (c.x + (x - W/2) / s, c.y - (y - H/2) / s), and zoomAt(p, k) keeps the world
// point under p where it was while the scale goes to k s.

namespace {

// A plan view W x H pixels, centred on (x, y) at `scale`, framed as a widget
// that has painted it would be.
ViewState& framedPlan(ViewSet& set, double width, double height, double x, double y,
                      double scale)
{
    ViewState& view = set.add(ViewKind::Plan);
    view.plan.resize(width, height);
    view.plan.center = katana::geometry::Point2(x, y);
    view.plan.scale = scale;
    view.planFramed = true;
    return view;
}

void expectShowing(const ViewState& view, double x, double y, double scale)
{
    EXPECT_EQ(view.plan.center.x, x) << "view " << view.id;
    EXPECT_EQ(view.plan.center.y, y) << "view " << view.id;
    EXPECT_EQ(view.plan.scale, scale) << "view " << view.id;
}

} // namespace

TEST(ViewSet, LinkedPlanViewsTakeTheCentreAndScaleOfTheViewThatMovedAndKeepTheirOwnSize)
{
    ViewSet set;
    ViewState& a = framedPlan(set, 300, 200, 10, 20, 4);
    ViewState& b = framedPlan(set, 600, 400, 0, 0, 1);
    const std::vector<ViewId> both{a.id, b.id};
    ASSERT_TRUE(set.link(both, a.id).ok());

    // A zoomed 2x at its top-left pixel (0, 0). By hand: the anchor there is
    // (10 + (0 - 150) / 4, 20 - (0 - 100) / 4) = (-27.5, 45); at scale 8 the
    // centre that keeps it at (0, 0) is (-27.5 + 150 / 8, 45 - 100 / 8) =
    // (-8.75, 32.5). Quarters and eighths: exact in binary, so compared equal.
    a.plan.zoomAt(katana::geometry::Point2(0, 0), 2.0);
    expectShowing(a, -8.75, 32.5, 8);
    set.noteMoved(a.id);
    EXPECT_EQ(set.follow(a.id), (std::vector<ViewId>{b.id}));

    expectShowing(b, -8.75, 32.5, 8);
    // Its own size: twice as wide, it shows twice as much about the same
    // centre.
    EXPECT_EQ(b.plan.widthPixels, 600.0);
    EXPECT_EQ(b.plan.heightPixels, 400.0);
}

TEST(ViewSet, AJoiningViewComesToTheViewTheUserMovedLast)
{
    // The Link button sends TO = linkLeaderFor(the view clicked). The user
    // zoomed the design view A and then linked the two in either order: the
    // as-built view B comes to A both times, and the zoom is never lost.
    for (const bool designFirst : {true, false}) {
        ViewSet set;
        ViewState& a = framedPlan(set, 300, 200, 100, 100, 3);
        ViewState& b = framedPlan(set, 300, 200, 0, 0, 1);
        set.noteMoved(a.id);
        const ViewId first = designFirst ? a.id : b.id;
        const ViewId second = designFirst ? b.id : a.id;

        auto waiting = set.link(std::vector<ViewId>{first}, set.linkLeaderFor(first));
        ASSERT_TRUE(waiting.ok()) << waiting.error().describe();
        EXPECT_TRUE(waiting->moved.empty()) << "a link of one moves nothing";
        auto joined = set.link(std::vector<ViewId>{second}, set.linkLeaderFor(second));
        ASSERT_TRUE(joined.ok()) << joined.error().describe();

        EXPECT_EQ(joined->leader, a.id) << (designFirst ? "design first" : "as-built first");
        EXPECT_EQ(joined->moved, (std::vector<ViewId>{b.id}));
        expectShowing(a, 100, 100, 3);
        expectShowing(b, 100, 100, 3);
    }
}

TEST(ViewSet, TwoViewsNobodyMovedFollowTheFirstListed)
{
    ViewSet set;
    ViewState& one = framedPlan(set, 300, 200, 1, 2, 3);
    ViewState& two = framedPlan(set, 300, 200, 4, 5, 6);
    // No TO and no link yet: the first id listed leads, whatever its number.
    auto change = set.link(std::vector<ViewId>{two.id, one.id});
    ASSERT_TRUE(change.ok()) << change.error().describe();
    EXPECT_EQ(change->leader, two.id);
    EXPECT_EQ(change->linked, (std::vector<ViewId>{one.id, two.id})) << "creation order";
    EXPECT_EQ(change->moved, (std::vector<ViewId>{one.id}));
    expectShowing(one, 4, 5, 6);
    // Nobody moved either, so the Link button of a third view would name the
    // link's lowest id.
    EXPECT_EQ(set.linkLeaderFor(framedPlan(set, 10, 10, 0, 0, 1).id), one.id);
}

TEST(ViewSet, LinkToNamesTheViewTheOthersComeTo)
{
    ViewSet set;
    ViewState& one = framedPlan(set, 300, 200, 1, 2, 3);
    ViewState& two = framedPlan(set, 300, 200, 4, 5, 6);
    ViewState& three = framedPlan(set, 300, 200, 7, 8, 9);
    ASSERT_TRUE(set.link(std::vector<ViewId>{one.id, two.id}).ok());
    expectShowing(two, 1, 2, 3);

    // Three joins and leads: the members already linked come to it too.
    auto change = set.link(std::vector<ViewId>{three.id}, three.id);
    ASSERT_TRUE(change.ok()) << change.error().describe();
    EXPECT_EQ(change->leader, three.id);
    EXPECT_EQ(change->moved, (std::vector<ViewId>{one.id, two.id}));
    expectShowing(one, 7, 8, 9);
    expectShowing(two, 7, 8, 9);

    // Without TO, a view joining a link comes to the link's lowest id.
    ViewState& four = framedPlan(set, 300, 200, -1, -1, 1);
    one.plan.center = katana::geometry::Point2(70, 80); // moved, not yet followed
    auto joined = set.link(std::vector<ViewId>{four.id});
    ASSERT_TRUE(joined.ok());
    EXPECT_EQ(joined->leader, one.id);
    expectShowing(four, 70, 80, 9);
}

TEST(ViewSet, ALinkRefusesAViewNotOpenOrALeaderOutsideItAndChangesNothing)
{
    ViewSet set;
    ViewState& one = framedPlan(set, 300, 200, 1, 2, 3);
    ViewState& two = framedPlan(set, 300, 200, 4, 5, 6);
    ViewState& three = framedPlan(set, 300, 200, 7, 8, 9);

    auto unknown = set.link(std::vector<ViewId>{one.id, 99});
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::NotFound);
    EXPECT_NE(unknown.error().message.find("no view 99 is open"), std::string::npos)
        << unknown.error().message;

    auto outside = set.link(std::vector<ViewId>{one.id, two.id}, three.id);
    ASSERT_FALSE(outside.ok());
    EXPECT_EQ(outside.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(outside.error().message.find("view 3 is neither"), std::string::npos)
        << outside.error().message;

    auto none = set.link(std::vector<ViewId>{});
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::InvalidArgument);

    EXPECT_TRUE(set.linkedViews().empty());
    expectShowing(one, 1, 2, 3);
    expectShowing(two, 4, 5, 6);

    auto unlinkUnknown = set.unlink(std::vector<ViewId>{42});
    ASSERT_FALSE(unlinkUnknown.ok());
    EXPECT_EQ(unlinkUnknown.error().code, ErrorCode::NotFound);
}

TEST(ViewSet, ALinkOfOneWaitsAndAnUnlinkThatLeavesOneDissolvesIt)
{
    ViewSet set;
    ViewState& one = framedPlan(set, 300, 200, 1, 2, 3);
    ViewState& two = framedPlan(set, 300, 200, 4, 5, 6);
    ViewState& three = framedPlan(set, 300, 200, 7, 8, 9);

    auto waiting = set.link(std::vector<ViewId>{one.id});
    ASSERT_TRUE(waiting.ok());
    EXPECT_EQ(waiting->linked, (std::vector<ViewId>{one.id}));
    EXPECT_TRUE(waiting->moved.empty());
    // Unlinking a view that is not linked is no error, and leaves the
    // waiting link waiting.
    auto nothing = set.unlink(std::vector<ViewId>{three.id});
    ASSERT_TRUE(nothing.ok());
    EXPECT_TRUE(nothing->empty());
    EXPECT_EQ(set.linkedViews(), (std::vector<ViewId>{one.id}));

    ASSERT_TRUE(set.link(std::vector<ViewId>{two.id}).ok());
    expectShowing(two, 1, 2, 3);

    // Two leaves; one is left alone, and a link of one it did not ask for is
    // no link: both have left.
    auto left = set.unlink(std::vector<ViewId>{two.id});
    ASSERT_TRUE(left.ok());
    EXPECT_EQ(*left, (std::vector<ViewId>{one.id, two.id}));
    EXPECT_TRUE(set.linkedViews().empty());
    EXPECT_FALSE(one.linked);
}

TEST(ViewSet, A3DOrSectionViewCannotJoinAndTheRefusalNamesItsKind)
{
    ViewSet set;
    ViewState& plan = framedPlan(set, 300, 200, 1, 2, 3);
    const ViewId model = set.add(ViewKind::Model3D).id;
    const ViewId section = set.add(ViewKind::Section).id;

    auto refused3d = set.link(std::vector<ViewId>{plan.id, model});
    ASSERT_FALSE(refused3d.ok());
    EXPECT_EQ(refused3d.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(refused3d.error().message, "only plan views link: view 2 is 3D");

    auto refusedSection = set.link(std::vector<ViewId>{section});
    ASSERT_FALSE(refusedSection.ok());
    EXPECT_EQ(refusedSection.error().message, "only plan views link: view 3 is Section");
    // Refused whole: the plan view listed with the 3D one did not join.
    EXPECT_TRUE(set.linkedViews().empty());
}

TEST(ViewSet, ChangingALinkedViewsKindTakesItOutOfTheLink)
{
    ViewSet set;
    ViewState& one = framedPlan(set, 300, 200, 1, 2, 3);
    ViewState& two = framedPlan(set, 300, 200, 4, 5, 6);
    ViewState& three = framedPlan(set, 300, 200, 7, 8, 9);
    ASSERT_TRUE(set.link(std::vector<ViewId>{one.id, two.id, three.id}).ok());

    ASSERT_TRUE(set.setKind(two.id, ViewKind::Model3D).ok());
    EXPECT_FALSE(two.linked);
    EXPECT_EQ(set.linkedViews(), (std::vector<ViewId>{one.id, three.id}));

    // Back to plan it stays out: joining is always asked for.
    ASSERT_TRUE(set.setKind(two.id, ViewKind::Plan).ok());
    EXPECT_FALSE(two.linked);

    // This change leaves one member, which is no link.
    ASSERT_TRUE(set.setKind(three.id, ViewKind::Section).ok());
    EXPECT_TRUE(set.linkedViews().empty());
}

TEST(ViewSet, ClosingALinkedViewLeavesTheOthersLinked)
{
    ViewSet set;
    ViewState& one = framedPlan(set, 300, 200, 1, 2, 3);
    const ViewId two = framedPlan(set, 300, 200, 4, 5, 6).id;
    const ViewId three = framedPlan(set, 300, 200, 7, 8, 9).id;
    ASSERT_TRUE(set.link(std::vector<ViewId>{one.id, two, three}).ok());

    ASSERT_TRUE(set.remove(two).ok());
    EXPECT_EQ(set.linkedViews(), (std::vector<ViewId>{one.id, three}));

    // A state handed over by take() is out of the link as well, and so is
    // the view it leaves alone.
    std::unique_ptr<ViewState> taken = set.take(three);
    ASSERT_NE(taken, nullptr);
    EXPECT_FALSE(taken->linked);
    EXPECT_TRUE(set.linkedViews().empty());
    EXPECT_FALSE(one.linked);
}

TEST(ViewSet, FollowReturnsOnlyTheLinkedViewsItChanged)
{
    ViewSet set;
    ViewState& one = framedPlan(set, 300, 200, 1, 2, 3);
    ViewState& two = framedPlan(set, 300, 200, 4, 5, 6);
    ViewState& three = framedPlan(set, 300, 200, 7, 8, 9);
    ASSERT_TRUE(set.link(std::vector<ViewId>{one.id, three.id}).ok());

    one.plan.center = katana::geometry::Point2(-3, -4);
    one.plan.scale = 0.5;
    EXPECT_EQ(set.follow(one.id), (std::vector<ViewId>{three.id}));
    expectShowing(three, -3, -4, 0.5);
    expectShowing(two, 4, 5, 6); // not linked: untouched

    // A view outside the link moves nobody, and neither does one not open.
    EXPECT_TRUE(set.follow(two.id).empty());
    EXPECT_TRUE(set.follow(99).empty());
    expectShowing(one, -3, -4, 0.5);
}

TEST(ViewSet, UnlinkingMovesNothing)
{
    ViewSet set;
    ViewState& one = framedPlan(set, 300, 200, 1, 2, 3);
    ViewState& two = framedPlan(set, 300, 200, 4, 5, 6);
    ASSERT_TRUE(set.link(std::vector<ViewId>{one.id, two.id}).ok());
    expectShowing(two, 1, 2, 3);

    one.plan.center = katana::geometry::Point2(50, 60);
    auto left = set.unlink(std::vector<ViewId>{one.id});
    ASSERT_TRUE(left.ok());
    expectShowing(two, 1, 2, 3);
    expectShowing(one, 50, 60, 3);
    EXPECT_TRUE(set.unlinkAll().empty()) << "nothing was left to unlink";
}

TEST(ViewSet, AnUnframedLeaderMovesNobodyUntilItIsFramed)
{
    ViewSet set;
    ViewState& fresh = set.add(ViewKind::Plan); // opened, never painted
    ViewState& shown = framedPlan(set, 300, 200, 4, 5, 6);

    auto change = set.link(std::vector<ViewId>{fresh.id, shown.id});
    ASSERT_TRUE(change.ok());
    EXPECT_EQ(change->leader, fresh.id);
    EXPECT_TRUE(change->moved.empty());
    expectShowing(shown, 4, 5, 6);

    // Its first paint frames it; the widget reports that as a move.
    fresh.plan.center = katana::geometry::Point2(9, 9);
    fresh.plan.scale = 7;
    fresh.planFramed = true;
    EXPECT_EQ(set.follow(fresh.id), (std::vector<ViewId>{shown.id}));
    expectShowing(shown, 9, 9, 7);
}

TEST(ViewSet, TheViewMovedMostRecentlyLeadsAndNoteMovedIgnoresAViewNotOpen)
{
    ViewSet set;
    const ViewId one = framedPlan(set, 300, 200, 1, 2, 3).id;
    const ViewId two = framedPlan(set, 300, 200, 4, 5, 6).id;
    EXPECT_EQ(set.linkLeaderFor(two), two) << "no link and no move: the view clicked";
    set.noteMoved(one);
    set.noteMoved(two);
    set.noteMoved(99);
    // No link: only the view asked about counts.
    EXPECT_EQ(set.linkLeaderFor(one), one);
    ASSERT_TRUE(set.link(std::vector<ViewId>{two}).ok());
    EXPECT_EQ(set.linkLeaderFor(one), two) << "two, the member, was moved after one";
    set.noteMoved(one);
    EXPECT_EQ(set.linkLeaderFor(one), one);
    EXPECT_GT(set.find(one)->lastMoved, set.find(two)->lastMoved);
}
