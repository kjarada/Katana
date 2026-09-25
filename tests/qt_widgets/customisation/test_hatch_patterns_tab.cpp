// The Hatch Patterns tab of Format > Styles and Linetypes (src/katana_qt/
// customisation/hatch_patterns_tab.hpp), driven by its object names as a
// person drives it, and judged by the lines it hands the executor and by the
// Document. The executor is the real interpreter on the same drawing, where
// the window's command line sends a HATCH line, so each line is also known to
// do what it says; qt_hatch_patterns_tab_headless runs it through the window.

#include <gtest/gtest.h>

#include <QApplication>
#include <QCheckBox>
#include <QGroupBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTabWidget>
#include <QTableWidget>

#include <memory>
#include <string>
#include <vector>

#include "command_runner.hpp"
#include "customisation/customisation_context.hpp"
#include "customisation/hatch_patterns_tab.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/math/numerics.hpp"
#include "style_manager.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::qt::CustomisationContext;
using katana::qt::HatchPatternsTab;
using katana::qt::VerbOutcome;

namespace {

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

void click(QWidget& parent, const char* name)
{
    auto* button = child<QPushButton>(parent, name);
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
    button->click();
    QApplication::processEvents();
}

QString cellText(QWidget& parent, const char* table, int row, int column)
{
    auto* grid = child<QTableWidget>(parent, table);
    if (grid == nullptr || grid->item(row, column) == nullptr) {
        ADD_FAILURE() << "no cell " << row << "," << column << " in " << table;
        return {};
    }
    return grid->item(row, column)->text();
}

// A drawing with "brick" (45 degrees every 0.25) hatching the layer "site",
// on which a closed polyline is drawn, and the tab over it.
struct Fixture {
    Fixture() : interpreter(document)
    {
        for (const char* line : {"HATCH NEW brick 45 0.25", "LAYER NEW site",
                                 "LAYER HATCH site brick", "LAYER SET site",
                                 "PLINE 0,0 10,0 10,10 0,10 CLOSE", "LAYER SET 0"}) {
            EXPECT_TRUE(interpreter.run(line).ok()) << line;
        }
        context.document = &document;
        context.log = [this](const QString& text, bool) { logged.push_back(text); };
        context.run = [this](const QString& line) {
            lines.push_back(line);
            VerbOutcome outcome;
            const auto reply = interpreter.run(line.toStdString());
            outcome.ok = reply.ok();
            if (reply.ok()) {
                outcome.reply = QString::fromStdString(*reply);
            } else {
                outcome.error = QString::fromStdString(reply.error().describe());
            }
            return outcome;
        };
        tab = std::make_unique<HatchPatternsTab>(context);
    }
    [[nodiscard]] const katana::entity::HatchPattern* pattern(const std::string& name) const
    {
        return document.model().hatchPatterns.find(name);
    }
    [[nodiscard]] QString last() const { return lines.empty() ? QString() : lines.back(); }
    [[nodiscard]] QString status() const
    {
        return tab->findChild<QLabel*>("hatchPatternStatus")->text();
    }

    Document document;
    CommandInterpreter interpreter;
    CustomisationContext context;
    std::vector<QString> lines;
    std::vector<QString> logged;
    std::unique_ptr<HatchPatternsTab> tab;
};

} // namespace

TEST(HatchPatternsTab, TheTableListsEveryPatternWithItsKindAndWhatUsesIt)
{
    Fixture f;
    auto* table = child<QTableWidget>(*f.tab, "hatchPatternTable");
    ASSERT_EQ(table->rowCount(), 2);
    EXPECT_EQ(cellText(*f.tab, "hatchPatternTable", 0, 0).toStdString(), "brick");
    EXPECT_EQ(cellText(*f.tab, "hatchPatternTable", 0, 1).toStdString(), "1 family");
    EXPECT_TRUE(cellText(*f.tab, "hatchPatternTable", 0, 2).startsWith("used by 1 layer"))
        << cellText(*f.tab, "hatchPatternTable", 0, 2).toStdString();
    EXPECT_EQ(cellText(*f.tab, "hatchPatternTable", 1, 0).toStdString(), "none");
    EXPECT_EQ(cellText(*f.tab, "hatchPatternTable", 1, 1).toStdString(), "draws nothing");
    // Nothing chosen yet: nothing to edit.
    EXPECT_FALSE(child<QGroupBox>(*f.tab, "hatchPatternEditor")->isEnabled());
}

