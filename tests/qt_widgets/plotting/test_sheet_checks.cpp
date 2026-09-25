// The preflight checks in the sheet editor (src/katana_qt/plotting/
// sheet_checks): that the checker is given what the painter knows, so the two
// agree about what prints empty; the Checks dock and its toolbar action; a
// double-click going to the finding; the re-check after a burst of edits,
// once; and a plot with errors that warns and still plots.
//
// The A3 numbers are the frame's: drawing area 23..410 x 35..287 mm, the
// title block's top at 34.625 mm (docs/plotting.md).
//
// With KATANA_SHEET_PNG set to a directory, the editor and its Checks dock
// are also grabbed there.

#include <gtest/gtest.h>

#include <cstdlib>
#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

#include <QAction>
#include <QDockWidget>
#include <QApplication>
#include <QElapsedTimer>
#include <QFileInfo>
#include <QImage>
#include <QLabel>
#include <QMouseEvent>
#include <QPainter>
#include <QStatusBar>
#include <QTemporaryDir>
#include <QTreeWidget>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/preflight.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/geometry/mesh.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "plotting/sheet_checks.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::Model;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::qt::SheetChecksDock;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

const Box2 kInside(Point2(100.0, 100.0), Point2(200.0, 180.0));
// Down to 20 mm: into the title block.
const Box2 kUnderTheTitleBlock(Point2(100.0, 20.0), Point2(200.0, 100.0));

plotting::Viewport notesAt(std::string id, Box2 rect)
{
    plotting::Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = plotting::ViewportKind::Notes;
    viewport.rect = rect;
    viewport.text = "ALL LEVELS IN METRES";
    return viewport;
}

plotting::Viewport planAt(std::string id, Box2 rect, double scale, Point2 centre)
{
    plotting::Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = plotting::ViewportKind::Plan;
    viewport.rect = rect;
    viewport.scale = scale;
    viewport.centre = centre;
    return viewport;
}

plotting::Sheet sheetOf(std::string id, std::string name, std::vector<plotting::Viewport> viewports)
{
    plotting::Sheet sheet;
    sheet.id = std::move(id);
    sheet.name = std::move(name);
    sheet.viewports = std::move(viewports);
    return sheet;
}

std::vector<plotting::Finding> withCode(const std::vector<plotting::Finding>& findings,
                                        std::string_view code)
{
    std::vector<plotting::Finding> out;
    for (const plotting::Finding& finding : findings) {
        if (finding.code == code) {
            out.push_back(finding);
        }
    }
    return out;
}

SheetEditor::SourceProvider sourceOf(const Document& document)
{
    return [&document] {
        SheetSource source;
        source.plan = katana::qt::planSourceOf(document);
        source.revision = document.modelRevision();
        source.fields.projectName = "Main Road";
        source.fields.plotDate = "25/09/26";
        return source;
    };
}

// An editor shown at a fixed size, its checks run once.
struct Shown {
    explicit Shown(Document& document) : editor(document, sourceOf(document))
    {
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
        editor.canvas()->fitPage();
        katana::qt::test::paint(*editor.canvas());
    }
    [[nodiscard]] SheetChecksDock& checks() const { return *editor.checks(); }

    SheetEditor editor;
};

void mouse(QWidget& widget, QEvent::Type type, QPoint at)
{
    const Qt::MouseButtons held =
        type == QEvent::MouseButtonRelease ? Qt::NoButton : Qt::MouseButtons(Qt::LeftButton);
    QMouseEvent event(type, QPointF(at), QPointF(widget.mapToGlobal(at)), Qt::LeftButton, held,
                      Qt::NoModifier);
    QApplication::sendEvent(&widget, &event);
}

// Runs the event loop until `done` or two seconds pass.
template <typename Done>
void waitFor(Done&& done)
{
    QElapsedTimer clock;
    clock.start();
    while (!done() && clock.elapsed() < 2000) {
        QCoreApplication::processEvents(QEventLoop::AllEvents | QEventLoop::WaitForMoreEvents, 20);
    }
}

} // namespace

// ---- the checker, given what the painter knows ----------------------------------------------

