// Terrain > DEM > Grid Points to DEM (src/katana_qt/geo/grid_dem_dialog.hpp):
// the line its fields describe, driven by object names as a person clicks
// them, handed to the executor exactly as shown - a stub runner's, and the
// window's own workbench's, which grids the points into a reference raster.

#include <gtest/gtest.h>

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QStringList>

#include <cmath>
#include <filesystem>
#include <memory>
#include <optional>
#include <system_error>
#include <vector>

#include "geo/geo_dialog_support.hpp"
#include "geo/geo_workbench.hpp"
#include "geo/grid_dem_dialog.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::GeoDialogContext;
using katana::qt::GridDemDialog;
using katana::qt::GridDemForm;
using katana::qt::gridDemCommandLine;
using katana::qt::JobId;
using katana::qt::VerbOutcome;

template <class T>
T* child(QWidget& dialog, const char* name)
{
    auto* found = dialog.findChild<T*>(name);
    EXPECT_NE(found, nullptr) << name;
    return found;
}

// A runner that keeps the lines it was handed and answers what it is told.
struct StubRunner {
    QStringList lines;
    VerbOutcome answer{true, "grid method=linear", {}};
    GeoDialogContext context()
    {
        GeoDialogContext made;
        made.run = [this](const QString& line) {
            lines << line;
            return answer;
        };
        made.headless = [] { return true; };
        return made;
    }
};

TEST(GridDemCommandLine, ABlankFieldIsLeftOutAndTheVerbsDefaultApplies)
{
    GridDemForm form;
    form.scope = "DRAWING";
    auto line = gridDemCommandLine(form);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "RASTER GRID DRAWING");
    form.z = "geometry";
    EXPECT_EQ(*gridDemCommandLine(form), "RASTER GRID DRAWING");
}

TEST(GridDemCommandLine, EveryFieldGivenIsItsWordOnTheLine)
{
    GridDemForm form;
    form.scope = "LAYERS spots WHERE TYPE=point";
    form.method = "invdist";
    form.cell = "2.5";
    form.z = "rl";
    form.extent = "0,0,100,80";
    form.power = "2";
    form.radius = "8";
    form.name = "ground model";
    auto line = gridDemCommandLine(form);
    ASSERT_TRUE(line.ok()) << line.error().describe();
    EXPECT_EQ(*line, "RASTER GRID LAYERS spots WHERE TYPE=point method=invdist cell=2.5 z=rl "
                     "extent=0,0,100,80 power=2 radius=8 NAME \"ground model\"");
}

TEST(GridDemCommandLine, WhatALineCannotSayIsRefusedNamingTheField)
{
    const auto refused = [](GridDemForm form, const char* says) {
        auto line = gridDemCommandLine(form);
        ASSERT_FALSE(line.ok()) << says;
        EXPECT_NE(line.error().message.find(says), std::string::npos) << line.error().message;
    };
    GridDemForm form;
    form.scope = "DRAWING";
    GridDemForm comma = form;
    comma.cell = "2,5"; // a comma is no decimal point on the command line
    refused(comma, "cell size");
    GridDemForm size = form;
    size.size = "400 by 300";
    refused(size, "size");
    GridDemForm extent = form;
    extent.extent = "0,0,100";
    refused(extent, "extent");
    GridDemForm z = form;
    z.z = "reduced level";
    refused(z, "height property");
    GridDemForm quote = form;
    quote.name = "the \"best\" dem";
    refused(quote, "double quote");
    GridDemForm noScope;
    noScope.scopeError = "tick a layer";
    refused(noScope, "tick a layer");
}

