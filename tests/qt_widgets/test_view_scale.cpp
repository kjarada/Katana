#include <gtest/gtest.h>

#include <QComboBox>
#include <QImage>
#include <QLabel>
#include <QLineEdit>
#include <QPainter>
#include <QPushButton>
#include <QStringList>
#include <QToolButton>

#include "dock_chrome.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/plot.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/style_library.hpp"
#include "plan_painter.hpp"
#include "view_scale_dialog.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

namespace {

using katana::cad::ViewKind;
using katana::entity::StyleUnits;
using katana::geometry::Point2;
using katana::qt::PlanPaintOptions;

struct Mark {
    katana::entity::Model model;
    katana::entity::StyleLibrary library;
    katana::qt::PlanFrame frame;
    PlanPaintOptions options;
    katana::qt::PlanPaintCache cache;
    katana::qt::PlanPaintStats stats;

    explicit Mark(StyleUnits units = StyleUnits::Paper, bool linestyle = false)
    {
        katana::entity::LineStyle definition;
        definition.name = "test mark";
        definition.units = units;
        if (linestyle) {
            definition.length = 4.0;
            definition.strokes = {
                {katana::entity::StrokeOp::Move, Point2(0.0, 0.0)},
                {katana::entity::StrokeOp::Draw, Point2(0.0, 2.0)}};
        } else {
            definition.symbol = true;
            definition.strokes = {
                {katana::entity::StrokeOp::Move, Point2(-1.0, -1.0)},
                {katana::entity::StrokeOp::Draw, Point2(1.0, -1.0)},
                {katana::entity::StrokeOp::Draw, Point2(1.0, 1.0)},
                {katana::entity::StrokeOp::Draw, Point2(-1.0, 1.0)},
                {katana::entity::StrokeOp::Draw, Point2(-1.0, -1.0)}};
        }
        EXPECT_TRUE(library.add(definition).ok());
        katana::entity::Style style;
        style.name = "marks";
        if (linestyle) {
            style.linetype = definition.name;
        } else {
            style.symbol = definition.name;
        }
        EXPECT_TRUE(model.styles.add(style).ok());
        katana::entity::Entity entity;
        entity.style = style.name;
        if (linestyle) {
            entity.geometry = katana::geometry::Segment2{Point2(-2.0, 0.0), Point2(2.0, 0.0)};
        } else {
            entity.geometry = katana::entity::PointGeometry{Point2(0.0, 0.0)};
        }
        EXPECT_TRUE(model.entities.add(entity).ok());
        frame.transform.resize(200.0, 200.0);
        frame.transform.scale = 20.0;
        options.pixelsPerMillimetre = 4.0;
        options.thinLines = true;
    }

    QImage paint()
    {
        QImage image(200, 200, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::black);
        katana::qt::PlanSource source;
        source.model = &model;
        source.library = &library;
        source.libraryGeneration = 1;
        QPainter painter(&image);
        stats = katana::qt::paintPlan(painter, source, frame, options, cache);
        painter.end();
        return image;
    }
};

QRect inkBounds(const QImage& image)
{
    QRect bounds;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            if (qGray(image.pixel(x, y)) > 0) {
                bounds = bounds.united(QRect(x, y, 1, 1));
            }
        }
    }
    return bounds;
}

void expectSize(const QImage& image, double width, double height)
{
    const QRect bounds = inkBounds(image);
    ASSERT_FALSE(bounds.isEmpty());
    // A one-pixel pen plus antialiasing can reach the neighbouring pixel on
    // either edge. The centreline dimensions below are calculated in metres.
    EXPECT_NEAR(bounds.width(), width, 2.0);
    EXPECT_NEAR(bounds.height(), height, 2.0);
}

} // namespace

TEST(ViewScale, AutomaticPaperSymbolsKeepTheirScreenSizeWhenZoomChanges)
{
    Mark mark;
    // The square is 2 mm across: Auto at 4 px/mm is 8 px at either zoom.
    expectSize(mark.paint(), 8.0, 8.0);
    mark.frame.transform.scale = 40.0;
    expectSize(mark.paint(), 8.0, 8.0);
}

TEST(ViewScale, FixedPaperSymbolsUseTheDenominatorAndZoomIndependently)
{
    Mark mark;
    // 2 mm at 1:500 = 1 m; at 20 px/m it is 20 px across.
    mark.options.paperStyleScale = 500.0;
    expectSize(mark.paint(), 20.0, 20.0);
    mark.options.paperStyleScale = 1000.0;
    expectSize(mark.paint(), 40.0, 40.0);
    mark.options.paperStyleScale = 500.0;
    mark.frame.transform.scale = 40.0;
    expectSize(mark.paint(), 40.0, 40.0);
    mark.options.paperStyleScale.reset();
    expectSize(mark.paint(), 8.0, 8.0);
}

