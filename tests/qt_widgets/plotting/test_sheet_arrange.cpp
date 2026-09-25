// The sheet editor's Arrange commands (src/katana_qt/plotting/sheet_arrange):
// the toolbar's Arrange menu, the "Choose paper for this scale" button and
// the Generate dialog's "Rotate the drawing to fill the sheet", driven by
// their object names as a person clicks them; each ONE undoable step. A plan
// turned to its best fit is painted at 4 px a millimetre and read back, so
// the rotation the command writes is shown to mean to the painter what it
// means to the model.
//
// With KATANA_SHEET_PNG set to a directory, the turned sheet is written there.

#include <gtest/gtest.h>

#include <cmath>
#include <cstdlib>
#include <numbers>
#include <string>
#include <vector>

#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QDialog>
#include <QImage>
#include <QMenu>
#include <QPainter>
#include <QPushButton>
#include <QTimer>
#include <QToolButton>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/arrange.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/commands/entity_commands.hpp"
#include "plotting/sheet_arrange.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::PaperSize;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Vec2;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

constexpr double kPi = std::numbers::pi;
constexpr double kDegree = kPi / 180.0;

SheetEditor::SourceProvider sourceOf(const Document& document)
{
    return [&document] {
        SheetSource source;
        source.plan = katana::qt::planSourceOf(document);
        source.revision = document.modelRevision();
        return source;
    };
}

plotting::Viewport viewportAt(std::string id, plotting::ViewportKind kind, Box2 rect)
{
    plotting::Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = rect;
    return viewport;
}

// One sheet holding `viewports`, added as one step; its ids are given out
// afresh from vp1, in the order written.
void addSheetWith(Document& document, std::vector<plotting::Viewport> viewports)
{
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.name = "TEST";
    sheet.viewports = std::move(viewports);
    ASSERT_TRUE(plotting::addSheet(document, sheet).ok());
}

const plotting::Viewport& viewportOf(const Document& document, std::string_view id)
{
    for (const plotting::Viewport& viewport : document.sheetSet().sheets.at(0).viewports) {
        if (viewport.id == id) {
            return viewport;
        }
    }
    ADD_FAILURE() << "no viewport " << id;
    return document.sheetSet().sheets.at(0).viewports.at(0);
}

void addLine(Document& document, Point2 a, Point2 b)
{
    ASSERT_TRUE(document.execute(katana::commands::createLine(a, b)).ok());
}

// A `length` x `width` strip along `turn`, centred on `centre`, drawn as its
// four sides; its corners.
std::vector<Point2> addStrip(Document& document, Point2 centre, double length, double width, double turn)
{
    std::vector<Point2> corners;
    for (const Vec2 corner : {Vec2(-length / 2, -width / 2), Vec2(length / 2, -width / 2),
                              Vec2(length / 2, width / 2), Vec2(-length / 2, width / 2)}) {
        corners.push_back(centre + corner.rotated(turn));
    }
    for (std::size_t i = 0; i < corners.size(); ++i) {
        addLine(document, corners[i], corners[(i + 1) % corners.size()]);
    }
    return corners;
}

bool overlap(const Box2& a, const Box2& b)
{
    return a.min.x < b.max.x - 1e-9 && b.min.x < a.max.x - 1e-9 && a.min.y < b.max.y - 1e-9 &&
           b.min.y < a.max.y - 1e-9;
}

// An editor shown at a fixed size, its page fitted, collecting what it says.
struct Shown {
    explicit Shown(Document& document) : editor(document, sourceOf(document))
    {
        editor.onMessage = [this](const QString& text, bool error) {
            messages.push_back(text);
            errors += error ? 1 : 0;
        };
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
        editor.canvas()->fitPage();
        katana::qt::test::paint(*editor.canvas());
    }

    // The toolbar Arrange menu's action `name`, triggered.
    void trigger(const char* name)
    {
        auto* menu = editor.findChild<QMenu*>(QStringLiteral("sheetArrangeMenu"));
        ASSERT_NE(menu, nullptr);
        auto* action = menu->findChild<QAction*>(QString::fromLatin1(name));
        ASSERT_NE(action, nullptr) << name;
        action->trigger();
    }

    SheetEditor editor;
    std::vector<QString> messages;
    int errors = 0;
};

} // namespace

