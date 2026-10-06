// The survey code manager, driven as a person drives it - through its widgets,
// found by objectName - over the committed hand-written customisation in
// tests/data/customisation (Katana customisation files, read through
// fixture_customisation.hpp), loaded into the Document as a session
// customisation. What is asserted is the Document and the buffer.
//
// The fixture, counted by hand from test_survey.customisation.json: 11 rules
// over 8 keys, in file order
//   #0 WM* feature    TEST SERVICES, blue, Line, TEST Water Main
//   #1 KB* feature    TEST ROADS, "sui test purple" (no colour table knows it)
//   #2 AC* feature    TEST FURNITURE, white, Point, "0"
//   #3 TR* feature    TEST VEGETATION, green, Point, "0"
//   #4 1*  feature    TEST TEXT, orange
//   #5 2*  feature    TEST TEXT, red
//   #6 PX* feature    TEST MISC, yellow, Point, "0"
//   #7 AC* symbol     TEST Survey Mark (at vertices), 1.5
//   #8 TR* symbol     TEST Tree (not at vertices, listed as a symbol)
//   #9 PX* symbol     TEST Missing Symbol (defined nowhere)
//   #10 *  attributes text Source = Katana test fixture
//
// A rule's section and fields are named here by the Katana customisation
// format's words ("feature", "layer", "surface", "at vertices"), which the
// survey code tools show since the survey code file's own (`map_data`,
// `model`, `tinable`, `mode vertex`) left the window. Each expectation holding
// such a word was changed old to new from the table "The words a rule is
// shown by" in docs/customisation.md, not from what a run printed.
//
// The fixture was three files of an older format until the managers read
// and wrote Katana customisation files. Its counts and names are the same -
// the files here are those three, written over by hand - and what a test
// expects of a FILE the manager writes (how it begins, that it reads back as
// the buffer) is taken from docs/customisation.md, "Layout" and "The reader
// is strict", not from a file a run wrote.

#include <gtest/gtest.h>

#include <cstdint>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iterator>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include <QComboBox>
#include <QCoreApplication>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QTabWidget>
#include <QTreeWidget>

#include "customisation/code_manager.hpp"
#include "customisation/definition_thumbnails.hpp"
#include "customisation/fixture_customisation.hpp"
#include "katana/cad/code_edit.hpp"
#include "katana/cad/code_table.hpp"
#include "katana/cad/colour_lookup.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/colour_names.hpp"
#include "katana/entity/customisation.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::SurveyMap;
using katana::qt::CustomisationContext;
using katana::qt::DefinitionThumbnails;
using katana::qt::SurveyCodeManagerDialog;
using katana::qt::test::customisationFixtureFile;
using katana::qt::test::installCustomisationFixtures;

namespace {

EntityId addPoint(Document& document, double x, const std::string& code,
                  const std::string& number)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{{x, 0.0}};
    entity.properties.insert_or_assign("code", katana::entity::PropertyValue(code));
    entity.properties.insert_or_assign("point", katana::entity::PropertyValue(number));
    EXPECT_TRUE(document.execute(katana::commands::createEntities({entity})).ok());
    const auto created = document.lastCreatedEntities();
    return created.empty() ? katana::entity::kInvalidEntityId : created.front();
}

struct ManagerFixture {
    Document document;
    DefinitionThumbnails thumbnails;
    katana::cad::LineworkCodes lineworkCodes;
    std::vector<std::pair<QString, bool>> logged;
    CustomisationContext context;

    ManagerFixture()
    {
        // The three fixtures in this order, so the session is named after
        // the first: "test_linestyles".
        installCustomisationFixtures(document,
                                     {"test_linestyles", "test_survey", "test_symbols"});
        context.document = &document;
        context.thumbnails = &thumbnails;
        context.lineworkCodes = &lineworkCodes;
        context.log = [this](const QString& message, bool isError) {
            logged.emplace_back(message, isError);
        };
    }

    [[nodiscard]] bool loggedContaining(const QString& part) const
    {
        for (const auto& [message, isError] : logged) {
            if (message.contains(part)) {
                return true;
            }
        }
        return false;
    }
};

template <typename Widget> Widget* child(QWidget& parent, const char* name)
{
    auto* found = parent.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

// The Code Table's key rows: the children of its group headings.
std::set<std::string> keysShown(QTreeWidget& tree)
{
    std::set<std::string> keys;
    for (int group = 0; group < tree.topLevelItemCount(); ++group) {
        const QTreeWidgetItem* heading = tree.topLevelItem(group);
        for (int row = 0; row < heading->childCount(); ++row) {
            keys.insert(heading->child(row)->text(0).toStdString());
        }
    }
    return keys;
}

const QTreeWidgetItem* rowWithText(const QTreeWidget& tree, int column, const QString& value)
{
    for (int row = 0; row < tree.topLevelItemCount(); ++row) {
        if (tree.topLevelItem(row)->text(column) == value) {
            return tree.topLevelItem(row);
        }
    }
    return nullptr;
}

// The Code Table's row for `key`, under whichever group heading it sits.
const QTreeWidgetItem* keyRow(const QTreeWidget& tree, const QString& key)
{
    for (int group = 0; group < tree.topLevelItemCount(); ++group) {
        const QTreeWidgetItem* heading = tree.topLevelItem(group);
        for (int row = 0; row < heading->childCount(); ++row) {
            if (heading->child(row)->text(0) == key) {
                return heading->child(row);
            }
        }
    }
    return nullptr;
}

// Selects rule 0 (WM* feature) and saves it with another layer, through the
// form's own fields and buttons.
void editWaterMainLayer(SurveyCodeManagerDialog& dialog, const QString& layer)
{
    dialog.selectRule(0);
    auto* model = child<QLineEdit>(dialog, "ruleModel");
    ASSERT_NE(model, nullptr);
    model->setText(layer);
    auto* save = child<QPushButton>(dialog, "ruleUpdate");
    ASSERT_NE(save, nullptr);
    ASSERT_TRUE(save->isEnabled());
    save->click();
}

std::string readBytes(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
}

void writeBytes(const std::filesystem::path& path, const std::string& bytes)
{
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    out << bytes;
    ASSERT_TRUE(out.good()) << path.string();
}

// A customisation written for these tests, holding one of everything a file
// can: a rule for a key the fixture lacks, a symbol, a colour and the
// linework codes.
const char* const kExtraCustomisation = R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "extra",
  "colours": {
    "sui test purple": "#800080"
  },
  "linework": {"start": "BEG"},
  "symbols": [
    {"name": "TEST Extra Mark", "strokes": [
      ["circle", 1]
    ]}
  ],
  "codes": [
    {"key": "ZZ*", "sets": "feature", "layer": "TEST MISC"}
  ]
}
)";

std::filesystem::path scratchFile(const std::string& name)
{
    const std::filesystem::path directory =
        std::filesystem::temp_directory_path() / "katana_code_manager_tests";
    std::filesystem::create_directories(directory);
    return directory / name;
}

} // namespace

TEST(SurveyCodeManager, IsANonModalDialogWithItsFiveTabsNamed)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    EXPECT_EQ(dialog.objectName(), QStringLiteral("surveyCodeManagerDialog"));
    EXPECT_FALSE(dialog.isModal());
    EXPECT_FALSE(dialog.interactive()) << "the tests run offscreen, which is never interactive";
    for (const char* tab :
         {"codeTableTab", "codesInDrawingTab", "codeIssuesTab", "applyCodesTab", "lineworkTab"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(QString::fromLatin1(tab)), nullptr) << tab;
    }
}

TEST(SurveyCodeManager, TheCodeTableShowsEveryKeyOfTheFixtureUnderItsGroup)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* tree = child<QTreeWidget>(dialog, "codeTree");
    ASSERT_NE(tree, nullptr);
    // The 8 distinct keys of the fixture's 11 rules.
    EXPECT_EQ(keysShown(*tree),
              (std::set<std::string>{"*", "1*", "2*", "AC*", "KB*", "PX*", "TR*", "WM*"}));
    // Groups in name order; the bare `*` says no group. "TEST - TEXT" holds
    // both text codes.
    const QTreeWidgetItem* text = rowWithText(*tree, 0, QStringLiteral("TEST - TEXT"));
    ASSERT_NE(text, nullptr);
    EXPECT_EQ(text->childCount(), 2);
    EXPECT_NE(rowWithText(*tree, 0, QStringLiteral("(no group)")), nullptr);
    // AC*'s row: its two rules beneath it, and what it resolves to - the
    // feature rule's layer and the vertex symbol with its size.
    const QTreeWidgetItem* furniture = rowWithText(*tree, 0, QStringLiteral("TEST - FURNITURE"));
    ASSERT_NE(furniture, nullptr);
    ASSERT_EQ(furniture->childCount(), 1);
    const QTreeWidgetItem* chamber = furniture->child(0);
    EXPECT_EQ(chamber->childCount(), 2);
    EXPECT_EQ(chamber->text(2), QStringLiteral("TEST FURNITURE"));
    EXPECT_EQ(chamber->text(4), QStringLiteral("Point"));
    EXPECT_EQ(chamber->text(6), QStringLiteral("TEST Survey Mark, 1.5"));
    EXPECT_EQ(chamber->child(0)->text(0), QStringLiteral("#2 AC* (feature)"));
    EXPECT_EQ(chamber->child(1)->text(0), QStringLiteral("#7 AC* (symbol)"));
    // KB*'s colour is a name no table knows: said so, not swatched.
    const QTreeWidgetItem* roads = rowWithText(*tree, 0, QStringLiteral("TEST - ROADS"));
    ASSERT_NE(roads, nullptr);
    EXPECT_EQ(roads->child(0)->text(3), QStringLiteral("sui test purple (no RGB)"));
}

