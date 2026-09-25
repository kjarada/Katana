#pragma once

// Help > Command Reference: every command the window's command line takes, in
// one searchable place (docs/desktop.md, "The Command Reference and the
// keyboard shortcuts").
//
// The reference is built from the texts the verbs' own code keeps - the
// interpreter's HELP (with the annotation verbs), HELP SHEETS, HELP UTILITY,
// the ONLINE usage - plus windowHelpText, the verbs only the window's front
// end runs, and the tool catalogue's names, aliases and tips. Nothing is
// written twice: a verb whose help changes changes here, and katana_cli -h and
// katana_mcp's katana_help print the same interpreter text.
//
// The dialog is non-modal and changes nothing; a double-click puts an entry's
// verb on the command line, for the person to finish and run. Every control
// has an object name, which is how the tests and the headless driver reach it:
//   commandReferenceDialog   the dialog (Help > Command Reference,
//                            helpCommandReference; Help > Sheets and Plotting
//                            Commands, helpSheetCommands, opens it at Sheets)
//   commandReferenceSearch   the filter: every word must occur in an entry
//                            (its section, title or text), in any case
//   commandReferenceTree     the sections and their entries; a double-click
//                            on an entry puts its verb on the command line
//   commandReferenceDetail   the chosen entry's whole text (read-only)
//   commandReferenceCount    how many entries the filter shows
//   commandReferenceCopy     copy the chosen entry, or every entry shown
//   commandReferenceClose    close

#include <QDialog>
#include <QString>

#include <functional>
#include <vector>

class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QTreeWidget;
class QTreeWidgetItem;

namespace katana::qt {

// One entry of the reference: a command, or a group of commands as the help
// text groups them ("Layers  LAYER LIST | NEW name ...").
struct ReferenceEntry {
    // What the tree shows: the help's label for a group ("Layers"), else the
    // command's leading words ("SHEET NEW", "ONLINE IMPORT").
    QString title;
    // What a double-click puts on the command line: the first command word
    // ("LAYER", "SHEET NEW"); empty for prose.
    QString verb;
    // The entry's lines as the help gives them.
    QString text;
};

struct ReferenceSection {
    QString name;
    std::vector<ReferenceEntry> entries;
};

// A help text read into entries, in the layout every help here uses: an entry
// starts at a line that does not start with a blank and runs on over the lines
// that do; the first paragraph, when a blank line ends it and more follows, is
// the section's introduction ("About"); "usage: " before a command is dropped.
[[nodiscard]] std::vector<ReferenceEntry> referenceEntries(const QString& help);

// The verbs the window's front end runs itself, beside the interpreter's, in
// the interpreter's help layout: what MainWindow::dispatchLine and
// runWorkbenchLine take before a line reaches the interpreter. A verb added
// there is added here - the typed HELP prints this after the interpreter's
// help, and the reference shows it as its Window section.
[[nodiscard]] QString windowHelpText();

// Every section of the reference: the interpreter's commands (the annotation
// verbs among them), sheets, subsurface utilities, online data, the window's
// own verbs, and the tools by menu.
[[nodiscard]] std::vector<ReferenceSection> commandReferenceSections();

class CommandReferenceDialog final : public QDialog {
  public:
    // `insertCommand` puts a verb on the command line; unset, a double-click
    // does nothing.
    CommandReferenceDialog(std::vector<ReferenceSection> sections,
                           std::function<void(const QString&)> insertCommand,
                           QWidget* parent = nullptr);

    // Shows the entries matching `text` (what typing in the search does).
    void setFilter(const QString& text);
    // Clears the filter and brings `section` forward, expanded, its first
    // entry chosen. False for a section the reference does not have.
    bool showSection(const QString& section);
    // How many entries the filter shows.
    [[nodiscard]] int shownEntries() const;
    // What Copy copies: the chosen entry's text, or every entry shown.
    [[nodiscard]] QString copyText() const;

  private:
    void showDetail();

    std::vector<ReferenceSection> sections_;
    std::function<void(const QString&)> insertCommand_;
    QLineEdit* search_ = nullptr;
    QTreeWidget* tree_ = nullptr;
    QPlainTextEdit* detail_ = nullptr;
    QLabel* count_ = nullptr;
};

} // namespace katana::qt
