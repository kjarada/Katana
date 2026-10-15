// The Leaders manager hands verb lines to the window's one executor
// (src/katana_qt/annotation/leader_manager.hpp, docs/annotation.md "In the
// window"; docs/desktop.md "One executor"): it changes nothing itself. Every
// button builds the LEADER or BALLOON line a person would type - only what
// changed, numbers written exactly - and the dialog's tests drive those lines.
// A note a command line cannot carry is refused with a sentence, and the
// shared Apply to / Only those that match controls choose which leaders the
// list shows and which entities and leaders the For Selection and Arrange
// tabs act on.
//
// Expected lines and counts are worked out by hand beside each assertion.

#include <gtest/gtest.h>

#include <functional>
#include <string>
#include <vector>

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QSpinBox>
#include <QStringList>
#include <QToolButton>

#include "annotation/leader_manager.hpp"
#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/entity.hpp"
#include "widget_harness.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::annotation::LeaderChange;
using katana::cad::annotation::LeaderNote;
using katana::entity::EntityId;
using katana::entity::LeaderGeometry;
using katana::qt::LeaderManagerContext;
using katana::qt::LeaderManagerDialog;
using katana::qt::VerbOutcome;
using katana::qt::test::processEvents;

namespace {

// A drawing, and the executor the window would give the dialog: it records
// each line and, unless told only to record, runs it through the interpreter.
struct Drawing {
    Document document;
    CommandInterpreter interpreter{document};
    QStringList lines;
    // False: the lines are recorded and answered "ok" but not run, so a test
    // can see the dialog change nothing itself.
    bool execute = true;

    EntityId run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << " -> " << reply.error().describe();
        const auto made = document.lastCreatedEntities();
        return made.empty() ? 0 : made.front();
    }
    LeaderManagerContext context()
    {
        LeaderManagerContext made;
        made.run = [this](const QString& line) {
            lines << line;
            if (!execute) {
                return VerbOutcome{true, {}, {}};
            }
            const auto reply = interpreter.run(line.toStdString());
            if (!reply) {
                return VerbOutcome{false, {}, QString::fromStdString(reply.error().describe())};
            }
            return VerbOutcome{true, QString::fromStdString(*reply), {}};
        };
        return made;
    }
    [[nodiscard]] const LeaderGeometry& leader(EntityId id) const
    {
        return std::get<LeaderGeometry>(document.model().entities.find(id)->geometry);
    }
    [[nodiscard]] std::size_t steps() const { return document.history().undoCount(); }
};

template <typename Widget> Widget* child(QWidget& dialog, const char* name)
{
    auto* widget = dialog.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(widget, nullptr) << name;
    return widget;
}

void chooseData(QComboBox* box, int value)
{
    box->setCurrentIndex(box->findData(value));
}

// Ticks `layer` in the layer list of a scope's controls.
void tick(QWidget& dialog, const char* listName, const char* layer)
{
    auto* layers = child<QListWidget>(dialog, listName);
    bool found = false;
    for (int i = 0; i < layers->count(); ++i) {
        if (layers->item(i)->text() == QString::fromLatin1(layer)) {
            layers->item(i)->setCheckState(Qt::Checked);
            found = true;
        }
    }
    EXPECT_TRUE(found) << layer;
}

constexpr int kBox = static_cast<int>(katana::entity::CalloutShape::Box);
constexpr int kText = 0;
constexpr int kTemplate = 1;

std::string id(EntityId value)
{
    return std::to_string(value);
}

} // namespace

// ---- the lines ---------------------------------------------------------------------------------

