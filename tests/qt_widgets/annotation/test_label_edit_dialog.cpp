// Annotate > Edit Label... and Label Layout Report... (src/katana_qt/
// annotation/label_edit_dialog.hpp, label_layout_report.hpp): the lines they
// write, worked out by hand, and the dialogs driven by object name with a
// runner that runs each line through the command interpreter on the same
// document - as the window's one executor does - so what a click does is
// checked on the drawing, one undo step at a time.

#include <gtest/gtest.h>

#include <string>
#include <variant>
#include <vector>

#include <QCheckBox>
#include <QComboBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>

#include "annotation/label_edit_dialog.hpp"
#include "annotation/label_layout_report.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::entity::EntityId;
using katana::entity::LabelGeometry;
using katana::geometry::Point2;
using katana::qt::LabelEditDialog;
using katana::qt::LabelEditForm;
using katana::qt::LabelLayoutReportDialog;
using katana::qt::VerbOutcome;

namespace {

// Runs each line through an interpreter on `document`, and keeps them.
struct Runner {
    explicit Runner(Document& on) : interpreter(on) {}

    CommandInterpreter interpreter;
    QStringList lines;

    VerbOutcome operator()(const QString& line)
    {
        lines << line;
        const auto reply = interpreter.run(line.toStdString());
        if (!reply) {
            return {false, {}, QString::fromStdString(reply.error().describe())};
        }
        return {true, QString::fromStdString(*reply), {}};
    }
};

void run(Document& document, const std::string& line)
{
    CommandInterpreter interpreter(document);
    const auto reply = interpreter.run(line);
    ASSERT_TRUE(reply.ok()) << line << "\n  -> " << reply.error().describe();
}

const LabelGeometry& labelIn(Document& document, EntityId id)
{
    return std::get<LabelGeometry>(document.model().entities.find(id)->geometry);
}

template <class Widget>
Widget* child(QWidget& dialog, const char* name)
{
    auto* widget = dialog.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(widget, nullptr) << name;
    return widget;
}

// A lot (1) labelled by its area (2) on layer 0, and a layer "labels".
void lotWithLabel(Document& document)
{
    run(document, "LABELSTYLE DEFAULTS");
    run(document, "LAYER NEW labels");
    run(document, "PLINE 0,0 30,0 30,40 0,40 CLOSE");
    run(document, "LABEL 1 style=\"Lot Area\"");
}

LabelEditForm unpinnedLot()
{
    LabelEditForm form;
    form.id = 2;
    form.style = "Lot Area";
    form.easting = "15";
    form.northing = "20";
    form.layer = "0";
    return form;
}

} // namespace

// ---- the LABEL SET line -----------------------------------------------------------------

TEST(LabelEditLine, OnlyWhatDiffersFromTheLabelIsNamed)
{
    const LabelEditForm current = unpinnedLot();
    EXPECT_EQ(*katana::qt::labelSetLine(current, current), "") << "nothing to change";

    LabelEditForm form = current;
    form.style = "Bearing Distance";
    EXPECT_EQ(*katana::qt::labelSetLine(form, current), "LABEL SET 2 style=\"Bearing Distance\"");

    form = current;
    form.pinned = true;
    form.easting = " 12.5 ";
    form.northing = "-3";
    EXPECT_EQ(*katana::qt::labelSetLine(form, current), "LABEL SET 2 at=12.5,-3");

    form = current;
    form.override = true;
    form.text = "LOT 7\nDP 1234";
    form.layer = "labels";
    EXPECT_EQ(*katana::qt::labelSetLine(form, current),
              "LABEL SET 2 text=\"LOT 7\\nDP 1234\" layer=labels");
}

TEST(LabelEditLine, UntickingTakesBackTheOwnTextAndThePinnedPlace)
{
    LabelEditForm current = unpinnedLot();
    current.override = true;
    current.text = "LOT 7";
    current.pinned = true;
    current.easting = "5";
    current.northing = "6";
    LabelEditForm form = current;
    form.override = false;
    form.pinned = false;
    EXPECT_EQ(*katana::qt::labelSetLine(form, current), "LABEL SET 2 text=none at=none");
    // The same place written another way is the same place.
    form = current;
    form.easting = "5.000";
    EXPECT_EQ(*katana::qt::labelSetLine(form, current), "");
}

TEST(LabelEditLine, FieldsThatMakeNoLineAreRefusedNamingTheField)
{
    const LabelEditForm current = unpinnedLot();
    const auto refused = [&](LabelEditForm form, const std::string& part) {
        const auto made = katana::qt::labelSetLine(form, current);
        ASSERT_FALSE(made.ok()) << part;
        EXPECT_NE(made.error().message.find(part), std::string::npos) << made.error().message;
    };
    LabelEditForm form = current;
    form.id = 0;
    refused(form, "no label is chosen");
    form = current;
    form.style = " ";
    refused(form, "Style:");
    form = current;
    form.layer.clear();
    refused(form, "Layer:");
    form = current;
    form.override = true;
    refused(form, "Own text:");
    form.text = "None";
    refused(form, "the word none");
    form.text = "6\" pipe";
    refused(form, "a double quote");
    form = current;
    form.pinned = true;
    form.easting = "12,5";
    refused(form, "Easting: '12,5' is not a number");
    form.easting = "1";
    form.northing = "north";
    refused(form, "Northing:");
}

