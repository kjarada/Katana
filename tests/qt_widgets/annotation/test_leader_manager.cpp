// The Leaders manager (src/katana_qt/annotation/leader_manager.hpp,
// docs/annotation.md "In the window"), driven by object name as a person
// clicks it: the form shows a leader and what the entity it is on offers,
// a template is checked and previewed as it is typed, and every button is
// one undo step - the same edit the LEADER verbs make.

#include <gtest/gtest.h>

#include <filesystem>
#include <string>
#include <utility>
#include <vector>

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
#include <QPushButton>
#include <QSpinBox>
#include <QTabWidget>
#include <QTableWidget>
#include <QToolBar>

#include "annotation/annotation_workbench.hpp"
#include "annotation/leader_manager.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/leader_values.hpp"
#include "widget_harness.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::entity::EntityId;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;
using katana::qt::LeaderManagerDialog;
using katana::qt::test::processEvents;

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

// The rows of a values table, "name=text".
QStringList valueRows(QWidget& dialog, const char* name = "leaderValues")
{
    QStringList rows;
    auto* table = child<QTableWidget>(dialog, name);
    for (int row = 0; row < table->rowCount(); ++row) {
        rows << table->item(row, 0)->text() + QStringLiteral("=") + table->item(row, 1)->text();
    }
    return rows;
}

// The row of a values table naming `name`; -1 for none.
int rowOf(QTableWidget* table, const QString& name)
{
    for (int row = 0; row < table->rowCount(); ++row) {
        if (table->item(row, 0)->text() == name) {
            return row;
        }
    }
    return -1;
}

void chooseData(QComboBox* box, int value)
{
    box->setCurrentIndex(box->findData(value));
}

constexpr int kBox = static_cast<int>(katana::entity::CalloutShape::Box);

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
    processEvents();
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
    auto* table = child<QTableWidget>(dialog, "leaderValues");
    const int length = rowOf(table, QStringLiteral("length"));
    ASSERT_GE(length, 0) << valueRows(dialog).join(", ").toStdString();
    // The cursor left where the form put it: the value goes at the END of
    // the note, after what it already says.
    emit table->cellDoubleClicked(length, 1);
    EXPECT_EQ(child<QComboBox>(dialog, "leaderNoteKind")->currentData().toInt(), kTemplate);
    EXPECT_EQ(note->toPlainText(), QStringLiteral("L={length}"));
    EXPECT_EQ(dialog.preview(), QStringLiteral("L=40.000"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.says(leader), "L=40.000");
}

TEST(LeaderManager, AnUntouchedFormIsNoStepAndWhatItsWidgetsCannotHoldIsKept)
{
    Drawing d;
    const EntityId line = d.run("LINE 0,0 40,0");
    // Values the form's widgets cannot hold exactly: a height and an arrow
    // finer than their 3 decimals, a landing past their largest size, a
    // place along finer than the percentage's decimals, and a non-breaking
    // space, which QPlainTextEdit::toPlainText() would make a plain one.
    const katana::entity::AnchorRef ref{line, katana::entity::AnchorPoint::Along, 0, 0.123456};
    const auto tip = katana::entity::resolveAnchor(*d.document.model().entities.find(line), ref);
    ASSERT_TRUE(tip);
    katana::entity::Entity entity;
    entity.geometry = LeaderGeometry{.vertices = {*tip, Point2(20, 10)},
                                     .text = "A B",
                                     .paperHeight = 2.3456,
                                     .arrowSize = 0.12345,
                                     .landing = 1500.0,
                                     .tipRef = ref};
    ASSERT_TRUE(d.document.execute(katana::commands::createEntities({entity})).ok());
    const EntityId leader = d.document.lastCreatedEntities().front();
    const LeaderGeometry drawn = d.leader(leader);
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    EXPECT_TRUE(dialog.formChange().empty());
    const std::size_t before = d.steps();
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before) << "an untouched form applied is no step";
    EXPECT_EQ(d.leader(leader), drawn);

    // One field changed is the one sent; everything else stays exactly.
    chooseData(child<QComboBox>(dialog, "leaderCallout"), kBox);
    const auto change = dialog.formChange();
    EXPECT_TRUE(change.callout);
    EXPECT_FALSE(change.note || change.arrow || change.textStyle || change.paperHeight ||
                 change.arrowSize || change.landing || change.tip || change.hang);
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    LeaderGeometry expected = drawn;
    expected.callout = katana::entity::CalloutShape::Box;
    EXPECT_EQ(d.leader(leader), expected);

    // Typed on, the note keeps its non-breaking space.
    processEvents();
    child<QPlainTextEdit>(dialog, "leaderNote")->insertPlainText(QStringLiteral("C"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(leader).text, "A BC");
    EXPECT_EQ(d.leader(leader).paperHeight, 2.3456);
    EXPECT_EQ(d.leader(leader).landing, 1500.0);
}