TEST(LeaderManagerLines, OnlyWhatChangedIsWrittenAndNumbersAreWrittenExactly)
{
    LeaderChange change;
    change.paperHeight = 2.3456;
    change.arrowSize = 0.125;
    change.landing = 1500.0;
    change.arrow = katana::entity::ArrowHead::Dot;
    change.callout = katana::entity::CalloutShape::Box;
    change.textStyle = "My Style";
    // Each number as core::formatExactReal writes it: the shortest text that
    // reads back as the same double, so 2.3456 is not cut to 2.346.
    const auto words = katana::qt::leaderOptionWords(change);
    ASSERT_TRUE(words.ok()) << words.error().describe();
    EXPECT_EQ(*words,
              " arrow=Dot callout=box style=\"My Style\" paper=2.3456 arrowsize=0.125 "
              "landing=1500");

    // A tip along an entity, written as the verb reads it: segment 2, a
    // fraction written exactly.
    LeaderChange moved;
    moved.tip = katana::cad::annotation::AnchoredPoint{
        katana::geometry::Point2(1, 2),
        katana::entity::AnchorRef{7, katana::entity::AnchorPoint::Along, 2, 0.123456}};
    const auto tip = katana::qt::leaderOptionWords(moved);
    ASSERT_TRUE(tip.ok());
    EXPECT_EQ(*tip, " tip=#7.along2:0.123456");

    // The ids, then the options; a change that sets nothing is no line.
    const auto line = katana::qt::leaderSetLine({3, 5}, change);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(line->toStdString(), "LEADER SET 3 5" + *words);
    const auto none = katana::qt::leaderSetLine({3}, LeaderChange{});
    ASSERT_TRUE(none.ok());
    EXPECT_TRUE(none->isEmpty());
    EXPECT_FALSE(katana::qt::leaderSetLine({}, change).ok()) << "no leader to change";
}

TEST(LeaderManagerLines, ANoteIsWrittenAsTextATemplateOrALabelStyleAndALineBreakAsBackslashN)
{
    LeaderChange change;
    change.note = LeaderNote{LeaderNote::Kind::Text, "IL 10\nsecond line"};
    // A blank makes it one quoted word; the break is the two characters the
    // verb reads back as one.
    EXPECT_EQ(*katana::qt::leaderOptionWords(change), " text=\"IL 10\\nsecond line\"");
    change.note = LeaderNote{LeaderNote::Kind::Template, "{rl:.3f}"};
    EXPECT_EQ(*katana::qt::leaderOptionWords(change), " template={rl:.3f}");
    change.note = LeaderNote{LeaderNote::Kind::LabelStyle, "Lot Area"};
    EXPECT_EQ(*katana::qt::leaderOptionWords(change), " labelstyle=\"Lot Area\"");
    // A bare leader: the empty note, which a word still says.
    change.note = LeaderNote{LeaderNote::Kind::Text, ""};
    EXPECT_EQ(*katana::qt::leaderOptionWords(change), " text=\"\"");
    // A backslash is written twice, so one before an n is not a line break.
    change.note = LeaderNote{LeaderNote::Kind::Text, "C:\\new"};
    EXPECT_EQ(*katana::qt::leaderOptionWords(change), " text=C:\\\\new");
}

TEST(LeaderManagerLines, ADoubleQuoteIsRefusedWithASentenceThatNamesTheField)
{
    LeaderChange change;
    change.note = LeaderNote{LeaderNote::Kind::Text, "say \"hi\""};
    const auto words = katana::qt::leaderOptionWords(change);
    ASSERT_FALSE(words.ok());
    EXPECT_NE(words.error().message.find("the note"), std::string::npos) << words.error().message;
    EXPECT_NE(words.error().message.find("double quote"), std::string::npos);

    LeaderChange style;
    style.textStyle = "My \"Style\"";
    const auto refused = katana::qt::leaderOptionWords(style);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("text style"), std::string::npos);

    // The line for the entities of a scope: the options first, then the
    // scope's words, which a WHERE would otherwise swallow them into.
    LeaderChange arrow;
    arrow.arrow = katana::entity::ArrowHead::Tick;
    const auto line = katana::qt::leadersForLine(
        false, QStringLiteral("LAYERS plan WHERE TYPE=point"), arrow, 45.0, 10.0);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(line->toStdString(),
              "LEADER FOR arrow=Tick angle=45 length=10 LAYERS plan WHERE TYPE=point");
    const auto balloons =
        katana::qt::leadersForLine(true, QStringLiteral("SELECTION"), LeaderChange{}, 30.5, 4.0);
    ASSERT_TRUE(balloons.ok());
    EXPECT_EQ(balloons->toStdString(), "BALLOON FOR angle=30.5 length=4 SELECTION");
    EXPECT_FALSE(katana::qt::leadersForLine(false, QString(), LeaderChange{}, 45.0, 10.0).ok())
        << "which entities?";
    LeaderChange tip;
    tip.tip = katana::cad::annotation::AnchoredPoint{katana::geometry::Point2(0, 0), {}};
    EXPECT_FALSE(
        katana::qt::leadersForLine(false, QStringLiteral("SELECTION"), tip, 45.0, 10.0).ok());
}

