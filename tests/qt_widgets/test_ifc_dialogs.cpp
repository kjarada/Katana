// File > Import IFC and Export IFC (src/katana_qt/ifc_dialogs.hpp): the
// choices, what refuses them, and the line each hands the window's command
// line - driven through the widgets, with the window's side supplied by the
// test as a command line that records the line and answers as the verb does
// (ifc/front_end.hpp's reply writers).

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
#include "katana/ifc/front_end.hpp"

using katana::qt::IfcExportContext;
using katana::qt::IfcExportDialog;
using katana::qt::IfcExportState;
using katana::qt::IfcImportContext;
using katana::qt::IfcImportDialog;

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

// A kerb and a pit: two classes a preview names.
katana::entity::Model kerbAndPit()
{
    katana::entity::Model model;
    katana::entity::Entity kerb;
    kerb.geometry = katana::geometry::Polyline2{{{0.0, 0.0}, {10.0, 0.0}}, false};
    kerb.layer = "Survey/Kerb";
    EXPECT_TRUE(model.entities.add(kerb).ok());
    katana::entity::Entity pit;
    pit.geometry = katana::entity::PointGeometry{{5.0, 2.0}};
    pit.layer = "Sewer/Pits";
    EXPECT_TRUE(model.entities.add(pit).ok());
    return model;
}

