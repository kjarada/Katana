// GIS > Analysis - GDAL > Boundary Around Features... and Clip to Boundary...
// (src/katana_qt/geo/hull_dialog.hpp, clip_dialog.hpp): the line each
// builds, and a clip through the window's executor. The verbs themselves are
// tested in tests/geo/test_hull_and_clip.cpp.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLineEdit>
#include <QListWidget>
#include <QMainWindow>
#include <QRadioButton>
#include <QStringList>

#include <filesystem>
#include <memory>
#include <variant>

#include "geo/clip_dialog.hpp"
#include "geo/geo_workbench.hpp"
#include "geo/hull_dialog.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::GisClipDialog;
using katana::qt::GisClipForm;
using katana::qt::GisDialogContext;
using katana::qt::GisHullDialog;
using katana::qt::GisHullForm;
using katana::qt::VerbOutcome;

TEST(GisHullLine, ConvexIsTheDefaultAndConcaveTakesItsRatio)
{
    GisHullForm form;
    form.scope.words = "LAYERS survey";
    EXPECT_EQ(*katana::qt::gisHullLine(form), "GIS HULL LAYERS survey");
    form.concave = true;
    form.ratio = "0.1";
    form.holes = true;
    form.layer = "extent";
    EXPECT_EQ(*katana::qt::gisHullLine(form),
              "GIS HULL LAYERS survey concave=0.1 holes TO LAYER extent");
    form.ratio = "1.5";
    EXPECT_FALSE(katana::qt::gisHullLine(form).ok());
}

TEST(GisClipLine, ABoundaryIsAScopeOrAFileAndReplaceDropsTheLayer)
{
    GisClipForm form;
    form.scope.words = "LAYERS pipes";
    form.by.words = "LAYERS site";
    EXPECT_EQ(*katana::qt::gisClipLine(form), "GIS CLIP LAYERS pipes BY LAYERS site");
    form.layer = "cut";
    form.replace = true;
    EXPECT_EQ(*katana::qt::gisClipLine(form), "GIS CLIP LAYERS pipes BY LAYERS site REPLACE");
    form.byFile = true;
    form.replace = false;
    EXPECT_FALSE(katana::qt::gisClipLine(form).ok()); // no file yet
    form.file = "C:/data/site.gpkg";
    form.fileWhere = "kind = 'site'";
    EXPECT_EQ(*katana::qt::gisClipLine(form),
              "GIS CLIP LAYERS pipes BY FILE C:/data/site.gpkg \"where=kind = 'site'\" TO LAYER cut");
}

TEST(GisHullDialog, EveryFieldHasItsObjectNameAndTheKindChoosesTheRatio)
{
    GisDialogContext context;
    context.run = [](const QString&) { return VerbOutcome{}; };
    GisHullDialog dialog(context);
    for (const char* name : {"gisHullScope", "gisHullKind", "gisHullRatio", "gisHullHoles",
                             "gisHullLayer", "gisHullCommand", "gisHullRun", "gisHullReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    auto* ratio = dialog.findChild<QLineEdit*>("gisHullRatio");
    EXPECT_FALSE(ratio->isEnabled());
    dialog.findChild<QRadioButton*>("gisHullScopeDrawing")->click();
    dialog.findChild<QComboBox*>("gisHullKind")->setCurrentText("concave");
    EXPECT_TRUE(ratio->isEnabled());
    EXPECT_EQ(dialog.findChild<QLineEdit*>("gisHullCommand")->text(),
              "GIS HULL DRAWING concave=0.1");
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
        services.scratch = std::filesystem::temp_directory_path() / "katana-gis-clip-dialog-test";
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

TEST(GisClipDialog, ItsLineCutsAPipeInPlaceThroughTheExecutor)
{
    // A 100 m pipe through a site from x = 10 to 40: 30 m of it stays, on
    // the pipe's own id.
    Window window;
    for (const char* line : {"LAYER NEW pipes", "LAYER SET pipes", "LINE 0,0 100,0",
                             "LAYER NEW site", "LAYER SET site", "RECT 10,-5 40,5"}) {
        ASSERT_TRUE(window.interpreter.run(line).ok()) << line;
    }
    GisClipDialog dialog(window.context());
    for (const char* name :
         {"gisClipScope", "gisClipByScope", "gisClipBySourceDrawing", "gisClipBySourceFile",
          "gisClipByFile", "gisClipByFileBrowse", "gisClipByFileLayer", "gisClipByFileWhere",
          "gisClipReplace", "gisClipLayer", "gisClipCommand", "gisClipRun", "gisClipReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    dialog.reload();
    dialog.findChild<QRadioButton*>("gisClipScopeLayers")->click();
    for (const auto& [list, layer] : {std::pair{"gisClipLayers", "pipes"},
                                      std::pair{"gisClipByLayers", "site"}}) {
        auto items = dialog.findChild<QListWidget*>(list)->findItems(layer, Qt::MatchExactly);
        ASSERT_EQ(items.size(), 1) << list;
        items.front()->setCheckState(Qt::Checked);
    }
    dialog.findChild<QCheckBox*>("gisClipReplace")->setChecked(true);
    EXPECT_FALSE(dialog.findChild<QLineEdit*>("gisClipLayer")->isEnabled());
    EXPECT_EQ(dialog.findChild<QLineEdit*>("gisClipCommand")->text(),
              "GIS CLIP LAYERS pipes BY LAYERS site REPLACE");
    dialog.run();
    EXPECT_TRUE(dialog.replyText().contains("target=in-place created=0 updated=1"))
        << dialog.replyText().toStdString();
    const auto* pipe = window.document.model().entities.find(1);
    ASSERT_NE(pipe, nullptr);
    EXPECT_NEAR(std::get<katana::geometry::Segment2>(pipe->geometry).length(), 30.0, 1e-9);
}

} // namespace