TEST(SurveyCodeManager, TheSearchAndTheSectionChipNarrowTheTable)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* tree = child<QTreeWidget>(dialog, "codeTree");
    auto* bar = child<QWidget>(dialog, "codeTableFilter");
    ASSERT_NE(tree, nullptr);
    ASSERT_NE(bar, nullptr);
    auto* search = bar->findChild<QLineEdit*>(QStringLiteral("filterText"));
    ASSERT_NE(search, nullptr);
    // "water" folds to the WM* comment "[WM*] Water main" and its linestyle.
    search->setText(QStringLiteral("water"));
    EXPECT_EQ(keysShown(*tree), (std::set<std::string>{"WM*"}));
    search->clear();
    // The Symbol chip: the three keys with a symbol rule.
    auto* symbolChip = bar->findChild<QAbstractButton*>(QStringLiteral("filterSymbol"));
    ASSERT_NE(symbolChip, nullptr);
    symbolChip->click();
    EXPECT_EQ(keysShown(*tree), (std::set<std::string>{"AC*", "PX*", "TR*"}));
    // A chip says its section's word with a capital, and a chip of several
    // sections the word of the first: `pipe` for the three pipe sections,
    // `attributes` for the two attribute ones. All seven are written here by
    // hand from the table of words in docs/customisation.md, since the dialog
    // builds them from entity's table. The two first labelled "Map" and
    // "Tinable" are still found by the objectNames those labels gave them.
    const std::vector<std::pair<const char*, const char*>> chips{
        {"filterAll", "All"},
        {"filterMap", "Feature"},
        {"filterSymbol", "Symbol"},
        {"filterText", "Text"},
        {"filterPipe", "Pipe"},
        {"filterAttributes", "Attributes"},
        {"filterTinable", "Surface"},
    };
    // The bar's own buttons are its chips (the search field's clear button
    // is the field's child, not the bar's).
    EXPECT_EQ(bar->findChildren<QAbstractButton*>(QString(), Qt::FindDirectChildrenOnly).size(), 7)
        << "a chip this does not name";
    for (const auto& [name, label] : chips) {
        const auto* chip = bar->findChild<QAbstractButton*>(QString::fromLatin1(name));
        ASSERT_NE(chip, nullptr) << name;
        EXPECT_EQ(chip->text(), QString::fromLatin1(label)) << name;
    }
}

TEST(SurveyCodeManager, TheFormOffersTheNineSectionsByTheWordsACustomisationFileHolds)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* section = child<QComboBox>(dialog, "ruleSection");
    ASSERT_NE(section, nullptr);
    QStringList offered;
    for (int row = 0; row < section->count(); ++row) {
        offered << section->itemText(row);
    }
    // What "sets" takes in a Katana customisation, in the same order.
    EXPECT_EQ(offered, (QStringList{"feature", "symbol", "text", "pipe", "vertexPipe",
                                    "segmentPipe", "attributes", "vertexAttributes",
                                    "surface"}));
    // The eighth column of the table, between Symbol and Attributes.
    auto* tree = child<QTreeWidget>(dialog, "codeTree");
    ASSERT_NE(tree, nullptr);
    EXPECT_EQ(tree->headerItem()->text(6), QStringLiteral("Symbol"));
    EXPECT_EQ(tree->headerItem()->text(7), QStringLiteral("Surface"));
    EXPECT_EQ(tree->headerItem()->text(8), QStringLiteral("Attributes"));
}

TEST(SurveyCodeManager, TestingACodeShowsTheRuleThatSetItsLayer)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* testCode = child<QLineEdit>(dialog, "testCode");
    auto* fields = child<QTreeWidget>(dialog, "explainFields");
    ASSERT_NE(testCode, nullptr);
    ASSERT_NE(fields, nullptr);
    testCode->setText(QStringLiteral("WM01"));
    // WM01 meets WM* (rule #0) and `*` (rule #10); its layer comes from #0.
    const QTreeWidgetItem* model = rowWithText(*fields, 0, QStringLiteral("layer"));
    ASSERT_NE(model, nullptr);
    EXPECT_EQ(model->text(1), QStringLiteral("TEST SERVICES"));
    EXPECT_EQ(model->text(2), QStringLiteral("#0 WM* (feature)"));
    auto* heading = child<QLabel>(dialog, "explainHeading");
    ASSERT_NE(heading, nullptr);
    EXPECT_TRUE(heading->text().contains(QStringLiteral("matched"))) << heading->text().toStdString();
    EXPECT_TRUE(heading->text().contains(QStringLiteral("WM*")));
    // The `*` rule's attribute, cited to it.
    const QTreeWidgetItem* attribute =
        rowWithText(*fields, 0, QStringLiteral("attribute (string)"));
    ASSERT_NE(attribute, nullptr);
    EXPECT_EQ(attribute->text(1), QStringLiteral("text Source = Katana test fixture"));
    EXPECT_EQ(attribute->text(2), QStringLiteral("#10 * (attributes)"));
}

TEST(SurveyCodeManager, ADefinitionDrawnAtVerticesIsSaidToBeAtVerticesWhereverItIsNamed)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* testCode = child<QLineEdit>(dialog, "testCode");
    auto* definitions = child<QLabel>(dialog, "explainDefinitions");
    auto* tree = child<QTreeWidget>(dialog, "codeTree");
    ASSERT_NE(testCode, nullptr);
    ASSERT_NE(definitions, nullptr);
    ASSERT_NE(tree, nullptr);
    const auto said = [&](const char* line) {
        return definitions->text().split(QLatin1Char('\n')).contains(QString::fromLatin1(line));
    };
    // As a SYMBOL. AC01's (#7) is TEST Survey Mark, which the library draws at
    // vertices: CODE EXPLAIN's own words for it. TR01's (#8) is TEST Tree,
    // which it does not, so nothing is added.
    testCode->setText(QStringLiteral("AC01"));
    EXPECT_TRUE(said("Symbol TEST Survey Mark: defined, at vertices"))
        << definitions->text().toStdString();
    testCode->setText(QStringLiteral("TR01"));
    EXPECT_TRUE(said("Symbol TEST Tree: defined")) << definitions->text().toStdString();

    // As a LINESTYLE, which it cannot be: a new rule gives the same name as
    // its linestyle. The table marks the name with what it is, and the
    // explanation says that and what is drawn instead - by the phrase the
    // Style Manager's "Drawn as" has for the same state.
    katana::entity::SurveyRule marks;
    marks.key = "SM*";
    marks.model = "TEST MARKS";
    marks.linestyle = "TEST Survey Mark";
    ASSERT_TRUE(dialog.addRule(marks).ok());
    const QTreeWidgetItem* row = keyRow(*tree, QStringLiteral("SM*"));
    ASSERT_NE(row, nullptr);
    EXPECT_EQ(row->text(5), QStringLiteral("TEST Survey Mark (an `at vertices` symbol)"));
    testCode->setText(QStringLiteral("SM01"));
    EXPECT_TRUE(said("Linestyle TEST Survey Mark: an `at vertices` symbol, not a linestyle: "
                     "drawn as a plain line"))
        << definitions->text().toStdString();
}

TEST(SurveyCodeManager, ACodeTypedInTheWrongCaseIsUnmatchedAndTheKeyItMissedIsNamed)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* testCode = child<QLineEdit>(dialog, "testCode");
    ASSERT_NE(testCode, nullptr);
    testCode->setText(QStringLiteral("wm01"));
    auto* heading = child<QLabel>(dialog, "explainHeading");
    auto* misses = child<QLabel>(dialog, "explainNearMisses");
    ASSERT_NE(heading, nullptr);
    ASSERT_NE(misses, nullptr);
    // Only `*` (attributes, no model) meets "wm01": fallback only (D5).
    EXPECT_TRUE(heading->text().contains(QStringLiteral("fallback only")))
        << heading->text().toStdString();
    EXPECT_TRUE(misses->text().contains(
        QStringLiteral("WM* (%1)")
            .arg(QString::fromLatin1(katana::cad::toString(katana::cad::NearMissKind::Case)))))
        << misses->text().toStdString();
}

TEST(SurveyCodeManager, EditingTheBufferLeavesTheDocumentAloneUntilApplyAndRevertRestoresIt)
{
    ManagerFixture f;
    const SurveyMap before = f.document.surveyMap();
    const auto generation = f.document.surveyMapGeneration();
    SurveyCodeManagerDialog dialog(f.context);
    editWaterMainLayer(dialog, QStringLiteral("TEST WATER"));

    ASSERT_EQ(dialog.buffer().size(), 11u);
    EXPECT_EQ(dialog.buffer().rules()[0].model, "TEST WATER");
    EXPECT_TRUE(dialog.dirty());
    EXPECT_TRUE(f.document.surveyMap() == before) << "the Document's map moved before Apply";
    EXPECT_EQ(f.document.surveyMapGeneration(), generation);
    auto* indicator = child<QLabel>(dialog, "dirtyIndicator");
    ASSERT_NE(indicator, nullptr);
    EXPECT_TRUE(indicator->text().contains(QStringLiteral("Unapplied")));

    auto* revert = child<QPushButton>(dialog, "revertMap");
    ASSERT_NE(revert, nullptr);
    revert->click();
    EXPECT_TRUE(dialog.buffer() == before);
    EXPECT_FALSE(dialog.dirty());
    EXPECT_TRUE(f.document.surveyMap() == before);
}