// The window's command line, as far as an EXPORT line goes: `model` written
// in memory, and answered as the verb answers.
std::function<katana::core::Result<std::string>(const QString&)>
exportLine(const katana::entity::Model& model, std::vector<QString>& lines)
{
    return [&model, &lines](const QString& line) -> katana::core::Result<std::string> {
        lines.push_back(line);
        const auto parsed = katana::ifc::parseExportArguments(
            line.mid(QStringLiteral("EXPORT").size()).toStdString());
        if (!parsed || !*parsed) {
            return katana::core::makeError(katana::core::ErrorCode::InvalidArgument, "not EXPORT");
        }
        const auto written = katana::ifc::writeIfc({&model, {}, {}});
        if (!written) {
            return written.error();
        }
        return katana::ifc::formatExportReply(*written, (*parsed)->path, (*parsed)->preview);
    };
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
          "ifcExportCommand",      "ifcExportClasses",        "ifcExportCheck",
          "ifcExportPreview",      "ifcExportExport",         "ifcExportClose"}) {
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
    // Nothing selected: the choice says so, and ticked, refuses the export
    // rather than widening it to every entity.
    EXPECT_TRUE(selected->text().contains("nothing is selected"));
    selected->setChecked(true);
    EXPECT_TRUE(dialog.checkWithoutFile().startsWith("Selected entities only is ticked, and "
                                                     "nothing is selected"))
        << dialog.checkWithoutFile().toStdString();
    selected->setChecked(false);
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

// The dialog's whole output is a line: every choice is a word of it, shown
// as it is edited, and the verb reads it back as the choices made.
TEST(IfcExportDialog, TheLineCarriesEveryChoiceAsTheVerbReadsIt)
{
    IfcExportDialog dialog(exportContext(drawing(5, 3)));
    auto* command = child<QLineEdit>(dialog, "ifcExportCommand");
    ASSERT_NE(command, nullptr);
    EXPECT_TRUE(command->isReadOnly());
    EXPECT_TRUE(command->text().isEmpty()); // no file yet: no line to run
    child<QLineEdit>(dialog, "ifcExportFile")->setText("C:/jobs/site plan.ifc");
    EXPECT_EQ(command->text(), "EXPORT \"C:/jobs/site plan.ifc\"");
    child<QLineEdit>(dialog, "ifcExportSchedule")->setText("schedule.csv");
    child<QLineEdit>(dialog, "ifcExportSchema")->setText("schema.csv");
    child<QLineEdit>(dialog, "ifcExportRules")->setText("rules.csv");
    child<QLineEdit>(dialog, "ifcExportSpacing")->setText("12.5");
    child<QCheckBox>(dialog, "ifcExportSelectedOnly")->setChecked(true);
    child<QCheckBox>(dialog, "ifcExportSurfaces")->setChecked(false);
    const QString expected = "EXPORT \"C:/jobs/site plan.ifc\" UTILITIES \"schedule.csv\" SCHEMA "
                             "\"schema.csv\" RULES \"rules.csv\" SPACING 12.5 NOSURFACES SELECTED";
    EXPECT_EQ(dialog.line(), expected);
    EXPECT_EQ(command->text(), expected);

    const auto back = katana::ifc::parseExportArguments(
        dialog.line().mid(QStringLiteral("EXPORT").size()).toStdString());
    ASSERT_TRUE(back && *back);
    EXPECT_EQ(**back, dialog.arguments());

    // The spacing the verb takes by default is not said; selected-only
    // means nothing once the entities are left out; leaving out every part
    // of the drawing is NODRAWING.
    child<QLineEdit>(dialog, "ifcExportSpacing")->setText("10");
    child<QCheckBox>(dialog, "ifcExportEntities")->setChecked(false);
    child<QCheckBox>(dialog, "ifcExportAlignments")->setChecked(false);
    EXPECT_EQ(dialog.line(), "EXPORT \"C:/jobs/site plan.ifc\" UTILITIES \"schedule.csv\" SCHEMA "
                             "\"schema.csv\" RULES \"rules.csv\" NODRAWING");
}

// The table is the writer's own account, read from the reply of the line
// with PREVIEW added: here the real writer on a real drawing, answered as
// the verb answers.
TEST(IfcExportDialog, PreviewShowsTheWritersAccountClassByClass)
{
    const katana::entity::Model model = kerbAndPit();
    std::vector<QString> lines;
    IfcExportContext context = exportContext(drawing(2));
    context.runLine = exportLine(model, lines);
    IfcExportDialog dialog(context);
    auto* table = child<QTableWidget>(dialog, "ifcExportClasses");
    auto* check = child<QLabel>(dialog, "ifcExportCheck");
    ASSERT_TRUE(table && check);
    child<QPushButton>(dialog, "ifcExportPreview")->click();
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(lines[0], "EXPORT \"preview.ifc\" PREVIEW"); // no file named yet
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
    context.runLine = [](const QString&) -> katana::core::Result<std::string> {
        return katana::core::makeError(katana::core::ErrorCode::NotFound, "cannot read s.csv");
    };
    IfcExportDialog refused(context);
    child<QPushButton>(refused, "ifcExportPreview")->click();
    EXPECT_EQ(child<QTableWidget>(refused, "ifcExportClasses")->rowCount(), 0);
    EXPECT_EQ(child<QLabel>(refused, "ifcExportCheck")->text(), "NotFound: cannot read s.csv");
}

TEST(IfcExportDialog, ExportHandsItsLineToTheWindowAndShowsWhatWasWritten)
{
    const katana::entity::Model model = kerbAndPit();
    std::vector<QString> lines;
    IfcExportContext context = exportContext(drawing(2));
    context.runLine = exportLine(model, lines);
    IfcExportDialog dialog(context);
    child<QLineEdit>(dialog, "ifcExportFile")->setText("out.ifc");
    child<QPushButton>(dialog, "ifcExportExport")->click();
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(lines[0], "EXPORT \"out.ifc\"");
    EXPECT_EQ(child<QTableWidget>(dialog, "ifcExportClasses")->rowCount(), 2);
    const auto written = katana::ifc::writeIfc({&model, {}, {}});
    ASSERT_TRUE(written.ok());
    EXPECT_EQ(child<QLabel>(dialog, "ifcExportCheck")->text(),
              "Wrote out.ifc: " + QString::number(written->instances) +
                  " instances (the log has the reply).");
}

// Headless, a Browse button would open a box no one can answer: it says so.
TEST(IfcExportDialog, BrowseAsksNoOneInAHeadlessSession)
{
    IfcExportDialog dialog(exportContext(drawing(5)));
    child<QPushButton>(dialog, "ifcExportBrowse")->click();
    EXPECT_TRUE(child<QLabel>(dialog, "ifcExportCheck")->text().contains("type its path instead"));
    child<QPushButton>(dialog, "ifcExportSaveRules")->click();
    EXPECT_TRUE(child<QLabel>(dialog, "ifcExportCheck")->text().contains("type IFC RULES"));
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
          "ifcImportTolerance", "ifcImportTakeCrs", "ifcImportCommand", "ifcImportCheck",
          "ifcImportImport", "ifcImportClose"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(QString::fromLatin1(name)), nullptr) << name;
    }
}

