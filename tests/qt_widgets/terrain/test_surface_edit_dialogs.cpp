// Terrain > Edit Surface's dialogs, Combine Surfaces and the Surface Edit
// toolbar (src/katana_qt/terrain/surface_edit_dialogs.hpp, surface_edit_ui.hpp;
// docs/surface_editing.md): the lines the fields describe, and the dialogs
// driven by their object names, running exactly the line they show through
// the runner they were given.

#include <gtest/gtest.h>

#include <QAction>
#include <QCheckBox>
#include <QComboBox>
#include <QLineEdit>
#include <QListWidget>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QStringList>
#include <QToolBar>
#include <QWidget>

#include <memory>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/cad/terrain/surface_edit_verbs.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "terrain/surface_edit_dialogs.hpp"
#include "terrain/surface_edit_ui.hpp"
#include "tools/tool_menus.hpp"

namespace {

using katana::qt::SurfaceCombineDialog;
using katana::qt::SurfaceCombineForm;
using katana::qt::SurfaceCombineMode;
using katana::qt::SurfaceEditDialogContext;
using katana::qt::SurfaceLevelsDialog;
using katana::qt::SurfaceLevelsForm;
using katana::qt::SurfaceLevelsKind;
using katana::qt::VerbOutcome;

std::shared_ptr<const katana::terrain::TinSurface> square()
{
    auto surface = katana::terrain::TinSurface::create(
        {{0, 0, 1}, {10, 0, 2}, {10, 10, 3}, {0, 10, 2}}, {{0, 1, 2}, {0, 2, 3}});
    EXPECT_TRUE(surface.ok());
    return std::make_shared<const katana::terrain::TinSurface>(*surface);
}

// The drawing with surfaces of these names.
void addSurfaces(katana::cad::Document& document, const QStringList& names)
{
    katana::terrain::SurfaceStore store = document.surfaces();
    for (const QString& name : names) {
        ASSERT_TRUE(store.add({name.toStdString(), square(), "test"}).ok());
    }
    ASSERT_TRUE(document.setSurfaces(std::move(store), "TEST").ok());
}

// A runner that keeps the lines and answers each as a headless run does.
struct Recorder {
    QStringList lines;
    SurfaceEditDialogContext context(katana::cad::Document* document)
    {
        SurfaceEditDialogContext made;
        made.run = [this](const QString& line) {
            lines << line;
            return VerbOutcome{true, "ran " + line, {}};
        };
        made.document = document;
        return made;
    }
};

std::string error(const katana::core::Result<QString>& line)
{
    return line ? std::string("no error: ") + line->toStdString() : line.error().message;
}

} // namespace

TEST(SurfaceEditLines, TheLevelsFieldsWriteTheLine)
{
    SurfaceLevelsForm form;
    form.surface = "ground";
    form.operation = 0;
    form.value = "1.5";
    EXPECT_EQ(*katana::qt::surfaceLevelsLine(SurfaceLevelsKind::Heights, form),
              "SURFACE EDIT ground RAISE 1.5");
    form.operation = 3;
    form.value = "2";
    form.base = "100";
    form.inside = true;
    form.scope = "LAYERS pads ONLY";
    EXPECT_EQ(*katana::qt::surfaceLevelsLine(SurfaceLevelsKind::Heights, form),
              "SURFACE EDIT ground SCALE 2 base=100 INSIDE FROM LAYERS pads ONLY");
    form.surface = "my ground";
    form.passes = 4;
    form.weight = "0.25";
    form.inside = false;
    EXPECT_EQ(*katana::qt::surfaceLevelsLine(SurfaceLevelsKind::Smooth, form),
              "SURFACE EDIT \"my ground\" SMOOTH passes=4 weight=0.25");
    form.surface = "ground";
    form.value = "0.02";
    EXPECT_EQ(*katana::qt::surfaceLevelsLine(SurfaceLevelsKind::Thin, form),
              "SURFACE EDIT ground THIN 0.02");
    form.value = "40";
    EXPECT_EQ(*katana::qt::surfaceLevelsLine(SurfaceLevelsKind::DeleteLong, form),
              "SURFACE EDIT ground DELETE LONG 40");
    form.longSides = false;
    form.value = "5";
    EXPECT_EQ(*katana::qt::surfaceLevelsLine(SurfaceLevelsKind::DeleteLong, form),
              "SURFACE EDIT ground DELETE THIN 5");
}