// ---- the buttons -------------------------------------------------------------------------------

TEST(LeaderManagerLines, ApplyHandsOverOneLineOfWhatChangedAndChangesNothingItself)
{
    Drawing d;
    d.execute = false;
    const EntityId pit = d.run("POINT 0,0");
    const EntityId leader = d.run("LEADER #" + id(pit) + " 5,5 text=A");
    LeaderManagerDialog dialog(d.document, d.context());
    ASSERT_TRUE(dialog.showLeader(leader));
    const std::size_t steps = d.steps();
    chooseData(child<QComboBox>(dialog, "leaderCallout"), kBox);
    child<QDoubleSpinBox>(dialog, "leaderPaperHeight")->setValue(3.5);
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    // The callout and the height, nothing else: the leader's note, arrow,
    // sizes and landing are what they were.
    EXPECT_EQ(d.lines, QStringList{QString::fromStdString("LEADER SET " + id(leader) +
                                                          " callout=box paper=3.5")});
    EXPECT_EQ(dialog.lastLine(), d.lines.back());
    EXPECT_EQ(child<QLineEdit>(dialog, "leaderLine")->text(), d.lines.back());
    EXPECT_EQ(d.steps(), steps) << "the dialog edited nothing";
    EXPECT_EQ(d.leader(leader).callout, katana::entity::CalloutShape::None);
}

TEST(LeaderManagerLines, AnUntouchedFormRunsNoLineAndAnExecutorWithNothingToRunOnIsSaidSo)
{
    Drawing d;
    const EntityId leader = d.run("LEADER 0,0 5,5 text=A");
    {
        LeaderManagerDialog dialog(d.document, d.context());
        ASSERT_TRUE(dialog.showLeader(leader));
        EXPECT_TRUE(dialog.apply());
        EXPECT_TRUE(d.lines.isEmpty()) << "nothing changed: no line";
    }
    // No executor at all: a refusal in the problem line, never a silent success.
    LeaderManagerDialog bare(d.document);
    ASSERT_TRUE(bare.showLeader(leader));
    child<QPlainTextEdit>(bare, "leaderNote")->setPlainText(QStringLiteral("B"));
    EXPECT_FALSE(bare.apply());
    EXPECT_TRUE(bare.problem().contains(QStringLiteral("no command line"))) << bare.problem().toStdString();
    EXPECT_EQ(d.leader(leader).text, "A");
}

TEST(LeaderManagerLines, ANoteWithADoubleQuoteIsRefusedAndNothingRuns)
{
    Drawing d;
    const EntityId leader = d.run("LEADER 0,0 5,5 text=A");
    LeaderManagerDialog dialog(d.document, d.context());
    ASSERT_TRUE(dialog.showLeader(leader));
    child<QPlainTextEdit>(dialog, "leaderNote")->setPlainText(QStringLiteral("say \"hi\""));
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("the note"))) << dialog.problem().toStdString();
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("double quote")));
    EXPECT_TRUE(d.lines.isEmpty());
    EXPECT_EQ(d.leader(leader).text, "A");
}