TEST(SheetArrangeEditor, TheArrangeMenuOffersEveryCommandByName)
{
    Document document;
    Shown shown(document);
    auto* button = shown.editor.findChild<QToolButton*>(QStringLiteral("sheetArrangeButton"));
    ASSERT_NE(button, nullptr);
    ASSERT_NE(button->menu(), nullptr);
    EXPECT_EQ(button->menu()->objectName(), QStringLiteral("sheetArrangeMenu"));
    for (const char* name :
         {"sheetArrangeAuto", "sheetAlignLeft", "sheetAlignRight", "sheetAlignTop", "sheetAlignBottom",
          "sheetAlignHCentre", "sheetAlignVCentre", "sheetDistributeHorizontal",
          "sheetDistributeVertical", "sheetFitToContent", "sheetRotateToBestFit"}) {
        EXPECT_NE(button->menu()->findChild<QAction*>(QString::fromLatin1(name)), nullptr) << name;
    }
    EXPECT_NE(button->menu()->findChild<QMenu*>(QStringLiteral("sheetMatchScaleMenu")), nullptr);

    // The canvas's context menu has the same commands in its Arrange
    // submenu: looked for while the menu is up, then closed.
    bool seen = false;
    QTimer::singleShot(0, &shown.editor, [&shown, &seen] {
        auto* arrange = shown.editor.findChild<QMenu*>(QStringLiteral("sheetContextArrangeMenu"));
        if (arrange != nullptr) {
            seen = arrange->findChild<QAction*>(QStringLiteral("sheetRotateToBestFit")) != nullptr &&
                   arrange->findChild<QAction*>(QStringLiteral("sheetArrangeAuto")) != nullptr &&
                   arrange->findChild<QMenu*>(QStringLiteral("sheetMatchScaleMenu")) != nullptr;
            if (auto* context = qobject_cast<QMenu*>(arrange->parent())) {
                context->close();
                return;
            }
        }
        if (auto* popup = QApplication::activePopupWidget()) {
            popup->close();
        }
    });
    ASSERT_TRUE(shown.editor.canvas()->onContextMenu);
    shown.editor.canvas()->onContextMenu(QPointF(200.0, 200.0), std::string{});
    EXPECT_TRUE(seen);

    // With no sheet, a command says so and changes nothing.
    shown.trigger("sheetArrangeAuto");
    ASSERT_FALSE(shown.messages.empty());
    EXPECT_EQ(shown.errors, 1);
    EXPECT_TRUE(shown.messages.back().contains(QStringLiteral("no sheet")));
}

TEST(SheetArrangeEditor, AutoArrangeMovesAPanelOffThePlanInOneStep)
{
    Document document;
    const Box2 main = plotting::presetCells(plotting::TilingPreset::MainRight,
                                            plotting::tilingArea(plotting::Sheet{}))[0];
    addSheetWith(document, {viewportAt("vp1", plotting::ViewportKind::Plan, main),
                            viewportAt("vp2", plotting::ViewportKind::Legend,
                                       Box2(Point2(100.0, 100.0), Point2(180.0, 200.0)))});
    Shown shown(document);
    const std::size_t before = document.history().undoCount();

    shown.trigger("sheetArrangeAuto");
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(viewportOf(document, "vp1").rect, main);
    EXPECT_FALSE(overlap(viewportOf(document, "vp1").rect, viewportOf(document, "vp2").rect));
    EXPECT_EQ(shown.errors, 0);
    EXPECT_TRUE(shown.messages.back().contains(QStringLiteral("moved 1 view")));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(viewportOf(document, "vp2").rect, Box2(Point2(100.0, 100.0), Point2(180.0, 200.0)));
}