TEST(GridDemDialog, TheFieldsBuildTheLineRunHandsTheRunner)
{
    StubRunner runner;
    GridDemDialog dialog(runner.context());
    child<QRadioButton>(dialog, "gridScopeDrawing")->click();
    child<QLineEdit>(dialog, "gridCell")->setText("5");
    child<QLineEdit>(dialog, "gridName")->setText("ground");
    auto* command = child<QLineEdit>(dialog, "gridCommand");
    EXPECT_EQ(command->text(), "RASTER GRID DRAWING method=linear cell=5 NAME ground");
    // A cell or a size: the size is not offered while a cell is given.
    EXPECT_FALSE(child<QLineEdit>(dialog, "gridSize")->isEnabled());

    child<QPushButton>(dialog, "gridRun")->click();
    ASSERT_EQ(runner.lines.size(), 1);
    EXPECT_EQ(runner.lines.front(), command->text());
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "gridReply")->toPlainText(), "grid method=linear");

    child<QPushButton>(dialog, "gridPreview")->click();
    ASSERT_EQ(runner.lines.size(), 2);
    EXPECT_EQ(runner.lines.back(), command->text() + " PREVIEW");
}

TEST(GridDemDialog, KeepAsASurfaceWritesToSurfaceWithTheName)
{
    // TO SURFACE works on every front end (RASTER GRID ... TO SURFACE g), so
    // the window offers it: the box was disabled, and a line with it could
    // not be built there.
    StubRunner runner;
    GridDemDialog dialog(runner.context());
    child<QRadioButton>(dialog, "gridScopeDrawing")->click();
    child<QLineEdit>(dialog, "gridCell")->setText("5");
    auto* keep = child<QCheckBox>(dialog, "gridToSurface");
    ASSERT_TRUE(keep->isEnabled());
    keep->click();
    auto* command = child<QLineEdit>(dialog, "gridCommand");
    EXPECT_EQ(command->text(), "RASTER GRID DRAWING method=linear cell=5 TO SURFACE dem");
    child<QLineEdit>(dialog, "gridName")->setText("ground");
    EXPECT_EQ(command->text(),
              "RASTER GRID DRAWING method=linear cell=5 NAME ground TO SURFACE ground");
}

TEST(GridDemDialog, PowerIsOfferedOnlyToAMethodThatTakesIt)
{
    StubRunner runner;
    GridDemDialog dialog(runner.context());
    auto* method = child<QComboBox>(dialog, "gridMethod");
    // The methods are GDAL's own, from its catalogue.
    EXPECT_GE(method->findText("linear"), 0);
    EXPECT_GE(method->findText("invdist"), 0);
    EXPECT_GE(method->findText("nearest"), 0);
    method->setCurrentText("linear");
    EXPECT_FALSE(child<QLineEdit>(dialog, "gridPower")->isEnabled());
    method->setCurrentText("invdist");
    auto* power = child<QLineEdit>(dialog, "gridPower");
    EXPECT_TRUE(power->isEnabled());
    power->setText("2");
    EXPECT_TRUE(child<QLineEdit>(dialog, "gridCommand")->text().contains("method=invdist power=2"));
    // Back to linear: the power it cannot take leaves the line.
    method->setCurrentText("linear");
    EXPECT_FALSE(child<QLineEdit>(dialog, "gridCommand")->text().contains("power="));
}

TEST(GridDemDialog, AnInteractiveRunShowsItsReplyWhenTheJobEnds)
{
    StubRunner runner;
    runner.answer = VerbOutcome{true, "job id=7 title=\"RASTER GRID\" state=started", {}};
    std::function<void(JobId, const VerbOutcome&)> told;
    GeoDialogContext context = runner.context();
    context.listen = [&told](std::function<void(JobId, const VerbOutcome&)> listener) {
        told = std::move(listener);
        return std::shared_ptr<void>(std::make_shared<int>(0));
    };
    GridDemDialog dialog(context);
    child<QRadioButton>(dialog, "gridScopeDrawing")->click();
    child<QPushButton>(dialog, "gridRun")->click();
    EXPECT_TRUE(dialog.runPanel().running());
    EXPECT_TRUE(child<QLabel>(dialog, "gridStatus")->text().contains("job 7"));
    ASSERT_TRUE(told);
    // Another job's end is not this run's.
    told(6, VerbOutcome{true, "other", {}});
    EXPECT_TRUE(dialog.runPanel().running());
    told(7, VerbOutcome{true, "output arg=output kind=raster target=reference id=1 name=dem", {}});
    EXPECT_FALSE(dialog.runPanel().running());
    EXPECT_TRUE(child<QPlainTextEdit>(dialog, "gridReply")->toPlainText().contains("name=dem"));
}

