// The dialogs of the terrain and road work on a screen too small for them
// (src/katana_qt/screen_fit.hpp, docs/desktop.md "Fitting the screen"): Close, Run and
// Add are the dialog's own buttons, and they stay on the dialog, below the scroll area,
// whatever is scrolled.
//
// The screen under the widget tests is small (800 px wide), but how tall a dialog is
// depends on its layout, which another change may shrink; each dialog is therefore made
// taller than the screen by giving it a minimum height over the screen's, so that the
// fit has to scroll it whatever its own layout needs, and what is tested is that the
// fit finds the dialog's real button row.

#include <gtest/gtest.h>

#include <QApplication>
#include <QDialog>
#include <QGuiApplication>
#include <QPoint>
#include <QPushButton>
#include <QRect>
#include <QScreen>
#include <QScrollArea>
#include <QScrollBar>
#include <QString>
#include <QWidget>

#include <initializer_list>

#include "bim/road_object_dialog.hpp"
#include "katana/cad/document.hpp"
#include "screen_fit.hpp"
#include "terrain/surface_edit_dialogs.hpp"
#include "terrain/volumes_dialog.hpp"

namespace {

using katana::qt::kScreenFitFooter;
using katana::qt::kScreenFitScroll;
using katana::qt::RoadObjectDialog;
using katana::qt::RoadObjectDialogContext;
using katana::qt::SurfaceCombineDialog;
using katana::qt::SurfaceEditDialogContext;
using katana::qt::SurfaceLevelsDialog;
using katana::qt::SurfaceLevelsKind;
using katana::qt::VerbOutcome;
using katana::qt::VolumesDialog;
using katana::qt::VolumesDialogContext;

// The application-wide filter, for one test, as test_screen_fit.cpp has it.
struct FilterGuard {
    QObject* filter = katana::qt::installScreenFit(*qApp);
    ~FilterGuard() { delete filter; }
};

QRect screenRoom()
{
    return QGuiApplication::primaryScreen()->availableGeometry();
}

// A run that answers every line as a headless run does; none of these tests runs one.
katana::qt::CommandRunner answering()
{
    return [](const QString&) { return VerbOutcome{true, "ran", {}}; };
}

// Taller than the screen, whatever the dialog's own layout needs.
void makeTallerThanTheScreen(QDialog& dialog)
{
    dialog.setMinimumHeight(screenRoom().height() + 400);
}

// `dialog` shown through the screen fit: it fits the screen, it was scrolled (the
// premise: what is tested is a scrolling dialog), and each widget named is on the
// dialog as it stands - in the footer, not in the scroll area - and stays where it is
// with the body scrolled right down.
void expectInSight(QDialog& dialog, std::initializer_list<const char*> names)
{
    dialog.show();
    QApplication::processEvents();
    EXPECT_LE(dialog.height(), screenRoom().height());
    auto* scroll = dialog.findChild<QScrollArea*>(kScreenFitScroll);
    ASSERT_NE(scroll, nullptr) << "a dialog over the screen's height scrolls";
    auto* footer = dialog.findChild<QWidget*>(kScreenFitFooter);
    ASSERT_NE(footer, nullptr) << "and its button row is found, so it is kept out of the scroll";
    for (const char* name : names) {
        auto* widget = dialog.findChild<QWidget*>(name);
        ASSERT_NE(widget, nullptr) << name;
        EXPECT_TRUE(footer->isAncestorOf(widget)) << name << " is not in the footer";
        EXPECT_FALSE(scroll->isAncestorOf(widget)) << name << " scrolls with the rest";
        ASSERT_TRUE(widget->isVisible()) << name;
        const auto placed = [&] { return QRect(widget->mapTo(&dialog, QPoint(0, 0)), widget->size()); };
        const QRect resting = placed();
        EXPECT_TRUE(dialog.rect().contains(resting)) << name << " is off the dialog";
        scroll->verticalScrollBar()->setValue(scroll->verticalScrollBar()->maximum());
        QApplication::processEvents();
        EXPECT_EQ(placed(), resting) << name << " moved with the body";
        scroll->verticalScrollBar()->setValue(0);
        QApplication::processEvents();
    }
}

} // namespace

TEST(ScreenFitDialogs, SurfaceEditHeightsKeepsPreviewRunCloseAndTheReplyInSight)
{
    const FilterGuard guard;
    SurfaceEditDialogContext context;
    context.run = answering();
    SurfaceLevelsDialog dialog(SurfaceLevelsKind::Heights, std::move(context));
    makeTallerThanTheScreen(dialog);
    expectInSight(dialog, {"surfaceHeightsPreview", "surfaceHeightsRun", "surfaceHeightsClose",
                           "surfaceHeightsReply"});
}

TEST(ScreenFitDialogs, SurfaceEditDeletePointsKeepsItsButtonsAndReplyInSightBelowItsScope)
{
    // The scope's own controls are in the body, above; the buttons are not scrolled
    // away behind them.
    const FilterGuard guard;
    SurfaceEditDialogContext context;
    context.run = answering();
    SurfaceLevelsDialog dialog(SurfaceLevelsKind::DeletePoints, std::move(context));
    makeTallerThanTheScreen(dialog);
    expectInSight(dialog, {"surfaceDeletePointsPreview", "surfaceDeletePointsRun",
                           "surfaceDeletePointsClose", "surfaceDeletePointsReply"});
}

TEST(ScreenFitDialogs, CombineSurfacesKeepsPreviewRunCloseAndTheReplyInSight)
{
    const FilterGuard guard;
    SurfaceEditDialogContext context;
    context.run = answering();
    SurfaceCombineDialog dialog(std::move(context));
    makeTallerThanTheScreen(dialog);
    expectInSight(dialog, {"surfaceCombinePreview", "surfaceCombineRun", "surfaceCombineClose",
                           "surfaceCombineReply"});
}

TEST(ScreenFitDialogs, VolumesKeepsItsCloseButtonInSight)
{
    const FilterGuard guard;
    VolumesDialogContext context;
    context.run = answering();
    VolumesDialog dialog(std::move(context));
    makeTallerThanTheScreen(dialog);
    expectInSight(dialog, {"volumesClose"});
}

TEST(ScreenFitDialogs, RoadObjectsKeepsPreviewAddCloseAndWhatTheyReplyInSight)
{
    // The placing dialog's line, Preview, Add and Close, then the reply and the status
    // line: one footer, as the dialog made its own scrolling body for them.
    const FilterGuard guard;
    katana::cad::Document document;
    RoadObjectDialogContext context;
    context.document = &document;
    context.run = answering();
    RoadObjectDialog dialog(std::move(context));
    makeTallerThanTheScreen(dialog);
    expectInSight(dialog, {"roadObjectLine", "roadObjectPreviewRun", "roadObjectAdd",
                           "roadObjectClose", "roadObjectReply", "roadObjectStatus"});
}
