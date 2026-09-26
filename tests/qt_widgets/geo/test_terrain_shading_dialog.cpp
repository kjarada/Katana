// Terrain > Analysis > Terrain Shading
// (src/katana_qt/geo/terrain_shading_dialog.hpp): the line the fields
// describe, and the dialog driven by its object names as a person clicks it,
// running exactly the line it shows through the runner it was given.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QStringList>

#include "geo/terrain_shading_dialog.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::ShadingForm;
using katana::qt::TerrainDialogContext;
using katana::qt::TerrainShadingDialog;
using katana::qt::VerbOutcome;

struct Recorder {
    QStringList lines;
    TerrainDialogContext context(const katana::interop::ReferenceData* reference,
                                 const katana::terrain::SurfaceStore* surfaces)
    {
        TerrainDialogContext made;
        made.run = [this](const QString& line) {
            lines << line;
            return VerbOutcome{true, "ran " + line, {}};
        };
        made.reference = reference;
        made.surfaces = surfaces;
        return made;
    }
};

TEST(ShadingLine, TheLightIsWrittenForAHillshadeOnlyWhereItIsNotGdalsDefault)
{
    ShadingForm form;
    form.source = "RASTER 2";
    EXPECT_EQ(*katana::qt::shadingLine(form), "RASTER SHADE RASTER 2 style=hillshade");
    form.azimuth = 270.0;
    form.zFactor = 2.5;
    form.variant = "combined";
    EXPECT_EQ(*katana::qt::shadingLine(form),
              "RASTER SHADE RASTER 2 style=hillshade azimuth=270 z=2.5 variant=combined");
    // Multidirectional light comes from every side: no azimuth for it.
    form.variant = "multidirectional";
    EXPECT_EQ(*katana::qt::shadingLine(form),
              "RASTER SHADE RASTER 2 style=hillshade z=2.5 variant=multidirectional");
    // A coloured style has no light, and its own ramp is not written.
    form.style = "relief";
    form.ramp = "terrain";
    EXPECT_EQ(*katana::qt::shadingLine(form), "RASTER SHADE RASTER 2 style=relief");
}

TEST(ShadingLine, TheRampTheRangeTheNameAndTheSaveAreWritten)
{
    ShadingForm form;
    form.source = "SURFACE ground";
    form.style = "relief+hillshade";
    form.ramp = "diverging";
    form.rangeMin = "-2";
    form.rangeMax = "2";
    form.name = "cut fill";
    form.save = "C:\\out\\cut fill.tif";
    form.overwrite = true;
    EXPECT_EQ(*katana::qt::shadingLine(form),
              "RASTER SHADE SURFACE ground style=relief+hillshade ramp=diverging range=-2,2 NAME "
              "\"cut fill\" save=\"C:/out/cut fill.tif\" OVERWRITE");
}

TEST(ShadingLine, WhatCannotBeWrittenIsRefusedNamingTheField)
{
    ShadingForm form;
    EXPECT_NE(katana::qt::shadingLine(form).error().message.find("Source"), std::string::npos);
    form.source = "RASTER 1";
    form.style = "slope";
    form.rangeMin = "0";
    EXPECT_NE(katana::qt::shadingLine(form).error().message.find("Range"), std::string::npos);
    form.rangeMax = "-1";
    EXPECT_NE(katana::qt::shadingLine(form).error().message.find("Range"), std::string::npos);
    form.rangeMin.clear();
    form.rangeMax.clear();
    form.name = "a\"b";
    EXPECT_NE(katana::qt::shadingLine(form).error().message.find("Name"), std::string::npos);
}

TEST(TerrainShadingDialog, EveryFieldHasItsNameAndRunRunsTheLineShown)
{
    katana::interop::ReferenceData reference;
    katana::interop::RasterOverlay raster;
    raster.name = "terrain";
    raster.source = "terrain.asc";
    const auto id = reference.add(raster);
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    TerrainShadingDialog dialog(recorder.context(&reference, &surfaces));
    EXPECT_EQ(dialog.objectName(), "terrainShadingDialog");
    for (const char* name :
         {"shadingSource", "shadingStyle", "shadingAzimuth", "shadingAltitude", "shadingZFactor",
          "shadingVariant", "shadingRamp", "shadingRangeMin", "shadingRangeMax", "shadingName",
          "shadingSave", "shadingSaveBrowse", "shadingOverwrite", "shadingCommand",
          "shadingPreview", "shadingRun", "shadingReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    EXPECT_EQ(dialog.findChild<QLineEdit*>("shadingCommand")->text(),
              QString("RASTER SHADE RASTER %1 style=hillshade").arg(id));
    // A hillshade is grey: the ramp waits for a coloured style.
    EXPECT_FALSE(dialog.findChild<QComboBox*>("shadingRamp")->isEnabled());

    auto* style = dialog.findChild<QComboBox*>("shadingStyle");
    style->setCurrentIndex(style->findData("slope"));
    EXPECT_TRUE(dialog.findChild<QComboBox*>("shadingRamp")->isEnabled());
    EXPECT_FALSE(dialog.findChild<QDoubleSpinBox*>("shadingAzimuth")->isEnabled());
    EXPECT_EQ(dialog.findChild<QComboBox*>("shadingRamp")->currentText(), "slope");
    dialog.findChild<QLineEdit*>("shadingRangeMin")->setText("0");
    dialog.findChild<QLineEdit*>("shadingRangeMax")->setText("30");
    dialog.findChild<QPushButton*>("shadingRun")->click();
    ASSERT_EQ(recorder.lines.size(), 1);
    EXPECT_EQ(recorder.lines.front(),
              QString("RASTER SHADE RASTER %1 style=slope range=0,30").arg(id));
    EXPECT_EQ(dialog.findChild<QPlainTextEdit*>("shadingReply")->toPlainText(),
              "ran " + recorder.lines.front());
    dialog.findChild<QPushButton*>("shadingPreview")->click();
    EXPECT_TRUE(recorder.lines.back().endsWith(" PREVIEW"));
}

TEST(TerrainShadingDialog, WithNothingToShadeRunIsOffAndTheCommandSaysWhy)
{
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    TerrainShadingDialog dialog(recorder.context(&reference, &surfaces));
    EXPECT_FALSE(dialog.findChild<QPushButton*>("shadingRun")->isEnabled());
    EXPECT_TRUE(
        dialog.findChild<QLineEdit*>("shadingCommand")->placeholderText().contains("Source"));
}

} // namespace