TEST(SheetChecks, TheOptionsCarryWhatThePainterDrawsFrom)
{
    Model model;
    SheetSource source;
    source.plan.model = &model;
    plotting::PreflightOptions options = katana::qt::preflightOptionsFor(source);
    // Nothing to cut sections from, no logo decoded, nothing but the drawing.
    EXPECT_EQ(options.sectionSurfaces, 0u);
    EXPECT_EQ(options.logoReadable, false);
    EXPECT_TRUE(options.otherContent.empty());
    ASSERT_TRUE(options.resolvePlan);

    // A mesh is drawn in plan as its footprint, so it is content; a hidden
    // one is not.
    katana::geometry::TriangleMesh mesh;
    mesh.vertices = {{10.0, 20.0, 0.0}, {30.0, 20.0, 1.0}, {30.0, 50.0, 2.0}};
    mesh.faces = {{0u, 1u, 2u}};
    std::vector<katana::cad::SceneMesh> meshes(1);
    meshes[0].mesh = &mesh;
    source.plan.meshes = &meshes;
    source.logo = QImage(40, 10, QImage::Format_RGB32);
    source.assets = "assets";
    options = katana::qt::preflightOptionsFor(source);
    ASSERT_EQ(options.otherContent.size(), 1u);
    EXPECT_EQ(options.otherContent[0], Box2(Point2(10.0, 20.0), Point2(30.0, 50.0)));
    EXPECT_EQ(options.logoReadable, true);
    EXPECT_EQ(options.assets, std::filesystem::path("assets"));
    meshes[0].visible = false;
    EXPECT_TRUE(katana::qt::preflightOptionsFor(source).otherContent.empty());
}

TEST(SheetChecks, AnAutomaticPlanIsCheckedWhereThePainterDrawsIt)
{
    Model model;
    katana::entity::Entity line;
    line.geometry = katana::geometry::Segment2{Point2(0.0, 0.0), Point2(300.0, 0.0)};
    line.layer = "0";
    ASSERT_TRUE(model.entities.add(std::move(line)).ok());
    SheetSource source;
    source.plan.model = &model;
    plotting::Viewport plan = planAt("vp1", Box2(Point2(23.0, 35.0), Point2(410.0, 287.0)), 1.0, Point2());
    plan.autoScale = true;
    plan.autoCentre = true;
    const auto painted = katana::qt::resolvePlanViewport(plan, source);
    const auto options = katana::qt::preflightOptionsFor(source);
    const plotting::PlanWindow checked = options.resolvePlan(plan);
    EXPECT_EQ(checked.scale, painted.scale);
    EXPECT_EQ(checked.centre, painted.centre);
}

TEST(SheetChecks, APlanOverOnlyAMeshIsNotEmpty)
{
    const Model model;
    plotting::SheetSet set;
    set.sheets.push_back(sheetOf("s1", "PLAN", {planAt("vp1", kInside, 500.0, Point2(50.0, 0.0))}));
    SheetSource source;
    source.plan.model = &model;
    EXPECT_EQ(withCode(katana::qt::checkSheetsFor(set, source), "plan.empty").size(), 1u);

    katana::geometry::TriangleMesh mesh;
    mesh.vertices = {{40.0, -5.0, 0.0}, {60.0, -5.0, 0.0}, {60.0, 5.0, 0.0}};
    mesh.faces = {{0u, 1u, 2u}};
    std::vector<katana::cad::SceneMesh> meshes(1);
    meshes[0].mesh = &mesh;
    source.plan.meshes = &meshes;
    EXPECT_TRUE(withCode(katana::qt::checkSheetsFor(set, source), "plan.empty").empty());
}