TEST(SurveyCodeManager, ApplyPutsTheBufferOnTheDocumentWhereExplainCodeSeesIt)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    editWaterMainLayer(dialog, QStringLiteral("TEST WATER"));
    auto* apply = child<QPushButton>(dialog, "applyMap");
    ASSERT_NE(apply, nullptr);
    apply->click();
    EXPECT_FALSE(dialog.dirty());
    EXPECT_TRUE(f.document.surveyMap() == dialog.buffer());

    const katana::cad::CodeExplanation explained = katana::cad::explainCode(
        f.document.surveyMap(), "WM01",
        [&](std::string_view name) { return f.document.definitionFor(name); },
        katana::cad::colourLookup(f.document));
    const auto model = std::find_if(explained.fields.begin(), explained.fields.end(),
                                    [](const auto& field) { return field.field == "layer"; });
    ASSERT_NE(model, explained.fields.end());
    EXPECT_EQ(model->value, "TEST WATER");
    EXPECT_EQ(model->rule, 0u);
}

TEST(SurveyCodeManager, AddDuplicateMoveAndDeleteEditTheBufferInPlace)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    // Duplicate #1 (KB*): the copy lands at #2 and everything after moves on.
    dialog.selectRule(1);
    child<QPushButton>(dialog, "ruleDuplicate")->click();
    ASSERT_EQ(dialog.buffer().size(), 12u);
    EXPECT_TRUE(dialog.buffer().rules()[2] == dialog.buffer().rules()[1]);
    EXPECT_EQ(dialog.currentRule(), std::optional<std::size_t>(2));
    // Up: the copy back to #1, the original now #2 - order is precedence.
    child<QPushButton>(dialog, "ruleUp")->click();
    EXPECT_EQ(dialog.currentRule(), std::optional<std::size_t>(1));
    // Delete it: the fixture's 11 rules again, as loaded.
    child<QPushButton>(dialog, "ruleDelete")->click();
    EXPECT_TRUE(dialog.buffer() == f.document.surveyMap());
    EXPECT_FALSE(dialog.dirty());

    // A new rule typed into the form, added at the end.
    child<QPushButton>(dialog, "ruleNew")->click();
    child<QLineEdit>(dialog, "ruleKey")->setText(QStringLiteral("FH*"));
    child<QLineEdit>(dialog, "ruleModel")->setText(QStringLiteral("TEST SERVICES"));
    child<QLineEdit>(dialog, "ruleComment")->setText(QStringLiteral("Fire hydrant"));
    child<QPushButton>(dialog, "ruleAdd")->click();
    ASSERT_EQ(dialog.buffer().size(), 12u);
    EXPECT_EQ(dialog.buffer().rules()[11].key, "FH*");
    EXPECT_EQ(dialog.buffer().rules()[11].model, "TEST SERVICES");
    EXPECT_EQ(dialog.buffer().rules()[11].comment, "Fire hydrant");
    EXPECT_EQ(f.document.surveyMap().size(), 11u);
}

TEST(SurveyCodeManager, AnInvalidLayerIsSaidLiveAndTheRuleIsRefusedLeavingTheBufferAlone)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    dialog.selectRule(0);
    // An empty path segment: "A//B" is not a layer path.
    child<QLineEdit>(dialog, "ruleModel")->setText(QStringLiteral("A//B"));
    auto* status = child<QLabel>(dialog, "ruleModelStatus");
    ASSERT_NE(status, nullptr);
    EXPECT_FALSE(status->text().contains(QStringLiteral("valid layer"))) << status->text().toStdString();
    child<QPushButton>(dialog, "ruleUpdate")->click();
    EXPECT_EQ(dialog.buffer().rules()[0].model, "TEST SERVICES");
    EXPECT_FALSE(dialog.dirty());
}

TEST(SurveyCodeManager, TheIssuesTabListsTheUnknownColourAndTheUndefinedSymbol)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* issues = child<QTreeWidget>(dialog, "issuesTable");
    ASSERT_NE(issues, nullptr);
    // KB*'s "sui test purple" (#1) and PX*'s "TEST Missing Symbol" (#9) - and
    // nothing else: every other colour is standard, "0" is a plain line, and
    // TR*'s TEST Tree is listed as a symbol by its customisation (D3).
    ASSERT_EQ(issues->topLevelItemCount(), 2);
    const QTreeWidgetItem* colour = issues->topLevelItem(0);
    EXPECT_EQ(colour->text(1), QStringLiteral("#1"));
    EXPECT_EQ(colour->text(2), QStringLiteral("KB*"));
    EXPECT_EQ(colour->text(4),
              QString::fromLatin1(katana::cad::toString(katana::cad::LintKind::UnknownColour)));
    const QTreeWidgetItem* symbol = issues->topLevelItem(1);
    EXPECT_EQ(symbol->text(1), QStringLiteral("#9"));
    EXPECT_EQ(symbol->text(2), QStringLiteral("PX*"));
    EXPECT_EQ(symbol->text(4),
              QString::fromLatin1(katana::cad::toString(katana::cad::LintKind::UnresolvedSymbol)));

    // A double-click takes the rule to the form on the Code Table.
    issues->itemDoubleClicked(issues->topLevelItem(1), 0);
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.currentRule(), std::optional<std::size_t>(9));
    auto* tabs = child<QTabWidget>(dialog, "codeManagerTabs");
    ASSERT_NE(tabs, nullptr);
    EXPECT_EQ(tabs->currentWidget()->objectName(), QStringLiteral("codeTableTab"));
}

TEST(SurveyCodeManager, ApplyCodesPreviewChangesNothingAndExecuteIsOneUndoStep)
{
    ManagerFixture f;
    const EntityId first = addPoint(f.document, 0.0, "WM01", "1");
    const EntityId second = addPoint(f.document, 5.0, "WM01", "2");
    const std::size_t undoBefore = f.document.history().undoCount();
    SurveyCodeManagerDialog dialog(f.context);

    child<QPushButton>(dialog, "applyPreview")->click();
    EXPECT_EQ(f.document.history().undoCount(), undoBefore) << "Preview executed something";
    EXPECT_EQ(f.document.model().entities.find(first)->layer, "0");
    auto* report = child<QPlainTextEdit>(dialog, "applyReport");
    ASSERT_NE(report, nullptr);
    EXPECT_TRUE(report->toPlainText().contains(QStringLiteral("WM01")))
        << report->toPlainText().toStdString();
    auto* rows = child<QTreeWidget>(dialog, "applyRows");
    ASSERT_NE(rows, nullptr);
    ASSERT_EQ(rows->topLevelItemCount(), 1);
    EXPECT_EQ(rows->topLevelItem(0)->text(0), QStringLiteral("WM01"));
    EXPECT_EQ(rows->topLevelItem(0)->text(1), QStringLiteral("2"));

    child<QPushButton>(dialog, "applyExecute")->click();
    EXPECT_EQ(f.document.history().undoCount(), undoBefore + 1);
    EXPECT_EQ(f.document.model().entities.find(first)->layer, "TEST SERVICES");
    EXPECT_EQ(f.document.model().entities.find(second)->layer, "TEST SERVICES");
    ASSERT_TRUE(f.document.undo().ok());
    EXPECT_EQ(f.document.model().entities.find(first)->layer, "0");
    EXPECT_EQ(f.document.model().entities.find(second)->layer, "0");
}

TEST(SurveyCodeManager, TheCensusClassesTheDrawingsCodesAndMakesARuleForAnUnmatchedOne)
{
    ManagerFixture f;
    const EntityId water = addPoint(f.document, 0.0, "WM01", "1");
    addPoint(f.document, 5.0, "WM01", "2");
    const EntityId stray = addPoint(f.document, 9.0, "ZZ9", "3");
    SurveyCodeManagerDialog dialog(f.context);
    auto* census = child<QTreeWidget>(dialog, "censusTable");
    ASSERT_NE(census, nullptr);
    // In code order: WM01 on two points, matched by WM*; ZZ9 met only by the
    // attribute-only `*`: fallback only (D5).
    ASSERT_EQ(census->topLevelItemCount(), 2);
    EXPECT_EQ(census->topLevelItem(0)->text(0), QStringLiteral("WM01"));
    EXPECT_EQ(census->topLevelItem(0)->text(1), QStringLiteral("2"));
    EXPECT_EQ(census->topLevelItem(0)->text(2), QStringLiteral("matched"));
    EXPECT_EQ(census->topLevelItem(1)->text(0), QStringLiteral("ZZ9"));
    EXPECT_EQ(census->topLevelItem(1)->text(2), QStringLiteral("fallback only"));

    census->setCurrentItem(census->topLevelItem(1));
    child<QPushButton>(dialog, "censusSelect")->click();
    EXPECT_EQ(f.document.selection().ids(), std::vector<EntityId>{stray});
    (void)water;

    child<QPushButton>(dialog, "censusNewRule")->click();
    EXPECT_EQ(child<QLineEdit>(dialog, "ruleKey")->text(), QStringLiteral("ZZ*"));
    EXPECT_FALSE(dialog.currentRule().has_value()) << "nothing is added until Add";
    EXPECT_EQ(dialog.buffer().size(), 11u);
    child<QLineEdit>(dialog, "ruleModel")->setText(QStringLiteral("TEST MISC"));
    child<QPushButton>(dialog, "ruleAdd")->click();
    // The census follows the buffer: ZZ9 is now matched by the new ZZ*.
    EXPECT_EQ(census->topLevelItem(1)->text(2), QStringLiteral("matched"));
}