TEST(SheetArrangeEditor, AligningActsOnTheSelectionOrOnEveryView)
{
    Document document;
    addSheetWith(document,
                 {viewportAt("vp1", plotting::ViewportKind::Notes, Box2(Point2(30.0, 40.0), Point2(80.0, 80.0))),
                  viewportAt("vp2", plotting::ViewportKind::Notes, Box2(Point2(100.0, 50.0), Point2(130.0, 120.0))),
                  viewportAt("vp3", plotting::ViewportKind::Notes, Box2(Point2(300.0, 45.0), Point2(360.0, 60.0)))});
    Shown shown(document);
    const std::size_t before = document.history().undoCount();

    // Nothing selected: every view, on the highest top.
    shown.trigger("sheetAlignTop");
    EXPECT_EQ(document.history().undoCount(), before + 1);
    for (const char* id : {"vp1", "vp2", "vp3"}) {
        EXPECT_EQ(viewportOf(document, id).rect.max.y, 120.0) << id;
    }

    // One selected: to the tiling area's edge, 24 mm from the left.
    shown.editor.canvas()->select("vp3");
    shown.trigger("sheetAlignLeft");
    EXPECT_EQ(document.history().undoCount(), before + 2);
    EXPECT_EQ(viewportOf(document, "vp3").rect.min.x, 24.0);
    EXPECT_EQ(viewportOf(document, "vp1").rect.min.x, 30.0);

    // Spacing with one selected spaces them all. vp3 is now first (24..84)
    // and vp2 last (100..130), with the 50 mm of vp1 to fit in the 16 mm
    // between them: refused, as a message, and nothing moves.
    shown.trigger("sheetDistributeHorizontal");
    EXPECT_EQ(document.history().undoCount(), before + 2);
    EXPECT_EQ(shown.errors, 1);
    ASSERT_FALSE(shown.messages.empty());
    EXPECT_TRUE(shown.messages.back().contains("overlap")) << shown.messages.back().toStdString();
    EXPECT_EQ(viewportOf(document, "vp1").rect.min.x, 30.0);

    // Back as they were - vp1 at 30..80, vp2 at 100..130, vp3 at 300..360 -
    // vp2 goes half way along the 220 mm between the others less its width.
    ASSERT_TRUE(document.undo().ok());
    ASSERT_TRUE(document.undo().ok());
    katana::qt::test::processEvents();
    shown.trigger("sheetDistributeHorizontal");
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(shown.errors, 1);
    const double gap = (300.0 - 80.0 - 30.0) / 2.0;
    EXPECT_NEAR(viewportOf(document, "vp2").rect.min.x, 80.0 + gap, 1e-9);
}

TEST(SheetArrangeEditor, MatchScaleListsTheOtherScaledViewsAndMatchesTheSelection)
{
    // A 300 x 100 m drawing in an automatic plan filling the sheet, centred
    // on it, is drawn at 1 : 1000 (it needs 1 : 810 with the painter's 4% to
    // spare).
    Document document;
    addLine(document, Point2(0.0, 0.0), Point2(300.0, 100.0));
    plotting::Viewport plan = viewportAt("vp1", plotting::ViewportKind::Plan,
                                         plotting::tilingArea(plotting::Sheet{}));
    plan.autoScale = true;
    plan.autoCentre = true;
    plan.scale = 250.0;
    plotting::Viewport section = viewportAt("vp2", plotting::ViewportKind::LongSection,
                                            Box2(Point2(30.0, 40.0), Point2(300.0, 120.0)));
    section.scale = 200.0;
    addSheetWith(document, {plan, section,
                            viewportAt("vp3", plotting::ViewportKind::Legend,
                                       Box2(Point2(320.0, 40.0), Point2(400.0, 100.0)))});
    Shown shown(document);
    shown.editor.canvas()->select("vp2");

    auto* match = shown.editor.findChild<QMenu*>(QStringLiteral("sheetMatchScaleMenu"));
    ASSERT_NE(match, nullptr);
    emit match->aboutToShow();
    // The plan is listed; the selected section and the legend are not.
    QAction* fromPlan = match->findChild<QAction*>(QStringLiteral("sheetMatchScale_vp1"));
    ASSERT_NE(fromPlan, nullptr);
    EXPECT_TRUE(fromPlan->text().contains(QStringLiteral("1:1000")));
    EXPECT_EQ(match->findChild<QAction*>(QStringLiteral("sheetMatchScale_vp2")), nullptr);
    EXPECT_EQ(match->findChild<QAction*>(QStringLiteral("sheetMatchScale_vp3")), nullptr);

    const std::size_t before = document.history().undoCount();
    fromPlan->trigger();
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(viewportOf(document, "vp2").scale, 1000.0);
    EXPECT_EQ(viewportOf(document, "vp1").scale, 250.0) << "the plan it came from is untouched";
    EXPECT_EQ(shown.errors, 0);
}