TEST(HatchPatternsTab, AnEditIsSavedAsOneHatchSetLineAndOneUndoStep)
{
    Fixture f;
    ASSERT_TRUE(f.tab->selectPattern("brick"));
    EXPECT_EQ(cellText(*f.tab, "hatchPatternFamilies", 0, 0).toStdString(), "45");
    EXPECT_EQ(cellText(*f.tab, "hatchPatternFamilies", 0, 1).toStdString(), "0.25");
    EXPECT_FALSE(child<QPushButton>(*f.tab, "hatchPatternSave")->isEnabled());

    child<QTableWidget>(*f.tab, "hatchPatternFamilies")->item(0, 1)->setText("0.5");
    // The table's own signal runs nothing.
    EXPECT_TRUE(f.lines.empty());
    click(*f.tab, "hatchFamilyAdd");
    auto* families = child<QTableWidget>(*f.tab, "hatchPatternFamilies");
    families->item(1, 0)->setText("135");
    families->item(1, 1)->setText("0.5");
    const std::size_t before = f.document.history().undoCount();
    click(*f.tab, "hatchPatternSave");
    EXPECT_EQ(f.last().toStdString(), "HATCH SET brick 45 0.5 135 0.5");
    ASSERT_EQ(f.pattern("brick")->families.size(), 2u);
    EXPECT_EQ(f.pattern("brick")->families[0].spacing, 0.5);
    EXPECT_EQ(f.document.history().undoCount(), before + 1);
    EXPECT_EQ(f.status().toStdString(), "hatch pattern brick updated (2 families)");
    EXPECT_EQ(cellText(*f.tab, "hatchPatternTable", 0, 1).toStdString(), "2 families");
    EXPECT_FALSE(child<QPushButton>(*f.tab, "hatchPatternSave")->isEnabled());
}

TEST(HatchPatternsTab, SolidIsSavedAsHatchSetSolidAndRevertPutsTheDrawingsBack)
{
    Fixture f;
    f.tab->selectPattern("brick");
    child<QCheckBox>(*f.tab, "hatchPatternSolid")->setChecked(true);
    EXPECT_FALSE(child<QTableWidget>(*f.tab, "hatchPatternFamilies")->isEnabled());
    click(*f.tab, "hatchPatternRevert");
    EXPECT_FALSE(child<QCheckBox>(*f.tab, "hatchPatternSolid")->isChecked());
    EXPECT_TRUE(f.lines.empty());
    child<QCheckBox>(*f.tab, "hatchPatternSolid")->setChecked(true);
    click(*f.tab, "hatchPatternSave");
    EXPECT_EQ(f.last().toStdString(), "HATCH SET brick SOLID");
    EXPECT_TRUE(f.pattern("brick")->solid);
}

TEST(HatchPatternsTab, AFamilyWithOneNumberIsRefusedBeforeAnythingRuns)
{
    Fixture f;
    f.tab->selectPattern("brick");
    click(*f.tab, "hatchFamilyAdd");
    child<QTableWidget>(*f.tab, "hatchPatternFamilies")->item(1, 0)->setText("90");
    click(*f.tab, "hatchPatternSave");
    EXPECT_TRUE(f.lines.empty());
    EXPECT_EQ(f.status().toStdString(), "family 2 needs an angle and a spacing");
}

TEST(HatchPatternsTab, UnsavedEditsSurviveAReloadThatLeftThePatternAlone)
{
    Fixture f;
    f.tab->selectPattern("brick");
    child<QTableWidget>(*f.tab, "hatchPatternFamilies")->item(0, 1)->setText("2");
    ASSERT_TRUE(f.interpreter.run("LAYER NEW other").ok());
    QApplication::processEvents();
    EXPECT_EQ(cellText(*f.tab, "hatchPatternFamilies", 0, 1).toStdString(), "2");
    // Changed under it, the editor shows the drawing's pattern again.
    ASSERT_TRUE(f.interpreter.run("HATCH SET brick 45 0.75").ok());
    QApplication::processEvents();
    EXPECT_EQ(cellText(*f.tab, "hatchPatternFamilies", 0, 1).toStdString(), "0.75");
}

TEST(HatchPatternsTab, NewNewSolidAndDuplicateRunTheirLinesUnderTheTypedName)
{
    Fixture f;
    f.tab->selectPattern("brick");
    auto* name = child<QLineEdit>(*f.tab, "hatchPatternName");
    // New takes the families in the grid.
    child<QTableWidget>(*f.tab, "hatchPatternFamilies")->item(0, 0)->setText("30");
    name->setText("slope");
    click(*f.tab, "hatchPatternNew");
    EXPECT_EQ(f.last().toStdString(), "HATCH NEW slope 30 0.25");
    ASSERT_NE(f.pattern("slope"), nullptr);
    EXPECT_EQ(f.tab->currentPattern().toStdString(), "slope");

    name->setText("fill");
    click(*f.tab, "hatchPatternNewSolid");
    EXPECT_EQ(f.last().toStdString(), "HATCH SOLID fill");
    ASSERT_NE(f.pattern("fill"), nullptr);

    // Duplicate takes the chosen pattern as the drawing holds it, in the
    // degrees it was typed in.
    f.tab->selectPattern("slope");
    name->setText("slope copy");
    click(*f.tab, "hatchPatternDuplicate");
    EXPECT_EQ(f.last().toStdString(), "HATCH NEW \"slope copy\" 30 0.25");
    ASSERT_NE(f.pattern("slope copy"), nullptr);
    EXPECT_EQ(f.pattern("slope copy")->families, f.pattern("slope")->families);
}