TEST(SurfaceEditLines, WhatCannotBeWrittenIsRefusedNamingTheField)
{
    SurfaceLevelsForm form;
    form.value = "1";
    EXPECT_NE(error(katana::qt::surfaceLevelsLine(SurfaceLevelsKind::Heights, form)).find("Surface"),
              std::string::npos);
    form.surface = "ground";
    form.value = "up";
    EXPECT_NE(error(katana::qt::surfaceLevelsLine(SurfaceLevelsKind::Heights, form)).find("By"),
              std::string::npos);
    form.value = "-1";
    EXPECT_NE(error(katana::qt::surfaceLevelsLine(SurfaceLevelsKind::Thin, form)).find("Tolerance"),
              std::string::npos);
    form.weight = "1.5";
    EXPECT_NE(error(katana::qt::surfaceLevelsLine(SurfaceLevelsKind::Smooth, form)).find("Weight"),
              std::string::npos);
    form.weight = "0.5";
    form.inside = true;
    form.scopeError = "no layer is ticked";
    EXPECT_NE(error(katana::qt::surfaceLevelsLine(SurfaceLevelsKind::Smooth, form))
                  .find("Only inside"),
              std::string::npos);
}

TEST(SurfaceEditLines, TheCombineFieldsWriteTheLine)
{
    SurfaceCombineForm form;
    form.merge = {"ground", "pad"};
    form.name = "merged";
    EXPECT_EQ(*katana::qt::surfaceCombineLine(form), "SURFACE MERGE ground pad NAME merged");
    form.mode = SurfaceCombineMode::Paste;
    form.top = "pad";
    form.base = "ground";
    form.name.clear();
    EXPECT_EQ(*katana::qt::surfaceCombineLine(form), "SURFACE PASTE pad INTO ground");
    form.name = "design";
    EXPECT_EQ(*katana::qt::surfaceCombineLine(form), "SURFACE PASTE pad INTO ground NAME design");
    form.mode = SurfaceCombineMode::Clip;
    form.surface = "ground";
    form.scope = "SELECTION";
    form.name = "part";
    EXPECT_EQ(*katana::qt::surfaceCombineLine(form), "SURFACE CLIP ground FROM SELECTION NAME part");
    form.mode = SurfaceCombineMode::Offset;
    form.dz = "-0.3";
    form.name = "subgrade";
    EXPECT_EQ(*katana::qt::surfaceCombineLine(form),
              "SURFACE OFFSET ground -0.3 NAME subgrade");
}

TEST(SurfaceEditLines, ACombineThatCannotBeWrittenIsRefusedNamingTheField)
{
    SurfaceCombineForm form;
    form.merge = {"ground"};
    form.name = "merged";
    EXPECT_NE(error(katana::qt::surfaceCombineLine(form)).find("Surfaces"), std::string::npos);
    form.merge = {"ground", "pad"};
    form.name.clear();
    EXPECT_NE(error(katana::qt::surfaceCombineLine(form)).find("New surface"), std::string::npos);
    form.mode = SurfaceCombineMode::Paste;
    form.top = "ground";
    form.base = "GROUND";
    EXPECT_NE(error(katana::qt::surfaceCombineLine(form)).find("Top"), std::string::npos);
    form.mode = SurfaceCombineMode::Offset;
    form.surface = "ground";
    form.dz = "up";
    form.name = "x";
    EXPECT_NE(error(katana::qt::surfaceCombineLine(form)).find("Raise by"), std::string::npos);
}

