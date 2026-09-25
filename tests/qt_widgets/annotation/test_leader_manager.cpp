// The Leaders manager (src/katana_qt/annotation/leader_manager.hpp,
// docs/annotation.md "In the window"), driven by object name as a person
// clicks it: the form shows a leader and what the entity it is on offers,
// a template is checked and previewed as it is typed, and every button is
// one undo step - the same edit the LEADER verbs make.

#include <gtest/gtest.h>

#include <string>

#include <QAction>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMainWindow>
#include <QMenu>
#include <QPlainTextEdit>
#include <QTabWidget>
#include <QTableWidget>
#include <QTextCursor>
#include <QToolBar>

#include "annotation/annotation_workbench.hpp"
#include "annotation/leader_manager.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/leader_values.hpp"
#include "widget_harness.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::entity::EntityId;
using katana::entity::LeaderGeometry;
using katana::qt::LeaderManagerDialog;

namespace {

struct Drawing {
    Document document;
    CommandInterpreter interpreter{document};

    // Runs a command line, and says the id it made when it made one.
    EntityId run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << " -> " << reply.error().describe();
        const auto made = document.lastCreatedEntities();
        return made.empty() ? 0 : made.front();
    }
    [[nodiscard]] const LeaderGeometry& leader(EntityId id) const
    {
        return std::get<LeaderGeometry>(document.model().entities.find(id)->geometry);
    }
    [[nodiscard]] std::string says(EntityId id) const
    {
        return katana::entity::leaderNote(document.model(), leader(id));
    }
    [[nodiscard]] std::size_t steps() const { return document.history().undoCount(); }
};

template <typename Widget> Widget* child(QWidget& dialog, const char* name)
{
    auto* widget = dialog.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(widget, nullptr) << name;
    return widget;
}

// The rows of the values table, "name=text".
QStringList valueRows(QWidget& dialog)
{
    QStringList rows;
    auto* table = child<QTableWidget>(dialog, "leaderValues");
    for (int row = 0; row < table->rowCount(); ++row) {
        rows << table->item(row, 0)->text() + QStringLiteral("=") + table->item(row, 1)->text();
    }
    return rows;
}

void chooseData(QComboBox* box, int value)
{
    box->setCurrentIndex(box->findData(value));
}

constexpr int kText = 0;
constexpr int kTemplate = 1;
constexpr int kLabelStyle = 2;

} // namespace

TEST(LeaderManager, TheFormShowsTheLeaderWhatItSaysAndWhatItsEntityOffers)
{
    Drawing d;
    const EntityId pit = d.run("POINT 10,10");
    d.run("SELECT " + std::to_string(pit));
    d.run("PROP SET invert 10.5");
    d.run("PROP SET elevation 12.25");
    const EntityId leader =
        d.run("LEADER #" + std::to_string(pit) + " 20,20 template=\"IL {prop.invert:.2f}\"");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    EXPECT_EQ(dialog.currentLeader(), leader);
    EXPECT_EQ(child<QComboBox>(dialog, "leaderNoteKind")->currentData().toInt(), kTemplate);
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "leaderNote")->toPlainText(),
              QStringLiteral("IL {prop.invert:.2f}"));
    EXPECT_TRUE(child<QLabel>(dialog, "leaderTarget")->text().startsWith(QStringLiteral("Point ")))
        << child<QLabel>(dialog, "leaderTarget")->text().toStdString();
    EXPECT_EQ(dialog.preview(), QStringLiteral("IL 10.50"));
    const QStringList rows = valueRows(dialog);
    EXPECT_TRUE(rows.contains(QStringLiteral("rl=12.250"))) << rows.join(", ").toStdString();
    EXPECT_TRUE(rows.contains(QStringLiteral("prop.invert=10.500")))
        << rows.join(", ").toStdString();
    // The list names every leader by what it says.
    auto* list = child<QListWidget>(dialog, "leaderList");
    ASSERT_EQ(list->count(), 1);
    EXPECT_TRUE(list->item(0)->text().endsWith(QStringLiteral("IL 10.50")))
        << list->item(0)->text().toStdString();
    EXPECT_FALSE(dialog.showLeader(pit)) << "a point is no leader";
}