TEST(LeaderManager, ANoteMadeAStylesAndBackKeepsTheUsersWordsAndLook)
{
    Drawing d;
    const EntityId pit = d.run("POINT 0,0");
    d.run("SELECT " + std::to_string(pit));
    d.run("PROP SET invert 10.5");
    d.run("TEXTSTYLE NEW Notes paper=3.5");
    d.run(
        "LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.2f}\" textstyle=Notes paper=3");
    const EntityId leader = d.run("LEADER #" + std::to_string(pit) + " 5,5 text=X paper=2");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    auto* kind = child<QComboBox>(dialog, "leaderNoteKind");
    auto* note = child<QPlainTextEdit>(dialog, "leaderNote");
    auto* textStyle = child<QComboBox>(dialog, "leaderTextStyle");
    auto* paper = child<QDoubleSpinBox>(dialog, "leaderPaperHeight");
    note->setPlainText(QStringLiteral("MY WORDS"));

    chooseData(kind, kLabelStyle);
    EXPECT_EQ(note->toPlainText(), QStringLiteral("INV {prop.invert:.2f}"))
        << "the style's, to read";
    EXPECT_EQ(textStyle->currentData().toString(), QStringLiteral("Notes")) << "lent by the style";
    EXPECT_EQ(paper->value(), 3.0);
    EXPECT_EQ(dialog.preview(), QStringLiteral("INV 10.50"));

    chooseData(kind, kText);
    EXPECT_EQ(note->toPlainText(), QStringLiteral("MY WORDS")) << "the user's words back";
    EXPECT_EQ(textStyle->currentData().toString(), QString()) << "and the look before";
    EXPECT_EQ(paper->value(), 2.0);
    const auto change = dialog.formChange();
    ASSERT_TRUE(change.note);
    EXPECT_EQ(*change.note, (katana::cad::annotation::LeaderNote{
                                katana::cad::annotation::LeaderNote::Kind::Text, "MY WORDS"}));
    EXPECT_FALSE(change.textStyle || change.paperHeight) << "the look is as it was";
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(leader).text, "MY WORDS");
    EXPECT_EQ(d.leader(leader).style, "");
    EXPECT_EQ(d.leader(leader).paperHeight, 2.0);
}

// The drawing the look-lending cases share: a point with an invert, a text
// style, and label styles lending all of a look (Invert), none of it
// (Plain) and a height finer than the form's 3 decimals (Fine).
struct StyledDrawing : Drawing {
    EntityId pit = 0;
    StyledDrawing()
    {
        pit = run("POINT 0,0");
        run("SELECT " + std::to_string(pit));
        run("PROP SET invert 10.5");
        run("TEXTSTYLE NEW Notes paper=3.5");
        run("LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.2f}\" textstyle=Notes "
            "paper=3");
        run("LABELSTYLE NEW Plain kind=point text=\"P {prop.invert:.1f}\"");
        run("LABELSTYLE NEW Fine kind=point text=\"F {id}\" paper=2.3456");
    }
    // A plain leader on the point, 2 mm high in the default text style.
    EntityId plainLeader(const char* hang)
    {
        return run("LEADER #" + std::to_string(pit) + " " + hang + " text=X paper=2");
    }
};

void chooseStyle(QWidget& dialog, const char* name)
{
    chooseData(child<QComboBox>(dialog, "leaderNoteKind"), kLabelStyle);
    auto* style = child<QComboBox>(dialog, "leaderLabelStyle");
    style->setCurrentIndex(style->findText(QString::fromLatin1(name)));
}

void chooseTextStyle(QComboBox* box, const char* name)
{
    box->setCurrentIndex(box->findData(QString::fromLatin1(name)));
}

TEST(LeaderManager, ALookSetBackOverAStylesIsKeptAndAnotherStyleLendsOverTheOneBefore)
{
    StyledDrawing d;
    const EntityId kept = d.plainLeader("5,5");
    LeaderManagerDialog dialog(d.document);
    auto* textStyle = child<QComboBox>(dialog, "leaderTextStyle");
    auto* paper = child<QDoubleSpinBox>(dialog, "leaderPaperHeight");

    // The style's words, the leader's own look: set back over the lent one,
    // it is what Apply gives, though the style would lend its own.
    ASSERT_TRUE(dialog.showLeader(kept));
    chooseStyle(dialog, "Invert");
    EXPECT_EQ(textStyle->currentData().toString(), QStringLiteral("Notes"));
    EXPECT_EQ(paper->value(), 3.0);
    chooseTextStyle(textStyle, "");
    paper->setValue(2.0);
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(kept).labelStyle, "Invert");
    EXPECT_EQ(d.leader(kept).style, "");
    EXPECT_EQ(d.leader(kept).paperHeight, 2.0);

    // A style lending nothing, chosen after one lending all: the look from
    // before either, as LEADER SET labelstyle=Plain gives.
    const EntityId other = d.plainLeader("5,-5");
    const EntityId typed = d.plainLeader("-5,-5");
    ASSERT_TRUE(dialog.showLeader(other));
    chooseStyle(dialog, "Invert");
    chooseStyle(dialog, "Plain");
    EXPECT_EQ(textStyle->currentData().toString(), QString());
    EXPECT_EQ(paper->value(), 2.0);
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    d.run("LEADER SET " + std::to_string(typed) + " labelstyle=Plain");
    EXPECT_EQ(d.leader(other).labelStyle, "Plain");
    EXPECT_EQ(d.leader(other).style, d.leader(typed).style);
    EXPECT_EQ(d.leader(other).paperHeight, d.leader(typed).paperHeight);
    EXPECT_EQ(d.leader(other).paperHeight, 2.0);
}