TEST(SurveyCodeManager, LineworkPreviewChangesNothingAndExecuteIsOneUndoStep)
{
    ManagerFixture f;
    // WM* is a Line code: three points of string WM01 make one line.
    addPoint(f.document, 0.0, "WM01", "1");
    addPoint(f.document, 5.0, "WM01", "2");
    addPoint(f.document, 10.0, "WM01", "3");
    const std::size_t undoBefore = f.document.history().undoCount();
    const std::size_t entitiesBefore = f.document.model().entities.size();
    SurveyCodeManagerDialog dialog(f.context);

    child<QPushButton>(dialog, "lineworkPreview")->click();
    EXPECT_EQ(f.document.history().undoCount(), undoBefore);
    EXPECT_EQ(f.document.model().entities.size(), entitiesBefore);
    auto* strings = child<QTreeWidget>(dialog, "lineworkStrings");
    ASSERT_NE(strings, nullptr);
    ASSERT_EQ(strings->topLevelItemCount(), 1);
    EXPECT_EQ(strings->topLevelItem(0)->text(0), QStringLiteral("WM01"));
    EXPECT_EQ(strings->topLevelItem(0)->text(2), QStringLiteral("3"));
    EXPECT_EQ(strings->topLevelItem(0)->text(5), QStringLiteral("TEST SERVICES"));

    child<QPushButton>(dialog, "lineworkExecute")->click();
    EXPECT_EQ(f.document.history().undoCount(), undoBefore + 1);
    EXPECT_EQ(f.document.model().entities.size(), entitiesBefore + 1);
    ASSERT_TRUE(f.document.undo().ok());
    EXPECT_EQ(f.document.model().entities.size(), entitiesBefore);
}

TEST(SurveyCodeManager, TheLineworkCodesAreTheSessionsAndAnAmbiguousSetIsRefused)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* start = child<QLineEdit>(dialog, "lineworkStart");
    auto* use = child<QPushButton>(dialog, "lineworkCodesUse");
    ASSERT_NE(start, nullptr);
    ASSERT_NE(use, nullptr);
    EXPECT_EQ(start->text(), QStringLiteral("ST"));
    start->setText(QStringLiteral("BEG"));
    use->click();
    EXPECT_EQ(f.lineworkCodes.start, "BEG");
    // "END" twice: a token meaning two things means neither.
    start->setText(QStringLiteral("END"));
    EXPECT_FALSE(use->isEnabled());
    EXPECT_EQ(f.lineworkCodes.start, "BEG");
}

TEST(SurveyCodeManager, ApplyCallsTheCommitHookBeforeTheMapIsSetAndWhatItHandsBackAfter)
{
    // CustomisationContext::beginCommit is how a session whose customisation
    // is the kept one stays kept across an editor's own commit. Its maker
    // has to be asked BEFORE the commit - Apply's setSurveyMap leaves the
    // session "not kept", so only before it can "was it kept?" be answered -
    // and what it hands back is called AFTER, which is where it keeps again.
    // The manager knows neither; it calls, in that order, as the definition
    // editor's Save does. A counting hook that notes at each call the survey
    // map's generation, and whether the session says it is kept, shows which
    // side of the commit the call came on.
    ManagerFixture f;
    // The session as a start leaves one: kept.
    ASSERT_TRUE(f.document
                    .installCustomisation(f.document.customisation(),
                                          katana::cad::CustomisationOrigin::Kept, true)
                    .ok());
    ASSERT_TRUE(f.document.customisationState().kept);
    int begun = 0;
    int finished = 0;
    bool handBack = true;
    std::uint64_t mapAtBegin = 0;
    std::uint64_t mapAtFinish = 0;
    bool keptAtBegin = false;
    bool keptAtFinish = true;
    f.context.beginCommit = [&]() -> std::function<void()> {
        ++begun;
        mapAtBegin = f.document.surveyMapGeneration();
        keptAtBegin = f.document.customisationState().kept;
        if (!handBack) {
            return {};
        }
        return [&] {
            ++finished;
            mapAtFinish = f.document.surveyMapGeneration();
            keptAtFinish = f.document.customisationState().kept;
        };
    };
    SurveyCodeManagerDialog dialog(f.context);

    // Opening the manager and editing its buffer commit nothing.
    editWaterMainLayer(dialog, QStringLiteral("TEST WATER"));
    EXPECT_EQ(begun, 0);

    const std::uint64_t before = f.document.surveyMapGeneration();
    ASSERT_TRUE(dialog.apply().ok());
    EXPECT_EQ(begun, 1);
    EXPECT_EQ(finished, 1);
    EXPECT_EQ(mapAtBegin, before) << "asked before the map was set";
    EXPECT_TRUE(keptAtBegin) << "and so while the session still said it was kept";
    EXPECT_EQ(mapAtFinish, before + 1) << "answered once it had been";
    EXPECT_FALSE(keptAtFinish) << "which is what the answer is there to put right";
    EXPECT_EQ(f.document.surveyMap().rules()[0].model, "TEST WATER");

    // A maker that hands nothing back - the session was not kept - is asked
    // all the same, and that is all.
    handBack = false;
    editWaterMainLayer(dialog, QStringLiteral("TEST MAINS"));
    ASSERT_TRUE(dialog.apply().ok());
    EXPECT_EQ(begun, 2);
    EXPECT_EQ(finished, 1);
    EXPECT_EQ(f.document.surveyMap().rules()[0].model, "TEST MAINS");

    // The Apply button is that call.
    handBack = true;
    editWaterMainLayer(dialog, QStringLiteral("TEST PIPES"));
    auto* applyButton = child<QPushButton>(dialog, "applyMap");
    ASSERT_NE(applyButton, nullptr);
    ASSERT_TRUE(applyButton->isEnabled());
    applyButton->click();
    EXPECT_EQ(begun, 3);
    EXPECT_EQ(finished, 2);
    EXPECT_EQ(f.document.surveyMap().rules()[0].model, "TEST PIPES");
}

TEST(SurveyCodeManager, ExportedCodesAreAKatanaCustomisationOfTheRulesAloneThatReadsBackAsTheBuffer)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    editWaterMainLayer(dialog, QStringLiteral("TEST WATER"));
    const std::filesystem::path path = scratchFile("exported.customisation.json");
    ASSERT_TRUE(dialog.exportCodes(path).ok());

    const std::string bytes = readBytes(path);
    // How a Katana customisation file begins and ends (docs/customisation.md,
    // "Layout", rules 1 to 4): `{`, then each member of the file on a line of
    // its own, two blanks in - format, version, name, then the lists - and
    // `}` with a line feed. The name is the session's, which is the first
    // fixture's. Of the three customisations that session was loaded from,
    // the one that brought rules is this file's source - one line, its name
    // and the kind it brought (rules 6 and 11) - and `codes` follows.
    const std::string head = "{\n"
                             "  \"format\": \"katana-customisation\",\n"
                             "  \"version\": 1,\n"
                             "  \"name\": \"test_linestyles\",\n"
                             "  \"sources\": [\n"
                             "    {\"name\": \"test_survey\", \"rules\": true}\n"
                             "  ],\n"
                             "  \"codes\": [\n";
    EXPECT_EQ(bytes.substr(0, head.size()), head);
    EXPECT_TRUE(bytes.ends_with("\n  ]\n}\n"))
        << bytes.substr(bytes.size() > 40 ? bytes.size() - 40 : 0);
    // A rule is one line, four blanks in, its members in the format's order
    // (rule 6): the fixture file's own first line with the edited layer.
    const std::string waterMain =
        "    {\"key\": \"WM*\", \"sets\": \"feature\", \"layer\": \"TEST WATER\", "
        "\"colour\": \"blue\", \"draw\": \"line\", \"linestyle\": \"TEST Water Main\", "
        "\"weight\": \"0\", \"group\": \"TEST - SERVICES\", "
        "\"comment\": \"[WM*] Water main\"},\n";
    EXPECT_NE(bytes.find(waterMain), std::string::npos) << bytes.substr(0, 400);

    auto read = katana::entity::customisationFromJson(bytes);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(read->map == dialog.buffer());
    EXPECT_EQ(read->map.rules()[0].model, "TEST WATER");
    // The rules alone: none of the session's seven definitions, and nothing
    // said of its settings - a file of codes for a colleague must not reset
    // their control codes. No colours either: the session defines none.
    EXPECT_TRUE(read->library.empty());
    EXPECT_TRUE(read->colours.empty());
    EXPECT_FALSE(read->linework.has_value());
    EXPECT_FALSE(read->automation.has_value());
    // The source that brought rules, said to have brought those alone; the
    // two that brought the session's definitions are no source of this file.
    EXPECT_EQ(read->sources, (std::vector<katana::entity::CustomisationSourceNote>{
                                 {"test_survey", false, true, {}}}));
    EXPECT_TRUE(read->notice.empty());
}