TEST(LeaderManagerLines, ABackslashBeforeAnNAndALineBreakInTheNoteArriveWhole)
{
    Drawing d;
    const EntityId leader = d.run("LEADER 0,0 5,5 text=A");
    LeaderManagerDialog dialog(d.document, d.context());
    ASSERT_TRUE(dialog.showLeader(leader));
    // C:\new\temp: a backslash before an n, which the verb would read as a
    // line break had it not been written twice.
    child<QPlainTextEdit>(dialog, "leaderNote")->setPlainText(QStringLiteral("C:\\new\\temp"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.lines.back().toStdString(), "LEADER SET " + id(leader) + " text=C:\\\\new\\\\temp");
    EXPECT_EQ(d.leader(leader).text, "C:\\new\\temp");
    processEvents();
    child<QPlainTextEdit>(dialog, "leaderNote")->setPlainText(QStringLiteral("one\ntwo"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(leader).text, "one\ntwo");
}

TEST(LeaderManagerLines, FreezeDetachAttachAndTheAttributeEachHandOverTheirLine)
{
    Drawing d;
    d.execute = false;
    const EntityId line = d.run("LINE 0,0 40,0");
    // A plain leader whose tip is at (10, 3): the place of the line nearest it
    // is (10, 0), a quarter of the way along a line of length 40.
    const EntityId leader = d.run("LEADER 10,3 20,10 text=A");
    LeaderManagerDialog dialog(d.document, d.context());
    ASSERT_TRUE(dialog.showLeader(leader));

    ASSERT_TRUE(dialog.freeze());
    ASSERT_TRUE(dialog.detach());
    EXPECT_EQ(d.lines, (QStringList{QString::fromStdString("LEADER FREEZE " + id(leader)),
                                    QString::fromStdString("LEADER DETACH " + id(leader))}));

    d.lines.clear();
    d.run("SELECT " + id(line) + " " + id(leader));
    ASSERT_TRUE(dialog.attachToSelected()) << dialog.problem().toStdString();
    EXPECT_EQ(d.lines, QStringList{QString::fromStdString("LEADER ATTACH " + id(leader) + " #" +
                                                          id(line) + ".along0:0.25")});

    d.lines.clear();
    child<QLineEdit>(dialog, "leaderAttributeName")->setText(QStringLiteral(" invert "));
    child<QLineEdit>(dialog, "leaderAttributeValue")->setText(QStringLiteral("10.5"));
    auto* type = child<QComboBox>(dialog, "leaderAttributeType");
    type->setCurrentIndex(type->findData(QStringLiteral("real")));
    ASSERT_TRUE(dialog.setAttribute());
    ASSERT_TRUE(dialog.removeAttribute());
    EXPECT_EQ(d.lines, (QStringList{QString::fromStdString("LEADER PROP " + id(leader) +
                                                           " SET invert 10.5 real"),
                                    QString::fromStdString("LEADER PROP " + id(leader) +
                                                           " DELETE invert")}));
    // A value with a blank is one quoted word; one with a quote is refused.
    d.lines.clear();
    child<QLineEdit>(dialog, "leaderAttributeValue")->setText(QStringLiteral("two words"));
    type->setCurrentIndex(0);
    ASSERT_TRUE(dialog.setAttribute());
    EXPECT_EQ(d.lines.back().toStdString(), "LEADER PROP " + id(leader) + " SET invert \"two words\"");
    child<QLineEdit>(dialog, "leaderAttributeValue")->setText(QStringLiteral("a\"b"));
    EXPECT_FALSE(dialog.setAttribute());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("attribute value"))) << dialog.problem().toStdString();
    EXPECT_EQ(d.lines.size(), 1) << "the refused one ran nothing";
}

