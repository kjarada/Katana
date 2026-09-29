// VIEWS and ZOOM (include/katana/cad/view_verbs.hpp): the window's views on
// the command line, run through the interpreter as the window runs them,
// against a real ViewSet held by a host that does what the window's workspace
// does with a request and records it. Every expected number is worked by hand
// from ViewTransform's definitions - a view W by H pixels centred on c at s
// pixels a unit shows x from c.x - W/2s to c.x + W/2s and y likewise - never
// read back from a run.

#include <gtest/gtest.h>

#include <cmath>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/cad/view_verbs.hpp"
#include "katana/core/text.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Point2;

namespace {

// What the window's workspace does with a request, minus the widgets: a plan
// view's IN, OUT, factor and CENTRE through applyPlanZoom, WINDOW fitted with
// the 0.08 margin Zoom Extents leaves, EXTENTS left where it is (it frames
// what a widget draws, which there is none of here); then the move is noted
// and the link follows, as ViewWorkspace::viewMoved does.
class RecordingHost final : public ViewVerbHost {
  public:
    ViewSet set;
    std::vector<ZoomRequest> zooms;
    std::vector<std::vector<ViewId>> changes;

    ViewSet& views() override { return set; }
    Result<ViewId> open(ViewKind kind) override
    {
        const ViewId id = set.add(kind).id;
        (void)set.activate(id);
        return id;
    }
    Status activate(ViewId id) override { return set.activate(id); }
    Result<std::vector<ViewId>> zoom(const ZoomRequest& request) override
    {
        zooms.push_back(request);
        ViewState& view = *set.find(request.view);
        if (view.kind == ViewKind::Plan) {
            if (!applyPlanZoom(view.plan, request) && request.kind == ZoomRequest::Kind::Window) {
                view.plan.fit(request.window, 0.08);
            }
            view.planFramed = true;
        }
        set.noteMoved(view.id);
        std::vector<ViewId> moved{view.id};
        for (const ViewId id : set.follow(view.id)) {
            moved.push_back(id);
        }
        return moved;
    }
    void changed(const std::vector<ViewId>& ids) override { changes.push_back(ids); }
};

class ViewVerbsTest : public ::testing::Test {
  protected:
    Document document;
    CommandInterpreter interpreter{document};
    RecordingHost host;

    ViewVerbsTest()
    {
        interpreter.setViewHost([this] { return &host; });
    }

    // A plan view 300 x 200 px centred on (10, 20) at 4 px a unit, framed.
    ViewState& plan(double x = 10, double y = 20, double scale = 4)
    {
        ViewState& view = host.set.add(ViewKind::Plan);
        view.plan.resize(300, 200);
        view.plan.center = Point2(x, y);
        view.plan.scale = scale;
        view.planFramed = true;
        return view;
    }

    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string();
    }

    katana::core::Error refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was not refused: " << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
};

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// The value of `key=` in a record line, up to the next blank.
std::string field(const std::string& record, const std::string& key)
{
    const std::size_t at = record.find(" " + key + "=");
    if (at == std::string::npos) {
        return {};
    }
    const std::size_t start = at + key.size() + 2;
    return record.substr(start, record.find(' ', start) - start);
}

} // namespace

TEST_F(ViewVerbsTest, ViewsListsEveryOpenViewAsOneRecordInCreationOrder)
{
    plan();
    ViewState& model = host.set.add(ViewKind::Model3D);
    model.camera.setTarget(katana::geometry::Point3(1, 2, 3));
    model.camera.setDistance(50);
    model.camera.setProjection(katana::render::Projection::Perspective);
    host.set.add(ViewKind::Section);

    const std::string reply = ok("VIEWS");
    const auto lines = katana::core::splitLines(reply);
    ASSERT_EQ(lines.size(), 4U) << reply;
    // 300 x 200 at 4 px a unit about (10, 20): x 10 -+ 37.5, y 20 -+ 25.
    EXPECT_EQ(lines[0], "view=1 kind=plan title=\"Plan 1\" active=yes linked=no centre=10,20 "
                        "scale=4 area=-27.5,-5,47.5,45");
    EXPECT_TRUE(lines[1].starts_with(
        "view=2 kind=3d title=\"3D 1\" active=no target=1,2,3 distance=50 azimuth="))
        << lines[1];
    // The angles in degrees: the camera's radians times 180 / pi.
    const double azimuth =
        *katana::core::parseFiniteDouble(field(std::string(lines[1]), "azimuth"));
    EXPECT_NEAR(azimuth, model.camera.azimuth() * 180.0 / 3.14159265358979323846, 1e-12);
    EXPECT_EQ(field(std::string(lines[1]), "projection"), "perspective");
    EXPECT_EQ(lines[2], "view=3 kind=section title=\"Section 1\" active=no");
    EXPECT_EQ(lines[3], "views=3 linked=0");
    // LIST is the same.
    EXPECT_EQ(ok("VIEWS LIST"), reply);
}

