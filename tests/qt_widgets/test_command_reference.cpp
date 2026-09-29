// Help > Command Reference and Help > Keyboard Shortcuts
// (src/katana_qt/command_reference_dialog.hpp, keyboard_shortcuts_dialog.hpp):
// how a help text is read into entries, that the reference holds the
// interpreter's, the sheets', the utilities', the online and the window's
// verbs and the tools, and the two dialogs driven by their object names.
//
// The reference is built from the verbs' own help texts, so what is asserted
// is that a command a person looks for is found, under the section it
// belongs to, with the verb a double-click puts on the command line.

#include <gtest/gtest.h>

#include <QApplication>
#include <QClipboard>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QTreeWidget>

#include <algorithm>
#include <vector>

#include "command_reference_dialog.hpp"
#include "keyboard_shortcuts_dialog.hpp"

namespace {

using katana::qt::CommandReferenceDialog;
using katana::qt::KeyboardShortcutsDialog;
using katana::qt::ReferenceEntry;
using katana::qt::ReferenceSection;
using katana::qt::ShortcutRow;

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

const ReferenceSection* sectionNamed(const std::vector<ReferenceSection>& sections,
                                     const QString& name)
{
    const auto found = std::ranges::find(sections, name, &ReferenceSection::name);
    return found != sections.end() ? &*found : nullptr;
}

const ReferenceEntry* entryTitled(const ReferenceSection& section, const QString& title)
{
    const auto found = std::ranges::find(section.entries, title, &ReferenceEntry::title);
    return found != section.entries.end() ? &*found : nullptr;
}

// The titles of the entries the tree shows, as "Section/Title".
QStringList shownTitles(CommandReferenceDialog& dialog)
{
    QStringList titles;
    auto* tree = child<QTreeWidget>(dialog, "commandReferenceTree");
    for (int s = 0; s < tree->topLevelItemCount(); ++s) {
        const QTreeWidgetItem* section = tree->topLevelItem(s);
        for (int e = 0; e < section->childCount(); ++e) {
            if (!section->child(e)->isHidden()) {
                titles << section->text(0) + "/" + section->child(e)->text(0);
            }
        }
    }
    return titles;
}

TEST(CommandReference, AHelpTextIsReadIntoLabelledGroupsWithTheirContinuations)
{
    const auto entries = katana::qt::referenceEntries(
        "Points: x,y | @dx,dy\n"
        "\n"
        "Draw      POINT p | LINE p p\n"
        "          CIRCLE centre radius\n"
        "Modify    (act on the selection)\n"
        "AnnoScale ANNOSCALE [N | 1:N]\n");
    ASSERT_EQ(entries.size(), 4U);
    EXPECT_EQ(entries[0].title, "About");
    EXPECT_EQ(entries[0].text, "Points: x,y | @dx,dy");
    EXPECT_EQ(entries[1].title, "Draw");
    EXPECT_EQ(entries[1].verb, "POINT");
    EXPECT_EQ(entries[1].text, "Draw      POINT p | LINE p p\n          CIRCLE centre radius");
    EXPECT_EQ(entries[2].title, "Modify");
    EXPECT_TRUE(entries[2].verb.isEmpty()) << "prose puts nothing on the command line";
    EXPECT_EQ(entries[3].title, "AnnoScale");
    EXPECT_EQ(entries[3].verb, "ANNOSCALE");
}

TEST(CommandReference, CommandLinesAreTitledByTheirLeadingCommandWords)
{
    const auto entries = katana::qt::referenceEntries(
        "usage: ONLINE IMPORT <provider> <layer> area=view\n"
        "SHEETS [LIST]                   every sheet and its views\n"
        "SHEET NEW [name] [paper=A3]\n"
        "  options  rect=x0,y0,x1,y1\n");
    ASSERT_EQ(entries.size(), 3U);
    EXPECT_EQ(entries[0].title, "ONLINE IMPORT");
    EXPECT_EQ(entries[0].verb, "ONLINE IMPORT");
    EXPECT_EQ(entries[0].text, "usage: ONLINE IMPORT <provider> <layer> area=view");
    EXPECT_EQ(entries[1].title, "SHEETS");
    EXPECT_EQ(entries[2].title, "SHEET NEW");
    EXPECT_TRUE(entries[2].text.endsWith("  options  rect=x0,y0,x1,y1"));
}

TEST(CommandReference, AnEmptyHelpHasNoEntries)
{
    EXPECT_TRUE(katana::qt::referenceEntries("").empty());
    EXPECT_TRUE(katana::qt::referenceEntries("\n\n").empty());
}

TEST(CommandReference, TheReferenceHoldsEveryFamilyOfVerbsAndTheTools)
{
    const auto sections = katana::qt::commandReferenceSections();
    for (const char* name : {"Commands", "Sheets", "Subsurface utilities", "Online data", "Window",
                             "Survey field files", "Draw tools", "Modify tools"}) {
        EXPECT_NE(sectionNamed(sections, name), nullptr) << name;
    }
    // SURVEY READ and IMPORT are the session's, and the window runs them too.
    const ReferenceSection* surveyFiles = sectionNamed(sections, "Survey field files");
    ASSERT_NE(surveyFiles, nullptr);
    const ReferenceEntry* surveyVerb = entryTitled(*surveyFiles, "Survey");
    ASSERT_NE(surveyVerb, nullptr);
    EXPECT_EQ(surveyVerb->verb, "SURVEY");
    const ReferenceSection* commands = sectionNamed(sections, "Commands");
    ASSERT_NE(commands, nullptr);
    const ReferenceEntry* dimStyle = entryTitled(*commands, "DimStyle");
    ASSERT_NE(dimStyle, nullptr);
    EXPECT_EQ(dimStyle->verb, "DIMSTYLE");
    // The annotation verbs come with the interpreter's help.
    EXPECT_NE(entryTitled(*commands, "LabelStyle"), nullptr);

    const ReferenceSection* window = sectionNamed(sections, "Window");
    ASSERT_NE(window, nullptr);
    const ReferenceEntry* copc = entryTitled(*window, "COPC");
    ASSERT_NE(copc, nullptr);
    EXPECT_EQ(copc->verb, "COPC");

    const ReferenceSection* online = sectionNamed(sections, "Online data");
    ASSERT_NE(online, nullptr);
    EXPECT_NE(entryTitled(*online, "ONLINE PROVIDERS"), nullptr);
    EXPECT_NE(entryTitled(*online, "ONLINE IMPORT"), nullptr);

    const ReferenceSection* sheets = sectionNamed(sections, "Sheets");
    ASSERT_NE(sheets, nullptr);
    EXPECT_EQ(sheets->entries.front().title, "About");
    EXPECT_NE(entryTitled(*sheets, "GENERATE"), nullptr);

    // A tool is found by its name, and a double-click starts it by its first
    // alias.
    const ReferenceSection* draw = sectionNamed(sections, "Draw tools");
    ASSERT_NE(draw, nullptr);
    const ReferenceEntry* line = entryTitled(*draw, "Line");
    ASSERT_NE(line, nullptr);
    EXPECT_EQ(line->verb, "LINE");
    EXPECT_TRUE(line->text.contains("id draw.line"));
}

TEST(CommandReference, TheWindowSectionNamesEveryVerbTheWindowRunsItself)
{
    // What MainWindow::dispatchLine and runWorkbenchLine take before the
    // interpreter: the list the typed HELP adds to the interpreter's.
    const QString help = katana::qt::windowHelpText();
    for (const char* verb : {"SCRIPT", "IMPORT", "EXPORT", "INFO <file>", "REFS", "COPC",
                             "CUSTOMISE", "PLOTSHEETS", "PLOT <file.pdf>", "SNAPSHOT", "ZOOM", "GRID", "SNAP", "ONLINE", "UTILITY",
                             "QUIT", "LABELSTYLE"}) {
        EXPECT_TRUE(help.contains(verb)) << verb;
    }
}

TEST(CommandReferenceDialog, SearchingFindsDimStyleCopcAndOnline)
{
    CommandReferenceDialog dialog(katana::qt::commandReferenceSections(), {});
    EXPECT_EQ(dialog.objectName(), "commandReferenceDialog");
    const int all = dialog.shownEntries();
    ASSERT_GT(all, 50);

    child<QLineEdit>(dialog, "commandReferenceSearch")->setText("dimstyle");
    EXPECT_LT(dialog.shownEntries(), all);
    EXPECT_TRUE(shownTitles(dialog).contains("Commands/DimStyle")) << shownTitles(dialog).join(", ").toStdString();

    dialog.setFilter("COPC");
    EXPECT_TRUE(shownTitles(dialog).contains("Window/COPC"));

    dialog.setFilter("ONLINE");
    const QStringList online = shownTitles(dialog);
    EXPECT_TRUE(online.contains("Online data/ONLINE IMPORT"));
    EXPECT_TRUE(online.contains("Window/Online"));
    EXPECT_EQ(child<QLabel>(dialog, "commandReferenceCount")->text(),
              QString("%1 of %2 entries").arg(online.size()).arg(all));

    // Every word must occur: two words narrow what one finds.
    dialog.setFilter("layer colour");
    const int both = dialog.shownEntries();
    dialog.setFilter("layer");
    EXPECT_LT(both, dialog.shownEntries());

    dialog.setFilter("no command has this word xyzzy");
    EXPECT_EQ(dialog.shownEntries(), 0);
    dialog.setFilter({});
    EXPECT_EQ(dialog.shownEntries(), all);
}

TEST(CommandReferenceDialog, ADoubleClickPutsTheVerbOnTheCommandLine)
{
    QString inserted;
    CommandReferenceDialog dialog(katana::qt::commandReferenceSections(),
                                  [&inserted](const QString& verb) { inserted = verb; });
    dialog.setFilter("DIMSTYLE LIST");
    auto* tree = child<QTreeWidget>(dialog, "commandReferenceTree");
    const QList<QTreeWidgetItem*> found = tree->findItems("DimStyle", Qt::MatchExactly | Qt::MatchRecursive);
    ASSERT_FALSE(found.isEmpty());
    // Beside the title, the group's first line without its label.
    EXPECT_TRUE(found.front()->text(1).startsWith("DIMSTYLE LIST")) << found.front()->text(1).toStdString();
    emit tree->itemDoubleClicked(found.front(), 0);
    EXPECT_EQ(inserted, "DIMSTYLE ");
    tree->setCurrentItem(found.front());
    EXPECT_TRUE(child<QPlainTextEdit>(dialog, "commandReferenceDetail")
                    ->toPlainText()
                    .startsWith("DimStyle  DIMSTYLE LIST"));
    EXPECT_EQ(dialog.copyText(), child<QPlainTextEdit>(dialog, "commandReferenceDetail")->toPlainText());
    child<QPushButton>(dialog, "commandReferenceCopy")->click();
    EXPECT_EQ(QApplication::clipboard()->text(), dialog.copyText());
}

TEST(CommandReferenceDialog, TheSheetsItemOpensTheReferenceAtTheSheetVerbs)
{
    CommandReferenceDialog dialog(katana::qt::commandReferenceSections(), {});
    dialog.setFilter("COPC");
    ASSERT_TRUE(dialog.showSection("Sheets"));
    EXPECT_TRUE(child<QLineEdit>(dialog, "commandReferenceSearch")->text().isEmpty());
    auto* tree = child<QTreeWidget>(dialog, "commandReferenceTree");
    ASSERT_NE(tree->currentItem(), nullptr);
    ASSERT_NE(tree->currentItem()->parent(), nullptr);
    EXPECT_EQ(tree->currentItem()->parent()->text(0), "Sheets");
    EXPECT_TRUE(child<QPlainTextEdit>(dialog, "commandReferenceDetail")
                    ->toPlainText()
                    .startsWith("Sheets on the command line"));
    EXPECT_FALSE(dialog.showSection("No such section"));
}

TEST(KeyboardShortcutsDialog, EveryKeyIsListedAndAClashIsMarked)
{
    const std::vector<ShortcutRow> rows = {
        {"Ctrl+L", "Layers", "Format", "The drawing's layers"},
        {"Ctrl+L", "Line", "Draw", "Draw a line"},
        {"Ctrl+S", "Save", "File", "Save the project"},
    };
    KeyboardShortcutsDialog dialog(rows, {"Ctrl+L: formatLayers, Line"});
    EXPECT_EQ(dialog.objectName(), "keyboardShortcutsDialog");
    auto* table = child<QTableWidget>(dialog, "keyboardShortcutsTable");
    ASSERT_EQ(table->rowCount(), 3);
    QStringList keys;
    for (int r = 0; r < table->rowCount(); ++r) {
        keys << table->item(r, 0)->text();
    }
    EXPECT_EQ(keys.count("Ctrl+L (clash)"), 2);
    EXPECT_EQ(keys.count("Ctrl+S"), 1);
    EXPECT_TRUE(child<QLabel>(dialog, "keyboardShortcutsClashes")
                    ->text()
                    .startsWith("1 key reaches more than one command"));

    child<QLineEdit>(dialog, "keyboardShortcutsSearch")->setText("file save");
    EXPECT_EQ(dialog.shownRows(), 1);
    dialog.setFilter("ctrl+l");
    EXPECT_EQ(dialog.shownRows(), 2);
    EXPECT_EQ(child<QLabel>(dialog, "keyboardShortcutsCount")->text(), "2 of 3 keys");
}

TEST(KeyboardShortcutsDialog, NoClashIsSaidAsSuch)
{
    KeyboardShortcutsDialog dialog({{"F7", "Grid", "View", "Show or hide the grid"}}, {});
    EXPECT_EQ(child<QLabel>(dialog, "keyboardShortcutsClashes")->text(),
              "Every key reaches one command.");
    EXPECT_EQ(child<QTableWidget>(dialog, "keyboardShortcutsTable")->item(0, 0)->text(), "F7");
}

} // namespace