TEST(LeaderManager, ALookTypedAfterALendIsKeptAndAStylesHeightIsLentExactly)
{
    StyledDrawing d;
    const EntityId typed = d.plainLeader("5,5");
    const EntityId fine = d.plainLeader("5,-5");
    LeaderManagerDialog dialog(d.document);
    auto* textStyle = child<QComboBox>(dialog, "leaderTextStyle");
    auto* paper = child<QDoubleSpinBox>(dialog, "leaderPaperHeight");

    // Back from the style, only what still shows the style's is put back.
    ASSERT_TRUE(dialog.showLeader(typed));
    chooseStyle(dialog, "Invert");
    paper->setValue(5.0);
    chooseData(child<QComboBox>(dialog, "leaderNoteKind"), kText);
    EXPECT_EQ(textStyle->currentData().toString(), QString()) << "lent, so put back";
    EXPECT_EQ(paper->value(), 5.0) << "the user's own since the lend";
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(typed).text, "X");
    EXPECT_EQ(d.leader(typed).style, "");
    EXPECT_EQ(d.leader(typed).paperHeight, 5.0);

    // A height the box shows to 3 decimals is lent whole, as labelstyle= does.
    ASSERT_TRUE(dialog.showLeader(fine));
    chooseStyle(dialog, "Fine");
    EXPECT_EQ(paper->value(), 2.346);
    EXPECT_FALSE(dialog.formChange().paperHeight) << "the style's own: left to the style";
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(fine).paperHeight, 2.3456);
}

TEST(LeaderManager, AStyleThatComesOrGoesUnderTheFormLendsItsLook)
{
    Drawing d;
    const EntityId pit = d.run("POINT 0,0");
    d.run("SELECT " + std::to_string(pit));
    d.run("PROP SET invert 10.5");
    d.run("TEXTSTYLE NEW Notes paper=3.5");
    const EntityId leader = d.run("LEADER #" + std::to_string(pit) + " 5,5 text=X paper=2");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    auto* style = child<QComboBox>(dialog, "leaderLabelStyle");
    auto* textStyle = child<QComboBox>(dialog, "leaderTextStyle");
    auto* paper = child<QDoubleSpinBox>(dialog, "leaderPaperHeight");
    chooseData(child<QComboBox>(dialog, "leaderNoteKind"), kLabelStyle);
    EXPECT_EQ(style->count(), 0);
    EXPECT_FALSE(dialog.apply()) << "there is no style to be";

    // The first style made while the form waits for one: named, and lending.
    d.run("LABELSTYLE NEW Zed kind=point text=\"Z {prop.invert:.1f}\" textstyle=Notes paper=3");
    processEvents();
    EXPECT_EQ(style->currentText(), QStringLiteral("Zed"));
    EXPECT_EQ(textStyle->currentData().toString(), QStringLiteral("Notes"));
    EXPECT_EQ(paper->value(), 3.0);
    EXPECT_EQ(dialog.preview(), QStringLiteral("Z 10.5"));

    // Another made leaves the choice; the chosen one deleted moves it, and
    // what that one lent goes with it.
    d.run("LABELSTYLE NEW Alpha kind=point text=\"A {prop.invert:.1f}\"");
    processEvents();
    EXPECT_EQ(style->currentText(), QStringLiteral("Zed"));
    d.run("LABELSTYLE DELETE Zed");
    processEvents();
    EXPECT_EQ(style->currentText(), QStringLiteral("Alpha"));
    EXPECT_EQ(textStyle->currentData().toString(), QString());
    EXPECT_EQ(paper->value(), 2.0);
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(leader).labelStyle, "Alpha");
    EXPECT_EQ(d.leader(leader).style, "");
    EXPECT_EQ(d.leader(leader).paperHeight, 2.0);
}

TEST(LeaderManager, ForSelectionShowsTheLookAStyleLendsAndMakesWhatItShows)
{
    StyledDrawing d;
    const EntityId other = d.run("POINT 20,0");
    d.run("SELECT " + std::to_string(other));
    d.run("PROP SET invert 9.25");
    d.run("SELECT " + std::to_string(d.pit));
    LeaderManagerDialog dialog(d.document);
    dialog.showTab(LeaderManagerDialog::Tab::ForSelection);
    auto* kind = child<QComboBox>(dialog, "leaderForNoteKind");
    auto* style = child<QComboBox>(dialog, "leaderForLabelStyle");
    auto* textStyle = child<QComboBox>(dialog, "leaderForTextStyle");
    auto* paper = child<QDoubleSpinBox>(dialog, "leaderForPaperHeight");
    chooseData(kind, kLabelStyle);
    style->setCurrentIndex(style->findText(QStringLiteral("Fine")));
    EXPECT_EQ(paper->value(), 2.346) << "Fine's height, to the box's decimals";
    style->setCurrentIndex(style->findText(QStringLiteral("Invert")));
    EXPECT_EQ(textStyle->currentData().toString(), QStringLiteral("Notes"));
    EXPECT_EQ(paper->value(), 3.0);
    EXPECT_EQ(dialog.forPreview(), QStringLiteral("INV 10.50"));
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    const EntityId lent = d.document.lastCreatedEntities().front();
    EXPECT_EQ(d.leader(lent).style, "Notes");
    EXPECT_EQ(d.leader(lent).paperHeight, 3.0);

    // Set to the default face and the text style's height, that is what the
    // leader gets - not the style's, lent over what the boxes show.
    chooseTextStyle(textStyle, "");
    paper->setValue(0.0);
    d.run("SELECT " + std::to_string(other));
    processEvents();
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    const EntityId own = d.document.lastCreatedEntities().front();
    EXPECT_EQ(d.leader(own).labelStyle, "Invert");
    EXPECT_EQ(d.leader(own).style, "");
    EXPECT_EQ(d.leader(own).paperHeight, 0.0);

    // Lent again, then the note a template: the look before comes back.
    style->setCurrentIndex(style->findText(QStringLiteral("Plain")));
    style->setCurrentIndex(style->findText(QStringLiteral("Invert")));
    EXPECT_EQ(textStyle->currentData().toString(), QStringLiteral("Notes"));
    chooseData(kind, kTemplate);
    EXPECT_EQ(textStyle->currentData().toString(), QString());
    EXPECT_EQ(paper->value(), 0.0);
}

