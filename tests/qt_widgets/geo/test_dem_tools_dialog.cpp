// Terrain > DEM > DEM Tools (src/katana_qt/geo/dem_tools_dialog.hpp): each
// tab's line, built from its fields as a person fills them - found by object
// name - and handed to the executor exactly as shown: a stub runner's, and
// the window's own workbench's, which clips the plane into a reference raster.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMainWindow>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QStringList>
#include <QTabWidget>

#include <filesystem>
#include <memory>
#include <system_error>

#include "geo/binding_picker.hpp"
#include "geo/dem_tools_dialog.hpp"
#include "geo/geo_dialog_support.hpp"
#include "geo/geo_workbench.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::DemTool;
using katana::qt::DemToolForm;
using katana::qt::DemToolsDialog;
using katana::qt::demToolCommandLine;
using katana::qt::GeoDialogContext;
using katana::qt::VerbOutcome;

// tests/geo/data/plane.asc: 40 x 30 cells of 1 m from (0,0).
const QString kPlane = QString(KATANA_QT_WIDGET_DATA) + "/../../geo/data/plane.asc";

template <class T>
T* child(QWidget& dialog, const char* name)
{
    auto* found = dialog.findChild<T*>(name);
    EXPECT_NE(found, nullptr) << name;
    return found;
}

struct StubRunner {
    QStringList lines;
    GeoDialogContext context()
    {
        GeoDialogContext made;
        made.run = [this](const QString& line) {
            lines << line;
            return VerbOutcome{true, "ok", {}};
        };
        made.headless = [] { return true; };
        return made;
    }
};

TEST(DemToolCommandLine, EachToolsLineIsItsVerbSourcesAndOptionsInOrder)
{
    DemToolForm mosaic;
    mosaic.tool = DemTool::Mosaic;
    mosaic.sources = {"FILE tiles/a.tif", "FILE tiles/b.tif"};
    mosaic.options = {{"resolution", "highest"}};
    mosaic.save = "C:/work/site dem.tif";
    EXPECT_EQ(*demToolCommandLine(mosaic),
              "RASTER MOSAIC FILE tiles/a.tif FILE tiles/b.tif resolution=highest SAVE "
              "\"C:/work/site dem.tif\"");

    DemToolForm clip;
    clip.tool = DemTool::Clip;
    clip.sources = {"RASTER 2"};
    clip.region = "LAYERS boundary";
    clip.name = "site";
    EXPECT_EQ(*demToolCommandLine(clip), "RASTER CLIP RASTER 2 LAYERS boundary NAME site");

    DemToolForm fill;
    fill.tool = DemTool::Fill;
    fill.sources = {"SURFACE ground CELL 1"};
    fill.options = {{"distance", "20"}, {"smoothing", ""}, {"strategy", "nearest"}};
    EXPECT_EQ(*demToolCommandLine(fill), "RASTER FILL SURFACE ground CELL 1 distance=20 strategy=nearest");

    DemToolForm footprint;
    footprint.tool = DemTool::Footprint;
    footprint.sources = {"RASTER 1"};
    footprint.layer = "survey/extent";
    EXPECT_EQ(*demToolCommandLine(footprint), "RASTER FOOTPRINT RASTER 1 TO LAYER survey/extent");

    DemToolForm difference;
    difference.tool = DemTool::Difference;
    difference.sources = {"RASTER 3", "RASTER 1"};
    difference.region = "SELECTION";
    EXPECT_EQ(*demToolCommandLine(difference), "RASTER DIFFERENCE RASTER 3 RASTER 1 SELECTION");
}

TEST(DemToolCommandLine, WhatALineCannotSayIsRefusedNamingTheField)
{
    const auto refused = [](const DemToolForm& form, const char* says) {
        auto line = demToolCommandLine(form);
        ASSERT_FALSE(line.ok()) << says;
        EXPECT_NE(line.error().message.find(says), std::string::npos) << line.error().message;
    };
    DemToolForm none;
    none.tool = DemTool::Fill;
    refused(none, "raster");
    DemToolForm clip;
    clip.tool = DemTool::Clip;
    clip.sources = {"RASTER 1"};
    refused(clip, "clip to");
    DemToolForm distance;
    distance.tool = DemTool::Fill;
    distance.sources = {"RASTER 1"};
    distance.options = {{"distance", "ten"}};
    refused(distance, "distance must be a number");
    DemToolForm crs;
    crs.tool = DemTool::Reproject;
    crs.sources = {"RASTER 1"};
    crs.options = {{"crs", "GDA94 MGA 56"}};
    refused(crs, "one word");
}