TEST(GridDemDialog, TheHeightPropertiesOfferedAreTheNumbersTheDrawingHolds)
{
    katana::cad::Document document;
    katana::entity::Entity point;
    point.geometry = katana::entity::PointGeometry{katana::geometry::Point2(0, 0)};
    point.properties["rl"] = 101.5;
    point.properties["code"] = std::string("TREE");
    katana::entity::setHeights(point.properties, {100.0});
    ASSERT_TRUE(document.execute(katana::commands::createEntities({point})).ok());
    StubRunner runner;
    GeoDialogContext context = runner.context();
    context.document = &document;
    GridDemDialog dialog(context);
    auto* z = child<QComboBox>(dialog, "gridZ");
    EXPECT_EQ(z->itemText(0), "geometry");
    EXPECT_GE(z->findText("rl"), 0);
    // A word is no height, and the geometry's own height is "geometry".
    EXPECT_LT(z->findText("code"), 0);
    EXPECT_LT(z->findText("elevation"), 0);
    z->setCurrentText("rl");
    EXPECT_TRUE(child<QLineEdit>(dialog, "gridCommand")->text().contains(" z=rl"));
}

// The window's side, as MainWindow hands it over (test_geo_workbench.cpp's),
// with a scratch folder of its own, emptied before and after: a raster left
// there by an earlier run would take the name the test expects.
struct Window {
    std::filesystem::path scratch =
        std::filesystem::temp_directory_path() / "katana-grid-dialog-test";
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    QStringList logged;
    QMainWindow main;
    std::unique_ptr<katana::qt::GeoWorkbench> workbench;

    Window()
    {
        katana::qt::GeoServices services;
        services.document = &document;
        services.interpreter = &interpreter;
        services.reference = &reference;
        services.surfaces = &surfaces;
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
        services.scratch = scratch;
        services.log = [this](const QString& text, bool) { logged << text; };
        services.headless = [] { return true; };
        // What runVerbLine does for a geoprocessing line: the workbench runs
        // it, and what it logged is the reply.
        services.run = [this](const QString& line) {
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
    }
    ~Window()
    {
        workbench.reset();
        std::error_code error;
        std::filesystem::remove_all(scratch, error);
    }
    Window(const Window&) = delete;
    Window& operator=(const Window&) = delete;
};

TEST(GridDemDialog, RunningItsLineGridsThePointsIntoAReferenceRaster)
{
    // Points every 10 m over 0..40 x 0..30 on z = 100 + x/10 + y/20.
    Window window;
    std::vector<katana::entity::Entity> points;
    for (int x = 0; x <= 40; x += 10) {
        for (int y = 0; y <= 30; y += 10) {
            katana::entity::Entity point;
            point.geometry = katana::entity::PointGeometry{katana::geometry::Point2(x, y)};
            katana::entity::setHeights(point.properties, {100.0 + x / 10.0 + y / 20.0});
            points.push_back(std::move(point));
        }
    }
    ASSERT_TRUE(window.document.execute(katana::commands::createEntities(std::move(points))).ok());
    GridDemDialog dialog(katana::qt::geoDialogContext(*window.workbench));
    child<QRadioButton>(dialog, "gridScopeDrawing")->click();
    child<QLineEdit>(dialog, "gridCell")->setText("5");
    child<QLineEdit>(dialog, "gridName")->setText("ground");
    child<QPushButton>(dialog, "gridRun")->click();
    const QString reply = child<QPlainTextEdit>(dialog, "gridReply")->toPlainText();
    // 0..40 x 0..30 at 5 m: 8 x 6 cells, by hand.
    EXPECT_TRUE(reply.contains("size=8x6")) << reply.toStdString();
    EXPECT_TRUE(reply.contains("scope arg=input scope=drawing matched=20 used=20 points=20"))
        << reply.toStdString();
    EXPECT_TRUE(reply.contains("target=reference id=1 name=ground raster=8x6")) << reply.toStdString();
    ASSERT_EQ(window.reference.rasters().size(), 1u);
    EXPECT_EQ(window.reference.rasters().front().derivation,
              "RASTER GRID DRAWING method=linear cell=5 NAME ground");
}

} // namespace
