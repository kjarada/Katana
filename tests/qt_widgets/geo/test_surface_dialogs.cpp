// Terrain > Surface From and GIS > Export Surface as DEM
// (src/katana_qt/surface_raster_dialog.hpp): the lines the fields describe,
// and the dialogs driven by their object names as a person clicks them, each
// running exactly the line it shows through the runner it was given.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QStringList>

#include <memory>

#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "surface_raster_dialog.hpp"

namespace {

using katana::qt::SurfaceExportForm;
using katana::qt::SurfaceFromDialog;
using katana::qt::SurfaceFromForm;
using katana::qt::SurfaceFromKind;
using katana::qt::SurfaceRasterDialog;
using katana::qt::TerrainDialogContext;
using katana::qt::VerbOutcome;

// A runner that keeps the lines and answers each as a headless run does.
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

TEST(SurfaceFromLine, EachSourceWritesItsOwnFormAndLeavesBlanksOut)
{
    SurfaceFromForm cloud;
    cloud.kind = SurfaceFromKind::Cloud;
    cloud.source = "CLOUD 2";
    cloud.classes = "2, 8";
    EXPECT_EQ(*katana::qt::surfaceFromLine(cloud), "SURFACE FROM CLOUD 2 classes=2,8");

    SurfaceFromForm raster;
    raster.kind = SurfaceFromKind::Raster;
    raster.source = "RASTER 3";
    raster.maxPoints = "100000";
    raster.area = "0,0, 40,30";
    raster.name = "site ground";
    EXPECT_EQ(*katana::qt::surfaceFromLine(raster),
              "SURFACE FROM RASTER 3 max=100000 AREA 0,0,40,30 NAME \"site ground\"");

    SurfaceFromForm drawing;
    drawing.scope = "LAYERS survey WHERE DRAWN";
    EXPECT_EQ(*katana::qt::surfaceFromLine(drawing), "SURFACE FROM LAYERS survey WHERE DRAWN");
}

TEST(SurfaceFromLine, WhatCannotBeWrittenIsRefusedNamingTheField)
{
    SurfaceFromForm raster;
    raster.kind = SurfaceFromKind::Raster;
    EXPECT_NE(katana::qt::surfaceFromLine(raster).error().message.find("Source"), std::string::npos);
    raster.source = "RASTER 1";
    raster.area = "0,0,40";
    EXPECT_NE(katana::qt::surfaceFromLine(raster).error().message.find("Window"), std::string::npos);
    raster.area.clear();
    raster.maxPoints = "lots";
    EXPECT_NE(katana::qt::surfaceFromLine(raster).error().message.find("Most points"),
              std::string::npos);
    SurfaceFromForm drawing;
    drawing.scopeError = "no layer is ticked";
    EXPECT_NE(katana::qt::surfaceFromLine(drawing).error().message.find("Apply to"),
              std::string::npos);
    drawing.scopeError.clear();
    drawing.name = "a \"b\"";
    EXPECT_NE(katana::qt::surfaceFromLine(drawing).error().message.find("Name"), std::string::npos);
}

TEST(SurfaceFromDialog, TheDrawingIsWhatIsDrawnAtFirstAndRunRunsTheLineShown)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    SurfaceFromDialog dialog(recorder.context(&document, &reference, &surfaces));
    EXPECT_EQ(dialog.objectName(), "surfaceFromDialog");
    dialog.showKind(SurfaceFromKind::Drawing);
    auto* command = dialog.findChild<QLineEdit*>("surfaceFromCommand");
    ASSERT_NE(command, nullptr);
    EXPECT_EQ(command->text(), "SURFACE FROM DRAWING WHERE DRAWN");
    ASSERT_NE(dialog.findChild<QWidget*>("surfaceFromScope"), nullptr);
    ASSERT_NE(dialog.findChild<QCheckBox*>("surfaceFromDrawnOnly"), nullptr);

    dialog.findChild<QLineEdit*>("surfaceFromName")->setText("ground");
    dialog.findChild<QPushButton*>("surfaceFromRun")->click();
    ASSERT_EQ(recorder.lines.size(), 1);
    EXPECT_EQ(recorder.lines.front(), "SURFACE FROM DRAWING WHERE DRAWN NAME ground");
    EXPECT_EQ(dialog.findChild<QPlainTextEdit*>("surfaceFromReply")->toPlainText(),
              "ran SURFACE FROM DRAWING WHERE DRAWN NAME ground");

    dialog.findChild<QPushButton*>("surfaceFromPreview")->click();
    EXPECT_EQ(recorder.lines.back(), "SURFACE FROM DRAWING WHERE DRAWN NAME ground PREVIEW");
}