TEST(LeaderManagerLines, SeveralSelectedLeadersAreOneLineNamingEachOfThem)
{
    Drawing d;
    d.execute = false;
    const EntityId a = d.run("LEADER 0,0 5,5 text=A");
    const EntityId b = d.run("LEADER 10,0 15,5 text=B");
    d.run("SELECT " + id(a) + " " + id(b));
    LeaderManagerDialog dialog(d.document, d.context());
    ASSERT_TRUE(dialog.showLeader(a));
    chooseData(child<QComboBox>(dialog, "leaderCallout"), kBox);
    ASSERT_TRUE(dialog.apply());
    EXPECT_EQ(d.lines, QStringList{QString::fromStdString("LEADER SET " + id(a) + " " + id(b) +
                                                          " callout=box")});
}

// ---- the list and its scope --------------------------------------------------------------------

TEST(LeaderManagerLines, TheListShowsEveryLeaderOrThoseTheScopeTakes)
{
    Drawing d;
    d.run("LAYER NEW notes");
    d.run("LAYER NEW other");
    d.run("LAYER SET notes");
    const EntityId a = d.run("LEADER 0,0 5,5 text=A");
    d.run("LAYER SET other");
    const EntityId b = d.run("LEADER 10,0 15,5 text=B");
    d.run("LAYER SET notes");
    const EntityId c = d.run("LEADER 20,0 25,5 text=C");
    d.run("LAYER SET 0");
    LeaderManagerDialog dialog(d.document, d.context());
    auto* list = child<QListWidget>(dialog, "leaderList");
    EXPECT_EQ(dialog.listedLeaders(), (std::vector<EntityId>{a, b, c}));
    EXPECT_EQ(list->count(), 3);

    // "Which leaders...": the whole drawing to begin with, so all three.
    auto* toggle = child<QToolButton>(dialog, "leaderListScopeToggle");
    toggle->setChecked(true);
    EXPECT_EQ(list->count(), 3);
    // The checked layers: notes holds a and c.
    child<QRadioButton>(dialog, "leaderListScopeLayers")->setChecked(true);
    EXPECT_EQ(list->count(), 0) << "no layer ticked yet";
    EXPECT_FALSE(child<QLabel>(dialog, "leaderListScopeProblem")->text().isEmpty());
    tick(dialog, "leaderListLayers", "notes");
    EXPECT_EQ(dialog.listedLeaders(), (std::vector<EntityId>{a, c}));
    EXPECT_EQ(list->count(), 2);
    EXPECT_TRUE(child<QLabel>(dialog, "leaderListScopeProblem")->text().isEmpty());
    // Only those that match: a layer pattern over the whole drawing.
    child<QRadioButton>(dialog, "leaderListScopeDrawing")->setChecked(true);
    child<QLineEdit>(dialog, "leaderListFilterLayer")->setText(QStringLiteral("oth*"));
    EXPECT_EQ(dialog.listedLeaders(), (std::vector<EntityId>{b}));
    // Off again: every leader.
    toggle->setChecked(false);
    EXPECT_EQ(list->count(), 3);
}

// ---- For Selection ------------------------------------------------------------------------------

namespace {

// Two points with a level on layer plan, one on layer other.
struct Levels : Drawing {
    EntityId q1 = 0, q2 = 0, q3 = 0;
    Levels()
    {
        run("LAYER NEW plan");
        run("LAYER NEW other");
        run("LAYER SET plan");
        q1 = run("POINT 0,0");
        q2 = run("POINT 10,0");
        run("LAYER SET other");
        q3 = run("POINT 20,0");
        run("LAYER SET 0");
        for (const EntityId point : {q1, q2, q3}) {
            run("SELECT " + id(point));
            run("PROP SET elevation 25");
        }
        run("SELECT NONE");
    }
};

} // namespace