TEST(DemToolsDialog, AClipTabBuildsItsLineAndRunHandsTheRunnerExactlyThat)
{
    StubRunner runner;
    DemToolsDialog dialog(runner.context());
    dialog.showTool(DemTool::Clip);
    child<QComboBox>(dialog, "demClipKind")->setCurrentText("File");
    child<QLineEdit>(dialog, "demClipFile")->setText("C:/data/dem.tif");
    child<QLineEdit>(dialog, "demClipArea")->setText("0,0,20,15");
    auto* command = child<QLineEdit>(dialog, "demClipCommand");
    EXPECT_EQ(command->text(), "RASTER CLIP FILE C:/data/dem.tif AREA 0,0,20,15");
    // To the boundaries of a scope instead: the scope's own words.
    child<QRadioButton>(dialog, "demClipByBoundaries")->click();
    child<QRadioButton>(dialog, "demClipBoundaryScopeDrawing")->click();
    EXPECT_EQ(command->text(), "RASTER CLIP FILE C:/data/dem.tif DRAWING");
    child<QPushButton>(dialog, "demClipRun")->click();
    ASSERT_EQ(runner.lines.size(), 1);
    EXPECT_EQ(runner.lines.front(), command->text());
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "demClipReply")->toPlainText(), "ok");
}

TEST(DemToolsDialog, TheMosaicsTilesAreAddedFromItsPickerOrThePickerIsTheOne)
{
    StubRunner runner;
    DemToolsDialog dialog(runner.context());
    child<QComboBox>(dialog, "demMosaicKind")->setCurrentText("File");
    auto* file = child<QLineEdit>(dialog, "demMosaicFile");
    auto* command = child<QLineEdit>(dialog, "demMosaicCommand");
    // None listed: the folder picked is the mosaic's source.
    file->setText("C:/survey/tiles");
    EXPECT_EQ(command->text(), "RASTER MOSAIC FILE C:/survey/tiles");
    file->setText("C:/survey/a.tif");
    child<QPushButton>(dialog, "demMosaicAdd")->click();
    file->setText("C:/survey/b tile.tif");
    child<QPushButton>(dialog, "demMosaicAdd")->click();
    EXPECT_EQ(child<QListWidget>(dialog, "demMosaicTiles")->count(), 2);
    EXPECT_EQ(command->text(),
              "RASTER MOSAIC FILE C:/survey/a.tif FILE \"C:/survey/b tile.tif\"");
    child<QLineEdit>(dialog, "demMosaicSave")->setText("C:/survey/site.tif");
    EXPECT_TRUE(command->text().endsWith(" SAVE C:/survey/site.tif"));
}

TEST(DemToolsDialog, AReferenceRasterIsPickedByItsId)
{
    katana::interop::ReferenceData reference;
    katana::interop::RasterOverlay dem;
    dem.name = "ground";
    dem.source = "C:/data/ground.tif";
    const auto id = reference.add(std::move(dem));
    StubRunner runner;
    GeoDialogContext context = runner.context();
    context.reference = &reference;
    DemToolsDialog dialog(context);
    child<QComboBox>(dialog, "demFillKind")->setCurrentText("Reference raster");
    EXPECT_EQ(child<QComboBox>(dialog, "demFillRaster")->currentText(),
              QString("%1  ground").arg(id));
    child<QLineEdit>(dialog, "demFillDistance")->setText("20");
    EXPECT_EQ(child<QLineEdit>(dialog, "demFillCommand")->text(),
              QString("RASTER FILL RASTER %1 distance=20").arg(id));
    // The rasters REPROJECT can align to are the same ones; choosing one
    // takes the place of a system.
    child<QComboBox>(dialog, "demReprojectKind")->setCurrentText("File");
    child<QLineEdit>(dialog, "demReprojectFile")->setText("C:/data/new.tif");
    child<QLineEdit>(dialog, "demReprojectCrs")->setText("EPSG:28356");
    child<QComboBox>(dialog, "demReprojectLike")->setCurrentIndex(1);
    EXPECT_FALSE(child<QLineEdit>(dialog, "demReprojectCrs")->isEnabled());
    EXPECT_EQ(child<QLineEdit>(dialog, "demReprojectCommand")->text(),
              QString("RASTER REPROJECT FILE C:/data/new.tif like=%1").arg(id));
}