TEST(IfcImportDialog, TheLineCarriesEveryChoiceAndLocalTakesNoCoordinateSystem)
{
    IfcImportDialog dialog(IfcImportContext{});
    auto* import = child<QPushButton>(dialog, "ifcImportImport");
    auto* command = child<QLineEdit>(dialog, "ifcImportCommand");
    ASSERT_TRUE(import && command);
    EXPECT_FALSE(import->isEnabled());
    EXPECT_EQ(dialog.check(), "Name the IFC file to import.");
    dialog.setFile("site.ifc");
    EXPECT_TRUE(import->isEnabled());
    // The dialog always answers the coordinate-system question, so its line
    // never asks: ticked by default, TAKECRS.
    EXPECT_EQ(command->text(), "IMPORT \"site.ifc\" TAKECRS");
    child<QCheckBox>(dialog, "ifcImportTakeCrs")->setChecked(false);
    EXPECT_EQ(dialog.line(), "IMPORT \"site.ifc\" KEEPCRS");

    child<QCheckBox>(dialog, "ifcImportLocal")->setChecked(true);
    child<QCheckBox>(dialog, "ifcImportSurfaces")->setChecked(false);
    child<QLineEdit>(dialog, "ifcImportTolerance")->setText("0.01");
    // Moved to the origin, the data is in no system the file names.
    EXPECT_FALSE(child<QCheckBox>(dialog, "ifcImportTakeCrs")->isEnabled());
    EXPECT_EQ(dialog.line(), "IMPORT \"site.ifc\" LOCAL NOSURFACES TOLERANCE 0.01");
    const auto back = katana::ifc::parseImportArguments(
        dialog.line().mid(QStringLiteral("IMPORT").size()).toStdString());
    ASSERT_TRUE(back && *back);
    EXPECT_EQ(**back, dialog.arguments());

    child<QLineEdit>(dialog, "ifcImportTolerance")->setText("0");
    EXPECT_FALSE(import->isEnabled());
    EXPECT_TRUE(command->text().isEmpty());
    child<QLineEdit>(dialog, "ifcImportTolerance")->setText("0.01");
    for (const char* name : {"ifcImportAlignments", "ifcImportElements"}) {
        child<QCheckBox>(dialog, name)->setChecked(false);
    }
    EXPECT_FALSE(import->isEnabled());
    EXPECT_TRUE(dialog.check().startsWith("Nothing to import"));
}

TEST(IfcImportDialog, DescribeRunsInfoAndImportRunsItsLine)
{
    std::vector<QString> lines;
    bool cancel = false;
    IfcImportContext context;
    context.runLine = [&](const QString& line) -> katana::core::Result<std::string> {
        lines.push_back(line);
        if (line == "INFO \"missing.ifc\"") {
            return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                           "the file cannot be read");
        }
        if (line.startsWith("INFO")) {
            return std::string("ifc described file=\"site.ifc\" schema=IFC4X3_ADD2");
        }
        if (cancel) {
            return katana::core::makeError(katana::core::ErrorCode::CommandRejected,
                                           "Import cancelled.");
        }
        return std::string("ifc imported file=\"site.ifc\" schema=IFC4X3_ADD2 crs=\"\" "
                           "entities=29 alignments=1 surfaces=0");
    };
    IfcImportDialog dialog(context);
    dialog.setFile("site.ifc");
    child<QPushButton>(dialog, "ifcImportDescribe")->click();
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(lines[0], "INFO \"site.ifc\"");
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "ifcImportSummary")->toPlainText(),
              "ifc described file=\"site.ifc\" schema=IFC4X3_ADD2");
    child<QPushButton>(dialog, "ifcImportImport")->click();
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(lines[1], "IMPORT \"site.ifc\" TAKECRS");
    EXPECT_EQ(child<QLabel>(dialog, "ifcImportCheck")->text(),
              "Imported 29 entities, 1 alignments and 0 surfaces from site.ifc: the log has the "
              "reply.");

    // A person who cancels the far-apart question has not had a failure.
    cancel = true;
    child<QPushButton>(dialog, "ifcImportImport")->click();
    EXPECT_EQ(child<QLabel>(dialog, "ifcImportCheck")->text(), "Import cancelled.");

    dialog.setFile("missing.ifc");
    child<QPushButton>(dialog, "ifcImportDescribe")->click();
    EXPECT_TRUE(child<QPlainTextEdit>(dialog, "ifcImportSummary")->toPlainText().isEmpty());
    EXPECT_EQ(child<QLabel>(dialog, "ifcImportCheck")->text(), "NotFound: the file cannot be read");
    // Read at last, the file's earlier failure no longer stands.
    dialog.setFile("site.ifc");
    child<QPushButton>(dialog, "ifcImportDescribe")->click();
    EXPECT_TRUE(child<QLabel>(dialog, "ifcImportCheck")->text().startsWith("Ready to import"));
}