TEST(LeaderManagerLines, ForSelectionMakesLeadersForTheSelectedEntitiesByDefault)
{
    Levels d;
    d.run("SELECT " + id(d.q1) + " " + id(d.q2));
    LeaderManagerDialog dialog(d.document, d.context());
    dialog.showTab(LeaderManagerDialog::Tab::ForSelection);
    EXPECT_EQ(child<QLabel>(dialog, "leaderForSelection")->text(), QStringLiteral("2 entities selected"));
    chooseData(child<QComboBox>(dialog, "leaderForNoteKind"), kTemplate);
    child<QPlainTextEdit>(dialog, "leaderForNote")->setPlainText(QStringLiteral("RL {rl:.3f}"));
    const std::size_t steps = d.steps();
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    // The note, the direction and distance as they stand (45 degrees, 10 mm),
    // and the selection as the scope: all in one line.
    EXPECT_EQ(d.lines,
              QStringList{QStringLiteral("LEADER FOR template=\"RL {rl:.3f}\" angle=45 length=10 "
                                         "SELECTION")});
    EXPECT_EQ(d.steps(), steps + 1);
    EXPECT_EQ(dialog.forReport(), QStringLiteral("made=2 skipped=0"));
    EXPECT_EQ(child<QLabel>(dialog, "leaderForScopeReport")->text(),
              QStringLiteral("scope=selection matched=2"));
    // What was made is selected, as it always was.
    EXPECT_EQ(d.document.selection().size(), 2U);
}

TEST(LeaderManagerLines, ForSelectionTakesALayerOrAFilterAndSaysWhatTheScopeTook)
{
    Levels d;
    LeaderManagerDialog dialog(d.document, d.context());
    dialog.showTab(LeaderManagerDialog::Tab::ForSelection);
    // Nothing selected, and the controls say selected entities: refused in the
    // verb's words and nothing made.
    const std::size_t steps = d.steps();
    EXPECT_FALSE(dialog.makeForSelection());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("nothing is selected")))
        << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), steps);

    // The points on layer plan: q1 and q2 (hand count; q3 is on other).
    child<QRadioButton>(dialog, "leaderForScopeLayers")->setChecked(true);
    tick(dialog, "leaderForLayers", "plan");
    EXPECT_EQ(child<QLabel>(dialog, "leaderForSelection")->text(),
              QStringLiteral("2 entities taken by Apply to"));
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    EXPECT_EQ(d.lines.back(), QStringLiteral("LEADER FOR text=\"\" angle=45 length=10 LAYERS plan"))
        << "no note typed: a bare arrow, said so";
    EXPECT_EQ(dialog.forReport(), QStringLiteral("made=2 skipped=0"));
    EXPECT_EQ(child<QLabel>(dialog, "leaderForScopeReport")->text(),
              QStringLiteral("scope=layers layers=plan sublayers=yes matched=2"));

    // The whole drawing, only the points: the three, as balloons.
    child<QRadioButton>(dialog, "leaderForScopeDrawing")->setChecked(true);
    child<QCheckBox>(dialog, "leaderForTypePoint")->setChecked(true);
    child<QCheckBox>(dialog, "leaderForBalloon")->setChecked(true);
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    EXPECT_EQ(d.lines.back(), QStringLiteral("BALLOON FOR angle=45 length=10 DRAWING WHERE TYPE=point"));
    EXPECT_EQ(dialog.forReport(), QStringLiteral("made=3 skipped=0"));
}

TEST(LeaderManagerLines, ForSelectionSaysWhyEntitiesWereSkipped)
{
    Levels d;
    d.run("DIM ALIGNED 0,50 10,50 at=0,55");
    const EntityId dimension = d.document.lastCreatedEntities().front();
    d.run("SELECT " + id(d.q1) + " " + id(dimension));
    LeaderManagerDialog dialog(d.document, d.context());
    dialog.showTab(LeaderManagerDialog::Tab::ForSelection);
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    // The point has a place for a leader; the dimension has none.
    EXPECT_TRUE(dialog.forReport().startsWith(QStringLiteral("made=1 skipped=1: id=") +
                                              QString::fromStdString(id(dimension))))
        << dialog.forReport().toStdString();
}