TEST(HatchPatternsTab, DeleteIsRefusedWhileALayerNamesThePatternAndPurgeTakesTheUnused)
{
    Fixture f;
    ASSERT_TRUE(f.interpreter.run("HATCH SOLID fill").ok());
    QApplication::processEvents();
    f.tab->selectPattern("brick");
    click(*f.tab, "hatchPatternDelete");
    EXPECT_EQ(f.last().toStdString(), "HATCH DELETE brick");
    EXPECT_NE(f.pattern("brick"), nullptr);
    EXPECT_TRUE(f.status().contains("site")) << f.status().toStdString();

    click(*f.tab, "hatchPatternPurge");
    EXPECT_EQ(f.last().toStdString(), "PURGE HATCHES");
    EXPECT_EQ(f.pattern("fill"), nullptr);
    EXPECT_NE(f.pattern("brick"), nullptr);
    EXPECT_NE(f.pattern("none"), nullptr);
}

TEST(HatchPatternsTab, NoneIsNeitherEditedNorDeleted)
{
    Fixture f;
    f.tab->selectPattern("none");
    EXPECT_FALSE(child<QGroupBox>(*f.tab, "hatchPatternEditor")->isEnabled());
    EXPECT_FALSE(child<QPushButton>(*f.tab, "hatchPatternDelete")->isEnabled());
    EXPECT_FALSE(child<QPushButton>(*f.tab, "hatchPatternDuplicate")->isEnabled());
}

TEST(HatchPatternsTab, SelectUsersSelectsWhatIsHatchedWithThePattern)
{
    Fixture f;
    f.tab->selectPattern("brick");
    click(*f.tab, "hatchPatternSelectUsers");
    ASSERT_EQ(f.document.selection().size(), 1u);
    EXPECT_EQ(f.document.model().entities.find(f.document.selection().ids().front())->layer,
              "site");
    EXPECT_EQ(f.status().toStdString(), "1 entity selected.");
}

TEST(HatchPatternsTab, NewStyleUsingThisMakesAStyleThatHatchesWithItInOneStep)
{
    Fixture f;
    f.tab->selectPattern("brick");
    child<QLineEdit>(*f.tab, "hatchPatternName")->setText("brick paving");
    const std::size_t before = f.document.history().undoCount();
    click(*f.tab, "hatchPatternNewStyle");
    EXPECT_EQ(f.last().toStdString(), "STYLE NEW \"brick paving\" HATCH brick");
    const katana::entity::Style* made = f.document.model().styles.find("brick paving");
    ASSERT_NE(made, nullptr);
    EXPECT_EQ(made->hatchPattern, "brick");
    EXPECT_EQ(f.document.history().undoCount(), before + 1);
}

TEST(HatchPatternsTab, WithoutAnExecutorItSaysSoAndChangesNothing)
{
    Document document;
    CustomisationContext context;
    context.document = &document;
    context.log = [](const QString&, bool) {};
    HatchPatternsTab tab(context);
    tab.findChild<QLineEdit*>("hatchPatternName")->setText("fill");
    click(tab, "hatchPatternNewSolid");
    EXPECT_TRUE(tab.findChild<QLabel*>("hatchPatternStatus")->text().startsWith("Nothing here"));
    EXPECT_EQ(document.model().hatchPatterns.find("fill"), nullptr);
}

TEST(HatchPatternsTab, TheStylesAndLinetypesManagerCarriesTheTab)
{
    Fixture f;
    katana::qt::StyleManagerDialog manager(f.context);
    auto* tabs = child<QTabWidget>(manager, "managerTabs");
    ASSERT_NE(tabs, nullptr);
    QStringList titles;
    for (int i = 0; i < tabs->count(); ++i) {
        titles << tabs->tabText(i);
    }
    EXPECT_EQ(titles, (QStringList{"Styles", "Linetypes", "Hatch Patterns", "Diagnostics"}));
    // No Q_OBJECT, so found as the QWidget it is.
    EXPECT_NE(manager.findChild<QWidget*>("hatchPatternsPage"), nullptr);
}