// ---- what the review found ----------------------------------------------------------

// The selection is the drawing's, and changes while the dialog stays open:
// read again before each export, a cleared one refuses "Selected entities
// only" instead of writing every entity.
TEST(IfcExportDialog, ASelectionClearedWhileItIsOpenRefusesSelectedOnly)
{
    IfcExportState now = drawing(5, 2);
    IfcExportContext context = exportContext(now);
    context.state = [&now] { return now; };
    bool ran = false;
    context.runLine = [&](const QString&) -> katana::core::Result<std::string> {
        ran = true;
        return std::string();
    };
    IfcExportDialog dialog(context);
    child<QLineEdit>(dialog, "ifcExportFile")->setText("out.ifc");
    child<QCheckBox>(dialog, "ifcExportSelectedOnly")->setChecked(true);
    EXPECT_TRUE(child<QPushButton>(dialog, "ifcExportExport")->isEnabled());
    now.selected = 0; // SELECT NONE, in the drawing beside it
    child<QPushButton>(dialog, "ifcExportExport")->click();
    EXPECT_FALSE(ran);
    EXPECT_TRUE(child<QLabel>(dialog, "ifcExportCheck")
                    ->text()
                    .startsWith("Selected entities only is ticked, and nothing is selected"));
    EXPECT_TRUE(child<QCheckBox>(dialog, "ifcExportSelectedOnly")->isChecked()); // as chosen
}

// A table beside "Ready" is what will be written: a changed choice clears
// it, and a problem replaces the last result instead of standing behind it.
TEST(IfcExportDialog, AChangedChoiceClearsThePreviewAndAProblemReplacesTheLastResult)
{
    const katana::entity::Model model = kerbAndPit();
    std::vector<QString> lines;
    IfcExportContext context = exportContext(drawing(2));
    context.runLine = exportLine(model, lines);
    IfcExportDialog dialog(context);
    auto* table = child<QTableWidget>(dialog, "ifcExportClasses");
    auto* check = child<QLabel>(dialog, "ifcExportCheck");
    auto* spacing = child<QLineEdit>(dialog, "ifcExportSpacing");
    child<QPushButton>(dialog, "ifcExportPreview")->click();
    ASSERT_EQ(table->rowCount(), 2);
    spacing->setText("12"); // grades differently: the account is no longer this one
    EXPECT_EQ(table->rowCount(), 0);

    child<QLineEdit>(dialog, "ifcExportFile")->setText("out.ifc");
    child<QPushButton>(dialog, "ifcExportExport")->click();
    ASSERT_TRUE(check->text().startsWith("Wrote out.ifc")) << check->text().toStdString();
    spacing->setText("0");
    EXPECT_EQ(check->text(), "The detected spacing must be a positive number of metres.");
}

// Preview needs everything but the file - not merely a missing file.
TEST(IfcExportDialog, PreviewWaitsForEverythingButTheFile)
{
    IfcExportDialog dialog(exportContext(drawing(5)));
    auto* preview = child<QPushButton>(dialog, "ifcExportPreview");
    EXPECT_TRUE(preview->isEnabled()); // no file yet
    child<QLineEdit>(dialog, "ifcExportSpacing")->setText("-5");
    EXPECT_FALSE(preview->isEnabled());
    EXPECT_EQ(dialog.checkWithoutFile(),
              "The detected spacing must be a positive number of metres.");
    child<QLineEdit>(dialog, "ifcExportSpacing")->setText("5");
    EXPECT_TRUE(preview->isEnabled());
}
