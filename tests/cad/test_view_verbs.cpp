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
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/tables.hpp"

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
    std::vector<ViewId> settingChanges;

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
            if (!applyPlanZoom(view.plan, request) &&
                (request.kind == ZoomRequest::Kind::Window ||
                 request.kind == ZoomRequest::Kind::Scope)) {
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
    void settingsChanged(ViewId id) override { settingChanges.push_back(id); }
};

class ViewVerbsTest : public ::testing::Test {
  protected:
    Document document;
    CommandInterpreter interpreter{document};
    RecordingHost host;

    ViewVerbsTest()
    {
        interpreter.setViewHost([this] { return &host; });
        // The scope word VIEW answered as the window answers it.
        interpreter.setScopeContext([this](std::optional<std::uint32_t> id) {
            return scopeViewOf(host.set, id);
        });
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
    // 300 x 200 at 4 px a unit about (10, 20): x 10 -+ 37.5, y 20 -+ 25. And
    // every kind of view ghosts the selection until told not to
    // (ViewState::selectionGhosts).
    EXPECT_EQ(lines[0], "view=1 kind=plan title=\"Plan 1\" active=yes linked=no centre=10,20 "
                        "scale=4 area=-27.5,-5,47.5,45 ghosts=on");
    EXPECT_TRUE(lines[1].starts_with(
        "view=2 kind=3d title=\"3D 1\" active=no target=1,2,3 distance=50 azimuth="))
        << lines[1];
    // The angles in degrees: the camera's radians times 180 / pi.
    const double azimuth =
        *katana::core::parseFiniteDouble(field(std::string(lines[1]), "azimuth"));
    EXPECT_NEAR(azimuth, model.camera.azimuth() * 180.0 / 3.14159265358979323846, 1e-12);
    EXPECT_EQ(field(std::string(lines[1]), "projection"), "perspective");
    EXPECT_EQ(lines[2], "view=3 kind=section title=\"Section 1\" active=no ghosts=on");
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

TEST_F(ViewVerbsTest, ZoomAllAndTheApostropheFormsAreTheExtentsAsInEveryCadProgram)
{
    // A script's commonest zoom lines. The old window zoomed the extents for
    // any ZOOM line; ZOOM A was then refused here, and a script that ran to
    // the end there stopped at it.
    plan();
    for (const char* line :
         {"ZOOM A", "Z A", "zoom all", "Z ALL", "'ZOOM", "'Z", "'zoom a", "'Z E"}) {
        const std::size_t before = host.zooms.size();
        (void)ok(line);
        ASSERT_EQ(host.zooms.size(), before + 1) << line;
        EXPECT_EQ(host.zooms.back().kind, ZoomRequest::Kind::Extents) << line;
    }
    // ALL with a filter after it is the scope word for the drawing: what it
    // matches, framed.
    (void)ok("LINE 10,10 40,30");
    const std::string filtered = ok("ZOOM ALL WHERE TYPE=line");
    EXPECT_TRUE(filtered.starts_with("scope=drawing where=\"TYPE=line\" matched=1\n")) << filtered;
    EXPECT_EQ(host.zooms.back().kind, ZoomRequest::Kind::Scope);
    // Only the view verbs have an apostrophe form.
    const auto other = refused("'LINE 0,0 1,1");
    EXPECT_EQ(other.message, "unknown command; type HELP");
}

TEST_F(ViewVerbsTest, AFactorWithAutoCadsXIsRelativeToTheView)
{
    // AutoCAD's "2X" is twice the current view's scale: what a bare factor
    // is here.
    const ViewState& view = plan(10, 20, 4);
    (void)ok("ZOOM 2X");
    EXPECT_EQ(view.plan.scale, 8.0);
    (void)ok("Z 0.5x");
    EXPECT_EQ(view.plan.scale, 4.0);
    EXPECT_EQ(view.plan.center, Point2(10, 20));
    // Relative to paper space means nothing in a plan view; no number, or a
    // factor not above 0, is refused as a bare one is.
    for (const char* line : {"ZOOM 2XP", "ZOOM X", "ZOOM 0X", "ZOOM -2X"}) {
        EXPECT_EQ(refused(line).code, ErrorCode::ParseFailure) << line;
    }
    EXPECT_EQ(view.plan.scale, 4.0);
}

TEST_F(ViewVerbsTest, DegenerateZoomAndViewsLinesAreRefusedNamingWhatIsMissing)
{
    // No view open at all.
    const auto none = refused("ZOOM");
    EXPECT_EQ(none.code, ErrorCode::InvalidState);
    EXPECT_EQ(none.message, "no view is open to zoom: VIEWS OPEN plan opens one");

    plan();
    const auto centre = refused("ZOOM CENTRE");
    EXPECT_EQ(centre.message, "ZOOM CENTRE takes the point to centre on: x,y");
    EXPECT_EQ(refused("ZOOM CENTRE 5").message, "ZOOM CENTRE takes the point to centre on: x,y");
    EXPECT_EQ(refused("VIEWS UNLINK").message,
              "name the views to unlink: VIEWS UNLINK <id>[,<id>...] | ALL");
    EXPECT_EQ(refused("VIEWS OPEN").message,
              "VIEWS OPEN takes what the view shows: plan, 3d, section or elevation");
    EXPECT_EQ(refused("VIEWS OPEN plan 3d").message,
              "VIEWS OPEN takes what the view shows: plan, 3d, section or elevation");
    EXPECT_EQ(refused("VIEWS OPEN map").message,
              "VIEWS OPEN takes plan, 3d, section or elevation, not 'map'");
    EXPECT_EQ(refused("VIEWS ACTIVATE").message, "VIEWS ACTIVATE takes one view id");
    EXPECT_EQ(refused("VIEWS ACTIVATE 1 2").message, "VIEWS ACTIVATE takes one view id");
    EXPECT_TRUE(host.zooms.empty()) << "nothing refused was asked of the window";
    EXPECT_EQ(host.set.size(), 1U) << "nothing opened";
}

TEST_F(ViewVerbsTest, ZoomSelectionOfOnePointCentresOnItAndKeepsTheScale)
{
    // A point has no extent to fit (ViewTransform::fit): the view centres on
    // it at the scale it had, and the scope says it took one.
    const ViewState& view = plan(10, 20, 4);
    (void)ok("POINT 70,80");
    (void)ok("SELECT ALL");
    const std::string reply = ok("ZOOM SELECTION");
    EXPECT_TRUE(reply.starts_with("scope=selection matched=1\nview=1 kind=plan centre=70,80 "))
        << reply;
    EXPECT_EQ(view.plan.center, Point2(70, 80));
    EXPECT_EQ(view.plan.scale, 4.0);
}

TEST(ViewZoom, TheKindsEachZoomTakesAreTheVerbsOneRule)
{
    // The table the bars, the View menu and the Zoom To dialog read
    // (cad::zoomTakes), as view_verbs.hpp states it: EXTENTS, IN, OUT and a
    // factor every kind, about its middle; a scope every kind but a section,
    // which shows where entities cross it and has no box of them to frame;
    // WINDOW and CENTRE a plan view, whose plan position they are.
    using Kind = ZoomRequest::Kind;
    for (const ViewKind kind :
         {ViewKind::Plan, ViewKind::Model3D, ViewKind::Section, ViewKind::Elevation}) {
        const bool plan = kind == ViewKind::Plan;
        EXPECT_TRUE(zoomTakes(kind, Kind::Extents));
        EXPECT_TRUE(zoomTakes(kind, Kind::In));
        EXPECT_TRUE(zoomTakes(kind, Kind::Out));
        EXPECT_TRUE(zoomTakes(kind, Kind::Factor));
        EXPECT_EQ(zoomTakes(kind, Kind::Scope), kind != ViewKind::Section);
        EXPECT_EQ(zoomTakes(kind, Kind::Window), plan);
        EXPECT_EQ(zoomTakes(kind, Kind::Centre), plan);
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
    const auto centre = refused("ZOOM CENTRE 1,2 view=2");
    EXPECT_TRUE(contains(centre.message, "ZOOM CENTRE centres a plan view: view 2 is 3D"))
        << centre.message;
    // EXTENTS frames any view; IN, OUT and a factor zoom any view about its
    // middle - a 3D view as its wheel zooms there - and reach it as asked.
    (void)ok("ZOOM EXTENTS view=2");
    (void)ok("ZOOM IN view=2");
    (void)ok("ZOOM 0.5 view=2");
    ASSERT_EQ(host.zooms.size(), 3U);
    EXPECT_EQ(host.zooms[1].kind, ZoomRequest::Kind::In);
    EXPECT_EQ(host.zooms[1].factor, 2.0);
    EXPECT_EQ(host.zooms[2].kind, ZoomRequest::Kind::Factor);
    EXPECT_EQ(host.zooms[2].factor, 0.5);
    EXPECT_EQ(host.zooms[2].view, 2U);
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

// ---- a view's own layers: HIDE, SHOW, ISOLATE ---------------------------------------------
//
// The drawing: the default layer 0, design with design/road beneath it, and
// asbuilt. What each line leaves hidden is worked out from LayerOverrides'
// contract (layer_overrides.hpp), not read back.

namespace {

void addLayers(Document& document, std::initializer_list<const char*> names)
{
    for (const char* name : names) {
        katana::entity::Layer layer;
        layer.name = name;
        ASSERT_TRUE(document.execute(katana::commands::createLayer(layer)).ok()) << name;
    }
}

} // namespace

TEST_F(ViewVerbsTest, ViewsHideShowIsolateChangeOnlyThatView)
{
    addLayers(document, {"design", "design/road", "asbuilt"});
    const ViewState& design = plan();
    const ViewState& asBuilt = plan();

    // The design view hides the as-built layer, the as-built view the design
    // one - and with it design/road, beneath it.
    EXPECT_TRUE(ok("VIEWS HIDE 1 asbuilt").ends_with(" hidden=asbuilt"));
    EXPECT_TRUE(ok("VIEWS HIDE 2 design").ends_with(" hidden=design"));
    EXPECT_TRUE(design.layers.hides("asbuilt"));
    EXPECT_FALSE(design.layers.hides("design/road"));
    EXPECT_TRUE(asBuilt.layers.hides("design/road"));
    EXPECT_FALSE(asBuilt.layers.hides("asbuilt"));
    EXPECT_EQ(host.settingChanges, (std::vector<ViewId>{1, 2})) << "each view redrawn, alone";

    // SHOW takes back exactly the entries named; a record with nothing
    // hidden says nothing about it.
    const std::string shown = ok("VIEWS SHOW 1 asbuilt");
    EXPECT_EQ(shown.find("hidden="), std::string::npos) << shown;
    EXPECT_TRUE(design.layers.empty());

    // ISOLATE design/road: at each level of the path, the siblings of the
    // next segment - 0 and asbuilt at the root; design has no child but road.
    EXPECT_TRUE(ok("VIEWS ISOLATE 1 design/road").ends_with(" hidden=0,asbuilt"));
    EXPECT_TRUE(ok("VIEWS SHOW 1 ALL").ends_with("area=-27.5,-5,47.5,45 ghosts=on"));
    EXPECT_TRUE(design.layers.empty());
    // The as-built view was never touched by any of that.
    EXPECT_EQ(asBuilt.layers.hidden().size(), 1U);

    // Two layers in one list, and in two words.
    (void)ok("VIEWS HIDE 1 design,asbuilt 0");
    EXPECT_EQ(design.layers.size(), 3U);
    // VIEWS lists the hidden layers in every record.
    const std::string listed = ok("VIEWS");
    EXPECT_NE(listed.find("hidden=0,asbuilt,design\n"), std::string::npos) << listed;
}

TEST_F(ViewVerbsTest, HidingALayerTheDrawingLacksIsRefusedNamingIt)
{
    addLayers(document, {"design", "design/road"});
    const ViewState& view = plan();
    const auto missing = refused("VIEWS HIDE 1 design,roads");
    EXPECT_EQ(missing.code, ErrorCode::NotFound);
    EXPECT_EQ(missing.message, "no layer 'roads' in the drawing (LAYER LIST lists them)");
    // Refused whole: design, which exists, was not hidden either.
    EXPECT_TRUE(view.layers.empty());
    EXPECT_TRUE(host.settingChanges.empty());
    EXPECT_EQ(refused("VIEWS ISOLATE 1 roads").code, ErrorCode::NotFound);
    EXPECT_EQ(refused("VIEWS SHOW 1 roads").code, ErrorCode::NotFound);
    EXPECT_TRUE(contains(refused("VIEWS ISOLATE 1 design design/road").message,
                         "ISOLATE takes one layer"));
    EXPECT_EQ(refused("VIEWS HIDE 9 design").code, ErrorCode::NotFound);
    EXPECT_TRUE(contains(refused("VIEWS HIDE 1").message, "takes a view id and the layers"));
    // A node of the tree with no layer of its own is a layer to hide: what
    // the Layers popup offers as "design" when only design/road existed.
    addLayers(document, {"survey/points"});
    EXPECT_TRUE(ok("VIEWS HIDE 1 survey").ends_with(" hidden=survey"));
}

TEST_F(ViewVerbsTest, ShowingALayerBeneathOneTheViewHidesIsRefusedNamingWhatHoldsIt)
{
    // design hidden hides design/road with it (LayerOverrides): SHOW of the
    // road alone would leave it hidden and answer with the same record, as
    // if it had done something.
    addLayers(document, {"design", "design/road", "asbuilt"});
    const ViewState& view = plan();
    (void)ok("VIEWS HIDE 1 design,asbuilt");
    host.settingChanges.clear();

    const auto held = refused("VIEWS SHOW 1 asbuilt,design/road");
    EXPECT_EQ(held.code, ErrorCode::CommandRejected);
    EXPECT_EQ(held.message,
              "view 1 hides 'design', which holds 'design/road': show 'design' (VIEWS SHOW 1 "
              "design)");
    // Read whole before any of it is shown: asbuilt, before it, is hidden still.
    EXPECT_TRUE(view.layers.hidesDirectly("asbuilt"));
    EXPECT_TRUE(host.settingChanges.empty());

    // With what holds it on the same line, both are shown.
    EXPECT_TRUE(ok("VIEWS SHOW 1 design/road,design").ends_with(" hidden=asbuilt"));
    EXPECT_FALSE(view.layers.hides("design/road"));
}

TEST_F(ViewVerbsTest, ViewsSetGhostsOffTurnsThatViewsGhostsOffAndOnAgain)
{
    const ViewState& design = plan();
    const ViewState& asBuilt = plan();
    ASSERT_TRUE(design.selectionGhosts) << "a view ghosts the selection when it opens";

    const std::string off = ok("VIEWS SET 2 ghosts=off");
    EXPECT_TRUE(off.starts_with("view=2 kind=plan ")) << off;
    EXPECT_TRUE(contains(off, " ghosts=off")) << off;
    EXPECT_FALSE(asBuilt.selectionGhosts);
    EXPECT_TRUE(design.selectionGhosts) << "only the view named";
    EXPECT_EQ(host.settingChanges, (std::vector<ViewId>{2})) << "that view redrawn, alone";

    // The words as a person types them.
    EXPECT_TRUE(contains(ok("views set 2 GHOSTS=On"), " ghosts=on"));
    EXPECT_TRUE(asBuilt.selectionGhosts);
    // VIEWS says so of every view.
    EXPECT_TRUE(contains(ok("VIEWS SET 1 ghosts=off"), " ghosts=off"));
    const auto lines = katana::core::splitLines(ok("VIEWS"));
    ASSERT_EQ(lines.size(), 3U);
    EXPECT_TRUE(lines[0].ends_with(" ghosts=off")) << lines[0];
    EXPECT_TRUE(lines[1].ends_with(" ghosts=on")) << lines[1];
}

TEST_F(ViewVerbsTest, ViewsSetRefusesWhatItDoesNotTakeAndChangesNothing)
{
    const ViewState& view = plan();
    EXPECT_EQ(refused("VIEWS SET 9 ghosts=off").code, ErrorCode::NotFound);
    const auto bare = refused("VIEWS SET 1");
    EXPECT_TRUE(contains(bare.message, "VIEWS SET <id> ghosts=on|off")) << bare.message;
    const auto value = refused("VIEWS SET 1 ghosts=maybe");
    EXPECT_EQ(value.message, "ghosts= is on or off, not 'maybe'");
    const auto key = refused("VIEWS SET 1 grid=on");
    EXPECT_TRUE(contains(key.message, "does not take 'grid=on'")) << key.message;
    EXPECT_TRUE(contains(refused("VIEWS SET 1 ghosts").message, "does not take 'ghosts'"));
    // Read whole before any of it is applied: the good word before the bad
    // one changed nothing.
    EXPECT_EQ(refused("VIEWS SET 1 ghosts=off ghosts=maybe").code, ErrorCode::ParseFailure);
    EXPECT_TRUE(view.selectionGhosts);
    EXPECT_TRUE(host.settingChanges.empty());
}

// ---- ZOOM on the shared scope -------------------------------------------------------------
//
// The line (10,10)-(40,30) fills a 300 x 200 view at 84% of either side:
// 0.84 x 300 / 30 = 8.4 = 0.84 x 200 / 20, about its middle, (25, 20).

TEST_F(ViewVerbsTest, ZoomSelectionFramesTheSelectedEntitiesAndSaysHowManyMatched)
{
    const ViewState& view = plan();
    (void)ok("LINE 10,10 40,30");
    (void)ok("LINE 100,100 110,120");
    (void)ok("SELECT 1");
    const std::string reply = ok("ZOOM SELECTION view=1");
    EXPECT_TRUE(reply.starts_with("scope=selection matched=1\nview=1 kind=plan centre=25,20 "))
        << reply;
    EXPECT_EQ(view.plan.center, Point2(25, 20));
    EXPECT_NEAR(view.plan.scale, 8.4, 1e-12);
    ASSERT_FALSE(host.zooms.empty());
    EXPECT_EQ(host.zooms.back().kind, ZoomRequest::Kind::Scope);
    EXPECT_EQ(host.zooms.back().window.min, Point2(10, 10));
    EXPECT_EQ(host.zooms.back().window.max, Point2(40, 30));
}

TEST_F(ViewVerbsTest, AScopeThatMatchesNothingMovesNoViewAndSaysSo)
{
    const ViewState& view = plan();
    (void)ok("LINE 10,10 40,30");
    // Nothing selected: an answer, not a refusal, and no view moves.
    EXPECT_EQ(ok("ZOOM SELECTION"), "scope=selection matched=0");
    EXPECT_EQ(ok("ZOOM LAYERS 0 WHERE TYPE=circle"),
              "scope=layers layers=0 sublayers=yes where=\"TYPE=circle\" matched=0");
    EXPECT_TRUE(host.zooms.empty());
    EXPECT_EQ(view.plan.center, Point2(10, 20));
    // A scope the drawing cannot answer is refused as every verb refuses it.
    EXPECT_EQ(refused("ZOOM LAYERS nosuch").code, ErrorCode::NotFound);
}

// ---- what a view shows of what a scope took ------------------------------------------------
//
// The design line (10,10)-(40,30) on "design" and the as-built line
// (100,100)-(110,120) on "asbuilt". A view frames what it shows of what a
// scope took (view_verbs.hpp): the design line alone is framed about its
// middle (25, 20) - at 8.4 px a unit in the 300 x 200 view, as above - and
// the two lines together are the box (10,10)-(110,120).

TEST_F(ViewVerbsTest, APlanViewFramesWhatItShowsOfTheSelectionAndSaysHowManyThatIs)
{
    addLayers(document, {"design", "asbuilt"});
    const ViewState& view = plan();
    (void)ok("LAYER SET design");
    (void)ok("LINE 10,10 40,30");
    (void)ok("LAYER SET asbuilt");
    (void)ok("LINE 100,100 110,120");
    const std::vector<katana::entity::EntityId> ids = document.model().entities.ids();
    ASSERT_EQ(ids.size(), 2U);
    (void)ok("SELECT ALL");
    ASSERT_EQ(document.selection().size(), 2U);

    // The view hides the as-built layer with its ghosts off: it shows the
    // design line alone, and frames that.
    (void)ok("VIEWS HIDE 1 asbuilt");
    (void)ok("VIEWS SET 1 ghosts=off");
    const std::string one = ok("ZOOM SELECTION view=1");
    EXPECT_TRUE(one.starts_with("scope=selection matched=2\nview=1 kind=plan centre=25,20 "))
        << one;
    EXPECT_TRUE(one.ends_with(" shown=1")) << one;
    ASSERT_EQ(host.zooms.size(), 1U);
    EXPECT_EQ(host.zooms.back().ids, (std::vector<katana::entity::EntityId>{ids[0]}));
    EXPECT_EQ(host.zooms.back().window.min, Point2(10, 10));
    EXPECT_EQ(host.zooms.back().window.max, Point2(40, 30));
    EXPECT_EQ(view.plan.center, Point2(25, 20));

    // Its ghosts on, it shows the as-built line too, faintly, and frames both.
    (void)ok("VIEWS SET 1 ghosts=on");
    const std::string both = ok("ZOOM SELECTION view=1");
    EXPECT_TRUE(both.ends_with(" shown=2")) << both;
    EXPECT_EQ(host.zooms.back().ids, ids);
    EXPECT_EQ(host.zooms.back().window.min, Point2(10, 10));
    EXPECT_EQ(host.zooms.back().window.max, Point2(110, 120));
    EXPECT_EQ(view.plan.center, Point2(60, 65));
}

TEST_F(ViewVerbsTest, AViewThatShowsNoneOfWhatTheScopeTookMovesNothingAndSaysSo)
{
    // It framed the box of everything the scope took and jumped - and took
    // its link - to empty ground, where nothing selected was drawn or
    // ghosted, while its tip promised nothing would move.
    addLayers(document, {"design"});
    const ViewState& view = plan();
    (void)ok("LAYER SET design");
    (void)ok("LINE 10,10 40,30");
    (void)ok("LAYER SET 0");
    (void)ok("SELECT ALL");
    ASSERT_EQ(document.selection().size(), 1U);
    const std::string none = "scope=selection matched=1\nview=1 kind=plan shown=0 moved=no";

    // The view hides the layer, its ghosts off.
    (void)ok("VIEWS HIDE 1 design");
    (void)ok("VIEWS SET 1 ghosts=off");
    EXPECT_EQ(ok("ZOOM SELECTION view=1"), none);

    // The drawing hides it: hidden in every view, a ghost or not.
    (void)ok("VIEWS SHOW 1 ALL");
    (void)ok("VIEWS SET 1 ghosts=on");
    (void)ok("LAYER HIDE design");
    EXPECT_EQ(ok("ZOOM SELECTION view=1"), none);

    // A ghost is of the selection alone: the design layer's line, taken by
    // LAYERS and not selected, is shown nowhere in a view that hides it.
    (void)ok("LAYER SHOW design");
    (void)ok("VIEWS HIDE 1 design");
    (void)ok("SELECT NONE");
    EXPECT_EQ(ok("ZOOM LAYERS design view=1"),
              "scope=layers layers=design sublayers=yes matched=1\n"
              "view=1 kind=plan shown=0 moved=no");

    EXPECT_TRUE(host.zooms.empty()) << "nothing was asked of the window";
    EXPECT_EQ(view.plan.center, Point2(10, 20));
    EXPECT_EQ(view.plan.scale, 4.0);
}

TEST_F(ViewVerbsTest, AThreeDViewAndAPlanViewFrameTheSelectionByOneRule)
{
    // The same rule for a 3D view, whose widget frames where its scene puts
    // what it is handed: nothing it does not show is handed to it. And the
    // one difference of kind: a plan view ghosts no label (the plan painter
    // places labels among the others), where a 3D view marks where a
    // ghosted one attaches.
    addLayers(document, {"design"});
    plan();
    host.set.add(ViewKind::Model3D);
    (void)ok("LAYER SET design");
    (void)ok("LINE 10,10 40,30");
    const katana::entity::EntityId line = document.model().entities.ids().back();
    katana::entity::LabelStyle style;
    style.name = "any";
    ASSERT_TRUE(document.execute(katana::commands::createLabelStyle(style)).ok());
    katana::entity::Entity label;
    label.layer = "design";
    label.geometry = katana::entity::LabelGeometry{
        .target = line, .style = "any", .anchor = Point2(25, 20)};
    ASSERT_TRUE(document.execute(katana::commands::createEntities({label})).ok());
    const katana::entity::EntityId labelId = document.model().entities.ids().back();
    ASSERT_NE(labelId, line);
    (void)ok("VIEWS HIDE 1 design");
    (void)ok("VIEWS HIDE 2 design");

    // Ghosts off: neither view shows the selected line, and neither moves.
    (void)ok("VIEWS SET 2 ghosts=off");
    (void)ok("SELECT " + std::to_string(line));
    EXPECT_EQ(ok("ZOOM SELECTION view=2"),
              "scope=selection matched=1\nview=2 kind=3d shown=0 moved=no");
    EXPECT_TRUE(host.zooms.empty());

    // Ghosts on, the 3D view is handed the line it ghosts.
    (void)ok("VIEWS SET 2 ghosts=on");
    (void)ok("ZOOM SELECTION view=2");
    ASSERT_EQ(host.zooms.size(), 1U);
    EXPECT_EQ(host.zooms.back().ids, (std::vector<katana::entity::EntityId>{line}));

    // The label alone, selected, on the layer both views hide with their
    // ghosts on: the 3D view shows it, the plan view does not.
    (void)ok("SELECT " + std::to_string(labelId));
    (void)ok("ZOOM SELECTION view=2");
    ASSERT_EQ(host.zooms.size(), 2U);
    EXPECT_EQ(host.zooms.back().ids, (std::vector<katana::entity::EntityId>{labelId}));
    EXPECT_EQ(ok("ZOOM SELECTION view=1"),
              "scope=selection matched=1\nview=1 kind=plan shown=0 moved=no");
    EXPECT_EQ(host.zooms.size(), 2U);
}

TEST_F(ViewVerbsTest, ZoomWithNoWordsIsStillExtentsNotTheSelection)
{
    plan();
    (void)ok("LINE 10,10 40,30");
    (void)ok("SELECT ALL");
    (void)ok("ZOOM");
    ASSERT_EQ(host.zooms.size(), 1U);
    EXPECT_EQ(host.zooms.back().kind, ZoomRequest::Kind::Extents);
}

TEST_F(ViewVerbsTest, ZoomAreaFramesWhatIsInTheBoxNotTheBox)
{
    const ViewState& view = plan();
    (void)ok("LINE 10,10 40,30");
    (void)ok("LINE 100,100 110,120");
    // AREA takes the entities in the box: the first line, framed about its
    // own middle (25, 20), not the box's (25, 25). WINDOW frames the box.
    const std::string reply = ok("ZOOM AREA 0,0,50,50");
    EXPECT_TRUE(reply.starts_with("scope=area area=0,0,50,50 matched=1\n")) << reply;
    EXPECT_EQ(view.plan.center, Point2(25, 20));
    (void)ok("ZOOM WINDOW 0,0,50,50");
    EXPECT_EQ(view.plan.center, Point2(25, 25));
}

TEST_F(ViewVerbsTest, ViewEqualsAfterWhereIsNotTakenForACondition)
{
    plan();
    plan();
    ASSERT_TRUE(host.set.activate(2).ok());
    (void)ok("LINE 10,10 40,30");
    (void)ok("CIRCLE 5,5 1");
    // view= stands after the filter, where a word with '=' would be a
    // condition, and is taken off before the scope is read.
    const std::string reply = ok("ZOOM DRAWING WHERE TYPE=line view=1");
    EXPECT_TRUE(reply.starts_with("scope=drawing where=\"TYPE=line\" matched=1\nview=1 "))
        << reply;
    EXPECT_EQ(host.zooms.back().view, 1U);
    EXPECT_EQ(host.set.find(1)->plan.center, Point2(25, 20));
}

TEST_F(ViewVerbsTest, ZoomOnASectionTakesExtentsInAndOutOnly)
{
    plan();
    host.set.add(ViewKind::Section);
    (void)ok("ZOOM IN view=2");
    EXPECT_EQ(host.zooms.back().kind, ZoomRequest::Kind::In);
    (void)ok("ZOOM OUT 3 view=2");
    EXPECT_EQ(host.zooms.back().factor, 3.0);
    (void)ok("ZOOM EXTENTS view=2");
    // Each refusal names the kinds that take it (cad::zoomTakes): WINDOW and
    // CENTRE a plan view's, a scope a plan, 3D or elevation view's.
    for (const auto& [line, takers] :
         {std::pair{"ZOOM WINDOW 0,0,1,1 view=2", "a plan view: view 2 is Section"},
          std::pair{"ZOOM CENTRE 0,0 view=2", "a plan view: view 2 is Section"},
          std::pair{"ZOOM SELECTION view=2", "a plan, 3D or elevation view: view 2 is Section"}}) {
        const auto refusal = refused(line);
        EXPECT_EQ(refusal.code, ErrorCode::InvalidArgument) << line;
        EXPECT_TRUE(contains(refusal.message, takers)) << refusal.message;
    }
    EXPECT_EQ(host.zooms.size(), 3U);
}

TEST(ViewZoom, TheExtentOfIdsIsTheirBoxPassingOverIdsThatNameNothing)
{
    Document document;
    CommandInterpreter lines(document);
    ASSERT_TRUE(lines.run("LINE 10,10 40,30").ok());
    ASSERT_TRUE(lines.run("CIRCLE 100,50 5").ok());
    // The line and the circle's own box (95,45)-(105,55); id 7 names nothing.
    const std::vector<katana::entity::EntityId> ids{1, 2, 7};
    const katana::geometry::Box2 box = extentOf(document.model(), ids);
    EXPECT_EQ(box.min, Point2(10, 10));
    EXPECT_EQ(box.max, Point2(105, 55));
    EXPECT_TRUE(extentOf(document.model(), std::vector<katana::entity::EntityId>{7}).empty());
    EXPECT_TRUE(extentOf(document.model(), {}).empty());
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

TEST_F(ViewVerbsTest, AScopeZoomOnA3DViewCarriesWhatTheScopeTookAndASectionRefusesIt)
{
    // A 3D view frames the entities a scope took, where its scene draws
    // them, so the request carries them - ascending, as matchScope gives
    // them - beside their plan extent, which a plan view frames.
    plan();
    host.set.add(ViewKind::Model3D);
    host.set.add(ViewKind::Section);
    (void)ok("LINE 0,0 10,0");
    (void)ok("LINE 50,50 60,60");
    (void)ok("LINE 100,0 110,0");
    const std::vector<katana::entity::EntityId> ids = document.model().entities.ids();
    ASSERT_EQ(ids.size(), 3U);
    document.selection().set({ids[2], ids[0]});

    (void)ok("ZOOM SELECTION view=2");
    ASSERT_EQ(host.zooms.size(), 1U);
    EXPECT_EQ(host.zooms.back().kind, ZoomRequest::Kind::Scope);
    EXPECT_EQ(host.zooms.back().ids, (std::vector<katana::entity::EntityId>{ids[0], ids[2]}));
    // Their plan extent by hand: x from 0 to 110, both on y = 0.
    EXPECT_EQ(host.zooms.back().window.min, Point2(0, 0));
    EXPECT_EQ(host.zooms.back().window.max, Point2(110, 0));

    // A section frames no scope, and says which kinds do.
    const auto refusal = refused("ZOOM SELECTION view=3");
    EXPECT_EQ(refusal.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(refusal.message,
                         "ZOOM on a scope frames a plan, 3D or elevation view: view 3 is Section"))
        << refusal.message;
    EXPECT_EQ(host.zooms.size(), 1U) << "nothing refused was asked of the window";
}

TEST_F(ViewVerbsTest, TheRecordOfAViewThatHasFramedNothingYetSaysSo)
{
    // A view frames what it draws at its first paint; until then its place
    // is the one every new view starts at, which VIEWS OPEN's record gave as
    // though the view looked there (view_verbs.hpp, framed=no).
    const std::string opened = ok("VIEWS OPEN plan");
    EXPECT_TRUE(contains(opened, " framed=no ghosts=on")) << opened;
    const std::string model = ok("VIEWS OPEN 3d");
    EXPECT_TRUE(contains(model, " framed=no ghosts=on")) << model;

    // Framed, a record says nothing of it.
    host.set.find(1)->planFramed = true;
    host.set.find(2)->cameraFramed = true;
    const std::string listed = ok("VIEWS");
    EXPECT_FALSE(contains(listed, "framed=")) << listed;
    // A section's record has no place to be the placeholder of.
    EXPECT_FALSE(contains(ok("VIEWS OPEN section"), "framed="));
}

TEST_F(ViewVerbsTest, HideOrShowOfCommasAloneIsRefusedAndChangesNothing)
{
    // "VIEWS HIDE 1 ," names no layer. It replied the view's record as
    // though it had worked, and changed nothing: a silent failure.
    addLayers(document, {"design"});
    plan();
    (void)ok("VIEWS HIDE 1 design");
    const std::size_t heard = host.settingChanges.size();
    for (const char* line :
         {"VIEWS HIDE 1 ,", "VIEWS SHOW 1 ,,", "VIEWS HIDE 1 , ,", "VIEWS ISOLATE 1 ,"}) {
        const auto refusal = refused(line);
        EXPECT_EQ(refusal.code, ErrorCode::ParseFailure) << line;
        EXPECT_TRUE(contains(refusal.message, "names no layer in ','") ||
                    contains(refusal.message, "names no layer in ',,'"))
            << line << ": " << refusal.message;
    }
    EXPECT_EQ(host.settingChanges.size(), heard) << "nothing changed, so no view was redrawn";
    EXPECT_TRUE(host.set.find(1)->layers.hides("design")) << "and design is hidden still";
}

TEST(CommandVerb, TheVerbOfALineIsTheInterpretersOwnReading)
{
    // What the window asks of a line typed inside a tool (verbOf): the
    // interpreter's alias table and its apostrophe rule, never a list of the
    // window's own.
    EXPECT_EQ(CommandInterpreter::verbOf("z"), "ZOOM");
    EXPECT_EQ(CommandInterpreter::verbOf("  Zoom in 2  "), "ZOOM");
    EXPECT_EQ(CommandInterpreter::verbOf("'z e"), "ZOOM");
    EXPECT_EQ(CommandInterpreter::verbOf("'ZOOM"), "ZOOM");
    EXPECT_EQ(CommandInterpreter::verbOf("views open plan"), "VIEWS");
    EXPECT_EQ(CommandInterpreter::verbOf("l 0,0 1,1"), "LINE");
    // An apostrophe makes no other word a verb; run() refuses it.
    EXPECT_EQ(CommandInterpreter::verbOf("'LINE 0,0"), "'LINE");
    EXPECT_EQ(CommandInterpreter::verbOf("'"), "'");
    EXPECT_EQ(CommandInterpreter::verbOf(""), "");
    EXPECT_EQ(CommandInterpreter::verbOf("   "), "");
}
