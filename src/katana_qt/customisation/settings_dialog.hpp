#pragma once

// Settings: what the session's customisation is, and the few things that are
// done to it WHOLE - a file loaded into it, the session written to a file, the
// built-in put back, the session kept for the next start (docs/desktop.md,
// "Settings"; docs/customisation.md, "Settings and the kept customisation").
//
// A page list beside a stack. One page today, Customisation; another is one
// more addPage.
//
// THE DIALOG DOES NONE OF THE WORK. Every button that changes anything builds
// the CUSTOMISE line a person would type (cad/customisation_verbs.hpp) and
// hands it to the window's one executor (command_runner.hpp), so it is echoed
// in the command log and a refusal is the verb's own, shown as it came. There
// is no Apply: each control runs its line when it is pressed, and the two
// automation boxes when they are toggled.
//
// IT SHOWS WHAT THE DOCUMENT SAYS, never what a button hoped for. The labels,
// the boxes and which buttons can be pressed are read from the Document when
// it says it changed (a DocumentWatcher, once a turn of the event loop), by
// its three generations: the customisation's own, the library's and the survey
// map's (cad/document.hpp - an edit of a session already edited moves only the
// last two). A box whose line was refused goes back to what the Document has.
//
// WHAT IS EDITED HERE IS KEPT. A session that was kept before Import, Reset or
// a toggled box, and is not after it, is kept again: the dialog runs CUSTOMISE
// KEEP behind its own line - when the host has a kept file, and only when the
// first line was carried out. A load typed on the command line or run by a
// script is not followed so; Settings then says "Not kept" beside Keep.
//
// A RESET SO KEPT SETS THE KEPT FILE ASIDE. CUSTOMISE KEEP writes no copy of
// the built-in: it renames the kept file to <kept file>.bak, and Revert to
// Kept then has none to read. Nothing is asked first - a question is a modal
// box - so Reset to Built-in's tip says it before the press, on a kept
// session, with the file's name and the way back (Import that file, with
// Replace); and Keep's tip says the same of a session that is the built-in
// and not kept. Only ONE such file is kept: the next KEEP that finds a kept
// file puts that one in its place (docs/desktop.md, "Settings").
//
// REFUSED HERE, with the reason in the status box and the log, and nothing
// run:
//   * Import, Reset to Built-in and Revert to Kept while the Survey Code
//     Manager's buffer holds unapplied edits: its later Apply would put the
//     rules it holds back over what was loaded, without a word;
//   * a file whose path no command line can carry (a double quote, a line
//     break - command_word.hpp's rule), and no file at all. One pair of
//     double quotes round the WHOLE path is no part of it and is taken off
//     first: a path copied from a file manager comes in them;
//   * Browse in a headless session, which opens no file dialog and names the
//     line to type instead. Browse is the only thing here that opens one.
// No button is a default, Enter in a field presses nothing, and no path
// through the dialog opens a modal box.
//
// Object names: settingsDialog; settingsPages (the list), settingsStack,
// settingsCustomisation (the page);
//   settingsActiveName, settingsActiveOrigin, settingsActiveDefinitions,
//   settingsActiveSymbols, settingsActiveRules, settingsActiveKept,
//   settingsActiveProblems (what went wrong at start-up; hidden when nothing
//   did), settingsShowNotice (hidden when there is no notice), settingsNotice
//   (read-only, hidden until asked);
//   settingsImportPath, settingsImportBrowse, settingsImportReplace,
//   settingsImport;  settingsExportPath, settingsExportBrowse, settingsExport;
//   settingsReset, settingsKeep, settingsRevert;
//   settingsAutoCodes, settingsAutoLinework;
//   settingsLinework (the seven spellings, read-only, each under the word
//   its one editor labels it with), settingsEditLinework;
//   settingsOpenCodes, settingsOpenSymbols;
//   settingsStatus (the line last run and what it answered; a refusal in the
//   error colour), settingsClose.
//
// LIFETIME. As the managers: the watcher is the last member, and nothing
// touches the Document once it has gone. Nothing tells the dialog that it
// has: the first press finds out, and every control that would act is then
// disabled and the status says why. What the page last showed stays to be
// read.

#include <cstdint>
#include <functional>
#include <memory>

#include <QDialog>
#include <QString>

#include "command_runner.hpp"

class QCheckBox;
class QLabel;
class QLineEdit;
class QListWidget;
class QPlainTextEdit;
class QPushButton;
class QStackedWidget;
class QWidget;

namespace katana::cad {
class Document;
} // namespace katana::cad