TEST(SurfaceLevelsDialog, EveryKindHasItsNamedControlsAndRunRunsTheLineShown)
{
    katana::cad::Document document;
    addSurfaces(document, {"ground"});
    Recorder recorder;
    SurfaceLevelsDialog heights(SurfaceLevelsKind::Heights, recorder.context(&document));
    EXPECT_EQ(heights.objectName(), "surfaceHeightsDialog");
    for (const char* name :
         {"surfaceHeightsSurface", "surfaceHeightsOperation", "surfaceHeightsValue",
          "surfaceHeightsBase", "surfaceHeightsInside", "surfaceHeightsScope",
          "surfaceHeightsCommand", "surfaceHeightsPreview", "surfaceHeightsRun",
          "surfaceHeightsReply"}) {
        EXPECT_NE(heights.findChild<QWidget*>(name), nullptr) << name;
    }
    EXPECT_EQ(heights.findChild<QLineEdit*>("surfaceHeightsCommand")->text(),
              "SURFACE EDIT ground RAISE 1");
    heights.findChild<QComboBox*>("surfaceHeightsOperation")->setCurrentIndex(2);
    heights.findChild<QLineEdit*>("surfaceHeightsValue")->setText("12.5");
    heights.findChild<QPushButton*>("surfaceHeightsRun")->click();
    ASSERT_EQ(recorder.lines.size(), 1);
    EXPECT_EQ(recorder.lines.front(), "SURFACE EDIT ground SETLEVEL 12.5");
    EXPECT_EQ(heights.findChild<QPlainTextEdit*>("surfaceHeightsReply")->toPlainText(),
              "ran SURFACE EDIT ground SETLEVEL 12.5");
    heights.findChild<QPushButton*>("surfaceHeightsPreview")->click();
    EXPECT_EQ(recorder.lines.back(), "SURFACE EDIT ground SETLEVEL 12.5 PREVIEW");

    // Only inside: the shared scope widget writes the polygon's scope.
    heights.findChild<QCheckBox*>("surfaceHeightsInside")->setChecked(true);
    EXPECT_TRUE(heights.findChild<QWidget*>("surfaceHeightsScope")->isEnabled());
    heights.findChild<QWidget*>("surfaceHeightsScopeDrawing")
        ->setProperty("checked", true);
    EXPECT_EQ(heights.findChild<QLineEdit*>("surfaceHeightsCommand")->text(),
              "SURFACE EDIT ground SETLEVEL 12.5 INSIDE FROM DRAWING");

    SurfaceLevelsDialog smooth(SurfaceLevelsKind::Smooth, recorder.context(&document));
    EXPECT_EQ(smooth.objectName(), "surfaceSmoothDialog");
    smooth.findChild<QSpinBox*>("surfaceSmoothPasses")->setValue(5);
    EXPECT_EQ(smooth.findChild<QLineEdit*>("surfaceSmoothCommand")->text(),
              "SURFACE EDIT ground SMOOTH passes=5 weight=0.5");

    SurfaceLevelsDialog thin(SurfaceLevelsKind::Thin, recorder.context(&document));
    EXPECT_EQ(thin.objectName(), "surfaceThinDialog");
    EXPECT_EQ(thin.findChild<QLineEdit*>("surfaceThinCommand")->text(),
              "SURFACE EDIT ground THIN 0.01");

    SurfaceLevelsDialog deleteLong(SurfaceLevelsKind::DeleteLong, recorder.context(&document));
    EXPECT_EQ(deleteLong.objectName(), "surfaceDeleteLongDialog");
    deleteLong.findChild<QComboBox*>("surfaceDeleteLongMeasure")->setCurrentIndex(1);
    deleteLong.findChild<QLineEdit*>("surfaceDeleteLongValue")->setText("10");
    EXPECT_EQ(deleteLong.findChild<QLineEdit*>("surfaceDeleteLongCommand")->text(),
              "SURFACE EDIT ground DELETE THIN 10");
}

TEST(SurfaceLevelsDialog, WithNoSurfaceRunIsOffAndTheCommandSaysWhy)
{
    katana::cad::Document document;
    Recorder recorder;
    SurfaceLevelsDialog dialog(SurfaceLevelsKind::Thin, recorder.context(&document));
    EXPECT_FALSE(dialog.findChild<QPushButton*>("surfaceThinRun")->isEnabled());
    EXPECT_TRUE(dialog.findChild<QLineEdit*>("surfaceThinCommand")
                    ->placeholderText()
                    .contains("Surface"));
    EXPECT_TRUE(recorder.lines.isEmpty());
}

