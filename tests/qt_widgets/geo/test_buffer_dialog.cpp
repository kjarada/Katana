// GIS > Analysis - GDAL > Buffer... and Dissolve... (src/katana_qt/geo/
// buffer_dialog.hpp, dissolve_dialog.hpp) and the frame they are built on
// (gis_tool_dialog.hpp): the line each builds from its fields, run through
// the window's executor, and its reply shown - headless at once, interactively
// when the job ends. The verbs themselves are tested in tests/geo.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLineEdit>
#include <QMainWindow>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QStringList>

#include <filesystem>
#include <memory>
#include <optional>
#include <variant>

#include "geo/buffer_dialog.hpp"
#include "geo/dissolve_dialog.hpp"
#include "geo/geo_workbench.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::GisBufferDialog;
using katana::qt::GisBufferForm;
using katana::qt::GisDialogContext;
using katana::qt::GisDissolveDialog;
using katana::qt::GisDissolveForm;
using katana::qt::VerbOutcome;

GisBufferForm bufferForm(const QString& distance)
{
    GisBufferForm form;
    form.scope.words = "DRAWING";
    form.distance = distance;
    return form;
}

TEST(GisBufferLine, AChoiceLeftAtItsDefaultIsLeftOut)
{
    auto line = katana::qt::gisBufferLine(bufferForm("5"));
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "GIS BUFFER DRAWING distance=5");
    GisBufferForm form = bufferForm("-3");
    form.side = "left";
    form.caps = "flat";
    form.joins = "mitre";
    form.layer = "design/set backs";
    line = katana::qt::gisBufferLine(form);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "GIS BUFFER DRAWING distance=-3 side=left caps=flat joins=mitre TO LAYER "
                     "\"design/set backs\"");
}

TEST(GisBufferLine, APropertyDistanceWinsAndDissolveNamesItsKeys)
{
    GisBufferForm form = bufferForm("5");
    form.distanceProperty = "clearance";
    form.dissolve = true;
    auto line = katana::qt::gisBufferLine(form);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "GIS BUFFER DRAWING distance=prop:clearance DISSOLVE");
    form.dissolveBy = "owner,stage";
    line = katana::qt::gisBufferLine(form);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "GIS BUFFER DRAWING distance=prop:clearance dissolve=owner,stage");
}

TEST(GisBufferLine, WhatCannotBeSaidIsRefusedNamingTheField)
{
    EXPECT_FALSE(katana::qt::gisBufferLine(bufferForm("")).ok());
    EXPECT_FALSE(katana::qt::gisBufferLine(bufferForm("0")).ok());
    EXPECT_FALSE(katana::qt::gisBufferLine(bufferForm("five")).ok());
    GisBufferForm form = bufferForm("5");
    form.scope.words.clear();
    form.scope.error = "tick at least one layer";
    const auto refused = katana::qt::gisBufferLine(form);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("tick at least one layer"), std::string::npos);
    form = bufferForm("5");
    form.layer = "a\"b";
    EXPECT_FALSE(katana::qt::gisBufferLine(form).ok());
}

TEST(GisDissolveLine, KeepAndReplaceAreSaidAsTheVerbTakesThem)
{
    GisDissolveForm form;
    form.scope.words = "LAYERS lots";
    auto line = katana::qt::gisDissolveLine(form);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "GIS DISSOLVE LAYERS lots");
    form.by = "owner";
    form.keepIdentical = true;
    form.replace = true;
    form.layer = "superlots";
    line = katana::qt::gisDissolveLine(form);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "GIS DISSOLVE LAYERS lots by=owner keep=identical TO LAYER superlots REPLACE");
}

// A runner that answers as the window does interactively: the line becomes a
// job, and its reply comes later.
struct FakeRunner {
    QStringList lines;
    VerbOutcome answer;
    GisDialogContext context()
    {
        GisDialogContext made;
        made.run = [this](const QString& line) {
            lines << line;
            return answer;
        };
        return made;
    }
};

