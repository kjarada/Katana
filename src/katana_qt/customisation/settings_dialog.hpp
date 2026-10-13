#pragma once

// Settings: what the session's customisation is, and the few things that are
// done to it WHOLE - a file loaded into it, the session written to a file, the
// built-in put back, the session kept for the next start (docs/desktop.md,
// "Settings"; docs/customisation.md, "Settings and the kept customisation").
//
// A page list beside a stack: Customisation, and Updates (below); another is
// one more addPage.
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
// THE BUILT-IN LIBRARY. A program may carry several built-in customisations
// (cad::builtIns(): Katana Standard, the default, and the alternates a build
// compiled in). "Built-in library" is a chooser of them and Use puts the chosen
// one in the place of the session's: the line CUSTOMISE BUILTIN "<name>",
// followed by KEEP like Reset (a kept session stays kept, so choosing one in
// an interactive window is what the next start gives). The row is hidden when
// the program has fewer than two to choose between, and Use is disabled while
// the chosen one is the one the session already is. "Reset to Built-in" is
// the default's, as ever (CUSTOMISE RESET).
//
// REFUSED HERE, with the reason in the status box and the log, and nothing
// run:
//   * Import, Reset to Built-in, Use and Revert to Kept while the Survey Code
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
// settingsCustomisation and settingsUpdates (the pages); of the first page:
//   settingsActiveName, settingsActiveOrigin, settingsActiveBuiltIn (the
//   built-in the session is, or the one it is based on), settingsActiveDefinitions,
//   settingsActiveSymbols, settingsActiveRules, settingsActiveKept,
//   settingsActiveProblems (what went wrong at start-up; hidden when nothing
//   did), settingsShowNotice (hidden when there is no notice), settingsNotice
//   (read-only, hidden until asked);
//   settingsImportPath, settingsImportBrowse, settingsImportReplace,
//   settingsImport;  settingsExportPath, settingsExportBrowse, settingsExport;
//   settingsBuiltInRow (hidden with fewer than two built-ins), settingsBuiltIn
//   (the chooser: each item the built-in's name, the default marked), and
//   settingsBuiltInUse;
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
//
// THE UPDATES PAGE (settingsUpdates; docs/desktop.md, "Settings", and the
// UPDATE verbs, src/katana_app/update_verbs.hpp) is of the program and not of
// the drawing, so it works with no Document and after one has gone. Like the
// first page it runs lines and changes nothing itself - with ONE exception,
// which is the reason it exists:
//   settingsUpdateNote         why this session checks nothing by itself or
//                              keeps no update settings (hidden when it does)
//   settingsUpdateAuto         "Check for updates automatically"; toggled, it
//                              runs UPDATE SET AUTO ON or OFF, and then shows
//                              what the state has, as the two automation boxes
//                              show the Document
//   settingsUpdateSends        what a check sends: only the user agent
//                              "Katana/<version> (<target>)", and that the
//                              server sees the computer's network address
//   settingsUpdateToken        an access token for private releases, typed or
//                              pasted (shown as dots)
//   settingsUpdateTokenSave    writes the token to the credential store
//                              DIRECTLY (update::CredentialStore::write). A
//                              token is never a word of a line: lines are
//                              echoed in the command log. Nothing of it is
//                              logged or shown; the field is emptied
//   settingsUpdateTokenClear   runs UPDATE TOKEN CLEAR
//   settingsUpdateTokenState   whether a token is stored, and where
//   settingsUpdateSkipped      the version UPDATE SKIP set aside, or that
//                              none is; nothing when nobody can say (the
//                              state file does not read)
//   settingsUpdateUnskip       "Offer the skipped version again": runs UPDATE
//                              SKIP NONE
//   settingsUpdateBlocked      whether a version is set aside because it was
//                              installed here, did not start and was put
//                              back, or that none is; nothing when nobody
//                              can say
//   settingsUpdateUnblock      "Offer the version that was put back again":
//                              runs UPDATE BLOCKED CLEAR, as Unskip runs its
//                              line. One start too slow for the update's
//                              health check sets a sound release aside, and
//                              this is the way back
// Each of the two buttons is off when its list is known to be empty, and ON
// when nobody can say what the list holds: the line is then run, and the verbs
// answer - with why they refuse it, for a state file that does not read.
// The page is there whatever the dialog was given; with no SettingsUpdates its
// controls are disabled, as a button with no callback is on the first page.

#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <vector>

#include <QDialog>
#include <QString>

#include "command_runner.hpp"
#include "katana/cad/customisation_host.hpp"

class QCheckBox;
class QComboBox;
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

namespace katana::update {
class CredentialStore;
} // namespace katana::update