TEST(LeaderManager, ANumberedBalloonMustStayACircle)
{
    Drawing d;
    const EntityId a = d.run("POINT 0,0");
    d.run("SELECT " + std::to_string(a));
    LeaderManagerDialog dialog(d.document);
    dialog.showTab(LeaderManagerDialog::Tab::ForSelection);
    child<QCheckBox>(dialog, "leaderForBalloon")->setChecked(true);
    chooseData(child<QComboBox>(dialog, "leaderForCallout"), kBox);
    // A box's number would not be counted, and so be given again.
    EXPECT_TRUE(dialog.forPreview().startsWith(QStringLiteral("a numbered balloon is a circle")))
        << dialog.forPreview().toStdString();
    const std::size_t before = d.steps();
    EXPECT_FALSE(dialog.makeForSelection());
    EXPECT_TRUE(dialog.problem().startsWith(QStringLiteral("a numbered balloon is a circle")))
        << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before);
    // With a note it is not numbered, and may be a box.
    child<QPlainTextEdit>(dialog, "leaderForNote")->setPlainText(QStringLiteral("A"));
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(d.document.lastCreatedEntities().front()).callout,
              katana::entity::CalloutShape::Box);
}

TEST(LeaderManager, AStylesNoteIsReadAfreshWhenTheStyleChanges)
{
    Drawing d;
    const EntityId pit = d.run("POINT 0,0");
    d.run("SELECT " + std::to_string(pit));
    d.run("PROP SET invert 10.5");
    d.run("LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.2f}\"");
    const EntityId leader = d.run("LEADER #" + std::to_string(pit) + " 5,5 labelstyle=Invert");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    auto* note = child<QPlainTextEdit>(dialog, "leaderNote");
    EXPECT_TRUE(note->isReadOnly());
    EXPECT_EQ(note->toPlainText(), QStringLiteral("INV {prop.invert:.2f}"));
    EXPECT_EQ(dialog.preview(), QStringLiteral("INV 10.50"));
    // The leader is as it was; its style is not.
    d.run("LABELSTYLE SET Invert text=\"IL {prop.invert:.3f}\"");
    processEvents();
    EXPECT_EQ(note->toPlainText(), QStringLiteral("IL {prop.invert:.3f}"));
    EXPECT_EQ(dialog.preview(), QStringLiteral("IL 10.500"));
    EXPECT_TRUE(dialog.formChange().empty()) << "nothing of the leader's changed";
}

TEST(LeaderManager, TheListChoosesAndFreezeWaitsForTheFormToBeApplied)
{
    Drawing d;
    const EntityId pit = d.run("POINT 0,0");
    const EntityId first = d.run("LEADER 20,0 25,5 text=ONE");
    const EntityId second = d.run("LEADER #" + std::to_string(pit) + " 5,5 template=\"PIT {id}\"");
    LeaderManagerDialog dialog(d.document);
    EXPECT_EQ(dialog.currentLeader(), first) << "the first there is, with nothing selected";
    auto* list = child<QListWidget>(dialog, "leaderList");
    ASSERT_EQ(list->count(), 2);
    list->setCurrentRow(1);
    EXPECT_EQ(dialog.currentLeader(), second);
    auto* freeze = child<QPushButton>(dialog, "leaderFreeze");
    auto* detach = child<QPushButton>(dialog, "leaderDetach");
    EXPECT_TRUE(freeze->isEnabled());
    EXPECT_TRUE(detach->isEnabled());

    // Freezing keeps what the leader says now, not the form's unapplied
    // note: while there is one, it waits.
    auto* note = child<QPlainTextEdit>(dialog, "leaderNote");
    note->insertPlainText(QStringLiteral("!"));
    EXPECT_FALSE(freeze->isEnabled());
    EXPECT_FALSE(detach->isEnabled());
    EXPECT_TRUE(freeze->toolTip().contains(QStringLiteral("not applied")));
    const std::size_t before = d.steps();
    EXPECT_FALSE(dialog.freeze());
    EXPECT_FALSE(dialog.detach());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("not applied")))
        << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before);
    EXPECT_TRUE(katana::entity::isSmart(d.leader(second)));

    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    processEvents();
    EXPECT_TRUE(freeze->isEnabled());
    ASSERT_TRUE(dialog.freeze());
    EXPECT_EQ(d.steps(), before + 2);
    EXPECT_EQ(d.leader(second).text, "PIT " + std::to_string(pit) + "!");
    EXPECT_FALSE(katana::entity::isSmart(d.leader(second)));
}

