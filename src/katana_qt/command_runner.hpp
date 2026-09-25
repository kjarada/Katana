#pragma once

// The window's ONE executor, as a dialog sees it (CLAUDE.md section 1; docs/desktop.md,
// "One executor: the command runner").
//
// A dialog that changes the drawing does not do the work itself: it builds the
// verb line a person would type and hands it to a CommandRunner, which the main
// window answers with MainWindow::runVerbLine. The line is echoed "> line" in
// the command log, run by the same dispatcher as a typed line - the online and
// utility workbenches' verbs, the window's own, the interpreter's - and so kept
// in the history and undone exactly as a typed one; what it logged comes back
// to the dialog. Unlike a typed line it is NEVER a running tool's answer: a tool
// waiting for a point or a text would take any typed line for one, and a
// dialog's line is never either. What was being typed on the command line is
// left as it was.
//
// A test hands a dialog a runner of its own, which is what lets a dialog be
// tested without a window.

#include <functional>

#include <QString>

namespace katana::qt {

struct VerbOutcome {
    // False when running the line logged an error it was refused for or
    // failed on. A line carried out with problems it only reports - PLOTSHEETS
    // with an image missing - is still ok.
    bool ok = false;
    // What the line logged that was not an error, a line each, in order: the
    // reply a dialog shows.
    QString reply;
    // What it logged as errors, a line each; empty when it logged none.
    QString error;
};

using CommandRunner = std::function<VerbOutcome(const QString& line)>;

} // namespace katana::qt