TEST_F(ViewVerbsTest, ViewsOpenAndActivateReplyTheViewsRecord)
{
    plan();
    const std::string opened = ok("VIEWS OPEN elevation");
    EXPECT_TRUE(
        opened.starts_with("view=2 kind=elevation title=\"Elevation 1\" active=yes target="))
        << opened;
    EXPECT_EQ(host.set.find(2)->kind, ViewKind::Elevation);
    const std::string activated = ok("views activate 1");
    EXPECT_TRUE(
        activated.starts_with("view=1 kind=plan title=\"Plan 1\" active=yes linked=no centre="))
        << activated;
    EXPECT_EQ(host.set.activeId(), 1U);

    EXPECT_TRUE(contains(refused("VIEWS OPEN map").message, "not 'map'"));
    EXPECT_EQ(refused("VIEWS ACTIVATE 9").code, ErrorCode::NotFound);
    const auto title = refused("VIEWS ACTIVATE Plan");
    EXPECT_TRUE(contains(title.message, "not the number in a view's title")) << title.message;
    EXPECT_TRUE(contains(refused("VIEWS SHUFFLE").message, "VIEWS does not take 'SHUFFLE'"));
}

TEST_F(ViewVerbsTest, ViewsLinkRepliesWithTheLeaderTheMembersAndWhoMoved)
{
    plan(10, 20, 4);
    ViewState& two = plan(-5, 7, 1);

    EXPECT_EQ(ok("VIEWS LINK 1,2"), "leader=1 linked=1,2 moved=2");
    EXPECT_EQ(two.plan.center, Point2(10, 20));
    EXPECT_EQ(two.plan.scale, 4.0);
    ASSERT_EQ(host.changes.size(), 1U);
    EXPECT_EQ(host.changes.back(), (std::vector<ViewId>{1, 2})) << "the window repaints both";

    // TO, ids in two words, and the link's record in VIEWS.
    EXPECT_EQ(ok("VIEWS LINK 2 1 TO 2"), "leader=2 linked=1,2 moved=1");
    EXPECT_TRUE(contains(ok("VIEWS"), "views=2 linked=2"));

    // The unlink that leaves one member dissolves the link: both left.
    EXPECT_EQ(ok("VIEWS UNLINK 2"), "unlinked=1,2 linked=none");
    EXPECT_EQ(host.changes.back(), (std::vector<ViewId>{1, 2}));
    EXPECT_EQ(ok("VIEWS UNLINK ALL"), "unlinked=none linked=none");
}

