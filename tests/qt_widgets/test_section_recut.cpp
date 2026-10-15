// An open section view follows the drawing's surfaces (docs/terrain.md, "Hidden is a
// matter of the picture"): it keeps the recipe it was cut from - the section's own
// alignment and its design profile - and is cut again when the surfaces change, with
// the pan and zoom a person made kept. When it cannot be cut again it says so.
//
// The surfaces are planes with corners at exact heights, so what a cut reads at the
// east end is worked out on paper:
//
//   ground   z = 10 + x / 10  over x 0..100:  20 at the east end
//   raised   z = 15 + x / 10:                 25 at the east end
//
// the window's behaviour in ViewWorkspace::recutSections and SectionViewWidget::
// replaceSection, with the same cut (cad::cutDrawingSection) the window gives them.

#include <gtest/gtest.h>

#include <memory>
#include <optional>
#include <string>
#include <utility>

#include <QMainWindow>

#include "dock_chrome.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/terrain/section_cut.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "section_view_widget.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

namespace {

using katana::cad::Document;
using katana::cad::Section;
using katana::cad::SectionDesign;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::qt::SectionViewWidget;
using katana::qt::ViewWorkspace;
using katana::qt::test::paint;
using katana::qt::test::processEvents;

// A rectangle x 0..100, y 0..20, `west` high at x = 0 and `east` at x = 100.
std::shared_ptr<const katana::terrain::TinSurface> slab(double west, double east)
{
    auto surface = katana::terrain::TinSurface::create(
        {{0.0, 0.0, west}, {100.0, 0.0, east}, {100.0, 20.0, east}, {0.0, 20.0, west}},
        {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(surface.ok());
    return std::make_shared<const katana::terrain::TinSurface>(std::move(*surface));
}

void addSurface(Document& document, const std::string& name,
                std::shared_ptr<const katana::terrain::TinSurface> surface)
{
    katana::terrain::SurfaceStore store = document.surfaces();
    ASSERT_TRUE(store.add({name, std::move(surface), "test"}).ok());
    ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST").ok());
}

void replaceSurface(Document& document, const std::string& name,
                    std::shared_ptr<const katana::terrain::TinSurface> surface)
{
    katana::terrain::SurfaceStore store = document.surfaces();
    ASSERT_TRUE(store.replace(name, std::move(surface)).ok());
    ASSERT_TRUE(document.setSurfaces(std::move(store), "EDIT").ok());
}

void setVisible(Document& document, const std::string& name, bool visible)
{
    katana::terrain::SurfaceStore store = document.surfaces();
    ASSERT_TRUE(store.setVisible(name, visible).ok());
    ASSERT_TRUE(document.setSurfaces(std::move(store), "VISIBLE").ok());
}

Polyline2 eastAlong(double y)
{
    Polyline2 line;
    line.vertices = {Point2(0.0, y), Point2(100.0, y)};
    return line;
}

// The elevation of the section's `series` at its last station.
double eastEnd(const Section& section, std::size_t series = 0)
{
    return *section.surfaces.at(series).samples.back().elevation;
}

// The window's arrangement: the workspace over a drawing, and the cut it is given.
struct Site {
    Document document;
    QMainWindow window;
    ViewWorkspace* views = nullptr;
    katana::qt::DockChrome* chrome = nullptr;
    // How many times the workspace asked for a cut.
    int cuts = 0;

    Site()
    {
        views = new ViewWorkspace(document, &window);
        window.setCentralWidget(views);
        chrome = new katana::qt::DockChrome(window);
        views->setChrome(chrome);
        window.resize(900, 600);
        window.show();
        processEvents();
    }

    // The cut MainWindow::recutSections gives the workspace.
    ViewWorkspace::SectionCutter cutter()
    {
        return [this](const Polyline2& alignment, const SectionDesign* design)
                   -> katana::core::Result<Section> {
            ++cuts;
            auto cut = katana::cad::cutDrawingSection(document, alignment, design);
            if (!cut) {
                return cut.error();
            }
            return std::move(cut->section);
        };
    }

    // The state of the section view (the workspace opens a plan view of its own first).
    ViewState* sectionState()
    {
        for (ViewState* state : views->viewSet().views()) {
            if (state->kind == ViewKind::Section) {
                return state;
            }
        }
        return nullptr;
    }

    // A first cut shown in a section view, as Terrain > Cut Section shows it.
    SectionViewWidget* show(const Polyline2& alignment, std::optional<SectionDesign> design = {})
    {
        auto cut = katana::cad::cutDrawingSection(document, alignment, design ? &*design : nullptr);
        EXPECT_TRUE(cut.ok());
        if (!cut) {
            return nullptr;
        }
        EXPECT_TRUE(views->showSection(std::move(cut->section), std::move(design)));
        processEvents();
        ViewState* state = sectionState();
        return state != nullptr ? views->sectionView(state->id) : nullptr;
    }
};

} // namespace

TEST(SectionRecut, AnOpenSectionFollowsASurfaceEditAnUndoAndAHideAfterItWasCut)
{
    Site site;
    addSurface(site.document, "ground", slab(10.0, 20.0));
    SectionViewWidget* view = site.show(eastAlong(10.0));
    ASSERT_NE(view, nullptr);
    ASSERT_TRUE(view->hasSection());
    EXPECT_NEAR(eastEnd(*view->section()), 20.0, 1e-9);
    EXPECT_FALSE(view->surfacesChanged());

    // Edited: the ground raised by five. The view is of the old one until it is cut
    // again, which says so; cut again, it is the new ground and says nothing.
    replaceSurface(site.document, "ground", slab(15.0, 25.0));
    EXPECT_TRUE(view->surfacesChanged());
    auto recut = site.views->recutSections(site.cutter());
    EXPECT_EQ(recut.cut.size(), 1u);
    EXPECT_TRUE(recut.refused.empty());
    EXPECT_NEAR(eastEnd(*view->section()), 25.0, 1e-9) << "10 + 10 + 5 at the east end";
    EXPECT_FALSE(view->surfacesChanged());

    // Undone: the first ground again.
    ASSERT_TRUE(site.document.undo().ok());
    EXPECT_TRUE(view->surfacesChanged());
    recut = site.views->recutSections(site.cutter());
    EXPECT_EQ(recut.cut.size(), 1u);
    EXPECT_NEAR(eastEnd(*view->section()), 20.0, 1e-9);
    EXPECT_FALSE(view->surfacesChanged());
}

TEST(SectionRecut, HidingASurfaceTakesItsLineOutOfTheOpenSectionAndShowingItPutsItBack)
{
    Site site;
    addSurface(site.document, "ground", slab(10.0, 20.0));
    addSurface(site.document, "raised", slab(15.0, 25.0));
    SectionViewWidget* view = site.show(eastAlong(10.0));
    ASSERT_NE(view, nullptr);
    ASSERT_EQ(view->section()->surfaces.size(), 2u);

    setVisible(site.document, "raised", false);
    auto recut = site.views->recutSections(site.cutter());
    EXPECT_EQ(recut.cut.size(), 1u);
    ASSERT_EQ(view->section()->surfaces.size(), 1u) << "a hidden surface is in no picture";
    EXPECT_EQ(view->section()->surfaces.front().name, "ground");

    setVisible(site.document, "raised", true);
    recut = site.views->recutSections(site.cutter());
    EXPECT_EQ(recut.cut.size(), 1u);
    ASSERT_EQ(view->section()->surfaces.size(), 2u);
    EXPECT_EQ(view->section()->surfaces[1].name, "raised");
    EXPECT_NEAR(eastEnd(*view->section(), 1), 25.0, 1e-9);
}

TEST(SectionRecut, ASectionThatCannotBeCutAgainSaysItsSurfacesChangedAndKeepsItsPicture)
{
    // The last surface hidden: there is nothing to cut. The view is not blanked (the
    // person did not ask for that) and not left looking current: it keeps what it was,
    // flagged stale (SectionViewWidget::surfacesChanged), and the refusal is returned
    // for the window to log. Shown again, it is cut and current.
    Site site;
    addSurface(site.document, "ground", slab(10.0, 20.0));
    SectionViewWidget* view = site.show(eastAlong(10.0));
    ASSERT_NE(view, nullptr);

    setVisible(site.document, "ground", false);
    auto recut = site.views->recutSections(site.cutter());
    EXPECT_TRUE(recut.cut.empty());
    ASSERT_EQ(recut.refused.size(), 1u);
    EXPECT_EQ(recut.refused.front().second.code, katana::core::ErrorCode::InvalidState);
    ASSERT_TRUE(view->hasSection());
    EXPECT_NEAR(eastEnd(*view->section()), 20.0, 1e-9) << "the picture it had";
    EXPECT_TRUE(view->surfacesChanged()) << "and it says it is of surfaces that changed";

    setVisible(site.document, "ground", true);
    recut = site.views->recutSections(site.cutter());
    EXPECT_EQ(recut.cut.size(), 1u);
    EXPECT_FALSE(view->surfacesChanged());
}

TEST(SectionRecut, TheDesignProfileAndItsNameRideIntoTheCutAgain)
{
    // A design of two PVIs, (0, 16) and (100, 18): a straight 2% grade, 18 at the east end.
    katana::geometry::VerticalAlignment definition;
    definition.pvis = {katana::geometry::ProfilePVI{0.0, 16.0},
                       katana::geometry::ProfilePVI{100.0, 18.0}};
    auto solved = katana::geometry::solveProfile(definition);
    ASSERT_TRUE(solved.ok());

    Site site;
    addSurface(site.document, "ground", slab(10.0, 20.0));
    SectionViewWidget* view = site.show(eastAlong(10.0), SectionDesign{*solved, "design Main"});
    ASSERT_NE(view, nullptr);
    ASSERT_EQ(view->section()->surfaces.size(), 2u);
    EXPECT_NEAR(eastEnd(*view->section(), 1), 18.0, 1e-9);

    replaceSurface(site.document, "ground", slab(15.0, 25.0));
    const auto recut = site.views->recutSections(site.cutter());
    ASSERT_EQ(recut.cut.size(), 1u);
    ASSERT_EQ(view->section()->surfaces.size(), 2u) << "the design is cut beside the ground again";
    EXPECT_EQ(view->section()->surfaces[1].name, "design Main");
    EXPECT_NEAR(eastEnd(*view->section(), 0), 25.0, 1e-9);
    EXPECT_NEAR(eastEnd(*view->section(), 1), 18.0, 1e-9);
    ASSERT_NE(site.sectionState(), nullptr);
    EXPECT_TRUE(site.sectionState()->sectionDesign.has_value());
}

TEST(SectionRecut, ASectionCutAlongALineHasNoDesignAndGetsNone)
{
    Site site;
    addSurface(site.document, "ground", slab(10.0, 20.0));
    SectionViewWidget* view = site.show(eastAlong(10.0));
    ASSERT_NE(view, nullptr);
    replaceSurface(site.document, "ground", slab(15.0, 25.0));
    ASSERT_EQ(site.views->recutSections(site.cutter()).cut.size(), 1u);
    EXPECT_EQ(view->section()->surfaces.size(), 1u);
    ASSERT_NE(site.sectionState(), nullptr);
    EXPECT_FALSE(site.sectionState()->sectionDesign.has_value());
}

TEST(SectionRecut, ASectionUpToDateIsNotCutAgainSoAskingTwiceCutsOnce)
{
    Site site;
    addSurface(site.document, "ground", slab(10.0, 20.0));
    ASSERT_NE(site.show(eastAlong(10.0)), nullptr);
    EXPECT_TRUE(site.views->recutSections(site.cutter()).cut.empty())
        << "nothing has changed since the cut";
    EXPECT_EQ(site.cuts, 0);

    replaceSurface(site.document, "ground", slab(15.0, 25.0));
    EXPECT_EQ(site.views->recutSections(site.cutter()).cut.size(), 1u);
    EXPECT_TRUE(site.views->recutSections(site.cutter()).cut.empty());
    EXPECT_EQ(site.cuts, 1) << "once for the one change";
}

TEST(SectionRecut, AWorkspaceWithNoSectionAsksForNoCut)
{
    Site site;
    addSurface(site.document, "ground", slab(10.0, 20.0));
    const auto recut = site.views->recutSections(site.cutter());
    EXPECT_TRUE(recut.cut.empty());
    EXPECT_TRUE(recut.refused.empty());
    EXPECT_EQ(site.cuts, 0);
}

TEST(SectionRecut, ASectionAViewKeepsAfterItWasTurnedToAnotherKindIsCutAgainToo)
{
    // "A view turned from a section into plan keeps its section to show again when it
    // turns back": its state holds the section and no widget does, so it is cut
    // again in the state, and is current when the view turns back.
    Site site;
    addSurface(site.document, "ground", slab(10.0, 20.0));
    ViewState& plan = site.views->viewSet().add(ViewKind::Plan);
    auto cut = katana::cad::cutDrawingSection(site.document, eastAlong(10.0));
    ASSERT_TRUE(cut.ok());
    plan.section = std::move(cut->section);
    plan.sectionSurfaces = site.document.surfacesGeneration();

    replaceSurface(site.document, "ground", slab(15.0, 25.0));
    const auto recut = site.views->recutSections(site.cutter());
    ASSERT_EQ(recut.cut.size(), 1u);
    EXPECT_EQ(recut.cut.front(), plan.id);
    EXPECT_NEAR(eastEnd(*plan.section), 25.0, 1e-9);
    EXPECT_EQ(plan.sectionSurfaces, site.document.surfacesGeneration());
}

TEST(SectionRecut, ASectionIsCutAgainFromItsOwnAlignmentWhateverTheDrawingHolds)
{
    // The recipe is the section's own polyline, not the selection the first cut came
    // from: a line cut along y = 5, with the drawing's other lines gone since, still
    // cuts along y = 5.
    Site site;
    addSurface(site.document, "ground", slab(10.0, 20.0));
    SectionViewWidget* view = site.show(eastAlong(5.0));
    ASSERT_NE(view, nullptr);
    replaceSurface(site.document, "ground", slab(15.0, 25.0));
    ASSERT_EQ(site.views->recutSections(site.cutter()).cut.size(), 1u);
    ASSERT_EQ(view->section()->alignment.vertices.size(), 2u);
    EXPECT_NEAR(view->section()->alignment.vertices.front().y, 5.0, 1e-12);
    EXPECT_NEAR(view->section()->alignment.vertices.back().y, 5.0, 1e-12);
}

// ---- the widget: the framing a person made ------------------------------------------

namespace {

// Two samples, `low` at station 0 and `high` at station 100, on a 100 m alignment.
Section ramp(double low, double high)
{
    Section section;
    section.alignment.vertices = {Point2(0.0, 0.0), Point2(100.0, 0.0)};
    section.length = 100.0;
    katana::cad::SectionSurface surface;
    surface.name = "ground";
    katana::cad::SectionSample first;
    first.station = 0.0;
    first.elevation = low;
    katana::cad::SectionSample last;
    last.station = 100.0;
    last.plan = Point2(100.0, 0.0);
    last.elevation = high;
    surface.samples = {first, last};
    surface.reasons = {katana::cad::SampleReason::Start, katana::cad::SampleReason::End};
    surface.minElevation = low;
    surface.maxElevation = high;
    section.surfaces.push_back(std::move(surface));
    return section;
}

} // namespace

TEST(SectionRecut, ACutAgainKeepsThePanAndZoomAPersonMadeAndTheMiddleOfThePlotStaysWhereItWas)
{
    Document document;
    ViewSet views;
    ViewState& state = views.add(ViewKind::Section);
    SectionViewWidget view(state);
    view.setDocument(&document);
    view.resize(800, 300);
    view.setSection(ramp(10.0, 20.0));
    paint(view);

    // The person zooms in about a point: a station and elevation under the pixel stay
    // under it, and the view is now theirs. The section is then replaced by one 100 m
    // higher, as a re-cut after a surface raised by that much would be.
    view.zoomAt(QPointF(200.0, 100.0), 2.0);
    paint(view);
    const QPointF middle = view.plotCentre();
    const QPointF before = view.stationElevationAt(middle);
    view.replaceSection(ramp(110.0, 120.0));
    paint(view);
    const QPointF after = view.stationElevationAt(middle);
    EXPECT_NEAR(after.x(), before.x(), 1e-9) << "the station at the middle of the plot";
    EXPECT_NEAR(after.y(), before.y(), 1e-9) << "and its elevation: not framed afresh";
    EXPECT_LT(after.y(), 100.0) << "so the new, higher section is out of view, as it was cut";
    EXPECT_DOUBLE_EQ(*view.section()->surfaces.front().samples.back().elevation, 120.0);
}

TEST(SectionRecut, AViewNotMovedByHandIsFramedAfreshOnTheNewSection)
{
    // Still fitted to all of its section, so a section 100 m higher is fitted: the
    // middle of the plot shows the middle of the new section's box, (110 + 120) / 2.
    Document document;
    ViewSet views;
    ViewState& state = views.add(ViewKind::Section);
    SectionViewWidget view(state);
    view.setDocument(&document);
    view.resize(800, 300);
    view.setSection(ramp(10.0, 20.0));
    paint(view);
    EXPECT_NEAR(view.stationElevationAt(view.plotCentre()).y(), 15.0, 1e-9);

    view.replaceSection(ramp(110.0, 120.0));
    paint(view);
    EXPECT_NEAR(view.stationElevationAt(view.plotCentre()).y(), 115.0, 1e-9);
    EXPECT_NEAR(view.stationElevationAt(view.plotCentre()).x(), 50.0, 1e-9);
}

TEST(SectionRecut, ASectionReplacedIsOfTheSurfacesTheDrawingHasNow)
{
    Document document;
    ViewSet views;
    ViewState& state = views.add(ViewKind::Section);
    SectionViewWidget view(state);
    view.setDocument(&document);
    view.resize(800, 300);
    view.setSection(ramp(10.0, 20.0));
    katana::terrain::SurfaceStore store = document.surfaces();
    ASSERT_TRUE(store.add({"ground", slab(10.0, 20.0), "test"}).ok());
    ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST").ok());
    ASSERT_TRUE(view.surfacesChanged());
    view.replaceSection(ramp(10.0, 20.0));
    EXPECT_FALSE(view.surfacesChanged());
}

TEST(SectionRecut, ANewCutForgetsTheDesignTheLastOneHad)
{
    // setSection is a new cut: the design of the cut before it is not the recipe of
    // this one (ViewWorkspace::showSection sets it when there is one).
    Document document;
    ViewSet views;
    ViewState& state = views.add(ViewKind::Section);
    SectionViewWidget view(state);
    view.setDocument(&document);
    katana::geometry::VerticalAlignment definition;
    definition.pvis = {katana::geometry::ProfilePVI{0.0, 16.0},
                       katana::geometry::ProfilePVI{100.0, 18.0}};
    state.sectionDesign = SectionDesign{*katana::geometry::solveProfile(definition), "design"};
    view.setSection(ramp(10.0, 20.0));
    EXPECT_FALSE(state.sectionDesign.has_value());
    state.sectionDesign = SectionDesign{*katana::geometry::solveProfile(definition), "design"};
    view.clearSection();
    EXPECT_FALSE(state.sectionDesign.has_value());
}
