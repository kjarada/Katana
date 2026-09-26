// GIS > Processing - GDAL > Formats (src/katana_qt/geo/formats_dialog.hpp):
// the FORMATS line the choices make, the table filled from its records, a
// driver's options on selecting it, and the line handed to the window's
// executor on Run. Found by object name, as a headless run finds them.

#include <gtest/gtest.h>

#include <QAction>
#include <QComboBox>
#include <QDialog>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QPushButton>
#include <QStringList>
#include <QTableWidget>

#include <filesystem>
#include <memory>

#include "geo/formats_dialog.hpp"
#include "geo/geo_workbench.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::FormatsDialog;
using katana::qt::GeoServices;
using katana::qt::GeoWorkbench;
using katana::qt::VerbOutcome;

struct Window {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    QStringList logged;
    QStringList ran; // the lines handed to the window's executor
    QMainWindow main;
    std::unique_ptr<GeoWorkbench> workbench;

    Window()
    {
        GeoServices services;
        services.document = &document;
        services.interpreter = &interpreter;
        services.reference = &reference;
        services.surfaces = &surfaces;
        services.scratch = std::filesystem::temp_directory_path() / "katana-formats-dialog-test";
        services.log = [this](const QString& text, bool) { logged << text; };
        services.headless = [] { return true; };
        services.run = [this](const QString& line) {
            ran << line;
            return VerbOutcome{true, {}, {}};
        };
        services.makeAction = [this](katana::qt::Icon, const QString& text, const QString&,
                                     const QKeySequence&, const QString& name) {
            auto* action = new QAction(text, &main);
            action->setObjectName(name);
            return action;
        };
        workbench = std::make_unique<GeoWorkbench>(main, std::move(services));
    }
};

template <typename T>
T* child(const QWidget& parent, const char* name)
{
    T* widget = parent.findChild<T*>(name);
    EXPECT_NE(widget, nullptr) << name;
    return widget;
}

QStringList column(const QTableWidget& table, int column)
{
    QStringList cells;
    for (int row = 0; row < table.rowCount(); ++row) {
        cells << table.item(row, column)->text();
    }
    return cells;
}

TEST(FormatsDialog, TheTableIsTheFormatsLineItsChoicesMake)
{
    Window window;
    FormatsDialog dialog(*window.workbench);
    auto* table = child<QTableWidget>(dialog, "gisFormatsTable");
    auto* command = child<QLabel>(dialog, "gisFormatsCommand");
    auto* count = child<QLabel>(dialog, "gisFormatsCount");
    ASSERT_TRUE(table && command && count);
    EXPECT_EQ(command->text(), "FORMATS");
    const int everything = dialog.formatCount();
    EXPECT_GT(everything, 50) << "GDAL has well over fifty formats";
    EXPECT_TRUE(count->text().startsWith(QString::number(everything) + " formats (GDAL 3."))
        << count->text().toStdString();

    child<QComboBox>(dialog, "gisFormatsKind")->setCurrentText("Vector");
    child<QComboBox>(dialog, "gisFormatsCapability")->setCurrentText("Writes");
    child<QLineEdit>(dialog, "gisFormatsFilter")->setText("fgb");
    EXPECT_EQ(command->text(), "FORMATS VECTOR WRITE fgb");
    EXPECT_EQ(dialog.line(), command->text());
    ASSERT_EQ(table->rowCount(), 1);
    EXPECT_EQ(table->item(0, 0)->text(), "FlatGeobuf");
    EXPECT_EQ(table->item(0, 2)->text(), "vector");
    EXPECT_EQ(table->item(0, 4)->text(), "vector");
    EXPECT_EQ(table->item(0, 5)->text(), "fgb");
    EXPECT_TRUE(window.logged.isEmpty()) << "the table fills without filling the log";
}

TEST(FormatsDialog, AFilterWordThatIsAKeywordStaysAWord)
{
    Window window;
    FormatsDialog dialog(*window.workbench);
    // A quote typed in the filter is dropped: the line has no escape for one.
    child<QLineEdit>(dialog, "gisFormatsFilter")->setText("raster \"tile\"");
    EXPECT_EQ(dialog.line(), "FORMATS \"raster\" tile");
    // Every row holds both words - GDAL's "GDAL Raster Tile Index" (GTI) does
    // - where RASTER unquoted would have listed every raster format.
    const auto* table = child<QTableWidget>(dialog, "gisFormatsTable");
    ASSERT_GT(table->rowCount(), 0);
    for (int row = 0; row < table->rowCount(); ++row) {
        const QString text = table->item(row, 0)->text() + ' ' + table->item(row, 1)->text() +
                             ' ' + table->item(row, 5)->text();
        EXPECT_TRUE(text.contains("raster", Qt::CaseInsensitive)) << text.toStdString();
        EXPECT_TRUE(text.contains("tile", Qt::CaseInsensitive)) << text.toStdString();
    }
}

TEST(FormatsDialog, SelectingADriverShowsItsOptions)
{
    Window window;
    FormatsDialog dialog(*window.workbench);
    child<QLineEdit>(dialog, "gisFormatsFilter")->setText("geopackage");
    auto* table = child<QTableWidget>(dialog, "gisFormatsTable");
    const int gpkg = static_cast<int>(column(*table, 0).indexOf("GPKG"));
    ASSERT_GE(gpkg, 0) << column(*table, 0).join(',').toStdString();
    table->selectRow(gpkg);
    auto* options = child<QTableWidget>(dialog, "gisFormatsOptions");
    const QStringList names = column(*options, 1);
    EXPECT_TRUE(names.contains("LIST_ALL_TABLES")) << names.join(',').toStdString();
    const int listAll = static_cast<int>(names.indexOf("LIST_ALL_TABLES"));
    EXPECT_EQ(options->item(listAll, 0)->text(), "open");
    EXPECT_EQ(options->item(listAll, 3)->text(), "AUTO");
    EXPECT_EQ(options->item(listAll, 4)->text(), "AUTO YES NO");
}

TEST(FormatsDialog, RunHandsTheLineToTheWindowsExecutor)
{
    Window window;
    FormatsDialog dialog(*window.workbench);
    child<QComboBox>(dialog, "gisFormatsKind")->setCurrentText("Raster");
    child<QPushButton>(dialog, "gisFormatsRun")->click();
    ASSERT_EQ(window.ran.size(), 1);
    EXPECT_EQ(window.ran.front(), "FORMATS RASTER");
}

TEST(FormatsDialog, TheMenuItemShowsOneKeptDialog)
{
    Window window;
    QMenu gis;
    QMenu terrain;
    katana::qt::GeoMenus menus(gis, terrain);
    katana::qt::addFormatsItem(menus, *window.workbench);
    auto* action = window.main.findChild<QAction*>("gisFormats");
    ASSERT_NE(action, nullptr);
    EXPECT_EQ(action->data().toString(), "gisFormatsDialog");
    EXPECT_TRUE(gis.actions().contains(action));
    action->trigger();
    const auto dialogs = window.main.findChildren<QDialog*>("gisFormatsDialog");
    ASSERT_EQ(dialogs.size(), 1);
    EXPECT_TRUE(dialogs.front()->isVisible());
    EXPECT_FALSE(dialogs.front()->isModal());
    action->trigger();
    EXPECT_EQ(window.main.findChildren<QDialog*>("gisFormatsDialog").size(), 1)
        << "the second time shows the same dialog";
}

} // namespace