TEST(ViewScale, PaperLinestyleTicksUseTheSameReferenceScaleAsSymbols)
{
    Mark mark(StyleUnits::Paper, true);
    mark.options.paperStyleScale = 500.0;
    // The tick is 2 mm high: 2 * 500 / 1000 m * 20 px/m = 20 px.
    EXPECT_NEAR(inkBounds(mark.paint()).height(), 20.0, 2.0);
    mark.options.paperStyleScale = 1000.0;
    EXPECT_NEAR(inkBounds(mark.paint()).height(), 40.0, 2.0);
    mark.options.paperStyleScale.reset();
    EXPECT_NEAR(inkBounds(mark.paint()).height(), 8.0, 2.0);
}

TEST(ViewScale, GroundSizedMarksAndPlotPaperMarksIgnoreTheScreenReferenceScale)
{
    for (const bool linestyle : {false, true}) {
        Mark ground(StyleUnits::World, linestyle);
        const QImage before = ground.paint();
        ground.options.paperStyleScale = 500.0;
        EXPECT_EQ(ground.paint(), before);
        ground.options.paperStyleScale = 2000.0;
        EXPECT_EQ(ground.paint(), before);

        Mark paper(StyleUnits::Paper, linestyle);
        katana::cad::PlotSettings plot;
        paper.options.medium = katana::qt::PlanMedium::Paper;
        paper.options.plot = &plot;
        const QImage plotted = paper.paint();
        paper.options.paperStyleScale = 500.0;
        EXPECT_EQ(paper.paint(), plotted);
        paper.options.paperStyleScale = 2000.0;
        EXPECT_EQ(paper.paint(), plotted);
    }
}

TEST(ViewScale, ASymbolSpriteIsRebuiltWhenOnlyTheReferenceScaleChanges)
{
    Mark mark;
    mark.options.symbolSprites = true;
    mark.options.paperStyleScale = 500.0;
    expectSize(mark.paint(), 20.0, 20.0);
    ASSERT_GT(mark.stats.spritesDrawn, 0U);
    mark.options.paperStyleScale = 1000.0;
    const QImage changed = mark.paint();
    expectSize(changed, 40.0, 40.0);
    Mark fresh;
    fresh.options = mark.options;
    EXPECT_EQ(changed, fresh.paint());
}

TEST(ViewScale, TheKeptPlanImageInvalidatesWhenOnlyItsPaperStyleScaleChanges)
{
    katana::cad::Document document;
    ASSERT_TRUE(document.execute(katana::commands::createPoint(Point2(0.0, 0.0))).ok());
    katana::cad::ViewSet views;
    auto& state = views.add(ViewKind::Plan);
    state.planFramed = true;
    state.plan.scale = 20.0;
    katana::qt::ViewportWidget widget(document, state);
    widget.resize(200, 200);
    katana::qt::test::paint(widget);
    const auto paints = widget.drawingPaintCount();
    state.paperStyleScale = 500.0;
    katana::qt::test::paint(widget);
    EXPECT_EQ(widget.drawingPaintCount(), paints + 1);
    katana::qt::test::paint(widget);
    EXPECT_EQ(widget.drawingPaintCount(), paints + 1);
    state.paperStyleScale.reset();
    katana::qt::test::paint(widget);
    EXPECT_EQ(widget.drawingPaintCount(), paints + 2);
    EXPECT_EQ(state.plan.scale, 20.0);
}