TEST_F(ViewVerbsTest, ViewsLinkOfA3DViewIsRefusedNamingItsKindAndChangesNothing)
{
    plan();
    host.set.add(ViewKind::Model3D);
    const auto refusal = refused("VIEWS LINK 1,2");
    EXPECT_EQ(refusal.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(refusal.message, "only plan views link: view 2 is 3D");
    EXPECT_TRUE(host.set.linkedViews().empty());
    EXPECT_TRUE(host.changes.empty());
    EXPECT_TRUE(contains(refused("VIEWS LINK 1 TO").message, "TO takes the one view"));
    EXPECT_EQ(refused("VIEWS LINK 1,x").code, ErrorCode::ParseFailure);
}

TEST_F(ViewVerbsTest, ZoomAloneIsTheActiveViewsExtents)
{
    plan();
    plan();
    ASSERT_TRUE(host.set.activate(2).ok());
    for (const char* line : {"ZOOM", "Z", "zoom extents", "ZOOM E"}) {
        const std::size_t before = host.zooms.size();
        (void)ok(line);
        ASSERT_EQ(host.zooms.size(), before + 1) << line;
        EXPECT_EQ(host.zooms.back().kind, ZoomRequest::Kind::Extents) << line;
        EXPECT_EQ(host.zooms.back().view, 2U) << line;
    }
}

TEST_F(ViewVerbsTest, ZoomCentreScaleSetsThePlanViewExactly)
{
    plan();
    // 300 x 200 at 8 px a unit about (50, 40): x 50 -+ 18.75, y 40 -+ 12.5.
    EXPECT_EQ(ok("ZOOM CENTRE 50,40 SCALE 8 view=1"),
              "view=1 kind=plan centre=50,40 scale=8 area=31.25,27.5,68.75,52.5");
    const ViewState& view = *host.set.find(1);
    EXPECT_EQ(view.plan.center, Point2(50, 40));
    EXPECT_EQ(view.plan.scale, 8.0);
    // Without SCALE only the centre moves; view= may stand first.
    (void)ok("ZOOM view=1 C -1.5,2.25");
    EXPECT_EQ(view.plan.center, Point2(-1.5, 2.25));
    EXPECT_EQ(view.plan.scale, 8.0);
}

TEST_F(ViewVerbsTest, ZoomInAndOutAreByTwoAboutTheCentreUnlessAFactorIsGiven)
{
    const ViewState& view = plan(10, 20, 4);
    (void)ok("ZOOM IN");
    EXPECT_EQ(view.plan.scale, 8.0);
    (void)ok("ZOOM OUT 4");
    EXPECT_EQ(view.plan.scale, 2.0);
    (void)ok("ZOOM 0.5");
    EXPECT_EQ(view.plan.scale, 1.0);
    // About the centre pixel, which is the centre exactly: it never moves.
    EXPECT_EQ(view.plan.center, Point2(10, 20));

    for (const char* line : {"ZOOM IN 0", "ZOOM OUT -2", "ZOOM 0"}) {
        EXPECT_EQ(refused(line).code, ErrorCode::ParseFailure) << line;
    }
    EXPECT_EQ(view.plan.scale, 1.0);
}

TEST_F(ViewVerbsTest, ZoomWindowFramesTheBoxAsZoomExtentsFramesTheDrawing)
{
    const ViewState& view = plan(10, 20, 4);
    // The box 0,0 to 50,40 in 300 x 200 with 8% each side: 84% of 300 / 50 =
    // 5.04 and 84% of 200 / 40 = 4.2; the smaller fits, about (25, 20).
    (void)ok("ZOOM WINDOW 50,40,0,0");
    ASSERT_EQ(host.zooms.back().kind, ZoomRequest::Kind::Window);
    EXPECT_EQ(host.zooms.back().window.min, Point2(0, 0));
    EXPECT_EQ(host.zooms.back().window.max, Point2(50, 40));
    EXPECT_EQ(view.plan.center, Point2(25, 20));
    EXPECT_NEAR(view.plan.scale, 4.2, 1e-12);
    // Two words are the same two corners.
    (void)ok("ZOOM W 0,0 50,40");
    EXPECT_EQ(host.zooms.back().window.max, Point2(50, 40));

    const auto same = refused("ZOOM WINDOW 5,5,5,5");
    EXPECT_EQ(same.message, "a window needs two different corners");
    EXPECT_EQ(refused("ZOOM WINDOW 1,2,3").code, ErrorCode::ParseFailure);
}

TEST_F(ViewVerbsTest, ZoomRefusesAWordItDoesNotKnowNamingIt)
{
    plan();
    // It once zoomed to the extents whatever followed ZOOM.
    const auto refusal = refused("ZOOM FOO BAR");
    EXPECT_EQ(refusal.code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(refusal.message, "ZOOM does not take 'FOO': ZOOM [EXTENTS | IN [f]"))
        << refusal.message;
    EXPECT_TRUE(contains(refused("ZOOM IN 2 BAR").message, "'BAR'"));
    EXPECT_TRUE(contains(refused("ZOOM view=1 view=1").message, "view= once"));
    EXPECT_TRUE(host.zooms.empty()) << "nothing refused was asked of the window";
}

TEST_F(ViewVerbsTest, ZoomWindowOnA3DViewIsRefusedNamingTheKind)
{
    plan();
    host.set.add(ViewKind::Model3D);
    const auto refusal = refused("ZOOM WINDOW 0,0,10,10 view=2");
    EXPECT_EQ(refusal.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(refusal.message, "ZOOM WINDOW frames a plan view: view 2 is 3D"))
        << refusal.message;
    EXPECT_TRUE(contains(refused("ZOOM IN view=2").message, "ZOOM IN zooms a plan view"));
    // EXTENTS frames any view.
    (void)ok("ZOOM EXTENTS view=2");
    EXPECT_EQ(host.zooms.size(), 1U);
}

TEST_F(ViewVerbsTest, ZoomOfAViewThatIsNotOpenIsRefusedNamingIt)
{
    plan();
    const auto refusal = refused("ZOOM view=9");
    EXPECT_EQ(refusal.code, ErrorCode::NotFound);
    EXPECT_EQ(refusal.message, "no view 9 is open (VIEWS lists them)");
    EXPECT_EQ(refused("ZOOM view=0").code, ErrorCode::ParseFailure);
    EXPECT_EQ(refused("ZOOM view=").code, ErrorCode::ParseFailure);
}

TEST_F(ViewVerbsTest, ZoomScaleOutsideTheLimitsIsRefused)
{
    const ViewState& view = plan();
    for (const char* line : {"ZOOM CENTRE 0,0 SCALE 1e8", "ZOOM CENTRE 0,0 SCALE 0",
                             "ZOOM CENTRE 0,0 SCALE -1", "ZOOM CENTRE 0,0 SCALE"}) {
        const auto refusal = refused(line);
        EXPECT_EQ(refusal.message, "SCALE is pixels per unit from 1e-07 to 1e+07") << line;
    }
    EXPECT_EQ(view.plan.center, Point2(10, 20));
    // At the limits themselves it is taken.
    (void)ok("ZOOM CENTRE 0,0 SCALE 1e7");
    EXPECT_EQ(view.plan.scale, 1e7);
}

TEST_F(ViewVerbsTest, ViewsAndZoomAreRefusedByNameWithoutAWindow)
{
    // What katana_cli and katana_mcp have: an interpreter and no views.
    Document alone;
    CommandInterpreter headless(alone);
    for (const auto& [line, verb] : std::vector<std::pair<std::string, std::string>>{
             {"VIEWS", "VIEWS"}, {"VIEWS LINK 1,2", "VIEWS"}, {"ZOOM", "ZOOM"}, {"Z", "ZOOM"},
             {"ZOOM IN view=1", "ZOOM"}}) {
        auto reply = headless.run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, ErrorCode::Unsupported) << line;
        EXPECT_TRUE(contains(reply.error().message, verb + " is the desktop window's"))
            << reply.error().message;
        EXPECT_TRUE(contains(reply.error().message, "katana --command")) << reply.error().message;
    }
    // A provider that answers null is no window either.
    headless.setViewHost([] { return static_cast<ViewVerbHost*>(nullptr); });
    EXPECT_EQ(headless.run("VIEWS").error().code, ErrorCode::Unsupported);
}