TEST(GisBufferDialog, EveryFieldHasItsObjectName)
{
    FakeRunner runner;
    GisBufferDialog dialog(runner.context());
    EXPECT_EQ(dialog.objectName(), "gisBufferDialog");
    for (const char* name :
         {"gisBufferScope", "gisBufferScopeDrawing", "gisBufferFilterLayer", "gisBufferDistance",
          "gisBufferDistanceProperty", "gisBufferSide", "gisBufferCaps", "gisBufferJoins",
          "gisBufferDissolve", "gisBufferDissolveBy", "gisBufferLayer", "gisBufferCommand",
          "gisBufferPreview", "gisBufferRun", "gisBufferStatus", "gisBufferReply",
          "gisBufferClose"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
}

TEST(GisBufferDialog, RunHandsTheLineItShowsToTheExecutor)
{
    FakeRunner runner;
    runner.answer.ok = true;
    runner.answer.reply = "gis op=buffer seconds=0.001 cancelled=no\noutput arg=output "
                          "kind=vector target=layer layer=gis/buffer created=2 updated=0 "
                          "deleted=0 skipped=0";
    GisBufferDialog dialog(runner.context());
    dialog.findChild<QRadioButton*>("gisBufferScopeDrawing")->click();
    auto* run = dialog.findChild<QPushButton*>("gisBufferRun");
    EXPECT_FALSE(run->isEnabled()); // no distance yet: nothing to run
    dialog.findChild<QLineEdit*>("gisBufferDistance")->setText("1");
    dialog.findChild<QComboBox*>("gisBufferCaps")->setCurrentText("flat");
    EXPECT_EQ(dialog.findChild<QLineEdit*>("gisBufferCommand")->text(),
              "GIS BUFFER DRAWING distance=1 caps=flat");
    ASSERT_TRUE(run->isEnabled());
    run->click();
    ASSERT_EQ(runner.lines.size(), 1);
    EXPECT_EQ(runner.lines.front(), "GIS BUFFER DRAWING distance=1 caps=flat");
    EXPECT_TRUE(dialog.replyText().contains("created=2"));
    EXPECT_TRUE(dialog.statusText().contains("one undo step")) << dialog.statusText().toStdString();
    dialog.findChild<QPushButton*>("gisBufferPreview")->click();
    EXPECT_EQ(runner.lines.back(), "GIS BUFFER DRAWING distance=1 caps=flat PREVIEW");
}

TEST(GisBufferDialog, AnInteractiveRunShowsItsJobsReplyWhenItEnds)
{
    FakeRunner runner;
    runner.answer.ok = true;
    runner.answer.reply = "job id=7 title=\"GIS BUFFER\" state=started";
    GisBufferDialog dialog(runner.context());
    dialog.findChild<QRadioButton*>("gisBufferScopeDrawing")->click();
    dialog.findChild<QLineEdit*>("gisBufferDistance")->setText("1");
    dialog.run();
    EXPECT_TRUE(dialog.statusText().contains("background job"));
    VerbOutcome other{true, "not this one", {}};
    dialog.jobFinished(8, other);
    EXPECT_FALSE(dialog.replyText().contains("not this one"));
    VerbOutcome ended{true, "gis op=buffer seconds=0.002 cancelled=no\nbuffer area=200.000", {}};
    dialog.jobFinished(7, ended);
    EXPECT_TRUE(dialog.replyText().contains("area=200.000"));
    VerbOutcome failed{false, {}, "error: InvalidState: cancelled"};
    dialog.run();
    dialog.jobFinished(7, failed);
    EXPECT_TRUE(dialog.statusText().contains("cancelled"));
}

// The window's side, as MainWindow hands it to the workbench, with the
// runner that captures what a line logged the way runVerbLine does.
struct Window {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    QStringList logged;
    QStringList errors;
    QMainWindow main;
    std::unique_ptr<katana::qt::GeoWorkbench> workbench;

    Window()
    {
        katana::qt::GeoServices services;
        services.document = &document;
        services.interpreter = &interpreter;
        services.reference = &reference;
        services.surfaces = &surfaces;
        services.scratch = std::filesystem::temp_directory_path() / "katana-gis-dialog-test";
        services.log = [this](const QString& text, bool isError) {
            (isError ? errors : logged) << text;
        };
        services.headless = [] { return true; };
        workbench = std::make_unique<katana::qt::GeoWorkbench>(main, std::move(services));
    }

    GisDialogContext context()
    {
        GisDialogContext made;
        made.document = &document;
        made.run = [this](const QString& line) {
            logged.clear();
            errors.clear();
            if (!workbench->runLine(line)) {
                return VerbOutcome{false, {}, "not a geoprocessing line"};
            }
            return VerbOutcome{errors.isEmpty(), logged.join('\n'), errors.join('\n')};
        };
        return made;
    }
};

TEST(GisBufferDialog, ItsLineThroughTheExecutorDrawsTheBufferAsOneStep)
{
    // A 100 m line, 1 m either side, flat ends: 200 m2, one undo step.
    Window window;
    ASSERT_TRUE(window.interpreter.run("LINE 0,0 100,0").ok());
    GisBufferDialog dialog(window.context());
    dialog.reload();
    dialog.findChild<QRadioButton*>("gisBufferScopeDrawing")->click();
    dialog.findChild<QLineEdit*>("gisBufferDistance")->setText("1");
    dialog.findChild<QComboBox*>("gisBufferCaps")->setCurrentText("flat");
    dialog.findChild<QLineEdit*>("gisBufferLayer")->setText("easement");
    dialog.run();
    EXPECT_TRUE(dialog.replyText().contains("area=200.000")) << dialog.replyText().toStdString();
    double area = 0.0;
    window.document.model().entities.forEach([&](const katana::entity::Entity& entity) {
        if (entity.layer == "easement") {
            area += std::get<katana::geometry::Polyline2>(entity.geometry).area();
        }
    });
    EXPECT_NEAR(area, 200.0, 1e-9);
    ASSERT_TRUE(window.interpreter.run("UNDO").ok());
    EXPECT_EQ(window.document.model().entities.size(), 1u);
}

TEST(GisDissolveDialog, EveryFieldHasItsObjectNameAndTheLineFollowsThem)
{
    FakeRunner runner;
    GisDissolveDialog dialog(runner.context());
    EXPECT_EQ(dialog.objectName(), "gisDissolveDialog");
    for (const char* name : {"gisDissolveScope", "gisDissolveBy", "gisDissolveKeepIdentical",
                             "gisDissolveReplace", "gisDissolveLayer", "gisDissolveCommand",
                             "gisDissolvePreview", "gisDissolveRun", "gisDissolveReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    dialog.findChild<QRadioButton*>("gisDissolveScopeDrawing")->click();
    dialog.findChild<QLineEdit*>("gisDissolveBy")->setText("owner");
    dialog.findChild<QCheckBox*>("gisDissolveReplace")->setChecked(true);
    EXPECT_EQ(dialog.findChild<QLineEdit*>("gisDissolveCommand")->text(),
              "GIS DISSOLVE DRAWING by=owner REPLACE");
}

} // namespace