// The file a session of four customisations, each with its author's notice,
// exports its codes to (fixture_customisation.hpp, installNoticedSession) -
// written out by hand from docs/customisation.md, "Layout". The session's own
// name, description and notice, and after it the notice of the one source the
// file leaves out; of its four sources the two that brought rules, each with
// its notice, and the table of colours, because the file carries one of its
// colours; the colour a rule names and not the other; and the rules. No
// definitions, nothing of the linework codes, and not what the session is
// based on.
//
// TWO THINGS HERE CHANGED when the managers took the one rule a part is
// written by (cad::customisationPart, which CUSTOMISE EXPORT ... CODES writes
// by too); this file first pinned the managers' own:
//   - "Marks drawn by hand." is in the notice. The marks' customisation
//     brought definitions alone, so nothing of it is written and it is no
//     source of the file; by the managers' rule its notice went with it. By
//     the one rule the notice of a source left out is written with the
//     part's own - nothing says whose a colour is, and no export drops an
//     author's notice.
//   - no "basedOn", though the session has one (the fixture's base says what
//     it was made from): a part is not an edition of the built-in, to be
//     told from another edition at a start. The managers' rule kept it.
const char* const kNoticedCodes =
    "{\n"
    "  \"format\": \"katana-customisation\",\n"
    "  \"version\": 1,\n"
    "  \"name\": \"base\",\n"
    "  \"description\": \"The base set.\",\n"
    "  \"notice\": [\n"
    "    \"Base: all rights reserved.\",\n"
    "    \"Marks drawn by hand.\"\n"
    "  ],\n"
    "  \"sources\": [\n"
    "    {\"name\": \"base\", \"rules\": true},\n"
    "    {\"name\": \"client\", \"rules\": true, "
    "\"notice\": [\"Client codes, for this job only.\"]},\n"
    "    {\"name\": \"tints\", \"notice\": [\"Tints: free to use.\"]}\n"
    "  ],\n"
    "  \"colours\": {\n"
    "    \"tint teal\": \"#008080\"\n"
    "  },\n"
    "  \"codes\": [\n"
    "    {\"key\": \"PG*\", \"sets\": \"feature\", \"layer\": \"BASE PEGS\", "
    "\"colour\": \"tint teal\", \"draw\": \"point\"},\n"
    "    {\"key\": \"FN*\", \"sets\": \"feature\", \"layer\": \"CLIENT FENCES\"}\n"
    "  ]\n"
    "}\n";

TEST(SurveyCodeManager, ExportedCodesCarryTheSessionsNoticeItsRuleSourcesAndTheColoursTheRulesName)
{
    Document document;
    katana::qt::test::installNoticedSession(document);
    // The session as the merge leaves it, before anything is asked of the
    // manager: four sources, and the first's notice the session's own.
    ASSERT_EQ(document.customisationState().name, "base");
    ASSERT_EQ(document.customisationState().notice,
              std::vector<std::string>{"Base: all rights reserved."});
    ASSERT_EQ(document.customisationState().sources.size(), 4u);
    ASSERT_EQ(document.customisationState().colours.size(), 2u);
    ASSERT_TRUE(document.customisation().basedOn.has_value());
    CustomisationContext context;
    context.document = &document;
    context.log = [](const QString&, bool) {};
    SurveyCodeManagerDialog dialog(context);
    ASSERT_EQ(dialog.buffer().size(), 2u);

    const std::filesystem::path path = scratchFile("noticed.customisation.json");
    ASSERT_TRUE(dialog.exportCodes(path).ok());
    EXPECT_EQ(readBytes(path), kNoticedCodes);

    // The colleague the file is for loads it as a whole customisation is
    // loaded (mergeCustomisation, then installCustomisation: what CUSTOMISE
    // <file> does). They are shown the notice - their session had no name, so
    // it takes the file's with its description and notice, the two lines the
    // file carries (was: the session's one) - and each source keeps its own.
    // And their own spelling of Start, BEG, is as they left it: the file says
    // nothing of the control codes, which a merge takes from any file that
    // does.
    {
        Document colleague;
        katana::entity::LineworkCodes theirs;
        theirs.start = "BEG";
        ASSERT_TRUE(colleague.setLineworkCodes(theirs).ok());
        katana::qt::test::installCustomisations(
            colleague, {katana::qt::test::customisationFromText(readBytes(path))});
        const katana::cad::CustomisationState& state = colleague.customisationState();
        EXPECT_EQ(state.name, "base");
        EXPECT_EQ(state.description, "The base set.");
        EXPECT_EQ(state.notice, (std::vector<std::string>{"Base: all rights reserved.",
                                                          "Marks drawn by hand."}));
        // Their session is not said to be made from the author's base.
        EXPECT_FALSE(colleague.customisation().basedOn.has_value());
        EXPECT_EQ(state.sources,
                  (std::vector<katana::entity::CustomisationSourceNote>{
                      {"base", false, true, {}},
                      {"client", false, true, {"Client codes, for this job only."}},
                      {"tints", false, false, {"Tints: free to use."}}}));
        EXPECT_EQ(colleague.surveyMap().size(), 2u);
        EXPECT_EQ(state.colours.find("tint teal"),
                  std::optional(katana::entity::Color{0, 128, 128, 255}));
        EXPECT_EQ(state.linework.start, "BEG");
    }

    // With the one rule that names a colour gone from the buffer, the file
    // carries no colour, and so nothing of the table of colours: it is not a
    // source of this file. Its notice is written with the part's own, after
    // the marks' (was: "does not travel with it" - the managers' own rule,
    // which the one rule replaced: no export drops an author's notice).
    ASSERT_TRUE(dialog.removeRule(0).ok());
    ASSERT_TRUE(dialog.exportCodes(path).ok());
    auto read = katana::entity::customisationFromJson(readBytes(path));
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(read->colours.empty());
    EXPECT_EQ(read->sources,
              (std::vector<katana::entity::CustomisationSourceNote>{
                  {"base", false, true, {}},
                  {"client", false, true, {"Client codes, for this job only."}}}));
    EXPECT_EQ(read->notice,
              (std::vector<std::string>{"Base: all rights reserved.", "Marks drawn by hand.",
                                        "Tints: free to use."}));
    EXPECT_FALSE(read->basedOn.has_value());
    EXPECT_EQ(read->description, "The base set.");
    EXPECT_TRUE(read->map == dialog.buffer());
}

TEST(SurveyCodeManager, ExportedCodesCarryTheColoursARulesSymbolAndTextNameToo)
{
    // Four colours of the session's own. A rule names a colour in three
    // places - its own, its symbol's and its text's - and one of the four is
    // named in none of them.
    ManagerFixture f;
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());
    ASSERT_TRUE(colours.add("sui test teal", katana::entity::Color{0, 128, 128, 255}).ok());
    ASSERT_TRUE(colours.add("sui test lime", katana::entity::Color{0, 255, 0, 255}).ok());
    ASSERT_TRUE(colours.add("sui test unused", katana::entity::Color{1, 2, 3, 255}).ok());
    f.document.setColourTable(colours);
    SurveyCodeManagerDialog dialog(f.context);
    // KB* (#1) is coloured "sui test purple" already. A symbol rule whose
    // symbol is teal, spelled another way than the table spells it - one
    // name, by the fold a colour name is compared in - and a text rule whose
    // text is lime.
    katana::entity::SurveyRule marked;
    marked.key = "MK*";
    marked.section = katana::entity::SurveySection::VertexSymbol;
    marked.symbol = katana::entity::SurveySymbol{.style = "TEST Valve", .colour = "SUI_Test_Teal"};
    ASSERT_TRUE(dialog.addRule(marked).ok());
    katana::entity::SurveyRule lettered;
    lettered.key = "LT*";
    lettered.section = katana::entity::SurveySection::VertexTextStyle;
    lettered.textStyle = katana::entity::SurveyTextStyle{.colour = "sui test lime"};
    ASSERT_TRUE(dialog.addRule(lettered).ok());

    const std::filesystem::path path = scratchFile("coloured.customisation.json");
    ASSERT_TRUE(dialog.exportCodes(path).ok());
    auto read = katana::entity::customisationFromJson(readBytes(path));
    ASSERT_TRUE(read.ok()) << read.error().describe();
    // In the order of their folded names, as the table spells them; the
    // standard names the other rules give (blue, white, green ...) are no
    // customisation's to define.
    const std::vector<katana::entity::ColourTable::Entry> carried = read->colours.entries();
    ASSERT_EQ(carried.size(), 3u);
    EXPECT_EQ(carried[0], (katana::entity::ColourTable::Entry{"sui test lime", {0, 255, 0, 255}}));
    EXPECT_EQ(carried[1],
              (katana::entity::ColourTable::Entry{"sui test purple", {128, 0, 128, 255}}));
    EXPECT_EQ(carried[2],
              (katana::entity::ColourTable::Entry{"sui test teal", {0, 128, 128, 255}}));
    EXPECT_TRUE(read->map == dialog.buffer());
}