namespace katana::qt {

class DocumentWatcher;

// What the dialog is given by whoever opens it. Every member has a default a
// test can leave alone: with no Document nothing is shown and nothing runs,
// and a button whose callback is empty is disabled.
struct SettingsContext {
    // The drawing whose customisation is shown. It may die before the dialog.
    katana::cad::Document* document = nullptr;
    // The window's one executor. Empty: a press says nothing can run its line.
    CommandRunner run{};
    // Where the dialog says what it refused before any line was built - the
    // window's log. The lines it runs are logged by the executor. May be
    // empty.
    std::function<void(const QString& message, bool isError)> log{};
    // True in a session nobody watches: Browse then opens no file dialog.
    // Asked at each press, since a window learns it after it is built. Empty:
    // interactive.
    std::function<bool()> headless{};
    // True while the Survey Code Manager's buffer holds edits that are not on
    // the drawing. Asked at each press. Empty: it holds none.
    std::function<bool()> codeManagerDirty{};
    // Open the Survey Code Manager, the same at its Linework tab - the ONE
    // editor of the control codes - and the Symbol Library.
    std::function<void()> openCodeManager{};
    std::function<void()> openLinework{};
    std::function<void()> openSymbolLibrary{};
    // What the session's host offers (cad::CustomisationHost): whether there
    // is a built-in customisation to reset to, and the kept file as text, for
    // showing - empty when this session keeps none. The file need not exist.
    bool hasBuiltIn = false;
    QString keptFile{};
};

class SettingsDialog final : public QDialog {
  public:
    explicit SettingsDialog(SettingsContext context, QWidget* parent = nullptr);
    ~SettingsDialog() override;

    SettingsDialog(const SettingsDialog&) = delete;
    SettingsDialog& operator=(const SettingsDialog&) = delete;

    // Everything the page shows, read from the Document again: what the
    // watcher does when the Document says it changed.
    void refresh();

  private:
    // What a line that changed the customisation is followed by.
    enum class Then {
        Nothing,
        // CUSTOMISE KEEP, when the session was kept before the line and is
        // not after it.
        KeepWhatWasKept,
    };

    // One more page: an entry of the list and the widget it shows.
    void addPage(const QString& title, QWidget* page);
    QWidget* buildCustomisationPage();

    [[nodiscard]] bool alive() const;
    // The status box, and for a refusal the log too.
    void say(const QString& message, bool isError);
    void refuse(const QString& message);
    // Every control that acts on the Document off, and the reason in the
    // status.
    void showClosed();

    // Runs `line` through the executor and shows what it answered, and after
    // it what `then` says.
    void run(const QString& line, Then then);
    // Whether a line that replaces the session's rules may run; says why not.
    [[nodiscard]] bool mayReplaceRules(const QString& what);

    void importFile();
    void exportFile();
    void browseImport();
    void browseExport();
    void automationToggled(const char* key, bool on);
    // The two boxes as the Document has them, telling no one.
    void showAutomation();

    SettingsContext context_;
    // The Document's generations the page last showed.
    std::uint64_t shownCustomisation_ = 0;
    std::uint64_t shownLibrary_ = 0;
    std::uint64_t shownSurveyMap_ = 0;
    // The boxes are being set from the Document, not by a person.
    bool loading_ = false;

    QListWidget* pages_ = nullptr;
    QStackedWidget* stack_ = nullptr;
    QLabel* name_ = nullptr;
    QLabel* origin_ = nullptr;
    QLabel* definitions_ = nullptr;
    QLabel* symbols_ = nullptr;
    QLabel* rules_ = nullptr;
    QLabel* kept_ = nullptr;
    QLabel* problems_ = nullptr;
    QCheckBox* showNotice_ = nullptr;
    QPlainTextEdit* notice_ = nullptr;
    QLineEdit* importPath_ = nullptr;
    QCheckBox* importReplace_ = nullptr;
    QPushButton* importBrowse_ = nullptr;
    QPushButton* import_ = nullptr;
    QLineEdit* exportPath_ = nullptr;
    QPushButton* exportBrowse_ = nullptr;
    QPushButton* export_ = nullptr;
    QPushButton* reset_ = nullptr;
    QPushButton* keep_ = nullptr;
    QPushButton* revert_ = nullptr;
    QCheckBox* autoCodes_ = nullptr;
    QCheckBox* autoLinework_ = nullptr;
    QLabel* linework_ = nullptr;
    QPushButton* editLinework_ = nullptr;
    QPushButton* openCodes_ = nullptr;
    QPushButton* openSymbols_ = nullptr;
    QPlainTextEdit* status_ = nullptr;

    // Last, so it is destroyed first: no delivery reaches a half-destroyed
    // dialog, and it is how everything here knows the Document is still there.
    std::unique_ptr<DocumentWatcher> watcher_;
};

} // namespace katana::qt