TEST(LeaderManager, SeveralSelectedLeadersAreChangedTogetherInOneStep)
{
    Drawing d;
    const EntityId pit = d.run("POINT 0,0");
    const EntityId a = d.run("LEADER #" + std::to_string(pit) + " 5,5 template=\"A {id}\"");
    const EntityId b = d.run("LEADER #" + std::to_string(pit) + " 5,-5 template=\"B {id}\"");
    const EntityId c = d.run("LEADER 30,0 35,5 text=C");
    d.run("SELECT " + std::to_string(a) + " " + std::to_string(b));
    LeaderManagerDialog dialog(d.document);
    EXPECT_EQ(dialog.currentLeader(), a);
    EXPECT_EQ(dialog.targets(), (std::vector<EntityId>{a, b}));
    auto* scope = child<QLabel>(dialog, "leaderScope");
    EXPECT_TRUE(scope->isVisibleTo(&dialog));
    EXPECT_TRUE(scope->text().contains(QStringLiteral("2 selected leaders")))
        << scope->text().toStdString();

    chooseData(child<QComboBox>(dialog, "leaderCallout"), kBox);
    const std::size_t before = d.steps();
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    EXPECT_EQ(d.leader(a).callout, katana::entity::CalloutShape::Box);
    EXPECT_EQ(d.leader(b).callout, katana::entity::CalloutShape::Box);
    EXPECT_EQ(d.leader(c).callout, katana::entity::CalloutShape::None) << "not selected";
    EXPECT_EQ(d.leader(b).text, "B {id}") << "only what was changed is sent";

    processEvents();
    ASSERT_TRUE(dialog.freeze()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 2);
    EXPECT_EQ(d.leader(a).text, "A " + std::to_string(pit));
    EXPECT_EQ(d.leader(b).text, "B " + std::to_string(pit));
    processEvents();
    ASSERT_TRUE(dialog.detach()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 3);
    EXPECT_FALSE(d.leader(a).tipRef.associated());
    EXPECT_FALSE(d.leader(b).tipRef.associated());

    // One selected: the form's leader alone.
    d.run("SELECT " + std::to_string(b));
    processEvents();
    EXPECT_EQ(dialog.targets(), (std::vector<EntityId>{b}));
    EXPECT_FALSE(scope->isVisibleTo(&dialog));
}

