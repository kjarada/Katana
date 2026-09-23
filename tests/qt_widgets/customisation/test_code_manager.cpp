// The survey code manager, driven as a person drives it - through its widgets,
// found by objectName - over the committed hand-written 12d customisation in
// tests/archive12d/data/customisation, loaded into the Document as a session
// customisation. What is asserted is the Document and the buffer.
//
// The fixture, counted by hand from test_survey.mapfile (its head comment
// carries the same counts): 11 rules over 8 keys, in file order
//   #0 WM* map_data   TEST SERVICES, blue, Line, TEST Water Main
//   #1 KB* map_data   TEST ROADS, "sui test purple" (no colour table knows it)
//   #2 AC* map_data   TEST FURNITURE, white, Point, "0"
//   #3 TR* map_data   TEST VEGETATION, green, Point, "0"
//   #4 1*  map_data   TEST TEXT, orange
//   #5 2*  map_data   TEST TEXT, red
//   #6 PX* map_data   TEST MISC, yellow, Point, "0"
//   #7 AC* vertex_symbol_data  TEST Survey Mark (mode vertex), 1.5
//   #8 TR* vertex_symbol_data  TEST Tree (not mode vertex, from a symbol file)
//   #9 PX* vertex_symbol_data  TEST Missing Symbol (defined nowhere)
//   #10 *  string_attribute_data  text Source = Katana test fixture

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
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
#include "katana/archive12d/customisation.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/cad/code_edit.hpp"
#include "katana/cad/code_table.hpp"
#include "katana/commands/entity_commands.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::SurveyMap;
using katana::qt::CustomisationContext;
using katana::qt::DefinitionThumbnails;
using katana::qt::SurveyCodeManagerDialog;

namespace {

// Beside this file: tests/qt_widgets/customisation -> tests/archive12d/data.
const std::filesystem::path kFixture = std::filesystem::path(__FILE__)
                                           .parent_path()
                                           .parent_path()
                                           .parent_path() /
                                       "archive12d" / "data" / "customisation";

katana::archive12d::Customisation loadFixture()
{
    auto loaded = katana::archive12d::readCustomisation(
        {kFixture / "test_linestyles.4d", kFixture / "test_survey.mapfile",
         kFixture / "test_symbols.4d"});
    EXPECT_TRUE(loaded.ok()) << (loaded.ok() ? "" : loaded.error().describe());
    return loaded.ok() ? std::move(*loaded) : katana::archive12d::Customisation{};
}

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
        katana::archive12d::Customisation fixture = loadFixture();
        document.setStyleLibrary(std::move(fixture.library));
        document.setSurveyMap(std::move(fixture.map));
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

// Selects rule 0 (WM* map_data) and saves it with another layer, through the
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
    // map_data layer and the vertex symbol with its size.
    const QTreeWidgetItem* furniture = rowWithText(*tree, 0, QStringLiteral("TEST - FURNITURE"));
    ASSERT_NE(furniture, nullptr);
    ASSERT_EQ(furniture->childCount(), 1);
    const QTreeWidgetItem* chamber = furniture->child(0);
    EXPECT_EQ(chamber->childCount(), 2);
    EXPECT_EQ(chamber->text(2), QStringLiteral("TEST FURNITURE"));
    EXPECT_EQ(chamber->text(4), QStringLiteral("Point"));
    EXPECT_EQ(chamber->text(6), QStringLiteral("TEST Survey Mark, 1.5"));
    EXPECT_EQ(chamber->child(0)->text(0), QStringLiteral("#2 AC* (map_data)"));
    EXPECT_EQ(chamber->child(1)->text(0), QStringLiteral("#7 AC* (vertex_symbol_data)"));
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
    // The Symbol chip: the three keys with a vertex_symbol_data rule.
    auto* symbolChip = bar->findChild<QAbstractButton*>(QStringLiteral("filterSymbol"));
    ASSERT_NE(symbolChip, nullptr);
    symbolChip->click();
    EXPECT_EQ(keysShown(*tree), (std::set<std::string>{"AC*", "PX*", "TR*"}));
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
    // WM01 meets WM* (rule #0) and `*` (rule #10); its layer - 12d's model -
    // comes from #0.
    const QTreeWidgetItem* model = rowWithText(*fields, 0, QStringLiteral("model"));
    ASSERT_NE(model, nullptr);
    EXPECT_EQ(model->text(1), QStringLiteral("TEST SERVICES"));
    EXPECT_EQ(model->text(2), QStringLiteral("#0 WM* (map_data)"));
    auto* heading = child<QLabel>(dialog, "explainHeading");
    ASSERT_NE(heading, nullptr);
    EXPECT_TRUE(heading->text().contains(QStringLiteral("matched"))) << heading->text().toStdString();
    EXPECT_TRUE(heading->text().contains(QStringLiteral("WM*")));
    // The `*` rule's attribute, cited to it.
    const QTreeWidgetItem* attribute =
        rowWithText(*fields, 0, QStringLiteral("attribute (string)"));
    ASSERT_NE(attribute, nullptr);
    EXPECT_EQ(attribute->text(1), QStringLiteral("text Source = Katana test fixture"));
    EXPECT_EQ(attribute->text(2), QStringLiteral("#10 * (string_attribute_data)"));
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
        [](std::string_view name) { return katana::archive12d::standardColour(name); });
    const auto model = std::find_if(explained.fields.begin(), explained.fields.end(),
                                    [](const auto& field) { return field.field == "model"; });
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
    // TR*'s TEST Tree came from a symbol file (D3).
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