TEST(SurveyCodeManager, ExportedCodesKeepTheirOrderWhereSectionsInterleave)
{
    // The fixture's rules are grouped by section as it is written - seven
    // feature rules, three symbol rules, one of attributes - so a writer
    // that gathered rules by section would give that very order back. AC*'s
    // symbol rule (#7) is moved up to #2, between two feature rules: order
    // is precedence, and the file must hold the rules as the buffer does.
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    ASSERT_TRUE(dialog.moveRule(7, -5).ok());
    ASSERT_EQ(dialog.buffer().rules()[1].section, katana::entity::SurveySection::Map);
    ASSERT_EQ(dialog.buffer().rules()[2].section, katana::entity::SurveySection::VertexSymbol);
    ASSERT_EQ(dialog.buffer().rules()[2].key, "AC*");
    ASSERT_EQ(dialog.buffer().rules()[3].section, katana::entity::SurveySection::Map);
    const std::filesystem::path path = scratchFile("interleaved.customisation.json");
    ASSERT_TRUE(dialog.exportCodes(path).ok());

    const std::string bytes = readBytes(path);
    // Three lines of the fixture file itself, in the buffer's order now:
    // KB*'s feature rule, AC*'s symbol rule, AC*'s feature rule.
    const std::string lines =
        "    {\"key\": \"KB*\", \"sets\": \"feature\", \"layer\": \"TEST ROADS\", "
        "\"colour\": \"sui test purple\", \"draw\": \"line\", "
        "\"linestyle\": \"TEST Dashed Kerb\", \"weight\": \"0\", \"group\": \"TEST - ROADS\", "
        "\"comment\": \"[KB*] Kerb, in a colour no table knows\"},\n"
        "    {\"key\": \"AC*\", \"sets\": \"symbol\", \"comment\": \"[AC*] Access chamber\", "
        "\"hide\": false, \"symbol\": {\"name\": \"TEST Survey Mark\", \"colour\": \"white\", "
        "\"size\": 1.5}},\n"
        "    {\"key\": \"AC*\", \"sets\": \"feature\", \"layer\": \"TEST FURNITURE\", "
        "\"colour\": \"white\", \"draw\": \"point\", \"linestyle\": \"0\", \"weight\": \"0\", "
        "\"group\": \"TEST - FURNITURE\", \"comment\": \"[AC*] Access chamber\"},\n";
    EXPECT_NE(bytes.find(lines), std::string::npos) << bytes;
    auto read = katana::entity::customisationFromJson(bytes);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(read->map == dialog.buffer());
}

TEST(SurveyCodeManager, ASessionWithNoNameExportsItsCodesUnderTheFilesOwnName)
{
    // A drawing nothing was loaded into: its session has no name, and a
    // customisation file must have one. It is then the file's, without the
    // format's `.customisation.json`.
    Document document;
    SurveyMap map;
    katana::entity::SurveyRule rule;
    rule.key = "KT*";
    rule.model = "TEST KERBS";
    ASSERT_TRUE(map.add(rule).ok());
    document.setSurveyMap(map);
    ASSERT_EQ(document.customisationState().name, "");
    CustomisationContext context;
    context.document = &document;
    context.log = [](const QString&, bool) {};
    SurveyCodeManagerDialog dialog(context);
    const std::filesystem::path path = scratchFile("site codes.customisation.json");
    ASSERT_TRUE(dialog.exportCodes(path).ok());
    auto read = katana::entity::customisationFromJson(readBytes(path));
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->name, "site codes");
    EXPECT_TRUE(read->map == map);
}

TEST(SurveyCodeManager, TheCodeListExportIsTheBuffersCsv)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    const std::filesystem::path path = scratchFile("codes.csv");
    ASSERT_TRUE(dialog.exportCodeList(path).ok());
    const std::string csv = readBytes(path);
    EXPECT_EQ(csv, katana::cad::codeListCsv(dialog.buffer()));
    // WM*'s record, worked from rules #0 and #10: the `*` rule sets none of
    // these columns.
    EXPECT_NE(csv.find("WM*,[WM*] Water main,TEST - SERVICES,TEST SERVICES,blue,line,"
                       "TEST Water Main,,,\r\n"),
              std::string::npos)
        << csv;
}

TEST(SurveyCodeManager, ImportMergesACustomisationsRulesIntoTheBufferAndNotIntoTheDocument)
{
    ManagerFixture f;
    f.document.setSurveyMap(SurveyMap{});
    SurveyCodeManagerDialog dialog(f.context);
    ASSERT_TRUE(dialog.importCodes(customisationFixtureFile("test_survey")).ok());
    EXPECT_EQ(dialog.buffer().size(), 11u);
    EXPECT_TRUE(f.document.surveyMap().empty());
    EXPECT_TRUE(dialog.dirty());
    // A key once for each section it has rules in: 7 feature keys, 3
    // symbol keys and the one attributes key. The load is named by its
    // customisation, "test_survey", and the word of the mode.
    EXPECT_TRUE(f.loggedContaining(QStringLiteral("test_survey (merge): 11 added, 0 replaced")));
}

TEST(SurveyCodeManager, ImportTakesAFilesRulesAndLeavesItsDefinitionsColoursAndSettingsAlone)
{
    ManagerFixture f;
    const std::filesystem::path path = scratchFile("extra.customisation.json");
    writeBytes(path, kExtraCustomisation);
    const auto library = f.document.libraryGeneration();
    SurveyCodeManagerDialog dialog(f.context);

    // Merged: ZZ* is a key the fixture's 11 rules do not have, so it follows
    // them.
    ASSERT_TRUE(dialog.importCodes(path).ok());
    ASSERT_EQ(dialog.buffer().size(), 12u);
    EXPECT_EQ(dialog.buffer().rules()[11].key, "ZZ*");
    EXPECT_EQ(dialog.buffer().rules()[11].model, "TEST MISC");
    EXPECT_TRUE(dialog.dirty());
    EXPECT_TRUE(f.loggedContaining(QStringLiteral("extra (merge): 1 added, 0 replaced")));
    // Nothing else the file holds reached the drawing: not its symbol, its
    // colour or its spelling of Start, and not yet its rule either.
    EXPECT_EQ(f.document.surveyMap().size(), 11u);
    EXPECT_EQ(f.document.styleLibrary().size(), 7u);
    EXPECT_FALSE(f.document.styleLibrary().contains("TEST Extra Mark"));
    EXPECT_TRUE(f.document.customisationState().colours.empty());
    EXPECT_EQ(f.document.customisationState().linework.start, "ST");
    EXPECT_EQ(f.document.libraryGeneration(), library);
    // ... and the log says what was left, each kind with its count.
    EXPECT_TRUE(f.loggedContaining(QStringLiteral(
        "Not imported from extra.customisation.json, as this manager edits the survey code "
        "rules: definitions (1), colours (1), the linework codes.")));

    // Replace, as the box beside the button asks for: the file's rules are
    // the whole buffer. ZZ* was there already (replaced), and the fixture's 8
    // keys are what went.
    f.logged.clear();
    ASSERT_TRUE(dialog.importCodes(path, katana::cad::LoadMode::Replace).ok());
    ASSERT_EQ(dialog.buffer().size(), 1u);
    EXPECT_EQ(dialog.buffer().rules()[0].key, "ZZ*");
    EXPECT_TRUE(f.loggedContaining(QStringLiteral("extra (replace): 0 added, 1 replaced")));
    EXPECT_TRUE(f.loggedContaining(QStringLiteral("8 keys removed by Replace.")));
    EXPECT_EQ(f.document.surveyMap().size(), 11u);
}

TEST(SurveyCodeManager, ImportReplacesAKeysRulesInASectionWhereTheyStandAndKeepsItsOthers)
{
    // A file that gives rules to two keys the buffer HAS and to one it has
    // not. A merge is by key and section (cad/customisation_merge.hpp): the
    // rules a load gives a key in a section take the place of that key's
    // rules in that section, going in ahead of the key's rules it leaves
    // standing, at the first of them; a key the buffer lacks follows. Rules
    // merely added to the end would leave 14, with the buffer's own KB* and
    // AC* rules still ahead of the file's and so still deciding.
    ManagerFixture f;
    const std::filesystem::path path = scratchFile("kerbs.customisation.json");
    writeBytes(path, R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "kerbs",
  "codes": [
    {"key": "KB*", "sets": "feature", "layer": "TEST KERBS", "draw": "line"},
    {"key": "AC*", "sets": "symbol", "symbol": {"name": "TEST Valve", "size": 2}},
    {"key": "ZZ*", "sets": "feature", "layer": "TEST MISC"}
  ]
}
)");
    SurveyCodeManagerDialog dialog(f.context);
    ASSERT_TRUE(dialog.importCodes(path).ok());

    // The fixture's 11, less its KB* feature rule (#1) and its AC* symbol
    // rule (#7), with the file's three: 12.
    using katana::entity::SurveySection;
    const std::vector<std::pair<std::string, SurveySection>> expected{
        {"WM*", SurveySection::Map},
        {"KB*", SurveySection::Map},          // the file's, where the buffer's stood
        {"AC*", SurveySection::VertexSymbol}, // the file's, ahead of AC*'s feature rule
        {"AC*", SurveySection::Map},          // the buffer's own, left standing
        {"TR*", SurveySection::Map},
        {"1*", SurveySection::Map},
        {"2*", SurveySection::Map},
        {"PX*", SurveySection::Map},
        {"TR*", SurveySection::VertexSymbol},
        {"PX*", SurveySection::VertexSymbol},
        {"*", SurveySection::StringAttribute},
        {"ZZ*", SurveySection::Map}, // a key the buffer did not have
    };
    const std::vector<katana::entity::SurveyRule>& rules = dialog.buffer().rules();
    ASSERT_EQ(rules.size(), expected.size());
    for (std::size_t index = 0; index < expected.size(); ++index) {
        EXPECT_EQ(rules[index].key, expected[index].first) << "#" << index;
        EXPECT_EQ(rules[index].section, expected[index].second) << "#" << index;
    }
    // Replaced whole, not field by field: the file's KB* rule gives no
    // colour, and the buffer's "sui test purple" has gone with its rule.
    EXPECT_EQ(rules[1].model, "TEST KERBS");
    EXPECT_EQ(rules[1].colour, "");
    EXPECT_EQ(rules[1].linestyle, "");
    ASSERT_TRUE(rules[2].symbol.has_value());
    EXPECT_EQ(rules[2].symbol->style, "TEST Valve");
    EXPECT_EQ(rules[2].symbol->size, 2.0);
    EXPECT_EQ(rules[3].model, "TEST FURNITURE");
    EXPECT_EQ(rules[11].model, "TEST MISC");
    // So AC01 is now drawn with the valve, and a kerb goes to the new layer.
    EXPECT_EQ(dialog.buffer()
                  .lookup("AC01")
                  .resolved.symbol.value_or(katana::entity::SurveySymbol{})
                  .style,
              "TEST Valve");
    EXPECT_EQ(dialog.buffer().lookup("KB01").resolved.model, "TEST KERBS");
    EXPECT_TRUE(f.loggedContaining(QStringLiteral("kerbs (merge): 1 added, 2 replaced")));
    // For review: the drawing still has its own 11.
    EXPECT_TRUE(dialog.dirty());
    EXPECT_EQ(f.document.surveyMap().size(), 11u);
    EXPECT_EQ(f.document.surveyMap().lookup("KB01").resolved.model, "TEST ROADS");
}