TEST(LeaderManager, AttachToSelectedPutsTheTipOnTheEntityAndAlongMovesIt)
{
    Drawing d;
    // Made first, so selected it comes before the line: it offers no place
    // for a tip and is passed over.
    const EntityId dim = d.run("DIM 0,20 10,20 2");
    const EntityId line = d.run("LINE 0,0 40,0");
    const EntityId leader = d.run("LEADER 10,1 20,10 text=X");
    LeaderManagerDialog dialog(d.document);
    auto* attach = child<QPushButton>(dialog, "leaderAttachSelected");
    auto* along = child<QDoubleSpinBox>(dialog, "leaderAlong");
    EXPECT_FALSE(attach->isEnabled()) << "nothing else is selected";
    EXPECT_FALSE(along->isEnabled()) << "a plain leader is along nothing";
    d.run("SELECT " + std::to_string(dim) + " " + std::to_string(leader) + " " +
          std::to_string(line));
    processEvents();
    ASSERT_EQ(dialog.currentLeader(), leader);
    EXPECT_TRUE(attach->isEnabled());

    // Attaching acts on the leader as drawn: with the form changed, it waits.
    auto* note = child<QPlainTextEdit>(dialog, "leaderNote");
    note->insertPlainText(QStringLiteral("!"));
    EXPECT_FALSE(attach->isEnabled());
    EXPECT_TRUE(attach->toolTip().contains(QStringLiteral("not applied")));
    const std::size_t before = d.steps();
    EXPECT_FALSE(dialog.attachToSelected());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("not applied")))
        << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before);
    EXPECT_FALSE(d.leader(leader).tipRef.associated());
    note->setPlainText(QStringLiteral("X"));
    EXPECT_TRUE(dialog.formChange().empty()) << "back to what it showed";
    EXPECT_TRUE(attach->isEnabled());

    ASSERT_TRUE(dialog.attachToSelected()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    // The tip (10,1) is over a quarter of the 40-long line: its foot (10,0).
    const LeaderGeometry& attached = d.leader(leader);
    EXPECT_EQ(attached.tipRef.entity, line);
    EXPECT_EQ(attached.tipRef.point, katana::entity::AnchorPoint::Along);
    EXPECT_EQ(attached.tipRef.parameter, 0.25);
    EXPECT_EQ(attached.vertices.front(), Point2(10, 0));

    processEvents();
    ASSERT_TRUE(dialog.attachToSelected()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1) << "on that place already: no step";
    EXPECT_TRUE(along->isEnabled());
    EXPECT_EQ(along->value(), 25.0);
    EXPECT_TRUE(
        child<QLabel>(dialog, "leaderTarget")->text().endsWith(QStringLiteral("25.0% along it")))
        << child<QLabel>(dialog, "leaderTarget")->text().toStdString();
    along->setValue(75.0);
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 2);
    EXPECT_EQ(d.leader(leader).tipRef.parameter, 0.75);
    EXPECT_EQ(d.leader(leader).vertices.front(), Point2(30, 0));
    EXPECT_EQ(d.leader(leader).vertices.back(), Point2(20, 10)) << "the note hangs where it did";

    // Along is one leader's: changed, then another leader selected with it,
    // Apply refuses rather than move both tips to one place.
    const EntityId other = d.run("LEADER 50,0 55,5 text=Y");
    const std::size_t made = d.steps();
    d.run("SELECT " + std::to_string(leader));
    processEvents();
    along->setValue(50.0);
    d.run("SELECT " + std::to_string(leader) + " " + std::to_string(other));
    processEvents();
    ASSERT_EQ(dialog.targets(), (std::vector<EntityId>{leader, other}));
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(dialog.problem().startsWith(QStringLiteral("Along moves one leader's tip")))
        << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), made);
    EXPECT_EQ(d.leader(other).vertices.front(), Point2(50, 0));
    EXPECT_FALSE(d.leader(other).tipRef.associated());
    // Set back, the rest of the form applies to both.
    EXPECT_TRUE(along->isEnabled());
    along->setValue(75.0);
    chooseData(child<QComboBox>(dialog, "leaderCallout"), kBox);
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(other).callout, katana::entity::CalloutShape::Box);
    EXPECT_EQ(d.leader(leader).tipRef.parameter, 0.75);
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
    processEvents();
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
    processEvents();
    EXPECT_EQ(child<QComboBox>(dialog, "leaderNoteKind")->currentData().toInt(), kText)
        << "the form shows the leader as it now is";
    ASSERT_TRUE(dialog.detach());
    EXPECT_FALSE(d.leader(leader).tipRef.associated());
    processEvents();
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
    processEvents();
    EXPECT_EQ(dialog.currentLeader(), second);

    // An edit elsewhere leaves unapplied edits in the form alone...
    auto* note = child<QPlainTextEdit>(dialog, "leaderNote");
    note->setPlainText(QStringLiteral("TWO, EDITED"));
    ASSERT_TRUE(d.document.execute(katana::commands::moveEntities({line}, {0.0, 1.0})).ok());
    processEvents();
    EXPECT_EQ(note->toPlainText(), QStringLiteral("TWO, EDITED"));
    // ...and an edit to the leader shows the leader as it now is.
    d.run("LEADER SET " + std::to_string(second) + " text=CHANGED");
    processEvents();
    EXPECT_EQ(note->toPlainText(), QStringLiteral("CHANGED"));
    d.run("UNDO");
    processEvents();
    EXPECT_EQ(note->toPlainText(), QStringLiteral("TWO"));

    // Erased, the form moves to a leader there still is.
    d.run("ERASE");
    processEvents();
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
    // The dimension first: the first selected offers no place for a leader,
    // so the preview passes over it to a, and not on to b.
    const EntityId dim = d.run("DIM 0,20 10,20 2");
    const EntityId a = d.run("POINT 0,0");
    const EntityId b = d.run("POINT 10,0");
    d.run("SELECT " + std::to_string(dim) + " " + std::to_string(a) + " " + std::to_string(b));
    LeaderManagerDialog dialog(d.document);
    dialog.showTab(LeaderManagerDialog::Tab::ForSelection);
    processEvents();
    EXPECT_EQ(child<QLabel>(dialog, "leaderForSelection")->text(),
              QStringLiteral("3 entities selected"));
    chooseData(child<QComboBox>(dialog, "leaderForNoteKind"), kTemplate);
    child<QPlainTextEdit>(dialog, "leaderForNote")->setPlainText(QStringLiteral("{type} {id}"));
    EXPECT_EQ(dialog.forPreview(), QStringLiteral("Point ") + QString::number(a));
    EXPECT_EQ(child<QLabel>(dialog, "leaderForCheck")->text(),
              QStringLiteral("For Point ") + QString::number(a) +
                  QStringLiteral(", the first selected:"));
    EXPECT_TRUE(
        valueRows(dialog, "leaderForValues").contains(QStringLiteral("id=") + QString::number(a)))
        << valueRows(dialog, "leaderForValues").join(", ").toStdString();
    const std::size_t before = d.steps();
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    EXPECT_TRUE(dialog.forReport().startsWith(QStringLiteral("made=2 skipped=1")))
        << dialog.forReport().toStdString();
    const auto made = d.document.lastCreatedEntities();
    ASSERT_EQ(made.size(), 2u);
    EXPECT_EQ(d.says(made[0]), "Point " + std::to_string(a));

    // What was made is selected, and so shown on the Leader tab.
    EXPECT_EQ(d.document.selection().ids(), made);
    processEvents();
    EXPECT_EQ(dialog.currentLeader(), made[0]);

    // Balloons with no note are numbered, whatever the kind the box is at.
    for (const int kind : {kText, kTemplate}) {
        child<QPlainTextEdit>(dialog, "leaderForNote")->clear();
        chooseData(child<QComboBox>(dialog, "leaderForNoteKind"), kind);
        child<QCheckBox>(dialog, "leaderForBalloon")->setChecked(true);
        d.run("SELECT " + std::to_string(a) + " " + std::to_string(b));
        processEvents();
        const int next = kind == kText ? 1 : 3;
        EXPECT_EQ(dialog.forPreview(), QString::number(next));
        ASSERT_TRUE(dialog.makeForSelection()) << kind << ": " << dialog.problem().toStdString();
        const auto balloons = d.document.lastCreatedEntities();
        ASSERT_EQ(balloons.size(), 2u);
        EXPECT_EQ(d.leader(balloons[0]).text, std::to_string(next));
        EXPECT_EQ(d.leader(balloons[1]).text, std::to_string(next + 1));
        EXPECT_EQ(d.leader(balloons[0]).callout, katana::entity::CalloutShape::Circle);
    }

    d.run("SELECT NONE");
    const std::size_t steps = d.steps();
    EXPECT_FALSE(dialog.makeForSelection());
    EXPECT_TRUE(dialog.problem().startsWith(QStringLiteral("nothing is selected")))
        << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), steps);
}