TEST(ViewScale, TheDialogOnlyBuildsAnExplicitViewsScaleLineAndReportsItsResult)
{
    katana::cad::ViewSet views;
    views.add(ViewKind::Plan);
    views.add(ViewKind::Model3D);
    views.add(ViewKind::Plan);
    QStringList lines;
    katana::qt::ViewScaleDialog dialog(
        views, 3, [&lines](const QString& line) {
            lines.append(line);
            katana::qt::VerbOutcome result;
            result.ok = true;
            result.reply = "view=3 kind=plan paper_scale=500 changed=yes";
            return result;
        });
    EXPECT_FALSE(dialog.isModal());
    auto* target = dialog.findChild<QComboBox*>("viewScaleView");
    auto* mode = dialog.findChild<QComboBox*>("viewScaleMode");
    auto* denominator = dialog.findChild<QLineEdit*>("viewScaleDenominator");
    auto* apply = dialog.findChild<QPushButton*>("viewScaleApply");
    auto* status = dialog.findChild<QLabel*>("viewScaleStatus");
    ASSERT_NE(target, nullptr);
    ASSERT_NE(mode, nullptr);
    ASSERT_NE(denominator, nullptr);
    ASSERT_NE(apply, nullptr);
    ASSERT_NE(status, nullptr);
    EXPECT_EQ(target->count(), 2) << "a model view cannot choose paper-style scale";
    EXPECT_EQ(target->currentData().toUInt(), 3U);
    mode->setCurrentIndex(1);
    denominator->setText("500");
    apply->click();
    EXPECT_EQ(lines, (QStringList{"VIEWS SCALE 3 500"}));
    EXPECT_TRUE(status->text().contains("paper_scale=500"));
    EXPECT_FALSE(views.find(3)->paperStyleScale.has_value()) << "the dialog does not edit";
    denominator->setText("nan");
    apply->click();
    EXPECT_EQ(lines.size(), 1);
    EXPECT_FALSE(status->text().isEmpty());
    mode->setCurrentIndex(0);
    apply->click();
    EXPECT_EQ(lines.back(), "VIEWS SCALE 3 AUTO");
}

TEST(ViewScale, TheViewBarRunsTheSharedScaleCommandAndUndoRestoresItsDisplayState)
{
    katana::cad::Document document;
    QMainWindow window;
    auto* workspace = new katana::qt::ViewWorkspace(document, &window);
    window.setCentralWidget(workspace);
    auto* chrome = new katana::qt::DockChrome(window);
    workspace->setChrome(chrome);
    window.resize(800, 600);
    window.show();
    katana::qt::test::processEvents();
    auto* button = workspace->dockFor(1)->findChild<QToolButton*>("ViewScaleButton");
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isVisible());
    button->click();
    katana::qt::test::processEvents();
    auto* dialog = workspace->findChild<QDialog*>("viewScaleDialog");
    ASSERT_NE(dialog, nullptr);
    dialog->findChild<QComboBox*>("viewScaleMode")->setCurrentIndex(1);
    dialog->findChild<QLineEdit*>("viewScaleDenominator")->setText("500");
    dialog->findChild<QPushButton*>("viewScaleApply")->click();
    EXPECT_EQ(workspace->viewSet().find(1)->paperStyleScale, 500.0);
    EXPECT_EQ(document.history().undoCount(), 1U);
    EXPECT_TRUE(button->toolTip().contains("1:500"));
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(workspace->viewSet().find(1)->paperStyleScale.has_value());
    EXPECT_EQ(dialog->findChild<QComboBox*>("viewScaleMode")->currentIndex(), 0);
    EXPECT_TRUE(button->toolTip().contains("Auto"));
    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(workspace->viewSet().find(1)->paperStyleScale, 500.0);
    ASSERT_TRUE(workspace->setViewKind(1, ViewKind::Model3D).ok());
    EXPECT_FALSE(button->isVisible());
    ASSERT_TRUE(workspace->setViewKind(1, ViewKind::Plan).ok());
    EXPECT_EQ(workspace->viewSet().find(1)->paperStyleScale, 500.0);
}

TEST(ViewScale, AClosedTargetIsRefusedWithoutEditingAnotherView)
{
    katana::cad::ViewSet views;
    views.add(ViewKind::Plan);
    views.add(ViewKind::Plan);
    int calls = 0;
    katana::qt::ViewScaleDialog dialog(
        views, 1, [&calls](const QString&) {
            ++calls;
            return katana::qt::VerbOutcome{true, {}, {}};
        });
    ASSERT_TRUE(views.remove(1).ok());
    EXPECT_FALSE(dialog.apply());
    EXPECT_EQ(calls, 0);
    EXPECT_FALSE(views.find(2)->paperStyleScale.has_value());
}

TEST(ViewScale, OpeningAndReplacingADrawingRestoresItsScaleWithoutMovingTheCamera)
{
    katana::cad::Document document;
    ASSERT_TRUE(katana::cad::setViewPaperScales(document, {{1, 500.0}}).ok());
    katana::qt::ViewWorkspace workspace(document);
    auto* view = workspace.viewSet().find(1);
    ASSERT_NE(view, nullptr);
    EXPECT_EQ(view->paperStyleScale, 500.0);
    view->plan.center = Point2(4.0, 5.0);
    view->plan.scale = 8.0;
    auto metadata = document.metadata();
    metadata.unknownKeys.erase("view_paper_scales");
    document.setMetadata(metadata);
    EXPECT_FALSE(view->paperStyleScale.has_value());
    EXPECT_EQ(view->plan.center, Point2(4.0, 5.0));
    EXPECT_EQ(view->plan.scale, 8.0);
}
