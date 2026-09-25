#pragma once

// Survey > Subsurface Utilities (AS 5488): the UTILITY verb in the window
// (docs/subsurface_utilities.md, docs/desktop.md).
//
// Built as the Online Data workbench is (gis_online.hpp): MainWindow only
// constructs it, hands it the Survey menu and a few callbacks
// (UtilityServices), and asks it to run any line that starts with UTILITY.
// The verb itself is the CommandInterpreter's, shared with katana_cli; what
// this class adds is what only a window has:
//   - the menu section and its five actions (utilityDraw, utilityReport,
//     utilityVerify, utilityClearance, utilityCheck), each opening the one
//     dialog (utility_dialog.hpp) on its own tab;
//   - after a UTILITY DRAW that worked - typed, or run from the dialog - every
//     plan view framed on the reply's bounds= box, read by the verb's own
//     utilities::drawReplyBounds. Survey data in a real coordinate system
//     usually lands far from what the view was showing, and left unframed a
//     successful draw looked like one that did nothing.
// The dialog's Run goes back through the window's one executor
// (UtilityServices::run, MainWindow::runVerbLine), so a line from the dialog
// is echoed, kept in the history and undone exactly as a typed one; it reaches
// runLine like any other, which is where its reply is kept for the dialog to
// show. A TYPED line reaches runLine only when no tool is waiting for typed
// text: then it is the tool's - a label may read "Utility pit" - where the
// dialog's line never is.

#include <QPointer>
#include <QString>

#include <array>
#include <functional>
#include <optional>
#include <string>

#include "command_runner.hpp"
#include "icons.hpp"
#include "katana/core/error.hpp"
#include "survey/utility_dialog.hpp"

class QAction;
class QKeySequence;
class QMainWindow;
class QMenu;

namespace katana::qt {

class ViewWorkspace;

struct UtilityServices {
    // The window's views, framed after a draw. May be null (a test's).
    ViewWorkspace* views = nullptr;
    std::function<QAction*(Icon icon, const QString& text, const QString& tip,
                           const QKeySequence& shortcut, const QString& objectName)>
        makeAction;
    // The command log. isError also flashes the message in the status bar.
    std::function<void(const QString& text, bool isError)> log;
    std::function<bool()> headless;
    // The window's CommandInterpreter: runs one line, returns its reply.
    std::function<katana::core::Result<std::string>(const std::string& line)> interpret;
    // The window's one executor (MainWindow::runVerbLine): `line` echoed in
    // the log and handed back to runLine, as a typed UTILITY line is - kept in
    // the history by `interpret` - but never to a running tool, which a typed
    // line may be.
    CommandRunner run;
};

class UtilityWorkbench {
  public:
    // Adds the "Subsurface Utilities (AS 5488)" section and its five actions
    // to `surveyMenu`, after what is already there.
    UtilityWorkbench(QMainWindow& window, UtilityServices services, QMenu& surveyMenu);
    // Deletes the dialog, which calls back into this object.
    ~UtilityWorkbench();

    UtilityWorkbench(const UtilityWorkbench&) = delete;
    UtilityWorkbench& operator=(const UtilityWorkbench&) = delete;

    [[nodiscard]] QAction* action(UtilityTool tool) const;

    // True when `line` is a UTILITY line, which it then runs through the
    // interpreter, replying to the log and, after a DRAW that worked, framing
    // what was drawn in every plan view; false leaves the line to whoever
    // asked.
    bool runLine(const QString& line);
    // What the dialog's Run does: `line` through the window's one executor,
    // and the reply runLine had for it - the interpreter's own, so a refusal
    // keeps its code and its report. InvalidState when the line never reached
    // runLine.
    [[nodiscard]] katana::core::Result<std::string> execute(const QString& line);

    // The dialog, made the first time it is asked for.
    UtilityToolsDialog& dialog();
    // Shows the dialog on `tool`'s tab.
    void open(UtilityTool tool);

  private:
    QMainWindow& window_;
    UtilityServices services_;
    std::array<QAction*, 5> actions_{};
    QPointer<UtilityToolsDialog> dialog_;
    // The reply of the last line runLine ran, for execute.
    std::optional<katana::core::Result<std::string>> lastReply_;
};

} // namespace katana::qt