TEST(LeaderManager, ForSelectionIsPreviewedForTheFirstAndPlacedAsTold)
{
    Drawing d;
    d.run("ANNOSCALE 500");
    d.run("TEXTSTYLE NEW Notes paper=3.5");
    const EntityId pit = d.run("POINT 0,0");
    d.run("SELECT " + std::to_string(pit));
    d.run("PROP SET invert 10.5");
    LeaderManagerDialog dialog(d.document);
    dialog.showTab(LeaderManagerDialog::Tab::ForSelection);
    chooseData(child<QComboBox>(dialog, "leaderForNoteKind"), kTemplate);
    auto* note = child<QPlainTextEdit>(dialog, "leaderForNote");
    note->insertPlainText(QStringLiteral("IL {prop.invert:.2f} "));
    EXPECT_EQ(dialog.forPreview(), QStringLiteral("IL 10.50 "));
    EXPECT_EQ(child<QLabel>(dialog, "leaderForCheck")->text(),
              QStringLiteral("For Point ") + QString::number(pit) +
                  QStringLiteral(", the first selected:"));
    EXPECT_TRUE(valueRows(dialog, "leaderForValues").contains(QStringLiteral("prop.invert=10.500")))
        << valueRows(dialog, "leaderForValues").join(", ").toStdString();
    // A double-clicked value goes where the user was typing.
    auto* values = child<QTableWidget>(dialog, "leaderForValues");
    const int id = rowOf(values, QStringLiteral("id"));
    ASSERT_GE(id, 0);
    emit values->cellDoubleClicked(id, 0);
    EXPECT_EQ(note->toPlainText(), QStringLiteral("IL {prop.invert:.2f} {id}"));
    EXPECT_EQ(dialog.forPreview(), QStringLiteral("IL 10.50 ") + QString::number(pit));
    note->insertPlainText(QStringLiteral("{nothing}"));
    EXPECT_TRUE(dialog.forPreview().contains(QStringLiteral("no value 'nothing'")))
        << dialog.forPreview().toStdString();
    note->setPlainText(QStringLiteral("IL {prop.invert:.2f} {id}"));
    EXPECT_EQ(dialog.forPreview(), QStringLiteral("IL 10.50 ") + QString::number(pit));

    child<QDoubleSpinBox>(dialog, "leaderForAngle")->setValue(90.0);
    child<QDoubleSpinBox>(dialog, "leaderForLength")->setValue(10.0);
    chooseData(child<QComboBox>(dialog, "leaderForCallout"), kBox);
    chooseData(child<QComboBox>(dialog, "leaderForArrow"),
               static_cast<int>(katana::entity::ArrowHead::Open));
    auto* textStyle = child<QComboBox>(dialog, "leaderForTextStyle");
    textStyle->setCurrentIndex(textStyle->findData(QStringLiteral("Notes")));
    // Not the text style's 3.5: the box's own is what is sent.
    child<QDoubleSpinBox>(dialog, "leaderForPaperHeight")->setValue(2.0);
    auto* arrowSize = child<QDoubleSpinBox>(dialog, "leaderForArrowSize");
    auto* landing = child<QDoubleSpinBox>(dialog, "leaderForLanding");
    EXPECT_EQ(arrowSize->value(), LeaderGeometry{}.arrowSize) << "a new leader's own, to start";
    EXPECT_EQ(landing->value(), LeaderGeometry{}.landing);
    arrowSize->setValue(4.0);
    landing->setValue(0.0);
    const std::size_t before = d.steps();
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    EXPECT_EQ(dialog.forReport(), QStringLiteral("made=1 skipped=0"));
    const auto made = d.document.lastCreatedEntities();
    ASSERT_EQ(made.size(), 1u);
    const LeaderGeometry& shape = d.leader(made[0]);
    // 10 mm on paper at 1:500 is 5 m, straight up (90 degrees) from the point.
    EXPECT_EQ(shape.vertices.front(), Point2(0, 0));
    EXPECT_NEAR(shape.vertices.back().x, 0.0, 1e-12);
    EXPECT_NEAR(shape.vertices.back().y, 5.0, 1e-12);
    EXPECT_EQ(shape.callout, katana::entity::CalloutShape::Box);
    EXPECT_EQ(shape.arrow, katana::entity::ArrowHead::Open);
    EXPECT_EQ(shape.style, "Notes");
    EXPECT_EQ(shape.paperHeight, 2.0);
    EXPECT_EQ(shape.arrowSize, 4.0);
    EXPECT_EQ(shape.landing, 0.0) << "none";
    EXPECT_EQ(d.says(made[0]), "IL 10.50 " + std::to_string(pit));
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

    d.run("SELECT NONE");
    EXPECT_FALSE(dialog.alignSelection());
    EXPECT_TRUE(dialog.problem().startsWith(QStringLiteral("no leader is selected")))
        << dialog.problem().toStdString();

    const EntityId seven = d.run("BALLOON 30,0 35,5 n=7");
    const EntityId three = d.run("BALLOON 10,20 15,25 n=3");
    auto* order = child<QComboBox>(dialog, "balloonRenumberOrder");
    order->setCurrentIndex(
        order->findData(static_cast<int>(katana::cad::annotation::BalloonOrder::X)));
    child<QSpinBox>(dialog, "balloonRenumberStart")->setValue(5);
    const std::size_t before = d.steps();
    ASSERT_TRUE(dialog.renumberBalloons()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    // Across by tip: the one at x = 10 first, from 5.
    EXPECT_EQ(d.leader(three).text, "5");
    EXPECT_EQ(d.leader(seven).text, "6");
    EXPECT_EQ(dialog.arrangeReport(), QStringLiteral("balloons=2 renumbered=2"));
    ASSERT_TRUE(dialog.renumberBalloons());
    EXPECT_EQ(d.steps(), before + 1) << "numbered so already: no step";
    EXPECT_EQ(dialog.arrangeReport(), QStringLiteral("balloons=2 renumbered=0"));
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
    // Each item opens the one kept dialog on its own tab.
    LeaderManagerDialog* dialog = nullptr;
    const std::pair<const char*, int> items[] = {
        {"annotateArrangeLeaders", 2}, {"annotateLeadersForSelection", 1}, {"annotateLeaders", 0}};
    for (const auto& [name, tab] : items) {
        for (QAction* action : annotate.actions()) {
            if (action->objectName() == QString::fromLatin1(name)) {
                action->trigger();
            }
        }
        // No moc here (docs/desktop.md): found as the QDialog it is.
        auto* shown = dynamic_cast<LeaderManagerDialog*>(
            window.findChild<QDialog*>(QStringLiteral("leaderManagerDialog")));
        ASSERT_NE(shown, nullptr) << name;
        EXPECT_TRUE(dialog == nullptr || shown == dialog) << name << ": one kept instance";
        dialog = shown;
        EXPECT_EQ(dialog->findChild<QTabWidget*>(QStringLiteral("leaderTabs"))->currentIndex(), tab)
            << name;
    }
    EXPECT_EQ(window.findChildren<QDialog*>(QStringLiteral("leaderManagerDialog")).size(), 1);
    EXPECT_EQ(&workbench.showLeaders(2), dialog);
}

TEST(LeaderManager, ADrawingReplacedStartsTheFormAfresh)
{
    const std::filesystem::path directory = std::filesystem::temp_directory_path() /
                                            "katana-qt-tests-leader-manager" / "replaced.katana";
    std::filesystem::remove_all(directory.parent_path());
    {
        Drawing d;
        const EntityId leader = d.run("LEADER 0,0 5,5 text=ONE");
        ASSERT_TRUE(d.document.saveAs(directory).ok());
        LeaderManagerDialog dialog(d.document);
        ASSERT_EQ(dialog.currentLeader(), leader);
        auto* note = child<QPlainTextEdit>(dialog, "leaderNote");
        note->setPlainText(QStringLiteral("EDITED"));
        // The very same leader, by id and shape, in the drawing opened: the
        // form's edits were for the one before and must not be applied to it.
        const auto opened = d.document.open(directory);
        ASSERT_TRUE(opened.ok()) << opened.error().describe();
        ASSERT_EQ(d.leader(leader).text, "ONE");
        processEvents();
        EXPECT_EQ(dialog.currentLeader(), leader);
        EXPECT_EQ(note->toPlainText(), QStringLiteral("ONE"));
        EXPECT_TRUE(dialog.formChange().empty());

        d.document.newDocument();
        processEvents();
        EXPECT_EQ(dialog.currentLeader(), 0u);
        EXPECT_EQ(child<QListWidget>(dialog, "leaderList")->count(), 0);
        EXPECT_FALSE(child<QPushButton>(dialog, "leaderApply")->isEnabled());
        EXPECT_FALSE(dialog.apply());
    }
    std::filesystem::remove_all(directory.parent_path());
}