namespace katana::qt {

class DocumentWatcher;

// What the Updates page shows, and the one thing it does without a line. Every
// member has a default a test can leave alone; the window fills it from its
// update workbench (UpdateWorkbench::settingsUpdates).
struct SettingsUpdates {
    // "Check for updates automatically" as the per-user state has it. Asked
    // whenever the page is filled, and after each of its lines. Empty: the box
    // is disabled.
    std::function<bool()> automatic{};
    // The version UPDATE SKIP set aside, empty when none is. Asked likewise.
    // nullopt when nobody can say - the state file does not read - and with
    // an empty function: the page says nothing of it and the button stays on,
    // so that the verb answers.
    std::function<std::optional<QString>()> skippedVersion{};
    // Whether a version is set aside because it was installed here, did not
    // start and was put back (the state's list of blocked releases holds one;
    // which version is not kept, only which build). Asked likewise, and
    // nullopt and an empty function mean the same: the page says nothing of
    // it and the button stays on.
    std::function<std::optional<bool>()> putBack{};
    // Where the access token is kept: Save writes it here and the page says
    // whether one is stored - never the token itself. Null: Save is disabled.
    katana::update::CredentialStore* credentials = nullptr;
    // Told once Save has changed the store, so the next check reads it again.
    std::function<void()> tokenChanged{};
    // What a check sends as its user agent, "Katana/0.2.0 (win64)". Empty: the
    // sentence gives the shape, "Katana/<version> (<target>)".
    QString userAgent{};
    // Who the check asks ("GitHub"). Empty: "the release server".
    QString server{};
    // One sentence, shown first: why this session checks nothing by itself or
    // keeps no update settings (a headless run, a development build,
    // KATANA_UPDATE_DISABLE). Empty: nothing is shown.
    QString note{};
};

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
    // The built-ins the chooser offers (cad::builtInsOf of the session's host).
    // Empty: those compiled into the program, cad::builtIns() - what the
    // window wants. A test gives it to stand in for the build, so that it is
    // the same test on a machine whose build compiled in other files.
    std::function<std::vector<katana::cad::BuiltInEntry>()> builtIns{};
    // The Updates page's (above). The Format workbench, which makes the dialog,
    // knows nothing of updates: the window hands these over with setUpdates
    // each time the dialog is opened.
    SettingsUpdates updates{};
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

    // What the Updates page shows and writes to, in the place of what the
    // context gave; the page is filled again from it.
    void setUpdates(SettingsUpdates updates);

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
    QWidget* buildUpdatesPage();

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
    // The chooser as the Document and the built-ins say, telling no one.
    void showBuiltIns();
    // Use as the chooser's choice leaves it: enabled, and what its tip says.
    void showBuiltInUse();
    void useBuiltIn();
    void browseImport();
    void browseExport();
    void automationToggled(const char* key, bool on);
    // The two boxes as the Document has them, telling no one.
    void showAutomation();

    // ---- the Updates page, which needs no Document.
    // The box, the two labels and which buttons can be pressed, as
    // context_.updates says now, telling no one.
    void showUpdates();
    // Runs an UPDATE line through the executor, shows what it answered and
    // fills the page again.
    void runUpdatesLine(const QString& line);
    // Save: the typed token into the credential store, with no line.
    void saveUpdateToken();

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
    QLabel* activeBuiltIn_ = nullptr;
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
    QWidget* builtInRow_ = nullptr;
    QComboBox* builtInChoice_ = nullptr;
    QPushButton* builtInUse_ = nullptr;
    // The built-ins the chooser lists, as it last listed them, in its order.
    std::vector<katana::cad::BuiltInChoice> builtInChoices_;
    QPushButton* reset_ = nullptr;
    QPushButton* keep_ = nullptr;
    QPushButton* revert_ = nullptr;
    QCheckBox* autoCodes_ = nullptr;
    QCheckBox* autoLinework_ = nullptr;
    QLabel* linework_ = nullptr;
    QPushButton* editLinework_ = nullptr;
    QPushButton* openCodes_ = nullptr;
    QPushButton* openSymbols_ = nullptr;
    // The Updates page.
    QLabel* updateNote_ = nullptr;
    QCheckBox* updateAuto_ = nullptr;
    QLabel* updateSends_ = nullptr;
    QLineEdit* updateToken_ = nullptr;
    QPushButton* updateTokenSave_ = nullptr;
    QPushButton* updateTokenClear_ = nullptr;
    QLabel* updateTokenState_ = nullptr;
    QLabel* updateSkipped_ = nullptr;
    QPushButton* updateUnskip_ = nullptr;
    QLabel* updateBlocked_ = nullptr;
    QPushButton* updateUnblock_ = nullptr;
    QPlainTextEdit* status_ = nullptr;

    // Last, so it is destroyed first: no delivery reaches a half-destroyed
    // dialog, and it is how everything here knows the Document is still there.
    std::unique_ptr<DocumentWatcher> watcher_;
};

} // namespace katana::qt
