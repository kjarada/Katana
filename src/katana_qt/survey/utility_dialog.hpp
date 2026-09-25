#pragma once

// Survey > Subsurface Utilities (AS 5488): the window onto the UTILITY verb
// (docs/subsurface_utilities.md).
//
// The dialog is non-modal and runs nothing of the AS 5488 library itself. Each
// tab is one of the verb's five forms; the dialog writes the UTILITY line its
// fields describe - shown, as it will run, in utilityCommand - and Run hands
// that line to the window's command executor exactly as if it had been typed:
// it is echoed in the command log, kept in the history, and a DRAW is one undo
// step like any typed command. The reply comes back into utilityOutput. So
// there is nothing the dialog can do that an agent cannot do by typing the
// same line, and the line the dialog shows is the one to type.
//
// The line is made by utilityCommandLine, a pure function of what the fields
// hold, so it is tested without a window: a path with blanks is quoted, an
// option left blank is left out (the verb's default applies), and a file field
// left empty or a number that does not read refuses the whole line, saying
// which field - nothing runs. Whether a named file can be read is not the
// dialog's to say: the line runs, and the verb refuses it (NotFound, by path).
//
// Every control has an object name, which is how the tests
// (tests/qt_widgets/survey/test_utility_dialog.cpp) and the headless --dialog,
// --fill and --press switches drive it:
//   utilityDialog          the dialog; every action of the menu section opens it
//                          (utilityDraw, utilityReport, utilityVerify,
//                          utilityClearance, utilityCheck), each on its own tab
//   utilityTabs            Draw / Report / Verify / Clearance / Check
//   utilitySchedule        the utility schedule (.csv), every tab's
//   utilityScheduleBrowse  choose it with a file dialog
//   utilitySpacing         the longest detected segment that keeps QL-B,
//                          metres (SPACING; Draw and Report)
//   utilityMinCover        the minimum cover to flag, metres (MINCOVER; Draw
//                          and Report)
//   utilityLayerPrefix     the top of the layers a Draw makes (LAYER)
//   utilityDesign          the proposed works (.csv) for Clearance
//   utilityDesignBrowse    choose it with a file dialog
//   utilityWidth           the works' width, metres (WIDTH)
//   utilityH, utilityV     the clearances the owners require, metres (H, V)
//   utilityMargin          the margin around a QL-C or QL-D service (MARGIN)
//   utilitySchema          the delivery schema (.csv) for Check
//   utilitySchemaBrowse    choose it with a file dialog
//   utilityCommand         the exact line Run will run (read-only)
//   utilityRun             run it
//   utilityOutput          the reply: the report, the drawn records, or why
//                          the line failed (read-only, monospace)
//   utilityCopy            copy the reply
//   utilitySave            save the reply to a file
//   utilityStatus          what happened last
//   utilityClose           close

#include <QDialog>
#include <QString>

#include <functional>
#include <optional>
#include <string>

#include "katana/core/error.hpp"

class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QTabWidget;

namespace katana::qt {

// The verb's forms, in the order of the dialog's tabs and of the menu.
enum class UtilityTool {
    Draw,
    Report,
    Verify,
    Clearance,
    Check,
};

// The verb's word for a tool: "DRAW", "REPORT" ...
[[nodiscard]] const char* utilityVerbWord(UtilityTool tool);

// What the dialog's fields hold, as typed. Only the fields of `tool` are
// read; a blank option is left out of the line.
struct UtilityForm {
    UtilityTool tool = UtilityTool::Draw;
    QString schedule;
    QString design;
    QString schema;
    QString minCover;
    QString spacing;
    QString width;
    QString horizontal;
    QString vertical;
    QString margin;
    QString layerPrefix;
};

// The UTILITY line `form` describes, exactly as it would be typed:
//   UTILITY DRAW <schedule> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
//   UTILITY REPORT <schedule> [MINCOVER <m>] [SPACING <m>]
//   UTILITY VERIFY <schedule>
//   UTILITY CLEARANCE <schedule> <design> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
//   UTILITY CHECK <schedule> SCHEMA <schema>
// A word holding a blank is double-quoted, as the command line reads it.
// InvalidArgument, naming the field, for a schedule, design or schema left empty,
// for a number that is not one (a comma is not a decimal point here, as it is
// not on the command line), and for a double quote in any word - the command
// line has no way to write one inside a quoted word. A negative number is
// written as typed: the verb says what it accepts.
[[nodiscard]] katana::core::Result<QString> utilityCommandLine(const UtilityForm& form);

// What the dialog asks of whoever shows it; the workbench supplies these, a
// test supplies its own.
struct UtilityDialogContext {
    // Runs `line` as the command line runs a typed one, and returns its reply:
    // the text it logged, or why it failed.
    std::function<katana::core::Result<std::string>(const QString& line)> execute;
    // True in a headless session, which opens no file dialog: Browse and Save
    // As say so instead of waiting on a box nobody can close.
    std::function<bool()> headless;
};

class UtilityToolsDialog final : public QDialog {
  public:
    explicit UtilityToolsDialog(UtilityDialogContext context, QWidget* parent = nullptr);

    // Brings `tool`'s tab to the front.
    void showTool(UtilityTool tool);
    [[nodiscard]] UtilityTool tool() const;
    // The fields as they are now.
    [[nodiscard]] UtilityForm form() const;
    // The line Run would run, or why there is none.
    [[nodiscard]] katana::core::Result<QString> command() const;
    // What Run does: the line handed to the executor and its reply shown; a
    // line that cannot be written runs nothing and the status says why.
    void run();
    // Writes the reply to `path` as UTF-8 text: what Save As does once a
    // file is chosen.
    [[nodiscard]] katana::core::Status saveOutputTo(const QString& path);
    // The file name Save As offers, "utility_draw.txt": named for the tool
    // whose reply utilityOutput shows, which stays while another tab is
    // brought forward to prepare the next run. The tab in front before
    // anything has run.
    [[nodiscard]] QString suggestedFileName() const;
    void setStatus(const QString& text, bool isError = false);

  private:
    void refreshCommand();
    void browse(QLineEdit& field, const QString& title, const QString& filter);
    void saveOutput();
    void showOutput(const QString& text);

    UtilityDialogContext context_;
    // The tool whose reply utilityOutput shows; nullopt before the first run.
    std::optional<UtilityTool> shown_;
    QTabWidget* tabs_ = nullptr;
    QLineEdit* schedule_ = nullptr;
    QLineEdit* spacing_ = nullptr;
    QLineEdit* layerPrefix_ = nullptr;
    QLineEdit* minCover_ = nullptr;
    QLineEdit* design_ = nullptr;
    QLineEdit* width_ = nullptr;
    QLineEdit* horizontal_ = nullptr;
    QLineEdit* vertical_ = nullptr;
    QLineEdit* margin_ = nullptr;
    QLineEdit* schema_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* output_ = nullptr;
    QPushButton* copy_ = nullptr;
    QPushButton* save_ = nullptr;
    QLabel* status_ = nullptr;
};

} // namespace katana::qt
