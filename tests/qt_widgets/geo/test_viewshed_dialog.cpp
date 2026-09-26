// Terrain > Analysis > Viewshed and Line of Sight
// (src/katana_qt/geo/viewshed_dialog.hpp): the lines the fields describe,
// and the dialog driven by its object names as a person clicks it, running
// exactly the line it shows through the runner it was given. A picked point
// is only written into the line.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QStringList>

#include "geo/viewshed_dialog.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::LosForm;
using katana::qt::TerrainDialogContext;
using katana::qt::VerbOutcome;
using katana::qt::ViewshedDialog;
using katana::qt::ViewshedForm;

struct Recorder {
    QStringList lines;
    TerrainDialogContext context(katana::cad::Document* document,
                                 const katana::interop::ReferenceData* reference,
                                 const katana::terrain::SurfaceStore* surfaces)
    {
        TerrainDialogContext made;
        made.run = [this](const QString& line) {
            lines << line;
            return VerbOutcome{true, "ran " + line, {}};
        };
        made.document = document;
        made.reference = reference;
        made.surfaces = surfaces;
        return made;
    }
};

TEST(ViewshedLine, ObserversOptionsAndNameAreWrittenInOrder)
{
    ViewshedForm form;
    form.source = "RASTER 4";
    form.observers = {"10,20", "30.5, 40"};
    EXPECT_EQ(*katana::qt::viewshedLine(form),
              "RASTER VIEWSHED RASTER 4 OBSERVER 10,20 OBSERVER 30.5,40 height=1.7 target=0");
    form.max = "500";
    form.curvature = "None";
    form.areas = "site/seen";
    form.name = "from the hill";
    EXPECT_EQ(*katana::qt::viewshedLine(form),
              "RASTER VIEWSHED RASTER 4 OBSERVER 10,20 OBSERVER 30.5,40 height=1.7 target=0 "
              "max=500 curvature=none areas=site/seen NAME \"from the hill\"");
    // The points of a scope instead.
    form.useScope = true;
    form.scope = "LAYERS survey/control";
    form.max.clear();
    form.curvature.clear();
    form.areas.clear();
    form.name.clear();
    EXPECT_EQ(*katana::qt::viewshedLine(form),
              "RASTER VIEWSHED RASTER 4 OBSERVERS LAYERS survey/control height=1.7 target=0");
}

TEST(ViewshedLine, WhatCannotBeWrittenIsRefusedNamingTheField)
{
    ViewshedForm form;
    EXPECT_NE(katana::qt::viewshedLine(form).error().message.find("Source"), std::string::npos);
    form.source = "SURFACE ground";
    EXPECT_NE(katana::qt::viewshedLine(form).error().message.find("Observers"), std::string::npos);
    form.observers = {"1,2"};
    form.height = "tall";
    EXPECT_NE(katana::qt::viewshedLine(form).error().message.find("Eye height"), std::string::npos);
    form.height = "1.7";
    form.max = "0";
    EXPECT_NE(katana::qt::viewshedLine(form).error().message.find("Look as far"),
              std::string::npos);
    form.max.clear();
    form.curvature = "flat";
    EXPECT_NE(katana::qt::viewshedLine(form).error().message.find("Curvature"), std::string::npos);
}

TEST(LosLine, BothEndsAndTheHeightsAreWritten)
{
    LosForm form;
    form.source = "SURFACE ground";
    EXPECT_NE(katana::qt::losLine(form).error().message.find("Observer"), std::string::npos);
    form.observer = "0,0";
    EXPECT_NE(katana::qt::losLine(form).error().message.find("Target"), std::string::npos);
    form.target = "100, 50";
    form.curvature = "none";
    EXPECT_EQ(*katana::qt::losLine(form),
              "LOS SURFACE ground OBSERVER 0,0 TARGET 100,50 height=1.7 target=0 curvature=none");
}