TEST(SurfaceFromDialog, ARasterSourceIsChosenByItsIdAndTheWindowIsWritten)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::interop::RasterOverlay raster;
    raster.name = "terrain";
    raster.source = "terrain.asc";
    const auto id = reference.add(raster);
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    SurfaceFromDialog dialog(recorder.context(&document, &reference, &surfaces));
    dialog.showKind(SurfaceFromKind::Raster);
    auto* source = dialog.findChild<QComboBox*>("surfaceFromSource");
    ASSERT_NE(source, nullptr);
    ASSERT_EQ(source->count(), 1);
    EXPECT_EQ(source->currentData().toString(), QString("RASTER %1").arg(id));
    dialog.findChild<QLineEdit*>("surfaceFromArea")->setText("0,0,20,20");
    EXPECT_EQ(dialog.findChild<QLineEdit*>("surfaceFromCommand")->text(),
              QString("SURFACE FROM RASTER %1 AREA 0,0,20,20").arg(id));
    // The drawing's controls are a drawing's: off for a raster.
    EXPECT_FALSE(dialog.findChild<QWidget*>("surfaceFromScope")->isEnabled());
}

TEST(SurfaceFromDialog, WithNoRasterRunIsOffAndTheCommandSaysWhy)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    SurfaceFromDialog dialog(recorder.context(&document, &reference, &surfaces));
    dialog.showKind(SurfaceFromKind::Raster);
    EXPECT_FALSE(dialog.findChild<QPushButton*>("surfaceFromRun")->isEnabled());
    EXPECT_TRUE(dialog.findChild<QLineEdit*>("surfaceFromCommand")
                    ->placeholderText()
                    .contains("no raster"));
    EXPECT_TRUE(recorder.lines.isEmpty());
}

// ---- Export Surface as DEM -----------------------------------------------------------------

TEST(SurfaceExportLine, TheLineNamesTheSurfaceTheFileAndTheOptions)
{
    SurfaceExportForm form;
    form.surface = "site ground";
    form.file = QDir::toNativeSeparators("C:/out/dem 1.tif");
    form.cell = 0.5;
    form.cog = true;
    form.overwrite = true;
    EXPECT_EQ(*katana::qt::surfaceExportLine(form),
              "SURFACE EXPORT \"site ground\" \"C:/out/dem 1.tif\" cell=0.5 type=Float32 COG "
              "OVERWRITE");
    form.file.clear();
    EXPECT_NE(katana::qt::surfaceExportLine(form).error().message.find("File"), std::string::npos);
    form.file = "dem.tif";
    form.cell = 0.0;
    EXPECT_NE(katana::qt::surfaceExportLine(form).error().message.find("Cell"), std::string::npos);
}

TEST(SurfaceRasterDialog, ItSuggestsACellForTheSurfaceAndRunsTheExportLine)
{
    katana::terrain::TinInput input;
    input.points = {{0, 0, 1}, {100, 0, 2}, {100, 50, 3}, {0, 50, 2}};
    auto built = katana::terrain::buildTin(input);
    ASSERT_TRUE(built.ok());
    katana::terrain::SurfaceStore surfaces;
    ASSERT_TRUE(surfaces
                    .add({"ground",
                          std::make_shared<const katana::terrain::TinSurface>(built->surface),
                          "test"})
                    .ok());
    katana::interop::ReferenceData reference;
    Recorder recorder;
    SurfaceRasterDialog dialog(recorder.context(nullptr, &reference, &surfaces));
    EXPECT_EQ(dialog.objectName(), "surfaceRasterDialog");
    // A 100 m extent at about 1000 cells along its longer side: 0.1.
    EXPECT_DOUBLE_EQ(dialog.findChild<QDoubleSpinBox*>("surfaceRasterCell")->value(), 0.1);
    EXPECT_EQ(dialog.findChild<QComboBox*>("surfaceRasterSource")->currentText(), "ground");
    EXPECT_FALSE(dialog.findChild<QPushButton*>("surfaceRasterRun")->isEnabled())
        << "no file yet";
    dialog.findChild<QLineEdit*>("surfaceRasterFile")->setText("dem.tif");
    dialog.findChild<QDoubleSpinBox*>("surfaceRasterCell")->setValue(1.0);
    dialog.findChild<QComboBox*>("surfaceRasterType")->setCurrentText("Float64");
    EXPECT_EQ(dialog.findChild<QLabel*>("surfaceRasterGrid")->text(), "100 x 50 cells");
    dialog.findChild<QPushButton*>("surfaceRasterRun")->click();
    ASSERT_EQ(recorder.lines.size(), 1);
    EXPECT_EQ(recorder.lines.front(), "SURFACE EXPORT ground dem.tif cell=1 type=Float64");
}

} // namespace