TEST(LeaderManager, ATemplateIsCheckedAsItIsTypedAndApplyIsOneStep)
{
    Drawing d;
    const EntityId pit = d.run("POINT 0,0");
    d.run("SELECT " + std::to_string(pit));
    d.run("PROP SET invert 10.5");
    const EntityId leader = d.run("LEADER #" + std::to_string(pit) + " 5,5 text=PIT");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    chooseData(child<QComboBox>(dialog, "leaderNoteKind"), kTemplate);
    auto* note = child<QPlainTextEdit>(dialog, "leaderNote");
    note->setPlainText(QStringLiteral("IL {invrt}"));
    EXPECT_TRUE(dialog.preview().contains(QStringLiteral("no value 'invrt'")))
        << dialog.preview().toStdString();
    const std::size_t before = d.steps();
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("invrt"))) << "said in the dialog";
    EXPECT_EQ(d.steps(), before);

    note->setPlainText(QStringLiteral("PIT {id}\nIL {prop.invert:.3f}"));
    EXPECT_EQ(dialog.preview(),
              QStringLiteral("PIT ") + QString::number(pit) + QStringLiteral("\nIL 10.500"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    EXPECT_TRUE(d.leader(leader).fields);
    EXPECT_EQ(d.says(leader), "PIT " + std::to_string(pit) + "\nIL 10.500");
    katana::qt::test::processEvents();
    ASSERT_TRUE(dialog.apply());
    EXPECT_EQ(d.steps(), before + 1) << "an unchanged form is no step";

    // The look, one more step.
    chooseData(child<QComboBox>(dialog, "leaderCallout"),
               static_cast<int>(katana::entity::CalloutShape::Box));
    child<QDoubleSpinBox>(dialog, "leaderPaperHeight")->setValue(3.5);
    ASSERT_TRUE(dialog.apply());
    EXPECT_EQ(d.leader(leader).callout, katana::entity::CalloutShape::Box);
    EXPECT_EQ(d.leader(leader).paperHeight, 3.5);
    EXPECT_EQ(d.steps(), before + 2);
}

TEST(LeaderManager, ADoubleClickedValueMakesTheNoteATemplate)
{
    Drawing d;
    const EntityId line = d.run("LINE 0,0 40,0");
    const EntityId leader = d.run("LEADER #" + std::to_string(line) + "@10,1 20,10 text=\"L=\"");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    auto* note = child<QPlainTextEdit>(dialog, "leaderNote");
    QTextCursor end = note->textCursor();
    end.movePosition(QTextCursor::End);
    note->setTextCursor(end);
    // As a double-click on the table's "length" row does.
    dialog.insertValue(QStringLiteral("length"));
    EXPECT_EQ(child<QComboBox>(dialog, "leaderNoteKind")->currentData().toInt(), kTemplate);
    EXPECT_EQ(note->toPlainText(), QStringLiteral("L={length}"));
    EXPECT_EQ(dialog.preview(), QStringLiteral("L=40.000"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.says(leader), "L=40.000");
}

TEST(LeaderManager, AnAttributeIsSetOnTheEntityThroughItsLeader)
{
    Drawing d;
    const EntityId pit = d.run("POINT 0,0");
    const EntityId leader =
        d.run("LEADER #" + std::to_string(pit) + " 5,5 template=\"PIT {id}\\nIL {prop.invert}\"");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    child<QLineEdit>(dialog, "leaderAttributeName")->setText(QStringLiteral("invert"));
    child<QLineEdit>(dialog, "leaderAttributeValue")->setText(QStringLiteral("9"));
    auto* type = child<QComboBox>(dialog, "leaderAttributeType");
    type->setCurrentIndex(type->findData(QStringLiteral("real")));
    const std::size_t before = d.steps();
    ASSERT_TRUE(dialog.setAttribute()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    const auto& properties = d.document.model().entities.find(pit)->properties;
    ASSERT_TRUE(properties.contains("invert"));
    EXPECT_EQ(std::get<double>(properties.at("invert")), 9.0) << "real, as stated";
    katana::qt::test::processEvents();
    EXPECT_TRUE(dialog.preview().endsWith(QStringLiteral("IL 9.000")))
        << dialog.preview().toStdString();
    EXPECT_TRUE(valueRows(dialog).contains(QStringLiteral("prop.invert=9.000")));
    ASSERT_TRUE(dialog.removeAttribute());
    EXPECT_FALSE(d.document.model().entities.find(pit)->properties.contains("invert"));
    child<QLineEdit>(dialog, "leaderAttributeValue")->setText(QStringLiteral("nine"));
    type->setCurrentIndex(type->findData(QStringLiteral("integer")));
    EXPECT_FALSE(dialog.setAttribute());
    EXPECT_FALSE(dialog.problem().isEmpty());
}

TEST(LeaderManager, FreezeKeepsTheWordsAndDetachLetsTheTipGo)
{
    Drawing d;
    const EntityId pit = d.run("POINT 0,0");
    const EntityId leader = d.run("LEADER #" + std::to_string(pit) + " 5,5 template=\"PIT {id}\"");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    ASSERT_TRUE(dialog.freeze());
    EXPECT_FALSE(katana::entity::isSmart(d.leader(leader)));
    EXPECT_EQ(d.leader(leader).text, "PIT " + std::to_string(pit));
    EXPECT_TRUE(d.leader(leader).tipRef.associated());
    katana::qt::test::processEvents();
    EXPECT_EQ(child<QComboBox>(dialog, "leaderNoteKind")->currentData().toInt(), kText)
        << "the form shows the leader as it now is";
    ASSERT_TRUE(dialog.detach());
    EXPECT_FALSE(d.leader(leader).tipRef.associated());
    katana::qt::test::processEvents();
    EXPECT_TRUE(
        child<QLabel>(dialog, "leaderTarget")->text().startsWith(QStringLiteral("Nothing")));
}

TEST(LeaderManager, TheFormFollowsTheSelectionAndAnUndoButKeepsUnappliedEdits)
{
    Drawing d;
    const EntityId first = d.run("LEADER 0,0 5,5 text=ONE");
    const EntityId second = d.run("LEADER 10,0 15,5 text=TWO");
    const EntityId line = d.run("LINE 100,0 110,0");
    LeaderManagerDialog dialog(d.document);
    d.run("SELECT " + std::to_string(second));
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.currentLeader(), second);

    // An edit elsewhere leaves unapplied edits in the form alone...
    auto* note = child<QPlainTextEdit>(dialog, "leaderNote");
    note->setPlainText(QStringLiteral("TWO, EDITED"));
    ASSERT_TRUE(d.document.execute(katana::commands::moveEntities({line}, {0.0, 1.0})).ok());
    katana::qt::test::processEvents();
    EXPECT_EQ(note->toPlainText(), QStringLiteral("TWO, EDITED"));
    // ...and an edit to the leader shows the leader as it now is.
    d.run("LEADER SET " + std::to_string(second) + " text=CHANGED");
    katana::qt::test::processEvents();
    EXPECT_EQ(note->toPlainText(), QStringLiteral("CHANGED"));
    d.run("UNDO");
    katana::qt::test::processEvents();
    EXPECT_EQ(note->toPlainText(), QStringLiteral("TWO"));

    // Erased, the form moves to a leader there still is.
    d.run("ERASE");
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.currentLeader(), first);
    EXPECT_EQ(child<QListWidget>(dialog, "leaderList")->count(), 1);
}

TEST(LeaderManager, ALabelStyleNoteLendsItsLookAndIsReadLive)
{
    Drawing d;
    const EntityId pit = d.run("POINT 0,0");
    d.run("SELECT " + std::to_string(pit));
    d.run("PROP SET invert 10.25");
    d.run("TEXTSTYLE NEW Notes paper=3.5");
    d.run(
        "LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.2f}\" textstyle=Notes paper=3");
    const EntityId leader = d.run("LEADER #" + std::to_string(pit) + " 5,5 text=X");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    auto* style = child<QComboBox>(dialog, "leaderLabelStyle");
    EXPECT_FALSE(style->isEnabled()) << "until the note is a style's";
    chooseData(child<QComboBox>(dialog, "leaderNoteKind"), kLabelStyle);
    EXPECT_TRUE(style->isEnabled());
    EXPECT_EQ(style->currentText(), QStringLiteral("Invert"));
    EXPECT_TRUE(child<QPlainTextEdit>(dialog, "leaderNote")->isReadOnly());
    EXPECT_EQ(child<QComboBox>(dialog, "leaderTextStyle")->currentData().toString(),
              QStringLiteral("Notes"));
    EXPECT_EQ(child<QDoubleSpinBox>(dialog, "leaderPaperHeight")->value(), 3.0);
    EXPECT_EQ(dialog.preview(), QStringLiteral("INV 10.25"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(leader).labelStyle, "Invert");
    EXPECT_EQ(d.leader(leader).style, "Notes");
    EXPECT_EQ(d.leader(leader).paperHeight, 3.0);
}

TEST(LeaderManager, ForSelectionMakesALeaderToEachSelectedEntityInOneStep)
{
    Drawing d;
    const EntityId a = d.run("POINT 0,0");
    const EntityId b = d.run("POINT 10,0");
    const EntityId dim = d.run("DIM 0,20 10,20 2");
    d.run("SELECT " + std::to_string(a) + " " + std::to_string(b) + " " + std::to_string(dim));
    LeaderManagerDialog dialog(d.document);
    dialog.showTab(LeaderManagerDialog::Tab::ForSelection);
    katana::qt::test::processEvents();
    EXPECT_EQ(child<QLabel>(dialog, "leaderForSelection")->text(),
              QStringLiteral("3 entities selected"));
    chooseData(child<QComboBox>(dialog, "leaderForNoteKind"), kTemplate);
    child<QPlainTextEdit>(dialog, "leaderForNote")->setPlainText(QStringLiteral("{type} {id}"));
    const std::size_t before = d.steps();
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    EXPECT_TRUE(dialog.forReport().startsWith(QStringLiteral("made=2 skipped=1")))
        << dialog.forReport().toStdString();
    const auto made = d.document.lastCreatedEntities();
    ASSERT_EQ(made.size(), 2u);
    EXPECT_EQ(d.says(made[0]), "Point " + std::to_string(a));

    // Balloons with no note are numbered.
    child<QPlainTextEdit>(dialog, "leaderForNote")->clear();
    chooseData(child<QComboBox>(dialog, "leaderForNoteKind"), kText);
    child<QCheckBox>(dialog, "leaderForBalloon")->setChecked(true);
    d.run("SELECT " + std::to_string(a) + " " + std::to_string(b));
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    const auto balloons = d.document.lastCreatedEntities();
    ASSERT_EQ(balloons.size(), 2u);
    EXPECT_EQ(d.leader(balloons[0]).text, "1");
    EXPECT_EQ(d.leader(balloons[1]).text, "2");
    EXPECT_EQ(d.leader(balloons[0]).callout, katana::entity::CalloutShape::Circle);

    d.run("SELECT NONE");
    EXPECT_FALSE(dialog.makeForSelection());
    EXPECT_FALSE(dialog.problem().isEmpty());
}

TEST(LeaderManager, ArrangeAlignsTheSelectedNotesAndRenumbersTheBalloons)
{
    Drawing d;
    d.run("ANNOSCALE 500");
    const EntityId a = d.run("LEADER 0,0 10,30 text=A");
    const EntityId b = d.run("LEADER 5,0 22,40 text=B");
    d.run("SELECT " + std::to_string(a) + " " + std::to_string(b));
    LeaderManagerDialog dialog(d.document);
    dialog.showTab(LeaderManagerDialog::Tab::Arrange);
    child<QCheckBox>(dialog, "leaderAlignUseSpacing")->setChecked(true);
    child<QDoubleSpinBox>(dialog, "leaderAlignSpacing")->setValue(8.0);
    ASSERT_TRUE(dialog.alignSelection()) << dialog.problem().toStdString();
    // B hangs highest and sets x = 22; 8 mm at 1:500 is 4 m below it.
    EXPECT_EQ(d.leader(b).vertices.back(), katana::geometry::Point2(22, 40));
    EXPECT_EQ(d.leader(a).vertices.back(), katana::geometry::Point2(22, 36));
    EXPECT_EQ(dialog.arrangeReport(), QStringLiteral("aligned=2"));

    d.run("BALLOON 30,0 35,5 n=7");
    d.run("BALLOON 10,20 15,25 n=3");
    auto* order = child<QComboBox>(dialog, "balloonRenumberOrder");
    order->setCurrentIndex(
        order->findData(static_cast<int>(katana::cad::annotation::BalloonOrder::X)));
    ASSERT_TRUE(dialog.renumberBalloons());
    EXPECT_EQ(dialog.arrangeReport(), QStringLiteral("balloons=2 renumbered=2"));
}

TEST(LeaderManager, TheAnnotateMenuOpensItOnEachTabWithLettersOfItsOwn)
{
    Document document;
    QMainWindow window;
    QMenu format;
    QToolBar toolBar;
    katana::qt::AnnotationWorkbench workbench(window, document, format, toolBar);
    // An Annotate menu whose tools have taken L and A already.
    QMenu annotate;
    annotate.addAction(QStringLiteral("&Leader"));
    annotate.addAction(QStringLiteral("&Aligned Dimension"));
    workbench.addLeaderActions(annotate);
    QStringList letters;
    QStringList names;
    for (const QAction* action : annotate.actions()) {
        if (action->isSeparator()) {
            continue;
        }
        const QString text = action->text();
        const auto at = text.indexOf(QLatin1Char('&'));
        ASSERT_GE(at, 0) << text.toStdString() << " has a menu letter";
        letters << text.mid(at + 1, 1).toUpper();
        names << action->objectName();
    }
    letters.removeDuplicates();
    EXPECT_EQ(letters.size(), 5) << "every item its own letter";
    for (const char* name :
         {"annotateLeaders", "annotateLeadersForSelection", "annotateArrangeLeaders"}) {
        EXPECT_TRUE(names.contains(QString::fromLatin1(name))) << name;
    }
    for (QAction* action : annotate.actions()) {
        if (action->objectName() == QStringLiteral("annotateArrangeLeaders")) {
            action->trigger();
        }
    }
    // No moc here (docs/desktop.md): found as the QDialog it is.
    auto* dialog = dynamic_cast<LeaderManagerDialog*>(
        window.findChild<QDialog*>(QStringLiteral("leaderManagerDialog")));
    ASSERT_NE(dialog, nullptr);
    EXPECT_EQ(dialog->findChild<QTabWidget*>(QStringLiteral("leaderTabs"))->currentIndex(), 2);
    EXPECT_EQ(&workbench.showLeaders(0), dialog) << "one kept instance";
    EXPECT_EQ(dialog->findChild<QTabWidget*>(QStringLiteral("leaderTabs"))->currentIndex(), 0);
}