TEST(SheetArrangeEditor, FitViewToContentSizesTheSelectedPlan)
{
    // 100 x 50 m at 1 : 500 is 200 x 100 mm, and 5% more, about the centre.
    Document document;
    addLine(document, Point2(0.0, 0.0), Point2(100.0, 50.0));
    plotting::Viewport plan = viewportAt("vp1", plotting::ViewportKind::Plan,
                                         Box2(Point2(100.0, 100.0), Point2(200.0, 200.0)));
    plan.scale = 500.0;
    addSheetWith(document, {plan, viewportAt("vp2", plotting::ViewportKind::Legend,
                                             Box2(Point2(320.0, 40.0), Point2(400.0, 100.0)))});
    Shown shown(document);
    shown.editor.canvas()->select("vp1");
    shown.trigger("sheetFitToContent");
    const Box2 rect = viewportOf(document, "vp1").rect;
    EXPECT_NEAR(rect.min.x, 45.0, 1e-9);
    EXPECT_NEAR(rect.max.x, 255.0, 1e-9);
    EXPECT_NEAR(rect.min.y, 97.5, 1e-9);
    EXPECT_NEAR(rect.max.y, 202.5, 1e-9);
    EXPECT_EQ(shown.errors, 0);

    // A legend has nothing to fit.
    shown.editor.canvas()->select("vp2");
    shown.trigger("sheetFitToContent");
    EXPECT_EQ(shown.errors, 1);
}

TEST(SheetArrangeEditor, ATurnedPlanIsDrawnWhereTheModelSaysItIs)
{
    // A 450 x 20 m strip along 45 degrees in a plan filling the sheet. Square
    // it needs 1 : 2000; turned, 1 : 1250 - and no further than that takes.
    Document document;
    const Point2 middle(5000.0, 7000.0);
    const std::vector<Point2> corners = addStrip(document, middle, 450.0, 20.0, 45.0 * kDegree);
    plotting::Viewport plan = viewportAt("vp1", plotting::ViewportKind::Plan,
                                         plotting::tilingArea(plotting::Sheet{}));
    plan.autoScale = true;
    plan.autoCentre = true;
    addSheetWith(document, {plan});
    Shown shown(document);
    const std::size_t before = document.history().undoCount();

    // Nothing selected: the sheet's main plan.
    shown.trigger("sheetRotateToBestFit");
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(shown.errors, 0);
    const plotting::Viewport turned = viewportOf(document, "vp1");
    EXPECT_EQ(turned.scale, 1250.0);
    EXPECT_GT(turned.rotation, 0.0);
    EXPECT_LT(turned.rotation, 45.0 * kDegree);
    EXPECT_NEAR(turned.centre.x, middle.x, 1e-6);
    EXPECT_NEAR(turned.centre.y, middle.y, 1e-6);

    // Painted: each corner of the strip where the view's rotation, centre and
    // scale put it, inked, and inside the view.
    const auto& sheet = document.sheetSet().sheets[0];
    constexpr double kPpmm = 4.0;
    const auto paper = katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    QImage image(static_cast<int>(std::lround(paper.widthMm * kPpmm)),
                 static_cast<int>(std::lround(paper.heightMm * kPpmm)),
                 QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::white);
    katana::qt::SheetPaintOptions options;
    options.pixelsPerMillimetre = kPpmm;
    katana::qt::SheetPaintCache cache;
    {
        QPainter painter(&image);
        (void)katana::qt::paintSheet(painter, document.sheetSet(), 0, shown.editor.source(), options,
                                     cache);
    }
    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        image.save(QString("%1/SheetArrangeTurned.png").arg(dir));
    }
    const auto inkNear = [&](Point2 at) {
        int ink = 0;
        const int cx = static_cast<int>(std::floor(at.x * kPpmm));
        const int cy = static_cast<int>(std::floor((paper.heightMm - at.y) * kPpmm));
        for (int y = cy - 3; y <= cy + 3; ++y) {
            for (int x = cx - 3; x <= cx + 3; ++x) {
                ink += image.pixel(x, y) != qRgb(255, 255, 255) ? 1 : 0;
            }
        }
        return ink;
    };
    const Point2 paperCentre = turned.rect.center();
    for (const Point2& corner : corners) {
        const Vec2 onPaper = (corner - turned.centre).rotated(-turned.rotation) * (1000.0 / turned.scale);
        const Point2 at = paperCentre + onPaper;
        EXPECT_TRUE(turned.rect.inflated(-1.0).contains(at)) << at.x << ", " << at.y;
        EXPECT_GT(inkNear(at), 0) << "no line at the corner " << at.x << ", " << at.y;
    }
    // The long sides run nearly across the paper: their middles are inked
    // too, where the strip's centre line crosses the view's middle.
    const Vec2 side = Vec2(0.0, 10.0).rotated(45.0 * kDegree - turned.rotation) * (1000.0 / 1250.0);
    EXPECT_GT(inkNear(paperCentre + side), 0);
    EXPECT_GT(inkNear(paperCentre - side), 0);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(viewportOf(document, "vp1").rotation, 0.0);
}