TEST(SheetChecks, TheCheckerAndThePainterAgreeAboutASectionWithNoSurface)
{
    // A road north up x = 50 over ground z = x / 2; a cross section at CH 40.
    std::vector<katana::geometry::Point3> vertices{
        {0.0, 0.0, 0.0}, {100.0, 0.0, 50.0}, {100.0, 100.0, 50.0}, {0.0, 100.0, 0.0}};
    std::vector<katana::terrain::TinTriangle> triangles{{0, 1, 2}, {0, 2, 3}};
    auto ground = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
    ASSERT_TRUE(ground.ok());
    Model model;
    katana::entity::Alignment road;
    road.name = "ROAD";
    road.horizontal.pis = {{Point2(50.0, 10.0)}, {Point2(50.0, 90.0)}};
    ASSERT_TRUE(model.alignments.add(road).ok());
    plotting::Viewport across;
    across.id = "vp1";
    across.kind = plotting::ViewportKind::CrossSections;
    across.rect = Box2(Point2(30.0, 40.0), Point2(210.0, 155.0));
    across.scale = 250.0;
    across.source.alignment = "ROAD";
    across.source.stations = {40.0};
    across.source.sectionHalfWidth = 20.0;
    across.autoCentre = true;
    plotting::SheetSet set;
    set.sheets.push_back(sheetOf("s1", "SECTIONS", {across}));

    SheetSource source;
    source.plan.model = &model;
    const auto painterProblems = [&] {
        QImage paper(420, 297, QImage::Format_RGB32);
        QPainter painter(&paper);
        katana::qt::SheetPaintOptions options;
        options.pixelsPerMillimetre = 1.0;
        katana::qt::SheetPaintCache cache;
        return katana::qt::paintSheet(painter, set, 0, source, options, cache).problems.size();
    };
    // No surface: the painter leaves it empty, and the checker says so.
    EXPECT_GT(painterProblems(), 0u);
    const auto without = withCode(katana::qt::checkSheetsFor(set, source), "section.no-surface");
    ASSERT_EQ(without.size(), 1u);
    EXPECT_EQ(without[0].severity, plotting::Severity::Error);
    // A hidden surface is no surface either.
    source.surfaces.push_back({"GROUND", &*ground});
    source.surfaces[0].visible = false;
    EXPECT_EQ(withCode(katana::qt::checkSheetsFor(set, source), "section.no-surface").size(), 1u);
    // A visible one: both are content.
    source.surfaces[0].visible = true;
    EXPECT_EQ(painterProblems(), 0u);
    EXPECT_TRUE(withCode(katana::qt::checkSheetsFor(set, source), "section.no-surface").empty());
}

TEST(SheetChecks, OnlyTheSheetsAskedForAreChecked)
{
    const Model model;
    plotting::SheetSet set;
    set.sheets.push_back(sheetOf("s1", "ONE", {}));
    set.sheets.push_back(sheetOf("s2", "TWO", {}));
    SheetSource source;
    source.plan.model = &model;
    const std::vector<std::size_t> second{1};
    const auto findings = withCode(katana::qt::checkSheetsFor(set, source, second), "sheet.empty");
    ASSERT_EQ(findings.size(), 1u);
    EXPECT_EQ(findings[0].sheetId, "s2");
    // With no drawing at all the sheets are still checked, as the painter
    // would draw them: empty.
    source.plan.model = nullptr;
    EXPECT_EQ(withCode(katana::qt::checkSheetsFor(set, source), "sheet.empty").size(), 2u);
}

TEST(SheetChecks, TheLogBeforeAPlotSummarisesAndListsTheErrors)
{
    const std::vector<plotting::Finding> findings{
        {plotting::Severity::Warning, "field.empty", {}, {}, {}, "organisation", "Organisation is blank", "Fill it in"},
        {plotting::Severity::Error, "viewport.outside", 1u, "s2", "vp3", {}, "NOTES (vp3) runs under the title block",
         "Drag it back"},
        {plotting::Severity::Info, "logo.missing", {}, {}, {}, {}, "No logo", {}},
    };
    const QStringList lines = katana::qt::preflightLog(findings);
    ASSERT_EQ(lines.size(), 2);
    EXPECT_TRUE(lines[0].startsWith("Sheet checks: 1 error, 1 warning, 1 note.")) << lines[0].toStdString();
    EXPECT_EQ(lines[1].toStdString(), plotting::findingLine(findings[1]));
    // Notes alone are not worth a word before a plot.
    const std::vector<plotting::Finding> notes{findings[2]};
    EXPECT_TRUE(katana::qt::preflightLog(notes).isEmpty());
}

TEST(SheetChecks, TheSeverityMarksArePainted)
{
    const auto at = [](const QIcon& icon, int x, int y) {
        return icon.pixmap(QSize(16, 16), 1.0).toImage().pixelColor(x, y);
    };
    EXPECT_EQ(at(katana::qt::severityIcon(plotting::Severity::Error), 3, 8), QColor(211, 47, 47));
    EXPECT_EQ(at(katana::qt::severityIcon(plotting::Severity::Warning), 4, 13), QColor(245, 166, 35));
    EXPECT_EQ(at(katana::qt::severityIcon(plotting::Severity::Info), 3, 8), QColor(25, 118, 210));
    EXPECT_FALSE(katana::qt::checkSheetsIcon().isNull());
}

// ---- the editor ------------------------------------------------------------------------------