TEST(SurveyCodeManager, ImportRefusesWhatIsNotAKatanaCustomisationOrHoldsNoRulesAndKeepsTheBuffer)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    const SurveyMap before = dialog.buffer();

    // A file of another format - here a definition as the older style
    // libraries wrote one - is not JSON, and is told what it is not.
    const std::filesystem::path other = scratchFile("older_library.txt");
    writeBytes(other, "worldstyle \"TEST Old\" {\n  move 0 0\n  draw 1 0\n}\n");
    const auto refused = dialog.importCodes(other);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::ParseFailure);
    EXPECT_NE(refused.error().message.find("not a Katana customisation file"), std::string::npos)
        << refused.error().describe();

    // A customisation of symbols alone holds nothing for this manager.
    const auto noRules = dialog.importCodes(customisationFixtureFile("test_symbols"));
    ASSERT_FALSE(noRules.ok());
    EXPECT_EQ(noRules.error().code, katana::core::ErrorCode::NotFound);
    EXPECT_EQ(noRules.error().message, "the file holds no survey code rules");

    // A file that is not there.
    const std::filesystem::path missing = scratchFile("no_such.customisation.json");
    std::filesystem::remove(missing);
    const auto absent = dialog.importCodes(missing);
    ASSERT_FALSE(absent.ok());
    EXPECT_EQ(absent.error().code, katana::core::ErrorCode::NotFound);

    EXPECT_TRUE(dialog.buffer() == before);
    EXPECT_FALSE(dialog.dirty());
}

TEST(SurveyCodeManager, AColourTheCustomisationDefinesIsOfferedSwatchedExplainedAndNoLongerAnIssue)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* tree = child<QTreeWidget>(dialog, "codeTree");
    auto* issues = child<QTreeWidget>(dialog, "issuesTable");
    auto* colour = child<QComboBox>(dialog, "ruleColour");
    ASSERT_NE(tree, nullptr);
    ASSERT_NE(issues, nullptr);
    ASSERT_NE(colour, nullptr);
    // KB* (#1) is coloured "sui test purple", which is not a standard name.
    // Until the session's customisation defines it, the fields offer no
    // colour and then the 27 standard names, and it is an issue.
    const std::vector<std::string> standard = katana::entity::standardColourNames();
    ASSERT_EQ(standard.size(), 27u);
    ASSERT_EQ(colour->count(), 1 + 27);
    ASSERT_EQ(issues->topLevelItemCount(), 2);
    dialog.selectRule(1);
    ASSERT_EQ(colour->currentText(), QStringLiteral("sui test purple"));

    // The customisation's own colours change while the manager is open:
    // purple, 128 0 128.
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());
    f.document.setColourTable(colours);
    katana::qt::test::processEvents();

    // OFFERED, in each of the three colour fields: the customisation's name
    // after the empty first entry and ahead of the standard ones, swatched.
    for (const char* name : {"ruleColour", "ruleSymbolColour", "ruleTextColour"}) {
        auto* field = child<QComboBox>(dialog, name);
        ASSERT_NE(field, nullptr);
        ASSERT_EQ(field->count(), 1 + 1 + 27) << name;
        EXPECT_EQ(field->itemText(0), QString()) << name;
        EXPECT_EQ(field->itemText(1), QStringLiteral("sui test purple")) << name;
        EXPECT_FALSE(field->itemIcon(1).isNull()) << name;
        EXPECT_EQ(field->itemText(2), QString::fromStdString(standard.front())) << name;
        EXPECT_EQ(field->itemText(28), QString::fromStdString(standard.back())) << name;
    }
    // What the form showed is still what it shows, and the rule is untouched.
    EXPECT_EQ(colour->currentText(), QStringLiteral("sui test purple"));
    EXPECT_FALSE(dialog.dirty());

    // DRAWN: the Code Table swatches it where it said "(no RGB)" ...
    const QTreeWidgetItem* kerb = keyRow(*tree, QStringLiteral("KB*"));
    ASSERT_NE(kerb, nullptr);
    EXPECT_EQ(kerb->text(3), QStringLiteral("sui test purple"));
    EXPECT_FALSE(kerb->icon(3).isNull());
    // ... the explanation gives its RGB ...
    child<QLineEdit>(dialog, "testCode")->setText(QStringLiteral("KB01"));
    EXPECT_TRUE(child<QLabel>(dialog, "explainDefinitions")
                    ->text()
                    .split(QLatin1Char('\n'))
                    .contains(QStringLiteral("Colour sui test purple: #800080")))
        << child<QLabel>(dialog, "explainDefinitions")->text().toStdString();
    // ... and of the two issues only PX*'s undefined symbol (#9) is left.
    ASSERT_EQ(issues->topLevelItemCount(), 1);
    EXPECT_EQ(issues->topLevelItem(0)->text(1), QStringLiteral("#9"));

    // A manager opened afterwards lists it from the start.
    SurveyCodeManagerDialog later(f.context);
    EXPECT_EQ(child<QComboBox>(later, "ruleColour")->itemText(1),
              QStringLiteral("sui test purple"));
}

// ---- the customisation's colours where the manager ACTS on them ---------------------
//
// The test above reaches the fields, the table and the explanation. These
// reach the three other places a colour name is resolved: the form's own
// issues, Apply Codes and Linework. "sui test purple" is 128 0 128, a colour
// only the session's customisation gives that name - the standard names do
// not know it - so each fails where the standard names alone are asked.

TEST(SurveyCodeManager, TheFormsOwnIssuesKnowAColourTheCustomisationDefines)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    auto* issues = child<QLabel>(dialog, "ruleIssues");
    ASSERT_NE(issues, nullptr);
    // KB* (#1) in the form: its one issue is its colour, which nothing
    // defines yet (of the fixture's two issues the other is #9's symbol).
    dialog.selectRule(1);
    EXPECT_TRUE(issues->text().contains(QStringLiteral("sui test purple")))
        << issues->text().toStdString();
    EXPECT_NE(issues->text(), QStringLiteral("No issues with this rule."));

    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());
    f.document.setColourTable(colours);
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.currentRule(), std::optional<std::size_t>(1));
    EXPECT_EQ(issues->text(), QStringLiteral("No issues with this rule."));
}

TEST(SurveyCodeManager, ApplyCodesColoursAStyleByTheCustomisationsOwnColour)
{
    ManagerFixture f;
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());
    f.document.setColourTable(colours);
    const EntityId kerb = addPoint(f.document, 0.0, "KB01", "1");
    SurveyCodeManagerDialog dialog(f.context);

    child<QPushButton>(dialog, "applyExecute")->click();

    // KB* gives the linestyle TEST Dashed Kerb and the colour "sui test
    // purple". No style draws that yet, so one is made, named after the
    // linestyle (cad/survey_coding.hpp), in the colour the name means HERE.
    const Entity* coded = f.document.model().entities.find(kerb);
    ASSERT_NE(coded, nullptr);
    EXPECT_EQ(coded->layer, "TEST ROADS");
    ASSERT_EQ(coded->style, "TEST Dashed Kerb");
    const katana::entity::Style* style = f.document.model().styles.find("TEST Dashed Kerb");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->color, std::optional(katana::entity::Color{128, 0, 128, 255}));
}

TEST(SurveyCodeManager, LineworkColoursItsLineByTheCustomisationsOwnColour)
{
    ManagerFixture f;
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());
    f.document.setColourTable(colours);
    // KB* is a Line code: three points of string KB01 make one line.
    addPoint(f.document, 0.0, "KB01", "1");
    addPoint(f.document, 5.0, "KB01", "2");
    addPoint(f.document, 10.0, "KB01", "3");
    SurveyCodeManagerDialog dialog(f.context);

    child<QPushButton>(dialog, "lineworkExecute")->click();

    // The one line Linework drew, styled as its code is: the style made for
    // it carries the colour the customisation gives the rule's name.
    std::vector<Entity> lines;
    f.document.model().entities.forEach([&](const Entity& entity) {
        if (entity.type() == katana::entity::EntityType::Polyline) {
            lines.push_back(entity);
        }
    });
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(lines.front().layer, "TEST ROADS");
    ASSERT_EQ(lines.front().style, "TEST Dashed Kerb");
    const katana::entity::Style* style = f.document.model().styles.find("TEST Dashed Kerb");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->color, std::optional(katana::entity::Color{128, 0, 128, 255}));
}

