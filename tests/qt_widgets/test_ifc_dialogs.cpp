// File > Import IFC and Export IFC (src/katana_qt/ifc_dialogs.hpp): the
// choices, what refuses them, and what each hands the window - driven
// through the widgets, with the window's side supplied by the test.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>

#include "ifc_dialogs.hpp"
#include "katana/entity/model.hpp"
#include "katana/ifc/export.hpp"

using katana::qt::IfcExportContext;
using katana::qt::IfcExportDialog;
using katana::qt::IfcExportRequest;
using katana::qt::IfcExportState;
using katana::qt::IfcImportContext;
using katana::qt::IfcImportDialog;
using katana::qt::IfcImportRequest;

namespace {

template <typename T> T* child(QWidget& parent, const char* name)
{
    auto* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

IfcExportState drawing(std::size_t entities, std::size_t selected = 0)
{
    IfcExportState state;
    state.entities = entities;
    state.selected = selected;
    state.alignments = 1;
    state.surfaces = 2;
    state.coordinateSystem = "EPSG:7856  GDA2020 / MGA zone 56";
    state.georeferenced = true;
    return state;
}

IfcExportContext exportContext(IfcExportState state)
{
    IfcExportContext context;
    context.state = [state] { return state; };
    context.headless = [] { return true; };
    return context;
}

} // namespace

// ---- export -------------------------------------------------------------------------

TEST(IfcExportDialog, EveryControlHasItsObjectName)
{
    IfcExportDialog dialog(exportContext(drawing(5)));
    EXPECT_EQ(dialog.objectName(), "fileExportIfcDialog"); // what --dialog fileExportIfc finds
    EXPECT_FALSE(dialog.isModal());
    for (const char* name :
         {"ifcExportFile",         "ifcExportBrowse",         "ifcExportEntities",
          "ifcExportSelectedOnly", "ifcExportAlignments",     "ifcExportSurfaces",
          "ifcExportSchedule",     "ifcExportScheduleBrowse", "ifcExportSchema",
          "ifcExportSchemaBrowse", "ifcExportSpacing",        "ifcExportRules",
          "ifcExportRulesBrowse",  "ifcExportSaveRules",      "ifcExportCrs",
          "ifcExportClasses",      "ifcExportCheck",          "ifcExportPreview",
          "ifcExportExport",       "ifcExportClose"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(QString::fromLatin1(name)), nullptr) << name;
    }
}

TEST(IfcExportDialog, ItShowsTheWindowsCountsSelectionAndCoordinateSystem)
{
    IfcExportDialog dialog(exportContext(drawing(5)));
    auto* entities = child<QCheckBox>(dialog, "ifcExportEntities");
    auto* selected = child<QCheckBox>(dialog, "ifcExportSelectedOnly");
    auto* crs = child<QLabel>(dialog, "ifcExportCrs");
    ASSERT_TRUE(entities && selected && crs);
    EXPECT_EQ(entities->text(), "Drawing entities (5)");
    // Nothing selected: the choice is off, and says why.
    EXPECT_FALSE(selected->isEnabled());
    EXPECT_TRUE(selected->text().contains("nothing is selected"));
    EXPECT_TRUE(crs->text().startsWith("Georeferenced in EPSG:7856")) << crs->text().toStdString();

    IfcExportState local = drawing(5, 2);
    local.coordinateSystem.clear();
    local.georeferenced = false;
    IfcExportDialog second(exportContext(local));
    auto* selectedHere = child<QCheckBox>(second, "ifcExportSelectedOnly");
    auto* crsHere = child<QLabel>(second, "ifcExportCrs");
    ASSERT_TRUE(selectedHere && crsHere);
    EXPECT_TRUE(selectedHere->isEnabled());
    EXPECT_EQ(selectedHere->text(), "Selected entities only (2 selected)");
    EXPECT_TRUE(crsHere->text().contains("Not georeferenced: the project has no coordinate system"))
        << crsHere->text().toStdString();
}

// What the verb refuses, the dialog refuses before the button is pressed.
TEST(IfcExportDialog, ExportWaitsForAFileAPositiveSpacingAndTheScheduleASchemaDescribes)
{
    IfcExportDialog dialog(exportContext(drawing(5)));
    auto* file = child<QLineEdit>(dialog, "ifcExportFile");
    auto* schedule = child<QLineEdit>(dialog, "ifcExportSchedule");
    auto* schema = child<QLineEdit>(dialog, "ifcExportSchema");
    auto* spacing = child<QLineEdit>(dialog, "ifcExportSpacing");
    auto* exportButton = child<QPushButton>(dialog, "ifcExportExport");
    auto* preview = child<QPushButton>(dialog, "ifcExportPreview");
    auto* check = child<QLabel>(dialog, "ifcExportCheck");
    ASSERT_TRUE(file && schedule && schema && spacing && exportButton && preview && check);

    EXPECT_FALSE(exportButton->isEnabled());
    EXPECT_TRUE(preview->isEnabled()); // a preview needs no file
    EXPECT_EQ(check->text(), "Name the .ifc file to write.");
    file->setText("site.dxf");
    EXPECT_EQ(dialog.check(), "The file must end in .ifc.");
    file->setText("site.IFC");
    EXPECT_TRUE(exportButton->isEnabled());
    EXPECT_TRUE(check->text().startsWith("Ready to write 5 entities, 1 alignments, 2 surfaces"))
        << check->text().toStdString();

    schema->setText("schema.csv");
    EXPECT_FALSE(exportButton->isEnabled());
    EXPECT_FALSE(preview->isEnabled());
    EXPECT_TRUE(dialog.check().contains("describes a schedule"));
    schedule->setText("schedule.csv");
    EXPECT_TRUE(exportButton->isEnabled());

    for (const char* bad : {"0", "-5", "ten", ""}) {
        spacing->setText(bad);
        EXPECT_FALSE(exportButton->isEnabled()) << bad;
        EXPECT_EQ(dialog.check(), "The detected spacing must be a positive number of metres.");
    }
    spacing->setText("15");
    EXPECT_TRUE(exportButton->isEnabled());

    // Nothing chosen to write, and no schedule: refused.
    schema->clear();
    schedule->clear();
    for (const char* name : {"ifcExportEntities", "ifcExportAlignments", "ifcExportSurfaces"}) {
        child<QCheckBox>(dialog, name)->setChecked(false);
    }
    EXPECT_FALSE(exportButton->isEnabled());
    EXPECT_TRUE(dialog.check().startsWith("Nothing to write"));
    schedule->setText("schedule.csv"); // the investigation alone
    EXPECT_TRUE(exportButton->isEnabled());
}

TEST(IfcExportDialog, TheRequestCarriesEveryChoiceAsTheVerbWouldReadIt)
{
    IfcExportDialog dialog(exportContext(drawing(5, 3)));
    child<QLineEdit>(dialog, "ifcExportFile")->setText("C:/jobs/site plan.ifc");
    child<QLineEdit>(dialog, "ifcExportSchedule")->setText("schedule.csv");
    child<QLineEdit>(dialog, "ifcExportSchema")->setText("schema.csv");
    child<QLineEdit>(dialog, "ifcExportRules")->setText("rules.csv");
    child<QLineEdit>(dialog, "ifcExportSpacing")->setText("12.5");
    child<QCheckBox>(dialog, "ifcExportSelectedOnly")->setChecked(true);
    child<QCheckBox>(dialog, "ifcExportSurfaces")->setChecked(false);

    const IfcExportRequest request = dialog.request();
    EXPECT_EQ(request.arguments.path, "C:/jobs/site plan.ifc");
    EXPECT_EQ(request.arguments.schedule, "schedule.csv");
    EXPECT_EQ(request.arguments.schema, "schema.csv");
    EXPECT_EQ(request.arguments.rules, "rules.csv");
    EXPECT_EQ(request.arguments.spacing, 12.5);
    EXPECT_TRUE(request.arguments.drawing);
    EXPECT_TRUE(request.entities);
    EXPECT_TRUE(request.selectedOnly);
    EXPECT_TRUE(request.alignments);
    EXPECT_FALSE(request.surfaces);

    // Selected-only means nothing once the entities are left out.
    child<QCheckBox>(dialog, "ifcExportEntities")->setChecked(false);
    EXPECT_FALSE(dialog.request().selectedOnly);
    child<QCheckBox>(dialog, "ifcExportAlignments")->setChecked(false);
    EXPECT_FALSE(dialog.request().arguments.drawing); // NODRAWING
}

// The table is the writer's own account: here made by the real writer from
// a real drawing, as the window's preview makes it.
TEST(IfcExportDialog, PreviewShowsTheWritersAccountClassByClass)
{
    katana::entity::Model model;
    katana::entity::Entity kerb;
    kerb.geometry = katana::geometry::Polyline2{{{0.0, 0.0}, {10.0, 0.0}}, false};
    kerb.layer = "Survey/Kerb";
    ASSERT_TRUE(model.entities.add(kerb).ok());
    katana::entity::Entity pit;
    pit.geometry = katana::entity::PointGeometry{{5.0, 2.0}};
    pit.layer = "Sewer/Pits";
    ASSERT_TRUE(model.entities.add(pit).ok());

    IfcExportContext context = exportContext(drawing(2));
    std::optional<IfcExportRequest> previewed;
    context.preview = [&](const IfcExportRequest& request) {
        previewed = request;
        return katana::ifc::writeIfc({&model, {}, {}});
    };
    IfcExportDialog dialog(context);
    auto* table = child<QTableWidget>(dialog, "ifcExportClasses");
    auto* check = child<QLabel>(dialog, "ifcExportCheck");
    ASSERT_TRUE(table && check);
    child<QPushButton>(dialog, "ifcExportPreview")->click();
    ASSERT_TRUE(previewed);
    EXPECT_EQ(previewed->arguments.path, "preview.ifc"); // no file named yet
    ASSERT_EQ(table->rowCount(), 2);
    EXPECT_EQ(table->item(0, 0)->text(), "layer Sewer/Pits");
    EXPECT_EQ(table->item(0, 2)->text(), "IfcDistributionChamberElement INSPECTIONPIT");
    EXPECT_EQ(table->item(0, 3)->text(), "SEWAGE");
    EXPECT_EQ(table->item(0, 4)->text(), "rule pit");
    EXPECT_EQ(table->item(1, 0)->text(), "layer Survey/Kerb");
    EXPECT_EQ(table->item(1, 1)->text(), "1");
    EXPECT_EQ(table->item(1, 2)->text(), "IfcKerb NOTDEFINED");
    EXPECT_TRUE(check->text().startsWith("Preview: 2 objects would be written"))
        << check->text().toStdString();

    // A preview that is refused says why, and shows nothing.
    context.preview = [](const IfcExportRequest&) -> katana::core::Result<katana::ifc::IfcExport> {
        return katana::core::makeError(katana::core::ErrorCode::NotFound, "cannot read s.csv");
    };
    IfcExportDialog refused(context);
    child<QPushButton>(refused, "ifcExportPreview")->click();
    EXPECT_EQ(child<QTableWidget>(refused, "ifcExportClasses")->rowCount(), 0);
    EXPECT_EQ(child<QLabel>(refused, "ifcExportCheck")->text(), "NotFound: cannot read s.csv");
}

TEST(IfcExportDialog, ExportHandsTheRequestToTheWindowAndShowsWhatWasWritten)
{
    IfcExportContext context = exportContext(drawing(5));
    std::optional<IfcExportRequest> ran;
    context.run = [&](const IfcExportRequest& request) {
        ran = request;
        katana::ifc::IfcExport report;
        report.instances = 1097;
        report.entitiesWritten = 5;
        report.tally = {{"layer Survey/Kerb", "IfcKerb", "NOTDEFINED", "", "", "rule kerb", 1}};
        return katana::core::Result<katana::ifc::IfcExport>(std::move(report));
    };
    IfcExportDialog dialog(context);
    child<QLineEdit>(dialog, "ifcExportFile")->setText("out.ifc");
    child<QPushButton>(dialog, "ifcExportExport")->click();
    ASSERT_TRUE(ran);
    EXPECT_EQ(ran->arguments.path, "out.ifc");
    EXPECT_EQ(child<QTableWidget>(dialog, "ifcExportClasses")->rowCount(), 1);
    const QString said = child<QLabel>(dialog, "ifcExportCheck")->text();
    EXPECT_TRUE(said.startsWith("Wrote out.ifc: 1,097 instances, 5 entities") ||
                said.startsWith("Wrote out.ifc: 1097 instances, 5 entities"))
        << said.toStdString();
}

// Headless, a Browse button would open a box no one can answer: it says so.
TEST(IfcExportDialog, BrowseAsksNoOneInAHeadlessSession)
{
    IfcExportDialog dialog(exportContext(drawing(5)));
    child<QPushButton>(dialog, "ifcExportBrowse")->click();
    EXPECT_TRUE(child<QLabel>(dialog, "ifcExportCheck")->text().contains("type its path instead"));
    child<QPushButton>(dialog, "ifcExportSaveRules")->click();
    EXPECT_TRUE(child<QLabel>(dialog, "ifcExportCheck")->text().contains("not available here"));
}

// ---- import -------------------------------------------------------------------------

TEST(IfcImportDialog, EveryControlHasItsObjectName)
{
    IfcImportDialog dialog(IfcImportContext{});
    EXPECT_EQ(dialog.objectName(), "fileImportIfcDialog");
    EXPECT_FALSE(dialog.isModal());
    for (const char* name :
         {"ifcImportFile", "ifcImportBrowse", "ifcImportDescribe", "ifcImportSummary",
          "ifcImportLocal", "ifcImportAlignments", "ifcImportElements", "ifcImportSurfaces",
          "ifcImportTolerance", "ifcImportTakeCrs", "ifcImportCheck", "ifcImportImport",
          "ifcImportClose"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(QString::fromLatin1(name)), nullptr) << name;
    }
}

TEST(IfcImportDialog, TheRequestCarriesEveryChoiceAndLocalTakesNoCoordinateSystem)
{
    IfcImportDialog dialog(IfcImportContext{});
    auto* import = child<QPushButton>(dialog, "ifcImportImport");
    ASSERT_NE(import, nullptr);
    EXPECT_FALSE(import->isEnabled());
    EXPECT_EQ(dialog.check(), "Name the IFC file to import.");
    dialog.setFile("site.ifc");
    EXPECT_TRUE(import->isEnabled());

    IfcImportRequest request = dialog.request();
    EXPECT_EQ(request.arguments.path, "site.ifc");
    EXPECT_FALSE(request.arguments.local);
    EXPECT_TRUE(request.alignments && request.elements && request.surfaces);
    EXPECT_EQ(request.curveTolerance, 0.001);
    EXPECT_EQ(request.takeCoordinateSystem, true);

    child<QCheckBox>(dialog, "ifcImportLocal")->setChecked(true);
    child<QCheckBox>(dialog, "ifcImportSurfaces")->setChecked(false);
    child<QLineEdit>(dialog, "ifcImportTolerance")->setText("0.01");
    // Moved to the origin, the data is in no system the file names.
    EXPECT_FALSE(child<QCheckBox>(dialog, "ifcImportTakeCrs")->isEnabled());
    request = dialog.request();
    EXPECT_TRUE(request.arguments.local);
    EXPECT_FALSE(request.surfaces);
    EXPECT_EQ(request.curveTolerance, 0.01);
    EXPECT_EQ(request.takeCoordinateSystem, false);

    child<QLineEdit>(dialog, "ifcImportTolerance")->setText("0");
    EXPECT_FALSE(import->isEnabled());
    child<QLineEdit>(dialog, "ifcImportTolerance")->setText("0.01");
    for (const char* name : {"ifcImportAlignments", "ifcImportElements"}) {
        child<QCheckBox>(dialog, name)->setChecked(false);
    }
    EXPECT_FALSE(import->isEnabled());
    EXPECT_TRUE(dialog.check().startsWith("Nothing to import"));
}

TEST(IfcImportDialog, DescribeShowsWhatTheFileHoldsAndImportHandsTheRequestOver)
{
    IfcImportContext context;
    context.describe = [](const QString& path) -> katana::core::Result<QString> {
        if (path == "missing.ifc") {
            return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                           "the file cannot be read");
        }
        return QString("site.ifc: IFC4X3_ADD2, EPSG:7856");
    };
    std::optional<IfcImportRequest> ran;
    context.run = [&](const IfcImportRequest& request) {
        ran = request;
        return true;
    };
    IfcImportDialog dialog(context);
    dialog.setFile("site.ifc");
    child<QPushButton>(dialog, "ifcImportDescribe")->click();
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "ifcImportSummary")->toPlainText(),
              "site.ifc: IFC4X3_ADD2, EPSG:7856");
    child<QPushButton>(dialog, "ifcImportImport")->click();
    ASSERT_TRUE(ran);
    EXPECT_EQ(ran->arguments.path, "site.ifc");
    EXPECT_TRUE(child<QLabel>(dialog, "ifcImportCheck")->text().startsWith("Imported site.ifc"));

    dialog.setFile("missing.ifc");
    child<QPushButton>(dialog, "ifcImportDescribe")->click();
    EXPECT_TRUE(child<QPlainTextEdit>(dialog, "ifcImportSummary")->toPlainText().isEmpty());
    EXPECT_EQ(child<QLabel>(dialog, "ifcImportCheck")->text(), "NotFound: the file cannot be read");
}
