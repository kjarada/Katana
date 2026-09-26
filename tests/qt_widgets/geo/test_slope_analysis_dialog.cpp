// Terrain > Analysis > Slope and Aspect
// (src/katana_qt/geo/slope_analysis_dialog.hpp): the line the fields
// describe, and the dialog driven by its object names as a person clicks it,
// running exactly the line it shows through the runner it was given.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QStringList>

#include "geo/slope_analysis_dialog.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::SlopeAnalysisDialog;
using katana::qt::SlopeForm;
using katana::qt::TerrainDialogContext;
using katana::qt::VerbOutcome;

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

TEST(SlopeLine, TheClassesTheirLayerAndTheLeastAreaAreWrittenTogether)
{
    SlopeForm form;
    form.source = "RASTER 3";
    EXPECT_EQ(*katana::qt::slopeLine(form), "RASTER SLOPE RASTER 3 unit=percent");
    form.classes = "5, 10, 25";
    form.areas = "site/batters";
    form.minArea = "25";
    form.name = "site slope";
    EXPECT_EQ(*katana::qt::slopeLine(form),
              "RASTER SLOPE RASTER 3 unit=percent classes=5,10,25 areas=site/batters "
              "min_area=25 NAME \"site slope\"");
    // Without classes there are no areas to put anywhere.
    form.classes.clear();
    EXPECT_EQ(*katana::qt::slopeLine(form), "RASTER SLOPE RASTER 3 unit=percent NAME \"site slope\"");
    // Aspect has no unit and no classes.
    form.aspect = true;
    form.classes = "5";
    form.clip = true;
    form.scope = "LAYERS lots";
    EXPECT_EQ(*katana::qt::slopeLine(form), "RASTER ASPECT RASTER 3 NAME \"site slope\" LAYERS lots");
}

TEST(SlopeLine, WhatCannotBeWrittenIsRefusedNamingTheField)
{
    SlopeForm form;
    EXPECT_NE(katana::qt::slopeLine(form).error().message.find("Source"), std::string::npos);
    form.source = "SURFACE ground";
    form.classes = "five";
    EXPECT_NE(katana::qt::slopeLine(form).error().message.find("Classes"), std::string::npos);
    form.classes = "5";
    form.minArea = "0";
    EXPECT_NE(katana::qt::slopeLine(form).error().message.find("Least area"), std::string::npos);
    form.minArea.clear();
    form.clip = true;
    form.scopeError = "no layer is ticked";
    EXPECT_NE(katana::qt::slopeLine(form).error().message.find("Keep inside"), std::string::npos);
}

TEST(SlopeAnalysisDialog, EveryFieldHasItsNameAndRunRunsTheLineShown)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::interop::RasterOverlay raster;
    raster.name = "terrain";
    raster.source = "terrain.asc";
    const auto id = reference.add(raster);
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    SlopeAnalysisDialog dialog(recorder.context(&document, &reference, &surfaces));
    EXPECT_EQ(dialog.objectName(), "slopeAnalysisDialog");
    for (const char* name :
         {"slopeSource", "slopeKind", "slopeUnit", "slopeClasses", "slopeAreasLayer",
          "slopeMinArea", "slopeName", "slopeClip", "slopeScope", "slopeCommand", "slopePreview",
          "slopeRun", "slopeReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    EXPECT_EQ(dialog.findChild<QLineEdit*>("slopeCommand")->text(),
              QString("RASTER SLOPE RASTER %1 unit=percent").arg(id));
    // The classes' layer and least area wait for classes.
    EXPECT_FALSE(dialog.findChild<QLineEdit*>("slopeAreasLayer")->isEnabled());
    dialog.findChild<QLineEdit*>("slopeClasses")->setText("5,25");
    EXPECT_TRUE(dialog.findChild<QLineEdit*>("slopeAreasLayer")->isEnabled());
    dialog.findChild<QLineEdit*>("slopeMinArea")->setText("4");
    dialog.findChild<QCheckBox*>("slopeClip")->setChecked(true);
    dialog.scopeControls().setChoice(katana::qt::ScopeChoice::Drawing);
    dialog.findChild<QPushButton*>("slopeRun")->click();
    ASSERT_EQ(recorder.lines.size(), 1);
    EXPECT_EQ(recorder.lines.front(),
              QString("RASTER SLOPE RASTER %1 unit=percent classes=5,25 areas=terrain/slope "
                      "min_area=4 DRAWING")
                  .arg(id));
    EXPECT_EQ(dialog.findChild<QPlainTextEdit*>("slopeReply")->toPlainText(),
              "ran " + recorder.lines.front());

    // Aspect: no unit, no classes.
    auto* kind = dialog.findChild<QComboBox*>("slopeKind");
    kind->setCurrentIndex(1);
    EXPECT_FALSE(dialog.findChild<QComboBox*>("slopeUnit")->isEnabled());
    EXPECT_FALSE(dialog.findChild<QLineEdit*>("slopeClasses")->isEnabled());
    dialog.findChild<QPushButton*>("slopePreview")->click();
    EXPECT_EQ(recorder.lines.back(), QString("RASTER ASPECT RASTER %1 DRAWING PREVIEW").arg(id));
}

TEST(SlopeAnalysisDialog, WithNothingToAnalyseRunIsOffAndTheCommandSaysWhy)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    SlopeAnalysisDialog dialog(recorder.context(&document, &reference, &surfaces));
    EXPECT_FALSE(dialog.findChild<QPushButton*>("slopeRun")->isEnabled());
    EXPECT_TRUE(dialog.findChild<QLineEdit*>("slopeCommand")->placeholderText().contains("Source"));
}

} // namespace