TEST(SurveyCodeManager, ClosingHeadlessWithUnappliedEditsAsksNothingAndKeepsThem)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    dialog.show();
    editWaterMainLayer(dialog, QStringLiteral("TEST WATER"));
    child<QPushButton>(dialog, "closeManager")->click();
    EXPECT_FALSE(dialog.isVisible());
    EXPECT_TRUE(dialog.dirty());
    EXPECT_EQ(dialog.buffer().rules()[0].model, "TEST WATER");
    EXPECT_TRUE(f.loggedContaining(QStringLiteral("unapplied edits")));
}

TEST(SurveyCodeManager, AMapLoadedElsewhereIsTakenUpWhenTheBufferIsUnedited)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    SurveyMap replacement;
    katana::entity::SurveyRule rule;
    rule.key = "QQ*";
    rule.model = "ELSEWHERE";
    ASSERT_TRUE(replacement.add(rule).ok());
    f.document.setSurveyMap(replacement);
    katana::qt::test::processEvents();
    EXPECT_TRUE(dialog.buffer() == replacement);
    EXPECT_FALSE(dialog.dirty());
}

TEST(SurveyCodeManager, EditsKeptOverAMapChangedElsewhereStayUnappliedWhenUndone)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    const SurveyMap original = dialog.buffer();
    // An edit (12 rules), then the drawing takes a one-rule map from
    // elsewhere, then the edit is undone: the buffer is the fixture's 11 rules
    // again. The drawing holds 1 rule, so the buffer still differs from what
    // the drawing has - Apply and Revert both still have work to do, and the
    // two tabs that run the drawing's map still say so.
    ASSERT_TRUE(dialog.duplicateRule(1).ok());
    SurveyMap replacement;
    katana::entity::SurveyRule rule;
    rule.key = "QQ*";
    rule.model = "ELSEWHERE";
    ASSERT_TRUE(replacement.add(rule).ok());
    f.document.setSurveyMap(replacement);
    katana::qt::test::processEvents();
    ASSERT_TRUE(f.loggedContaining(QStringLiteral("changed elsewhere")));
    ASSERT_TRUE(dialog.removeRule(2).ok());
    ASSERT_TRUE(dialog.buffer() == original);
    ASSERT_EQ(f.document.surveyMap().size(), 1u);

    EXPECT_TRUE(dialog.dirty());
    EXPECT_TRUE(child<QPushButton>(dialog, "applyMap")->isEnabled());
    EXPECT_TRUE(child<QPushButton>(dialog, "revertMap")->isEnabled());
    EXPECT_TRUE(child<QLabel>(dialog, "dirtyIndicator")->text().startsWith(
        QStringLiteral("Unapplied edits")));
    EXPECT_FALSE(child<QLabel>(dialog, "applyDirtyNote")->isHidden());
    EXPECT_FALSE(child<QLabel>(dialog, "lineworkDirtyNote")->isHidden());

    // Revert now takes the drawing's one rule, and nothing is left unapplied.
    child<QPushButton>(dialog, "revertMap")->click();
    EXPECT_TRUE(dialog.buffer() == replacement);
    EXPECT_FALSE(dialog.dirty());
}

TEST(SurveyCodeManager, TheDialogOutlivesItsDocumentAndThenRefusesToApply)
{
    auto document = std::make_unique<Document>();
    CustomisationContext context;
    context.document = document.get();
    context.log = [](const QString&, bool) {};
    SurveyCodeManagerDialog dialog(context);
    document.reset();
    katana::qt::test::processEvents();
    EXPECT_FALSE(dialog.apply().ok());
    child<QLineEdit>(dialog, "testCode")->setText(QStringLiteral("WM01"));
    child<QPushButton>(dialog, "applyPreview")->click();
    child<QPushButton>(dialog, "lineworkPreview")->click();
    child<QPushButton>(dialog, "ruleNew")->click();
    katana::qt::test::paint(dialog);
}

TEST(SurveyCodeManager, OnceItsDocumentHasGoneTheFormSavesEachRuleWithItsOwnNames)
{
    auto document = std::make_unique<Document>();
    installCustomisationFixtures(*document);
    CustomisationContext context;
    context.document = document.get();
    context.log = [](const QString&, bool) {};
    SurveyCodeManagerDialog dialog(context);
    const SurveyMap original = dialog.buffer();
    // While the drawing is open the pickers are loaded last with #7's symbol
    // ("TEST Survey Mark") and then #0's linestyle ("TEST Water Main"). Once
    // it has gone they can no longer be loaded (they read its library).
    dialog.selectRule(7);
    dialog.selectRule(0);
    document.reset();
    katana::qt::test::processEvents();

    // Saved with no edit, a rule is itself: #4 (1* feature) keeps linestyle
    // "0" and #8 (TR* symbol) symbol "TEST Tree" - from the
    // fixture's text - not the names the pickers were last loaded with.
    for (const std::size_t index : {std::size_t{4}, std::size_t{8}}) {
        dialog.selectRule(index);
        auto* save = child<QPushButton>(dialog, "ruleUpdate");
        ASSERT_TRUE(save->isEnabled());
        save->click();
        EXPECT_TRUE(dialog.buffer() == original) << "after saving #" << index;
    }
    EXPECT_EQ(dialog.buffer().at(4)->linestyle, "0");
    EXPECT_EQ(dialog.buffer().at(8)->symbol.value_or(katana::entity::SurveySymbol{}).style,
              "TEST Tree");
    EXPECT_FALSE(dialog.dirty());

    // What a picker shows is what Save writes, and it cannot be changed:
    // #8 was loaded last, so the symbol picker shows "TEST Tree".
    auto* symbol = child<QComboBox>(dialog, "ruleSymbol");
    EXPECT_FALSE(symbol->isEnabled());
    EXPECT_EQ(symbol->currentText(), QStringLiteral("TEST Tree"));

    // A new rule has no linestyle, whatever the picker was last loaded with.
    child<QPushButton>(dialog, "ruleNew")->click();
    const auto fresh = dialog.formRule();
    ASSERT_TRUE(fresh.ok()) << fresh.error().describe();
    EXPECT_EQ(fresh->linestyle, "");
}

TEST(SurveyCodeManager, ATypedColourNameIsKeptAsWrittenListedInAnotherCaseOrNotAtAll)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    dialog.selectRule(0);
    auto* colour = child<QComboBox>(dialog, "ruleColour");
    ASSERT_NE(colour, nullptr);
    ASSERT_NE(colour->lineEdit(), nullptr);
    // "Blue" is listed only as "blue": Enter must not fold it into the listed
    // name (a combo's own lookup at the end of an edit would, with a
    // case-insensitive completer), because a map keeps names as written.
    for (const QString& typed : {QStringLiteral("Blue"), QStringLiteral("sui water potable")}) {
        colour->lineEdit()->selectAll();
        colour->lineEdit()->setText(typed);
        QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
        QCoreApplication::sendEvent(colour->lineEdit(), &enter);
        const auto rule = dialog.formRule();
        ASSERT_TRUE(rule.ok()) << rule.error().describe();
        EXPECT_EQ(rule->colour, typed.toStdString());
    }
}

TEST(SurveyCodeManager, EnterInAFieldPressesNoButton)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    ASSERT_TRUE(katana::qt::test::showActive(dialog));
    // Every field a person types a name into and confirms with Enter: none
    // may reach a dialog default button - the first button, Import Codes,
    // would open a file dialog, and Delete or Apply would edit.
    for (const char* name : {"testCode", "ruleModel", "ruleKey", "lineworkStart"}) {
        auto* field = child<QLineEdit>(dialog, name);
        ASSERT_NE(field, nullptr);
        QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
        QCoreApplication::sendEvent(field, &enter);
    }
    katana::qt::test::processEvents();
    EXPECT_TRUE(f.logged.empty()) << f.logged.front().first.toStdString();
    EXPECT_TRUE(dialog.isVisible());
    EXPECT_EQ(dialog.buffer().size(), 11u);
}

TEST(SurveyCodeManager, DoubleClickingAnExplainedFieldOpensTheRuleThatSetIt)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    child<QLineEdit>(dialog, "testCode")->setText(QStringLiteral("AC01"));
    auto* fields = child<QTreeWidget>(dialog, "explainFields");
    ASSERT_NE(fields, nullptr);
    // AC01's symbol comes from #7, AC* symbol.
    const QTreeWidgetItem* symbol = rowWithText(*fields, 0, QStringLiteral("symbol"));
    ASSERT_NE(symbol, nullptr);
    EXPECT_EQ(symbol->text(2), QStringLiteral("#7 AC* (symbol)"));
    fields->itemDoubleClicked(const_cast<QTreeWidgetItem*>(symbol), 0);
    // The jump waits for the click to return (it rebuilds this list).
    EXPECT_FALSE(dialog.currentRule().has_value());
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.currentRule(), std::optional<std::size_t>(7));
    EXPECT_EQ(child<QLineEdit>(dialog, "ruleSymbolSize")->text(), QStringLiteral("1.5"));
}