// ---- the Edit Label dialog --------------------------------------------------------------

TEST(LabelEditDialog, WithNoLabelSelectedItSaysSoAndOffersNothingToRun)
{
    Document document;
    lotWithLabel(document);
    Runner runner(document);
    LabelEditDialog dialog(document, std::ref(runner));
    EXPECT_FALSE(dialog.loadSelection());
    EXPECT_EQ(dialog.status(), "Select one label, then choose Edit Label.");
    EXPECT_FALSE(child<QPushButton>(dialog, "labelEditOk")->isEnabled());
    EXPECT_FALSE(child<QPushButton>(dialog, "labelEditApply")->isEnabled());
    document.selection().set({1});
    EXPECT_FALSE(dialog.loadSelection());
    EXPECT_NE(dialog.status().toStdString().find("not a label"), std::string::npos)
        << dialog.status().toStdString();
    EXPECT_TRUE(runner.lines.isEmpty());
}

TEST(LabelEditDialog, ApplyRunsTheLineItShowsAsOneUndoStep)
{
    Document document;
    lotWithLabel(document);
    Runner runner(document);
    LabelEditDialog dialog(document, std::ref(runner));
    document.selection().set({2});
    ASSERT_TRUE(dialog.loadSelection());
    EXPECT_EQ(child<QLabel>(dialog, "labelEditTarget")->text(), "Label 2 of entity 1 (Polyline)");
    EXPECT_EQ(child<QComboBox>(dialog, "labelEditStyle")->currentText(), "Lot Area");
    EXPECT_FALSE(child<QCheckBox>(dialog, "labelEditPinned")->isChecked());
    EXPECT_FALSE(child<QLineEdit>(dialog, "labelEditEasting")->isEnabled());

    child<QCheckBox>(dialog, "labelEditPinned")->setChecked(true);
    EXPECT_TRUE(child<QLineEdit>(dialog, "labelEditEasting")->isEnabled());
    child<QLineEdit>(dialog, "labelEditEasting")->setText("12");
    child<QLineEdit>(dialog, "labelEditNorthing")->setText("34");
    child<QCheckBox>(dialog, "labelEditOverride")->setChecked(true);
    child<QLineEdit>(dialog, "labelEditText")->setText("LOT 7");
    child<QComboBox>(dialog, "labelEditLayer")->setCurrentText("labels");
    const QString line = "LABEL SET 2 text=\"LOT 7\" at=12,34 layer=labels";
    EXPECT_EQ(child<QLineEdit>(dialog, "labelEditCommand")->text(), line);

    const std::size_t steps = document.history().undoCount();
    child<QPushButton>(dialog, "labelEditApply")->click();
    ASSERT_EQ(runner.lines, QStringList{line});
    EXPECT_EQ(dialog.status(), "updated label id=2");
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    const LabelGeometry& label = labelIn(document, 2);
    EXPECT_EQ(label.textOverride, "LOT 7");
    EXPECT_EQ(label.position, std::optional<Point2>(Point2(12.0, 34.0)));
    EXPECT_EQ(document.model().entities.find(2)->layer, "labels");
    // Shown afresh: the fields now match the label, so nothing is left to run.
    EXPECT_EQ(dialog.line(), "");
    child<QPushButton>(dialog, "labelEditApply")->click();
    EXPECT_EQ(runner.lines.size(), 1) << "an unchanged label runs nothing";
    EXPECT_EQ(dialog.status(), "Nothing to change.");

    // Unticked again: back to the placer and the style's words.
    child<QCheckBox>(dialog, "labelEditPinned")->setChecked(false);
    child<QCheckBox>(dialog, "labelEditOverride")->setChecked(false);
    EXPECT_TRUE(dialog.apply());
    EXPECT_EQ(runner.lines.back(), "LABEL SET 2 text=none at=none");
    EXPECT_FALSE(labelIn(document, 2).position.has_value());
    EXPECT_TRUE(labelIn(document, 2).textOverride.empty());
}

TEST(LabelEditDialog, ARefusedLineLeavesTheLabelAndSaysWhy)
{
    Document document;
    lotWithLabel(document);
    Runner runner(document);
    LabelEditDialog dialog(document, std::ref(runner));
    ASSERT_TRUE(dialog.load(2));
    child<QComboBox>(dialog, "labelEditLayer")->setCurrentText("nowhere");
    EXPECT_FALSE(dialog.apply());
    EXPECT_EQ(runner.lines, QStringList{"LABEL SET 2 layer=nowhere"});
    EXPECT_FALSE(dialog.status().isEmpty());
    EXPECT_EQ(document.model().entities.find(2)->layer, "0");
    // A field that makes no line is said before anything runs.
    child<QCheckBox>(dialog, "labelEditPinned")->setChecked(true);
    child<QLineEdit>(dialog, "labelEditEasting")->setText("east");
    EXPECT_FALSE(dialog.apply());
    EXPECT_EQ(runner.lines.size(), 1);
    EXPECT_EQ(dialog.status(), "Easting: 'east' is not a number");
}

