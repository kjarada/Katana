// GIS > Analysis - GDAL > Overlay... (src/katana_qt/geo/overlay_dialog.hpp):
// the line its two scopes, or its scope and a file, make, and a run through
// the window's executor. The verb itself is tested in
// tests/geo/test_overlay_verb.cpp.

#include <gtest/gtest.h>

#include <QComboBox>
#include <QLineEdit>
#include <QListWidget>
#include <QMainWindow>
#include <QPushButton>
#include <QRadioButton>
#include <QStringList>

#include <filesystem>
#include <memory>
#include <variant>

#include "geo/geo_workbench.hpp"
#include "geo/overlay_dialog.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::GisDialogContext;
using katana::qt::GisOverlayDialog;
using katana::qt::GisOverlayForm;
using katana::qt::VerbOutcome;

TEST(GisOverlayLine, TwoScopesAreJoinedByWith)
{
    GisOverlayForm form;
    form.subject.words = "LAYERS lots";
    form.with.words = "LAYERS corridor";
    EXPECT_EQ(*katana::qt::gisOverlayLine(form),
              "GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor");
    form.operation = "difference";
    form.keep = "owner";
    form.keepWith = "none";
    form.layer = "net area";
    form.csv = "C:/out/rows.csv";
    form.overwrite = true;
    EXPECT_EQ(*katana::qt::gisOverlayLine(form),
              "GIS OVERLAY difference LAYERS lots WITH LAYERS corridor keep=owner keepwith=none "
              "csv=C:/out/rows.csv TO LAYER \"net area\" OVERWRITE");
}

TEST(GisOverlayLine, AFileTakesItsLayerAndWhere)
{
    GisOverlayForm form;
    form.subject.words = "DRAWING";
    form.withFile = true;
    EXPECT_FALSE(katana::qt::gisOverlayLine(form).ok()); // no file yet
    form.file = "C:/data/zones.gpkg";
    form.fileLayer = "flood";
    form.fileWhere = "depth > 0.5";
    EXPECT_EQ(*katana::qt::gisOverlayLine(form),
              "GIS OVERLAY intersection DRAWING WITH FILE C:/data/zones.gpkg LAYER flood "
              "\"where=depth > 0.5\"");
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
        services.scratch = std::filesystem::temp_directory_path() / "katana-gis-overlay-dialog-test";
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

void tick(QListWidget* list, const QString& layer)
{
    ASSERT_NE(list, nullptr);
    const auto items = list->findItems(layer, Qt::MatchExactly);
    ASSERT_EQ(items.size(), 1) << layer.toStdString();
    items.front()->setCheckState(Qt::Checked);
}

TEST(GisOverlayDialog, EveryFieldHasItsObjectNameAndTheTwoScopesMakeTheLine)
{
    // Two 50 x 40 lots and a 4 m corridor across both: 200 m2 on each.
    Window window;
    for (const char* line : {"LAYER NEW lots", "LAYER SET lots", "RECT 0,0 50,40",
                             "RECT 50,0 100,40", "LAYER NEW corridor", "LAYER SET corridor",
                             "RECT -10,18 110,22"}) {
        ASSERT_TRUE(window.interpreter.run(line).ok()) << line;
    }
    GisOverlayDialog dialog(window.context());
    for (const char* name :
         {"gisOverlayScope", "gisOverlayWithScope", "gisOverlayOperation",
          "gisOverlayWithSourceDrawing", "gisOverlayWithSourceFile", "gisOverlayWithFile",
          "gisOverlayWithFileBrowse", "gisOverlayWithFileLayer", "gisOverlayWithFileWhere",
          "gisOverlayKeep", "gisOverlayKeepWith", "gisOverlayLayer", "gisOverlayCsv",
          "gisOverlayOverwrite", "gisOverlayCommand", "gisOverlayPreview", "gisOverlayRun",
          "gisOverlayReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    dialog.reload();
    dialog.findChild<QRadioButton*>("gisOverlayScopeLayers")->click();
    tick(dialog.findChild<QListWidget*>("gisOverlayLayers"), "lots");
    tick(dialog.findChild<QListWidget*>("gisOverlayWithLayers"), "corridor");
    EXPECT_EQ(dialog.findChild<QLineEdit*>("gisOverlayCommand")->text(),
              "GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor");
    dialog.run();
    EXPECT_TRUE(dialog.replyText().contains("overlay operation=intersection features=2 area=400.000"))
        << dialog.replyText().toStdString();
    // A file instead: the second scope is set aside.
    dialog.findChild<QRadioButton*>("gisOverlayWithSourceFile")->click();
    EXPECT_FALSE(dialog.withControls().isEnabled());
    dialog.findChild<QLineEdit*>("gisOverlayWithFile")->setText("zones.gpkg");
    EXPECT_EQ(dialog.findChild<QLineEdit*>("gisOverlayCommand")->text(),
              "GIS OVERLAY intersection LAYERS lots WITH FILE zones.gpkg");
}

} // namespace