TEST(SheetChecks, TheEditorHasAChecksDockAndACheckSheetsAction)
{
    Document document;
    ASSERT_TRUE(plotting::addSheet(document, sheetOf("s1", "NOTES", {notesAt("vp1", kUnderTheTitleBlock)})).ok());
    Shown shown(document);
    ASSERT_EQ(shown.editor.findChild<QDockWidget*>(QStringLiteral("sheetChecksDock")), &shown.checks());
    SheetChecksDock* dock = &shown.checks();
    auto* action = shown.editor.findChild<QAction*>(QStringLiteral("sheetCheck"));
    ASSERT_NE(action, nullptr);
    auto* list = shown.editor.findChild<QTreeWidget*>(QStringLiteral("sheetChecksList"));
    ASSERT_EQ(list, dock->list());
    ASSERT_NE(shown.editor.findChild<QLabel*>(QStringLiteral("sheetChecksSummary")), nullptr);

    dock->close();
    const int before = dock->runs();
    action->trigger();
    EXPECT_EQ(dock->runs(), before + 1);
    EXPECT_TRUE(dock->isVisible());
    // Its rows are the findings, each marked: errors first, then warnings,
    // then notes, each in the checker's order.
    const auto& findings = dock->findings();
    ASSERT_EQ(list->topLevelItemCount(), static_cast<int>(findings.size()));
    const auto outside = withCode(findings, "viewport.outside");
    ASSERT_EQ(outside.size(), 1u);
    int lastRow = -1;
    for (const plotting::Severity severity :
         {plotting::Severity::Error, plotting::Severity::Warning, plotting::Severity::Info}) {
        for (std::size_t index = 0; index < findings.size(); ++index) {
            if (findings[index].severity == severity) {
                EXPECT_EQ(dock->rowOf(index), lastRow + 1) << findings[index].code;
                lastRow = dock->rowOf(index);
            }
        }
    }
    for (std::size_t index = 0; index < findings.size(); ++index) {
        const QTreeWidgetItem* item = list->topLevelItem(dock->rowOf(index));
        ASSERT_NE(item, nullptr);
        const plotting::Finding& finding = findings[index];
        EXPECT_FALSE(item->icon(0).isNull());
        EXPECT_EQ(item->text(3).toStdString(), finding.message);
        EXPECT_EQ(item->text(4).toStdString(), finding.fix);
        if (finding.code == "viewport.outside") {
            EXPECT_EQ(item->text(0), QStringLiteral("Error"));
            EXPECT_EQ(item->text(1), QStringLiteral("1  NOTES"));
            EXPECT_EQ(item->text(2), QStringLiteral("vp1"));
        } else if (!finding.sheetIndex) {
            EXPECT_EQ(item->text(1), QStringLiteral("All sheets"));
        }
    }
    EXPECT_TRUE(dock->summary()->text().startsWith(QStringLiteral("1 error"))) << dock->summary()->text().toStdString();
    EXPECT_TRUE(dock->windowTitle().startsWith(QStringLiteral("Checks (1 error"))) << dock->windowTitle().toStdString();

    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        (void)shown.editor.grab().save(QString("%1/SheetChecks.png").arg(dir));
    }
}

TEST(SheetChecks, DoubleClickingAFindingGoesToItsSheetAndView)
{
    Document document;
    ASSERT_TRUE(plotting::addSheet(document, sheetOf("s1", "FIRST", {notesAt("vp1", kInside)})).ok());
    ASSERT_TRUE(plotting::addSheet(document, sheetOf("s2", "SECOND", {notesAt("vp2", kInside),
                                                                     notesAt("vp3", kUnderTheTitleBlock)}))
                    .ok());
    Shown shown(document);
    shown.editor.setCurrentSheet(0);
    (void)shown.editor.checkSheets();
    katana::qt::test::processEvents();
    const auto& findings = shown.checks().findings();
    std::size_t index = findings.size();
    for (std::size_t i = 0; i < findings.size(); ++i) {
        if (findings[i].code == "viewport.outside") {
            index = i;
        }
    }
    ASSERT_LT(index, findings.size());
    ASSERT_EQ(findings[index].viewportId, "vp3");
    // The one error heads the list.
    ASSERT_EQ(shown.checks().rowOf(index), 0);

    QTreeWidget* list = shown.checks().list();
    QTreeWidgetItem* item = list->topLevelItem(0);
    list->scrollToItem(item);
    katana::qt::test::processEvents();
    const QPoint at = list->visualItemRect(item).center();
    QWidget& viewport = *list->viewport();
    ASSERT_TRUE(viewport.rect().contains(at)) << at.x() << ", " << at.y() << " in " << viewport.width()
                                              << " x " << viewport.height();
    mouse(viewport, QEvent::MouseButtonPress, at);
    mouse(viewport, QEvent::MouseButtonRelease, at);
    mouse(viewport, QEvent::MouseButtonDblClick, at);
    mouse(viewport, QEvent::MouseButtonRelease, at);
    EXPECT_EQ(shown.editor.currentSheet(), 1u);
    EXPECT_EQ(shown.editor.canvas()->selected(), "vp3");
    EXPECT_TRUE(shown.editor.statusBar()->currentMessage().contains("title block"))
        << shown.editor.statusBar()->currentMessage().toStdString();

    // A finding about the set has no sheet to go to: the sheet stays.
    shown.editor.setCurrentSheet(0);
    shown.editor.showFinding(plotting::Finding{plotting::Severity::Info, "logo.missing", {}, {}, {}, {},
                                               "The title block's logo slot is empty", "Choose one"});
    EXPECT_EQ(shown.editor.currentSheet(), 0u);
    // One whose sheet was removed since, neither.
    shown.editor.showFinding(plotting::Finding{plotting::Severity::Error, "viewport.outside", 5u, "s9", "vp9",
                                               {}, "Gone", {}});
    EXPECT_EQ(shown.editor.currentSheet(), 0u);
}