// ---- the Label Layout Report ------------------------------------------------------------

TEST(LabelLayoutReport, TheReplysCountsAndSuppressedLabelsAreRead)
{
    const auto summary = katana::qt::parseLabelLayoutReply(
        "scale=500 considered=5 placed=1 displaced=0 suppressed=4 orphaned=1\n"
        "label=2 piece=0 x=1 y=2 candidate=0 displaced=no text=\"P 1\"\n"
        "label=4 piece=0 suppressed=yes\n"
        "label=6 piece=0 suppressed=yes\n"
        "label=6 piece=1 suppressed=yes\n");
    EXPECT_TRUE(summary.read);
    EXPECT_EQ(summary.scale, 500.0);
    EXPECT_EQ(summary.considered, 5u);
    EXPECT_EQ(summary.placed, 1u);
    EXPECT_EQ(summary.suppressed, 4u);
    EXPECT_EQ(summary.orphaned, 1u);
    EXPECT_EQ(summary.suppressedLabels, (std::vector<EntityId>{4, 6})) << "each label once";
    EXPECT_EQ(katana::qt::selectLine(summary.suppressedLabels), "SELECT 4 6");
    EXPECT_EQ(katana::qt::selectLine({}), "");
    EXPECT_FALSE(katana::qt::parseLabelLayoutReply("created labels=1 ids=5").read);
    EXPECT_EQ(katana::qt::labelLayoutLine(false), "LABEL LAYOUT collisions=off");
}

TEST(LabelLayoutReport, RunShowsTheCountsAndSelectsTheLabelsThatFoundNoRoom)
{
    // Five numbered points at one place, labelled in a style that may not
    // move its labels: the first is placed, the other four find no room
    // (the placer's own WithNoRoomALabelIsSuppressedAndCounted).
    Document document;
    run(document, "LABELSTYLE NEW pt kind=point text={point} displace=off");
    for (int i = 1; i <= 5; ++i) {
        run(document, "POINT 0,0");
        const EntityId id = document.lastCreatedEntities().front();
        run(document, "SELECT " + std::to_string(id));
        run(document, "PROP SET point P" + std::to_string(i));
    }
    run(document, "LABEL 1 2 3 4 5 style=pt"); // labels 6 to 10
    Runner runner(document);
    LabelLayoutReportDialog dialog(std::ref(runner));
    EXPECT_FALSE(child<QPushButton>(dialog, "labelLayoutSelectSuppressed")->isEnabled());
    child<QPushButton>(dialog, "labelLayoutRun")->click();
    ASSERT_EQ(runner.lines, QStringList{"LABEL LAYOUT"});
    EXPECT_EQ(child<QLabel>(dialog, "labelLayoutPlaced")->text(), "1");
    EXPECT_EQ(child<QLabel>(dialog, "labelLayoutSuppressed")->text(), "4");
    EXPECT_EQ(child<QLabel>(dialog, "labelLayoutOrphaned")->text(), "0");
    EXPECT_EQ(dialog.summary().suppressedLabels, (std::vector<EntityId>{7, 8, 9, 10}));
    ASSERT_TRUE(child<QPushButton>(dialog, "labelLayoutSelectSuppressed")->isEnabled());
    child<QPushButton>(dialog, "labelLayoutSelectSuppressed")->click();
    EXPECT_EQ(runner.lines.back(), "SELECT 7 8 9 10");
    EXPECT_EQ(document.selection().ids(), (std::vector<EntityId>{7, 8, 9, 10}));

    // Unticked, every label is put at its own place: none is suppressed.
    child<QCheckBox>(dialog, "labelLayoutCollisions")->setChecked(false);
    EXPECT_EQ(child<QLineEdit>(dialog, "labelLayoutCommand")->text(), "LABEL LAYOUT collisions=off");
    EXPECT_TRUE(dialog.run());
    EXPECT_EQ(child<QLabel>(dialog, "labelLayoutPlaced")->text(), "5");
    EXPECT_EQ(child<QLabel>(dialog, "labelLayoutSuppressed")->text(), "0");
    EXPECT_FALSE(child<QPushButton>(dialog, "labelLayoutSelectSuppressed")->isEnabled());
}

TEST(LabelLayoutReport, WithNoRunnerItSaysSoRatherThanShowingNothing)
{
    LabelLayoutReportDialog dialog{katana::qt::CommandRunner{}};
    EXPECT_FALSE(dialog.run());
    EXPECT_EQ(dialog.status(), "There is no command line to run it on.");
    EXPECT_EQ(child<QLabel>(dialog, "labelLayoutPlaced")->text(), "-");
}