TEST(DemToolsDialog, ADifferenceIsTheFirstMinusTheSecondWithinAnOptionalScope)
{
    StubRunner runner;
    DemToolsDialog dialog(runner.context());
    child<QComboBox>(dialog, "demDifferenceKind")->setCurrentText("File");
    child<QLineEdit>(dialog, "demDifferenceFile")->setText("C:/data/design.tif");
    child<QComboBox>(dialog, "demDifferenceSecondKind")->setCurrentText("File");
    child<QLineEdit>(dialog, "demDifferenceSecondFile")->setText("C:/data/ground.tif");
    auto* command = child<QLineEdit>(dialog, "demDifferenceCommand");
    EXPECT_EQ(command->text(), "RASTER DIFFERENCE FILE C:/data/design.tif FILE C:/data/ground.tif");
    // The scope is offered only when asked for.
    EXPECT_FALSE(child<QRadioButton>(dialog, "demDifferenceWithinScopeDrawing")->isEnabled());
    child<QCheckBox>(dialog, "demDifferenceLimit")->setChecked(true);
    child<QRadioButton>(dialog, "demDifferenceWithinScopeDrawing")->click();
    EXPECT_EQ(command->text(),
              "RASTER DIFFERENCE FILE C:/data/design.tif FILE C:/data/ground.tif DRAWING");
}

TEST(DemToolsDialog, AHeadlessBrowseSaysToFillTheField)
{
    StubRunner runner;
    DemToolsDialog dialog(runner.context());
    child<QComboBox>(dialog, "demFootprintKind")->setCurrentText("File");
    child<QPushButton>(dialog, "demFootprintBrowse")->click();
    EXPECT_TRUE(child<QLabel>(dialog, "demFootprintStatus")->text().contains("demFootprintFile"));
    EXPECT_TRUE(runner.lines.isEmpty());
}

TEST(DemToolsDialog, RunningAClipThroughTheWindowsExecutorKeepsAReferenceRaster)
{
    std::filesystem::path scratch = std::filesystem::temp_directory_path() / "katana-dem-dialog-test";
    std::error_code error;
    std::filesystem::remove_all(scratch, error);
    {
        katana::cad::Document document;
        katana::cad::CommandInterpreter interpreter{document};
        katana::interop::ReferenceData reference;
        katana::terrain::SurfaceStore surfaces;
        QStringList logged;
        QMainWindow main;
        std::unique_ptr<katana::qt::GeoWorkbench> workbench;
        katana::qt::GeoServices services;
        services.document = &document;
        services.interpreter = &interpreter;
        services.reference = &reference;
        services.surfaces = &surfaces;
        services.scratch = scratch;
        services.log = [&logged](const QString& text, bool) { logged << text; };
        services.headless = [] { return true; };
        services.run = [&](const QString& line) {
            logged.clear();
            VerbOutcome outcome;
            outcome.ok = workbench->runLine(line);
            outcome.reply = logged.join('\n');
            outcome.ok = outcome.ok && !outcome.reply.startsWith("error:");
            if (!outcome.ok) {
                outcome.error = outcome.reply;
            }
            return outcome;
        };
        workbench = std::make_unique<katana::qt::GeoWorkbench>(main, std::move(services));
        DemToolsDialog dialog(katana::qt::geoDialogContext(*workbench));
        dialog.showTool(DemTool::Clip);
        child<QComboBox>(dialog, "demClipKind")->setCurrentText("File");
        child<QLineEdit>(dialog, "demClipFile")->setText(kPlane);
        child<QLineEdit>(dialog, "demClipArea")->setText("0,0,20,15");
        child<QLineEdit>(dialog, "demClipName")->setText("west");
        child<QPushButton>(dialog, "demClipRun")->click();
        const QString reply = child<QPlainTextEdit>(dialog, "demClipReply")->toPlainText();
        // 0..20 x 0..15 of 1 m cells: 20 x 15, by hand.
        EXPECT_TRUE(reply.contains("name=west raster=20x15")) << reply.toStdString();
        ASSERT_EQ(reference.rasters().size(), 1u);
        // What the job made is offered at once to the next tool.
        EXPECT_GE(child<QComboBox>(dialog, "demFillRaster")->findText("1  west"), 0);
    }
    std::filesystem::remove_all(scratch, error);
}

} // namespace