// ---- Arrange -----------------------------------------------------------------------------------

TEST(LeaderManagerLines, ArrangeHandsOverAlignAndRenumberLinesWithTheirScope)
{
    Drawing d;
    d.execute = false;
    d.run("LAYER NEW notes");
    LeaderManagerDialog dialog(d.document, d.context());
    dialog.showTab(LeaderManagerDialog::Tab::Arrange);

    // The notes of the selected leaders, at the topmost's x.
    ASSERT_TRUE(dialog.alignSelection());
    EXPECT_EQ(d.lines.back(), QStringLiteral("LEADER ALIGN SELECTION"));
    child<QCheckBox>(dialog, "leaderAlignUseX")->setChecked(true);
    child<QDoubleSpinBox>(dialog, "leaderAlignX")->setValue(22.5);
    child<QCheckBox>(dialog, "leaderAlignUseSpacing")->setChecked(true);
    child<QDoubleSpinBox>(dialog, "leaderAlignSpacing")->setValue(8.0);
    ASSERT_TRUE(dialog.alignSelection());
    EXPECT_EQ(d.lines.back(), QStringLiteral("LEADER ALIGN x=22.5 spacing=8 SELECTION"));
    // The leaders of a layer.
    child<QRadioButton>(dialog, "leaderArrangeScopeLayers")->setChecked(true);
    tick(dialog, "leaderArrangeLayers", "notes");
    ASSERT_TRUE(dialog.alignSelection());
    EXPECT_EQ(d.lines.back(), QStringLiteral("LEADER ALIGN x=22.5 spacing=8 LAYERS notes"));

    // The balloons: every numbered one, as it always was, unless ticked.
    ASSERT_TRUE(dialog.renumberBalloons());
    EXPECT_EQ(d.lines.back(), QStringLiteral("BALLOON RENUMBER start=1 order=id"));
    child<QSpinBox>(dialog, "balloonRenumberStart")->setValue(5);
    chooseData(child<QComboBox>(dialog, "balloonRenumberOrder"),
               static_cast<int>(katana::cad::annotation::BalloonOrder::X));
    child<QCheckBox>(dialog, "balloonRenumberScoped")->setChecked(true);
    ASSERT_TRUE(dialog.renumberBalloons());
    EXPECT_EQ(d.lines.back(), QStringLiteral("BALLOON RENUMBER start=5 order=x LAYERS notes"));
}

TEST(LeaderManagerLines, AlignAndRenumberReportWhatTheVerbsReplied)
{
    Drawing d;
    d.run("LAYER NEW notes");
    d.run("LAYER SET notes");
    const EntityId one = d.run("BALLOON 0,0 5,5");
    const EntityId two = d.run("BALLOON 10,0 15,5");
    d.run("LAYER SET 0");
    const EntityId other = d.run("BALLOON 20,0 25,5");
    LeaderManagerDialog dialog(d.document, d.context());
    dialog.showTab(LeaderManagerDialog::Tab::Arrange);
    // The two on notes become 10 and 11; the one on layer 0 keeps its 3.
    child<QSpinBox>(dialog, "balloonRenumberStart")->setValue(10);
    child<QCheckBox>(dialog, "balloonRenumberScoped")->setChecked(true);
    child<QRadioButton>(dialog, "leaderArrangeScopeLayers")->setChecked(true);
    tick(dialog, "leaderArrangeLayers", "notes");
    ASSERT_TRUE(dialog.renumberBalloons()) << dialog.problem().toStdString();
    EXPECT_EQ(d.leader(one).text, "10");
    EXPECT_EQ(d.leader(two).text, "11");
    EXPECT_EQ(d.leader(other).text, "3");
    EXPECT_EQ(dialog.arrangeReport(), QStringLiteral("balloons=2 renumbered=2"));
}