TEST(SheetArrangeEditor, ChoosePaperPutsTheSheetOnPaperThatHoldsItsPlan)
{
    // 500 x 300 m at 1 : 1000 filling the sheet: 520 x 312 mm with 4% to
    // spare, which A3 cannot hold and A2 landscape can.
    Document document;
    addStrip(document, Point2(250.0, 150.0), 500.0, 300.0, 0.0);
    plotting::Viewport plan = viewportAt("vp1", plotting::ViewportKind::Plan,
                                         plotting::tilingArea(plotting::Sheet{}));
    plan.scale = 1000.0;
    addSheetWith(document, {plan});
    Shown shown(document);
    auto* button = shown.editor.findChild<QPushButton*>(QStringLiteral("sheetChoosePaper"));
    ASSERT_NE(button, nullptr);
    EXPECT_TRUE(button->isEnabled());
    const std::size_t before = document.history().undoCount();
    button->click();
    katana::qt::test::processEvents();
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(shown.errors, 0);
    const auto& sheet = document.sheetSet().sheets[0];
    EXPECT_EQ(sheet.paper, PaperSize::A2);
    EXPECT_TRUE(sheet.landscape);
    EXPECT_EQ(viewportOf(document, "vp1").rect, plotting::tilingArea(sheet));
    EXPECT_TRUE(shown.messages.back().contains(QStringLiteral("A2 landscape")));

    // A sheet with no plan has nothing to choose paper for.
    ASSERT_TRUE(plotting::editSheet(document, 0, [](plotting::Sheet& s) {
                    s.viewports.front().kind = plotting::ViewportKind::Legend;
                    return katana::core::Status{};
                }).ok());
    // The properties are rebuilt on the change; the old button, deleted
    // later, must not be the one found.
    katana::qt::test::processEvents();
    auto* disabled = shown.editor.findChild<QPushButton*>(QStringLiteral("sheetChoosePaper"));
    ASSERT_NE(disabled, nullptr);
    EXPECT_FALSE(disabled->isEnabled());
}

TEST(SheetArrangeEditor, TheFittedDrawingIsGeneratedTurnedWhenAsked)
{
    // The strip along 45 degrees, on its own sheet (no legend beside it): in
    // the 385 x 250 mm tiling area it is 1 : 2000 square and 1 : 1250 turned
    // by the least turn that reaches it.
    Document document;
    addStrip(document, Point2(0.0, 0.0), 450.0, 20.0, 45.0 * kDegree);
    Shown shown(document);
    const auto answer = [&](bool rotate) {
        QTimer::singleShot(0, [&shown, rotate] {
            for (QDialog* dialog : shown.editor.findChildren<QDialog*>()) {
                if (!dialog->isVisible()) {
                    continue;
                }
                auto* turn = dialog->findChild<QCheckBox*>(QStringLiteral("sheetGenerateRotate"));
                ASSERT_NE(turn, nullptr);
                EXPECT_TRUE(turn->isEnabled());
                turn->setChecked(rotate);
                for (QCheckBox* box : dialog->findChildren<QCheckBox*>()) {
                    if (box->text().contains(QStringLiteral("legend"))) {
                        box->setChecked(false);
                    }
                }
                dialog->accept();
            }
        });
        shown.editor.generateSheets();
    };

    answer(false);
    ASSERT_EQ(document.sheetSet().sheets.size(), 1u);
    EXPECT_EQ(document.sheetSet().sheets[0].viewports.front().scale, 2000.0);
    EXPECT_EQ(document.sheetSet().sheets[0].viewports.front().rotation, 0.0);

    answer(true);
    ASSERT_EQ(document.sheetSet().sheets.size(), 2u);
    const plotting::Viewport& turned = document.sheetSet().sheets[1].viewports.front();
    const double t = 45.0 * kDegree - std::atan2(225.0, 10.0) + std::acos(156.25 / std::hypot(225.0, 10.0));
    EXPECT_EQ(turned.scale, 1250.0);
    EXPECT_NEAR(turned.rotation, t, 1e-9);
}
