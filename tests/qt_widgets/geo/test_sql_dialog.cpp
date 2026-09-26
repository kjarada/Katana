// GIS > Analysis - GDAL > Query with SQL... (src/katana_qt/geo/sql_dialog.hpp):
// the line its statement makes, the tables it lists for the scope, and the
// rows of a run through the window's executor in its grid. The verb itself
// is tested in tests/geo/test_sql_verb.cpp.

#include <gtest/gtest.h>

#include <QComboBox>
#include <QLineEdit>
#include <QMainWindow>
#include <QPlainTextEdit>
#include <QRadioButton>
#include <QStringList>
#include <QTableWidget>
#include <QTreeWidget>

#include <filesystem>
#include <memory>

#include "geo/geo_workbench.hpp"
#include "geo/sql_dialog.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::GisDialogContext;
using katana::qt::GisSqlDialog;
using katana::qt::GisSqlForm;
using katana::qt::VerbOutcome;

TEST(GisSqlLine, TheStatementIsOneWordOfTheLine)
{
    GisSqlForm form;
    form.scope.words = "DRAWING";
    form.sql = "SELECT owner,\n  SUM(ST_Area(geometry)) AS area\nFROM polygons GROUP BY owner";
    EXPECT_EQ(*katana::qt::gisSqlLine(form),
              "GIS SQL \"SELECT owner,   SUM(ST_Area(geometry)) AS area FROM polygons GROUP BY "
              "owner\" DRAWING");
    // SQLite reads [gis.source] as "gis.source", and a line can carry it.
    form.sql = "SELECT \"gis.source\" FROM polygons WHERE owner = 'Smith'";
    form.as = "select";
    EXPECT_EQ(*katana::qt::gisSqlLine(form),
              "GIS SQL \"SELECT [gis.source] FROM polygons WHERE owner = 'Smith'\" DRAWING AS "
              "SELECT");
    form.as = "layer";
    EXPECT_FALSE(katana::qt::gisSqlLine(form).ok()); // no layer named
    form.layer = "results";
    form.dialect = "ogrsql";
    EXPECT_FALSE(katana::qt::gisSqlLine(form).ok()); // OGR SQL cannot take the brackets
    form.sql = "SELECT * FROM polygons";
    EXPECT_EQ(*katana::qt::gisSqlLine(form),
              "GIS SQL \"SELECT * FROM polygons\" DRAWING dialect=ogrsql AS LAYER results");
    form.sql = "  ";
    EXPECT_FALSE(katana::qt::gisSqlLine(form).ok());
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
        services.scratch = std::filesystem::temp_directory_path() / "katana-gis-sql-dialog-test";
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

TEST(GisSqlDialog, TheTablesAreListedAndTheRowsFillTheGrid)
{
    // Two 50 x 40 lots, one owned by Smith: 2000 m2 each.
    Window window;
    for (const char* line :
         {"RECT 0,0 50,40", "RECT 50,0 100,40", "SELECT 1", "MODIFY SET PROP=owner:Smith"}) {
        ASSERT_TRUE(window.interpreter.run(line).ok()) << line;
    }
    GisSqlDialog dialog(window.context());
    for (const char* name : {"gisSqlScope", "gisSqlTables", "gisSqlTablesRefresh", "gisSqlText",
                             "gisSqlDialect", "gisSqlAs", "gisSqlLayer", "gisSqlCsv",
                             "gisSqlResults", "gisSqlCommand", "gisSqlRun", "gisSqlReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    dialog.findChild<QRadioButton*>("gisSqlScopeDrawing")->click();
    dialog.refreshTables();
    auto* tables = dialog.findChild<QTreeWidget*>("gisSqlTables");
    ASSERT_EQ(tables->topLevelItemCount(), 1);
    QTreeWidgetItem* polygons = tables->topLevelItem(0);
    EXPECT_EQ(polygons->text(0), "polygons");
    QStringList columns;
    for (int c = 0; c < polygons->childCount(); ++c) {
        columns << polygons->child(c)->text(0) + ":" + polygons->child(c)->text(1);
    }
    EXPECT_TRUE(columns.contains("katana_id:integer")) << columns.join(' ').toStdString();
    EXPECT_TRUE(columns.contains("owner:string")) << columns.join(' ').toStdString();
    dialog.findChild<QPlainTextEdit*>("gisSqlText")
        ->setPlainText("SELECT katana_id, owner, ST_Area(geometry) AS area FROM polygons ORDER BY "
                       "katana_id");
    dialog.run();
    auto* grid = dialog.findChild<QTableWidget*>("gisSqlResults");
    ASSERT_EQ(grid->rowCount(), 2) << dialog.replyText().toStdString();
    ASSERT_EQ(grid->columnCount(), 3);
    EXPECT_EQ(grid->horizontalHeaderItem(2)->text(), "area");
    EXPECT_EQ(grid->item(0, 1)->text(), "Smith");
    EXPECT_EQ(grid->item(0, 2)->text(), "2000");
    EXPECT_EQ(grid->item(1, 1)->text(), ""); // null
    EXPECT_TRUE(dialog.statusText().startsWith("2 rows")) << dialog.statusText().toStdString();
    // AS SELECT: the rows' katana_id is the selection.
    dialog.findChild<QPlainTextEdit*>("gisSqlText")
        ->setPlainText("SELECT katana_id FROM polygons WHERE owner = 'Smith'");
    dialog.findChild<QComboBox*>("gisSqlAs")->setCurrentText("select");
    dialog.run();
    ASSERT_EQ(window.document.selection().ids().size(), 1u) << dialog.replyText().toStdString();
    EXPECT_EQ(window.document.selection().ids().front(), 1u);
}

} // namespace