TEST(SurfaceCombineDialog, EachModeWritesItsLineAndRunRunsIt)
{
    katana::cad::Document document;
    addSurfaces(document, {"ground", "pad"});
    Recorder recorder;
    SurfaceCombineDialog dialog(recorder.context(&document));
    EXPECT_EQ(dialog.objectName(), "surfaceCombineDialog");
    for (const char* name :
         {"surfaceCombineMode", "surfaceCombineMergeList", "surfaceCombineUp",
          "surfaceCombineDown", "surfaceCombineTop", "surfaceCombineBase",
          "surfaceCombineSource", "surfaceCombineDz", "surfaceCombineScope",
          "surfaceCombineName", "surfaceCombineCommand", "surfaceCombinePreview",
          "surfaceCombineRun", "surfaceCombineReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    // Merge: tick both, the pad last so it wins.
    auto* list = dialog.findChild<QListWidget*>("surfaceCombineMergeList");
    ASSERT_EQ(list->count(), 2);
    list->item(0)->setCheckState(Qt::Checked);
    list->item(1)->setCheckState(Qt::Checked);
    dialog.findChild<QLineEdit*>("surfaceCombineName")->setText("merged");
    EXPECT_EQ(dialog.findChild<QLineEdit*>("surfaceCombineCommand")->text(),
              "SURFACE MERGE ground pad NAME merged");
    // Down moves the chosen one later in the order: the ground now wins.
    list->setCurrentRow(0);
    dialog.findChild<QPushButton*>("surfaceCombineDown")->click();
    EXPECT_EQ(dialog.findChild<QLineEdit*>("surfaceCombineCommand")->text(),
              "SURFACE MERGE pad ground NAME merged");

    // Paste in place: the top is the first surface, the base the other.
    dialog.setMode(SurfaceCombineMode::Paste);
    dialog.findChild<QLineEdit*>("surfaceCombineName")->clear();
    dialog.findChild<QComboBox*>("surfaceCombineTop")->setCurrentText("pad");
    dialog.findChild<QComboBox*>("surfaceCombineBase")->setCurrentText("ground");
    dialog.findChild<QPushButton*>("surfaceCombineRun")->click();
    ASSERT_FALSE(recorder.lines.isEmpty());
    EXPECT_EQ(recorder.lines.back(), "SURFACE PASTE pad INTO ground");

    dialog.setMode(SurfaceCombineMode::Offset);
    dialog.findChild<QComboBox*>("surfaceCombineSource")->setCurrentText("ground");
    dialog.findChild<QLineEdit*>("surfaceCombineDz")->setText("-0.5");
    dialog.findChild<QLineEdit*>("surfaceCombineName")->setText("subgrade");
    dialog.findChild<QPushButton*>("surfaceCombinePreview")->click();
    EXPECT_EQ(recorder.lines.back(), "SURFACE OFFSET ground -0.5 NAME subgrade PREVIEW");
}

TEST(SurfaceEditUi, TheSubmenuToolbarAndDialogItemsAreNamedAndTheToolsFillThem)
{
    katana::cad::Document document;
    addSurfaces(document, {"ground", "pad"});
    QWidget window;
    katana::qt::SurfaceEditServices services;
    services.window = &window;
    services.document = &document;
    services.run = [](const QString& line) { return VerbOutcome{true, line, {}}; };
    services.makeAction = [&window](katana::qt::Icon icon, const QString& text,
                                    const QString& tip, const QKeySequence&,
                                    const QString& name) {
        auto* action = new QAction(katana::qt::icon(icon), text, &window);
        action->setStatusTip(tip);
        action->setObjectName(name);
        return action;
    };
    services.makeToolBar = [&window](const QString& title) {
        auto* bar = new QToolBar(title, &window);
        bar->setObjectName(title + "ToolBar");
        return bar;
    };
    katana::qt::SurfaceEditUi ui(std::move(services));
    EXPECT_EQ(ui.menu()->objectName(), "terrainEditMenu");
    EXPECT_EQ(ui.toolBar()->objectName(), "SurfaceEditToolBar");

    // The window hands the catalogue this menu and toolbar for "Terrain".
    katana::qt::tools::ToolMenuTargets targets;
    targets.menus["Terrain"] = ui.menu();
    targets.toolBars["Terrain"] = ui.toolBar();
    const auto actions = katana::qt::tools::fillToolMenus(
        katana::cad::toolCatalog(), targets, &window, [](const std::string&) {});
    // The other categories have no target in this test and are left out;
    // Terrain's tools all went where the window puts them.
    for (const std::string& category : actions.unplaced()) {
        EXPECT_NE(category, "Terrain");
    }
    QMenu terrain;
    ui.finish(terrain);
    QStringList names;
    for (QAction* action : ui.menu()->actions()) {
        names << action->objectName();
    }
    for (const char* name :
         {"terrain.edit.deletetriangle", "terrain.edit.flip", "terrain.edit.addpoint",
          "terrain.edit.deletepoint", "terrain.edit.deleteinside", "terrain.edit.breakline",
          "terrain.edit.boundary", "terrain.edit.hole", "terrainSurfaceHeights",
          "terrainSurfaceSmooth", "terrainSurfaceThin", "terrainSurfaceDeleteLong"}) {
        EXPECT_TRUE(names.contains(name)) << name;
    }
    auto* heights = window.findChild<QAction*>("terrainSurfaceHeights");
    ASSERT_NE(heights, nullptr);
    EXPECT_EQ(heights->data().toString(), "surfaceHeightsDialog");
    EXPECT_NE(window.findChild<QAction*>("terrainCombineSurfaces"), nullptr);
    EXPECT_TRUE(terrain.actions().contains(ui.menu()->menuAction()));

    // The toolbar's list is the drawing's surfaces; choosing one points the tools at it.
    katana::cad::setSurfaceEditTarget("");
    auto* active = ui.toolBar()->findChild<QComboBox*>("surfaceEditActive");
    ASSERT_NE(active, nullptr);
    ASSERT_EQ(active->count(), 2);
    EXPECT_EQ(active->currentText(), "ground");
    active->setCurrentIndex(1);
    emit active->activated(1);
    EXPECT_EQ(katana::cad::surfaceEditTarget(), "pad");
    EXPECT_EQ(*katana::cad::activeEditSurface(document), "pad");
    katana::cad::setSurfaceEditTarget("");
}