TEST(ViewshedDialog, EveryFieldHasItsNameAndEachTabRunsItsLine)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::interop::RasterOverlay raster;
    raster.name = "terrain";
    raster.source = "terrain.asc";
    const auto id = reference.add(raster);
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    TerrainDialogContext context = recorder.context(&document, &reference, &surfaces);
    // A picker that answers at once, as a click in a plan view would.
    context.pickPoint = [](katana::qt::PickedPoint picked) {
        picked(katana::geometry::Point2(12.0, 34.5));
    };
    ViewshedDialog dialog(std::move(context));
    EXPECT_EQ(dialog.objectName(), "viewshedDialog");
    for (const char* name :
         {"viewshedTabs", "viewshedSource", "viewshedObserver", "viewshedAdd", "viewshedPick",
          "viewshedObservers", "viewshedRemove", "viewshedUseScope", "viewshedScope",
          "viewshedHeight", "viewshedTarget", "viewshedMax", "viewshedCurvature", "viewshedAreas",
          "viewshedName", "viewshedCommand", "viewshedPreview", "viewshedRun", "viewshedReply",
          "losSource", "losObserver", "losPickObserver", "losTarget", "losPickTarget", "losHeight",
          "losTargetHeight", "losCurvature", "losCommand", "losRun", "losReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    // Nothing to run until there is an observer: one typed, one picked.
    EXPECT_FALSE(dialog.findChild<QPushButton*>("viewshedRun")->isEnabled());
    dialog.findChild<QLineEdit*>("viewshedObserver")->setText("1,2");
    dialog.findChild<QPushButton*>("viewshedAdd")->click();
    dialog.findChild<QPushButton*>("viewshedPick")->click();
    ASSERT_EQ(dialog.findChild<QListWidget*>("viewshedObservers")->count(), 2);
    dialog.findChild<QLineEdit*>("viewshedMax")->setText("250");
    dialog.findChild<QPushButton*>("viewshedRun")->click();
    ASSERT_EQ(recorder.lines.size(), 1);
    EXPECT_EQ(recorder.lines.back(),
              QString("RASTER VIEWSHED RASTER %1 OBSERVER 1,2 OBSERVER 12.000,34.500 height=1.7 "
                      "target=0 max=250")
                  .arg(id));
    EXPECT_EQ(dialog.findChild<QPlainTextEdit*>("viewshedReply")->toPlainText(),
              "ran " + recorder.lines.back());
    // The points of the whole drawing instead: the list is off.
    dialog.findChild<QCheckBox*>("viewshedUseScope")->setChecked(true);
    EXPECT_FALSE(dialog.findChild<QListWidget*>("viewshedObservers")->isEnabled());
    dialog.scopeControls().setChoice(katana::qt::ScopeChoice::Drawing);
    dialog.findChild<QPushButton*>("viewshedPreview")->click();
    EXPECT_EQ(recorder.lines.back(),
              QString("RASTER VIEWSHED RASTER %1 OBSERVERS DRAWING height=1.7 target=0 max=250 "
                      "PREVIEW")
                  .arg(id));

    // Line of sight: the observer picked, the target typed.
    EXPECT_FALSE(dialog.findChild<QPushButton*>("losRun")->isEnabled());
    dialog.findChild<QPushButton*>("losPickObserver")->click();
    EXPECT_EQ(dialog.findChild<QLineEdit*>("losObserver")->text(), "12.000,34.500");
    dialog.findChild<QLineEdit*>("losTarget")->setText("80,20");
    dialog.findChild<QLineEdit*>("losCurvature")->setText("none");
    dialog.findChild<QPushButton*>("losRun")->click();
    EXPECT_EQ(recorder.lines.back(),
              QString("LOS RASTER %1 OBSERVER 12.000,34.500 TARGET 80,20 height=1.7 target=0 "
                      "curvature=none")
                  .arg(id));
    EXPECT_EQ(dialog.findChild<QPlainTextEdit*>("losReply")->toPlainText(),
              "ran " + recorder.lines.back());
}

TEST(ViewshedDialog, WithNoPickerThePickButtonsAreOff)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    ViewshedDialog dialog(recorder.context(&document, &reference, &surfaces));
    for (const char* name : {"viewshedPick", "losPickObserver", "losPickTarget"}) {
        EXPECT_FALSE(dialog.findChild<QPushButton*>(name)->isEnabled()) << name;
    }
    EXPECT_TRUE(dialog.findChild<QLineEdit*>("viewshedCommand")->placeholderText().contains("Source"));
}

} // namespace