TEST(SheetChecks, ABurstOfEditsIsCheckedOnceWhenItStops)
{
    Document document;
    ASSERT_TRUE(plotting::addSheet(document, sheetOf("s1", "NOTES", {notesAt("vp1", kInside)})).ok());
    Shown shown(document);
    SheetChecksDock& dock = shown.checks();
    dock.setDelay(30);
    (void)dock.checkNow();
    EXPECT_TRUE(withCode(dock.findings(), "viewport.outside").empty());
    const int before = dock.runs();

    // Three edits, the last moving the view under the title block.
    for (const double bottom : {90.0, 60.0, 20.0}) {
        ASSERT_TRUE(plotting::editViewport(document, "vp1", [bottom](plotting::Viewport& viewport) {
                        viewport.rect.min.y = bottom;
                        return katana::core::Status{};
                    }).ok());
    }
    EXPECT_TRUE(dock.pending());
    EXPECT_EQ(dock.runs(), before);
    waitFor([&dock] { return !dock.pending(); });
    EXPECT_EQ(dock.runs(), before + 1);
    EXPECT_EQ(withCode(dock.findings(), "viewport.outside").size(), 1u);

    // An undo is an edit too.
    ASSERT_TRUE(document.undo().ok());
    ASSERT_TRUE(document.undo().ok());
    ASSERT_TRUE(document.undo().ok());
    waitFor([&dock] { return !dock.pending(); });
    EXPECT_EQ(dock.runs(), before + 2);
    EXPECT_TRUE(withCode(dock.findings(), "viewport.outside").empty());
}

TEST(SheetChecks, APlotWithErrorsWarnsAndStillPlots)
{
    Document document;
    ASSERT_TRUE(plotting::addSheet(document, sheetOf("s1", "CLEAN", {notesAt("vp1", kInside)})).ok());
    ASSERT_TRUE(plotting::addSheet(document, sheetOf("s2", "WRONG", {notesAt("vp2", kUnderTheTitleBlock)})).ok());
    Shown shown(document);
    std::vector<QString> messages;
    shown.editor.onMessage = [&messages](const QString& text, bool) { messages.push_back(text); };
    QTemporaryDir directory;
    ASSERT_TRUE(directory.isValid());
    SheetChecksDock& dock = shown.checks();

    // The clean sheet alone: nothing to warn about, and the dock stays shut.
    dock.close();
    shown.editor.setCurrentSheet(0);
    const QString one = directory.filePath("one.pdf");
    ASSERT_TRUE(shown.editor.plotToPdf(one, false).ok());
    EXPECT_TRUE(QFileInfo::exists(one));
    ASSERT_FALSE(messages.empty());
    EXPECT_FALSE(messages.back().contains("checks found")) << messages.back().toStdString();
    EXPECT_FALSE(dock.isVisible());

    // Every sheet: the error on the second is reported and the dock comes up,
    // but the PDF is written.
    const QString all = directory.filePath("all.pdf");
    ASSERT_TRUE(shown.editor.plotToPdf(all, true).ok());
    EXPECT_TRUE(QFileInfo::exists(all));
    EXPECT_TRUE(messages.back().contains("The checks found 1 error on the sheets"))
        << messages.back().toStdString();
    EXPECT_TRUE(dock.isVisible());
    EXPECT_EQ(withCode(dock.findings(), "viewport.outside").size(), 1u);
}
