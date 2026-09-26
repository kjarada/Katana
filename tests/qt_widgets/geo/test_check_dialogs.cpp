// GIS > Check - GDAL > Check Geometry..., Repair Geometry... and Gaps and
// Overlaps... (src/katana_qt/geo/geometry_check_dialog.hpp,
// coverage_dialog.hpp): the line each builds, the problems table a check's
// reply fills, and a run through the window's executor. The verbs themselves
// are tested in tests/geo/test_check_verbs.cpp.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLineEdit>
#include <QMainWindow>
#include <QPushButton>
#include <QRadioButton>
#include <QStringList>
#include <QTableWidget>

#include <filesystem>
#include <memory>
#include <variant>

#include "geo/coverage_dialog.hpp"
#include "geo/geo_workbench.hpp"
#include "geo/geometry_check_dialog.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::GisCheckDialog;
using katana::qt::GisCoverageDialog;
using katana::qt::GisCoverageForm;
using katana::qt::GisDialogContext;
using katana::qt::GisRepairDialog;
using katana::qt::VerbOutcome;

TEST(GisCheckLines, EachIsSaidAsItsVerbTakesIt)
{
    katana::qt::GisCheckForm check;
    check.scope.words = "LAYERS lots";
    EXPECT_EQ(*katana::qt::gisCheckLine(check), "GIS CHECK LAYERS lots");
    check.markers = "qa/geometry";
    EXPECT_EQ(*katana::qt::gisCheckLine(check), "GIS CHECK LAYERS lots markers=qa/geometry");
    katana::qt::GisRepairForm repair;
    repair.scope.words = "SELECTION";
    EXPECT_EQ(*katana::qt::gisRepairLine(repair), "GIS REPAIR SELECTION");
    repair.method = "structure";
    EXPECT_EQ(*katana::qt::gisRepairLine(repair), "GIS REPAIR SELECTION method=structure");
}

TEST(GisCheckLines, ACleanIsWrittenOnlyWithReplace)
{
    GisCoverageForm form;
    form.scope.words = "DRAWING";
    form.gap = "0.05";
    form.markers = "qa";
    EXPECT_EQ(*katana::qt::gisCoverageLine(form), "GIS COVERAGE CHECK DRAWING gap=0.05 markers=qa");
    form.clean = true;
    form.snap = "0.001";
    const auto refused = katana::qt::gisCoverageLine(form);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("Replace"), std::string::npos);
    form.replace = true;
    form.merge = "max-area";
    EXPECT_EQ(*katana::qt::gisCoverageLine(form),
              "GIS COVERAGE CLEAN DRAWING gap=0.05 snap=0.001 merge=max-area REPLACE");
    form.gap = "narrow";
    EXPECT_FALSE(katana::qt::gisCoverageLine(form).ok());
}

GisDialogContext answering(QStringList& lines, const VerbOutcome& answer)
{
    GisDialogContext context;
    context.run = [&lines, answer](const QString& line) {
        lines << line;
        return answer;
    };
    return context;
}

TEST(GisCheckDialog, EveryFieldHasItsObjectNameAndProblemsFillTheTable)
{
    QStringList lines;
    VerbOutcome answer{true,
                       "gis op=check seconds=0.001 cancelled=no\nproblem kind=self-intersection "
                       "entity=7 at=35,5 reason=Self-intersection\ncheck features=2 problems=1 "
                       "entities=1",
                       {}};
    GisCheckDialog dialog(answering(lines, answer));
    for (const char* name : {"gisCheckScope", "gisCheckMarkers", "gisCheckProblems",
                             "gisCheckCommand", "gisCheckPreview", "gisCheckRun", "gisCheckReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    dialog.findChild<QRadioButton*>("gisCheckScopeDrawing")->click();
    dialog.findChild<QLineEdit*>("gisCheckMarkers")->setText("qa");
    dialog.findChild<QPushButton*>("gisCheckRun")->click();
    ASSERT_EQ(lines.size(), 1);
    EXPECT_EQ(lines.front(), "GIS CHECK DRAWING markers=qa");
    auto* table = dialog.findChild<QTableWidget*>("gisCheckProblems");
    ASSERT_EQ(table->rowCount(), 1);
    EXPECT_EQ(table->item(0, 0)->text(), "self-intersection");
    EXPECT_EQ(table->item(0, 1)->text(), "7");
    EXPECT_EQ(table->item(0, 2)->text(), "35,5");
    EXPECT_TRUE(dialog.statusText().startsWith("1 problems on 1 entities"))
        << dialog.statusText().toStdString();
}

TEST(GisCoverageDialog, TheModeChoosesWhichFieldsApply)
{
    QStringList lines;
    GisCoverageDialog dialog(answering(lines, VerbOutcome{true, "", {}}));
    for (const char* name : {"gisCoverageScope", "gisCoverageMode", "gisCoverageGap",
                             "gisCoverageSnap", "gisCoverageMerge", "gisCoverageReplace",
                             "gisCoverageMarkers", "gisCoverageProblems", "gisCoverageRun"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    auto* snap = dialog.findChild<QLineEdit*>("gisCoverageSnap");
    auto* markers = dialog.findChild<QLineEdit*>("gisCoverageMarkers");
    EXPECT_FALSE(snap->isEnabled());
    EXPECT_TRUE(markers->isEnabled());
    dialog.findChild<QRadioButton*>("gisCoverageScopeDrawing")->click();
    dialog.findChild<QComboBox*>("gisCoverageMode")->setCurrentText("clean");
    EXPECT_TRUE(snap->isEnabled());
    EXPECT_FALSE(markers->isEnabled());
    // A clean without Replace ticked is no line at all: Run is off.
    EXPECT_FALSE(dialog.findChild<QPushButton*>("gisCoverageRun")->isEnabled());
    dialog.findChild<QCheckBox*>("gisCoverageReplace")->setChecked(true);
    EXPECT_EQ(dialog.findChild<QLineEdit*>("gisCoverageCommand")->text(),
              "GIS COVERAGE CLEAN DRAWING REPLACE");
}

// The window's side, with the runner that captures what a line logged.
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
        services.scratch = std::filesystem::temp_directory_path() / "katana-gis-check-dialog-test";
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

TEST(GisCheckDialog, ABowTieIsFoundThenRepairedThroughTheExecutor)
{
    // (30,0) (40,10) (40,0) (30,10) crosses itself at (35,5); repaired, it is
    // two triangles of 25 m2.
    Window window;
    ASSERT_TRUE(window.interpreter.run("PLINE 30,0 40,10 40,0 30,10 CLOSE").ok());
    GisCheckDialog check(window.context());
    check.reload();
    check.findChild<QRadioButton*>("gisCheckScopeDrawing")->click();
    check.run();
    auto* table = check.findChild<QTableWidget*>("gisCheckProblems");
    ASSERT_EQ(table->rowCount(), 1) << check.replyText().toStdString();
    EXPECT_EQ(table->item(0, 2)->text(), "35,5");
    GisRepairDialog repair(window.context());
    repair.reload();
    repair.findChild<QRadioButton*>("gisRepairScopeDrawing")->click();
    repair.run();
    EXPECT_TRUE(repair.replyText().contains("split entity=1 parts=2")) << repair.replyText().toStdString();
    double area = 0.0;
    window.document.model().entities.forEach([&](const katana::entity::Entity& entity) {
        area += std::get<katana::geometry::Polyline2>(entity.geometry).area();
    });
    EXPECT_NEAR(area, 50.0, 1e-9);
    check.run();
    EXPECT_EQ(table->rowCount(), 0);
}

} // namespace