TEST(SurveyCodeManager, AnExportedMapfileIsUtf16WithAByteOrderMarkAndReadsBackAsTheBuffer)
{
    ManagerFixture f;
    SurveyCodeManagerDialog dialog(f.context);
    editWaterMainLayer(dialog, QStringLiteral("TEST WATER"));
    const std::filesystem::path path = scratchFile("exported.mapfile");
    ASSERT_TRUE(dialog.exportMapfile(path).ok());

    const std::string bytes = readBytes(path);
    ASSERT_GE(bytes.size(), 4u);
    // FF FE, then '<' as 3C 00: UTF-16 little-endian, as 12d writes.
    EXPECT_EQ(static_cast<unsigned char>(bytes[0]), 0xFFu);
    EXPECT_EQ(static_cast<unsigned char>(bytes[1]), 0xFEu);
    EXPECT_EQ(bytes[2], '<');
    EXPECT_EQ(bytes[3], '\0');

    auto read = katana::archive12d::readCustomisation({path});
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(read->map == dialog.buffer());
    EXPECT_EQ(read->map.rules()[0].model, "TEST WATER");
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

TEST(SurveyCodeManager, ImportMergesAMapfileIntoTheBufferAndNotIntoTheDocument)
{
    ManagerFixture f;
    f.document.setSurveyMap(SurveyMap{});
    SurveyCodeManagerDialog dialog(f.context);
    ASSERT_TRUE(dialog.importMapfile(kFixture / "test_survey.mapfile").ok());
    EXPECT_EQ(dialog.buffer().size(), 11u);
    EXPECT_TRUE(f.document.surveyMap().empty());
    EXPECT_TRUE(dialog.dirty());
    // A key once for each section it has rules in: 7 map_data keys, 3
    // vertex_symbol_data keys and the one string_attribute_data key.
    EXPECT_TRUE(f.loggedContaining(QStringLiteral("11 added, 0 replaced")));
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
    // may reach a dialog default button - the first button, Import Mapfile,
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
    // AC01's symbol comes from #7, AC* vertex_symbol_data.
    const QTreeWidgetItem* symbol = rowWithText(*fields, 0, QStringLiteral("symbol"));
    ASSERT_NE(symbol, nullptr);
    EXPECT_EQ(symbol->text(2), QStringLiteral("#7 AC* (vertex_symbol_data)"));
    fields->itemDoubleClicked(const_cast<QTreeWidgetItem*>(symbol), 0);
    // The jump waits for the click to return (it rebuilds this list).
    EXPECT_FALSE(dialog.currentRule().has_value());
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.currentRule(), std::optional<std::size_t>(7));
    EXPECT_EQ(child<QLineEdit>(dialog, "ruleSymbolSize")->text(), QStringLiteral("1.5"));
}
