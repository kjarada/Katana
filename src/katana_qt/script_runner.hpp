#pragma once

// File > Run Script and the SCRIPT verb: a file of command lines, run one after
// another through the window's one executor (command_runner.hpp), as katana_cli
// runs a .kcs file (docs/desktop.md, "Run Script").
//
// A script is what katana_cli reads: a command a line, UTF-8, a Windows line
// end taken off, and a blank line or one whose first non-blank is '#'
// skipped - the session's rule too (katana_app/session.cpp, runLine), which
// once refused an indented note. So a file written for katana_cli or
// katana_mcp's katana_run_script runs in the window, and one written in the
// window runs in both. Each line is echoed and run as a
// dialog's line is - never a running tool's answer, since a script names its
// points (LINE 0,0 10,0, not LINE and then its points) - and so each is its own
// undo step, as in katana_cli. The run stops at the first line refused unless
// it was asked to continue, and QUIT or EXIT ends it, as it ends katana_cli's,
// without closing the window.
//
// The reading and the running are functions of their inputs, tested without a
// window (tests/qt_widgets/test_script_runner.cpp); the window adds the
// progress dialog, the log and the Recent Scripts list.
//
// The dialog is non-modal and runs nothing itself: it writes the SCRIPT line
// its fields describe, shown in scriptCommand, and hands it to the runner.
// Every control has an object name, which is how the tests and the headless
// --dialog, --fill and --press switches drive it:
//   fileRunScriptDialog     the dialog (File > Run Script, fileRunScript)
//   scriptPath              the script file
//   scriptBrowse            choose it with a file dialog (not in a headless
//                           session, which says so)
//   scriptPreview           the command lines the file holds, numbered as in
//                           the file (read-only)
//   scriptContinueOnError   run every line, counting the refused ones, rather
//                           than stop at the first
//   scriptCommand           the exact line Run will run (read-only)
//   scriptRun               run it
//   scriptStatus            what happened last
//   scriptClose             close

#include <QDialog>
#include <QString>

#include <functional>
#include <span>
#include <vector>

#include "command_runner.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;

namespace katana::qt {

// A command line of a script and where it is in the file, counted from 1, so a
// report can say which line stopped the run.
struct ScriptLine {
    int number = 0;
    QString text;
};

// A comment: a line whose first non-blank is '#', as katana_cli skips it. The
// command line takes a typed one for a note, and runs nothing.
[[nodiscard]] bool isScriptComment(const QString& line);

// The command lines of `text`: a line each, "\r\n" and "\r" ends read as
// "\n", surrounding blanks taken off, and blank lines and '#' comments left
// out.
[[nodiscard]] std::vector<ScriptLine> scriptLines(const QString& text);

// The command lines of the file at `path`, read as UTF-8. NotFound for a file
// that does not exist, FileImportFailure for one that cannot be read.
[[nodiscard]] katana::core::Result<std::vector<ScriptLine>> readScript(const QString& path);

// SCRIPT <file> [CONTINUE], read by the interpreter's own rules (a path with
// blanks quoted); InvalidArgument, with the usage, for anything else.
struct ScriptCommand {
    QString path;
    bool continueOnError = false;
};
[[nodiscard]] katana::core::Result<ScriptCommand> parseScriptCommand(const QString& line);
// The line parseScriptCommand reads back: the path always quoted, with '/'
// separators, as the COPC menu item writes its paths.
[[nodiscard]] QString scriptCommandLine(const QString& path, bool continueOnError);

// How a run ended.
struct ScriptReport {
    int lines = 0;  // the command lines the script holds
    int ran = 0;    // how many were run
    int failed = 0; // how many of those were refused or failed
    // The file line number of the line the run stopped at - refused, QUIT or
    // cancelled - or 0 when it ran to the end.
    int stoppedAt = 0;
    bool quit = false;      // stopped at QUIT or EXIT, which is not a failure
    bool cancelled = false; // stopped by the progress dialog's Cancel
    [[nodiscard]] bool ok() const { return failed == 0 && !cancelled; }
};

struct ScriptOptions {
    bool continueOnError = false;
    // Asked before each line with how many have run and how many there are;
    // false cancels the rest. Unset, nothing is asked.
    std::function<bool(int done, int total)> progress;
};

// Runs `lines` in order through `run`, as ScriptOptions says.
[[nodiscard]] ScriptReport runScriptLines(std::span<const ScriptLine> lines,
                                          const CommandRunner& run, const ScriptOptions& options);

// The record a run ends with: script="name" lines=N ran=N failed=N, then
// stopped_at=K (a refusal), quit_at=K or cancelled_at=K when it stopped early.
// An empty name is lines pasted on the command line: script=pasted, unquoted,
// so it cannot be read for a file of that name.
[[nodiscard]] QString formatScriptReport(const QString& name, const ScriptReport& report);

// What the dialog asks of whoever shows it; the window supplies these, a test
// supplies its own.
struct ScriptDialogContext {
    CommandRunner run;
    // True in a headless session, which opens no file dialog.
    std::function<bool()> headless;
};

class ScriptRunDialog final : public QDialog {
  public:
    explicit ScriptRunDialog(ScriptDialogContext context, QWidget* parent = nullptr);

    // Fills the path, as a pick from a file dialog does.
    void setScriptPath(const QString& path);
    // The line Run would run, or why there is none (no file named).
    [[nodiscard]] katana::core::Result<QString> command() const;
    // What Run does: the SCRIPT line handed to the runner and the outcome
    // said in scriptStatus.
    void run();

  private:
    void browse();
    void refresh();

    ScriptDialogContext context_;
    QLineEdit* path_ = nullptr;
    QPlainTextEdit* preview_ = nullptr;
    QCheckBox* continueOnError_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* run_ = nullptr;
    QLabel* status_ = nullptr;
};

} // namespace katana::qt