TEST_F(ViewVerbsTest, ZoomRepliesWithTheViewThenEveryViewThatFollowed)
{
    plan(10, 20, 4);
    plan(0, 0, 1);
    plan(3, 3, 3);
    (void)ok("VIEWS LINK 1,2");
    // View 2 twice as close about its centre: 8 px a unit at (10, 20); view
    // 1 follows. View 3 is not linked and is not mentioned.
    const std::string reply = ok("ZOOM IN view=2");
    const auto lines = katana::core::splitLines(reply);
    ASSERT_EQ(lines.size(), 2U) << reply;
    EXPECT_EQ(lines[0], "view=2 kind=plan centre=10,20 scale=8 area=-8.75,7.5,28.75,32.5");
    EXPECT_EQ(lines[1],
              "view=1 kind=plan followed=2 centre=10,20 scale=8 area=-8.75,7.5,28.75,32.5");
    EXPECT_EQ(host.set.find(3)->plan.scale, 3.0);
}

TEST(ViewZoom, ApplyPlanZoomLeavesWindowAndExtentsToTheWidget)
{
    ViewTransform view;
    view.resize(300, 200);
    view.center = Point2(10, 20);
    view.scale = 4;
    ZoomRequest request;
    request.kind = ZoomRequest::Kind::Extents;
    EXPECT_FALSE(applyPlanZoom(view, request));
    request.kind = ZoomRequest::Kind::Window;
    request.window = katana::geometry::Box2(Point2(0, 0), Point2(1, 1));
    EXPECT_FALSE(applyPlanZoom(view, request));
    EXPECT_EQ(view.center, Point2(10, 20));
    EXPECT_EQ(view.scale, 4.0);

    // IN clamps as the wheel does: at the largest scale it stays there.
    view.scale = ViewTransform::kMaximumScale;
    request.kind = ZoomRequest::Kind::In;
    request.factor = 2;
    EXPECT_TRUE(applyPlanZoom(view, request));
    EXPECT_EQ(view.scale, ViewTransform::kMaximumScale);
}
