#pragma once

// Survey > Subsurface Utilities (AS 5488): the window onto the UTILITY verb
// (docs/subsurface_utilities.md).
//
// The dialog is non-modal and runs nothing of the AS 5488 library itself. Each
// tab is one of the verb's seven forms; the dialog writes the UTILITY line its
// fields describe - shown, as it will run, in utilityCommand - and Run hands
// that line to the window's command executor exactly as if it had been typed:
// it is echoed in the command log, kept in the history, and a DRAW or a
// REGRADE is one undo step like any typed command. The reply comes back into
// utilityOutput. So there is nothing the dialog can do that an agent cannot do
// by typing the same line, and the line the dialog shows is the one to type.
//
// Where the services come from (docs/cad.md, "Scope and filter"): a schedule
// file, or what is drawn - the lines UTILITY DRAW drew, taken by the same
// "Apply to" and "Only those that match" controls as Global Modify
// (ScopeFilterWidget), which give the line its scope and filter words
// (SELECTION | DRAWING | VIEW <id> [EXTENTS] | LAYERS a,b [ONLY], then
// WHERE ...). Report, Verify, Clearance
// and Check take either; Draw always reads a file; Regrade and Schedule always
// read the drawing. Clearance's proposed works are a design file, a line or
// polyline in the drawing (#id, with a LEVEL) or one of the document's
// alignments.
//
// The line is made by utilityCommandLine, a pure function of what the fields
// hold, so it is tested without a window: a path with blanks is quoted, an
// option left blank is left out (the verb's default applies), and a file field
// left empty, a number that does not read or a scope that cannot be said
// refuses the whole line, saying which field - nothing runs. Whether a named
// file can be read is not the dialog's to say: the line runs, and the verb
// refuses it (NotFound, by path).
//
// Every control has an object name, which is how the tests
// (tests/qt_widgets/survey/test_utility_dialog.cpp) and the headless --dialog,
// --fill and --press switches drive it:
//   utilityDialog          the dialog; every action of the menu section opens it
//                          (utilityDraw, utilityReport, utilityVerify,
//                          utilityClearance, utilityCheck, utilityRegrade,
//                          utilityWriteSchedule), each on its own tab
//   utilityTabs            Draw / Report / Verify / Clearance / Check /
//                          Regrade / Schedule
//   utilitySourceFile      the services come from the schedule file (a radio
//   utilitySourceDrawing   button each; --press chooses one)
//   utilitySchedule        the utility schedule (.csv): Draw's, and the other
//                          tabs' when the source is the file
//   utilityScheduleBrowse  choose it with a file dialog
//   utilityScope           the drawing's scope and filter (ScopeFilterWidget,
//                          its controls named utilityScopeSelection,
//                          utilityScopeView, utilityView, utilityOnScreen,
//                          utilityScopeLayers, utilityLayers, utilitySublayers,
//                          utilityScopeDrawing, utilityType<Point|Line|...>,
//                          utilityFilterLayer, utilityFilterStyle,
//                          utilityFilterColour, utilityFilterProperty,
//                          utilityFilterValue, utilityFilterText,
//                          utilityDrawnOnly); live when the source is the
//                          drawing
//   utilitySpacing         the longest detected segment that keeps QL-B,
//                          metres (SPACING; Draw, Report and Regrade)
//   utilityMinCover        the minimum cover to flag, metres (MINCOVER; Draw,
//                          Report and Regrade)
//   utilityLayerPrefix     the top of the layers a Draw makes (LAYER)
//   utilityDesignFile      Clearance's works are a design file (radio) ...
//   utilityDesign          ... this one (.csv)
//   utilityDesignBrowse    choose it with a file dialog
//   utilityDesignEntity    ... or a line or polyline in the drawing (radio) ...
//   utilityDesignEntityId  ... this one, "#12" or "12"
//   utilityDesignUseSelected  the one selected entity's id into it
//   utilityDesignLevel     ... at this level, metres (LEVEL; else its own)
//   utilityDesignAlignment ... or one of the document's alignments (radio) ...
//   utilityDesignAlignmentName  ... this one (a choice of their names)
//   utilityWidth           the works' width, metres (WIDTH)
//   utilityH, utilityV     the clearances the owners require, metres (H, V)
//   utilityMargin          the margin around a QL-C or QL-D service (MARGIN)
//   utilitySchema          the delivery schema (.csv) for Check
//   utilitySchemaBrowse    choose it with a file dialog
//   utilityScheduleOut     the schedule Schedule writes (.csv)
//   utilityScheduleOutBrowse  choose it with a save dialog
//   utilityScheduleSchema  a delivery schema whose words it is written in
//                          (optional; SCHEMA)
//   utilityScheduleSchemaBrowse  choose it with a file dialog
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
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "customisation/scope_filter_widget.hpp"
#include "katana/core/error.hpp"

class QComboBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QRadioButton;
class QShowEvent;
class QTabWidget;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class DocumentWatcher;

// The verb's forms, in the order of the dialog's tabs and of the menu.
enum class UtilityTool {
    Draw,
    Report,
    Verify,
    Clearance,
    Check,
    Regrade,
    Schedule,
};

inline constexpr int kUtilityToolCount = 7;

// The verb's word for a tool: "DRAW", "REPORT" ...
[[nodiscard]] const char* utilityVerbWord(UtilityTool tool);

// Where the services a tool reads come from.
enum class UtilitySource {
    File,    // the schedule file
    Drawing, // what UTILITY DRAW drew, by the scope and filter words
};

// Where Clearance's proposed works come from.
enum class UtilityDesignSource {
    File,      // a design .csv
    Entity,    // a line or polyline in the drawing, #id
    Alignment, // one of the document's alignments, by name
};

// What the dialog's fields hold, as typed. Only the fields of `tool` are
// read; a blank option is left out of the line.
struct UtilityForm {
    UtilityTool tool = UtilityTool::Draw;
    // Report, Verify, Clearance and Check read either; Draw always the file,
    // Regrade and Schedule always the drawing (utilitySourceOf).
    UtilitySource source = UtilitySource::File;
    QString schedule;
    // The drawing's scope and filter words, as ScopeFilterWidget::verbWords
    // gives them, or - in scopeError - why the controls say none.
    QString scope;
    QString scopeError;
    UtilityDesignSource designSource = UtilityDesignSource::File;
    QString design;
    QString designEntity;
    QString designLevel;
    QString alignment;
    QString schema;
    QString scheduleOut;
    QString scheduleSchema;
    QString minCover;
    QString spacing;
    QString width;
    QString horizontal;
    QString vertical;
    QString margin;
    QString layerPrefix;
};

// Where `form`'s tool reads its services from: its own for Draw (the file),
// Regrade and Schedule (the drawing), else the form's choice.
[[nodiscard]] UtilitySource utilitySourceOf(const UtilityForm& form);

// The UTILITY line `form` describes, exactly as it would be typed, where
// <source> is the schedule's path or the scope words:
//   UTILITY DRAW <schedule> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
//   UTILITY REPORT <source> [MINCOVER <m>] [SPACING <m>]
//   UTILITY VERIFY <source>
//   UTILITY CLEARANCE <schedule> <design> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
//   UTILITY CLEARANCE <source> DESIGN <design> | #<id> [LEVEL <z>] | ALIGNMENT <name>
//                     [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
//   UTILITY CHECK <source> SCHEMA <schema>
//   UTILITY REGRADE <scope> [SPACING <m>] [MINCOVER <m>]
//   UTILITY SCHEDULE <out.csv> <scope> [SCHEMA <schema>]
// A schedule file measured against a design file keeps the positional form
// it always had, so a line the dialog wrote before is the line it writes now;
// every other design follows DESIGN, which also ends a WHERE filter plainly.
// A word holding a blank is double-quoted, as the command line reads it.
// InvalidArgument, naming the field, for a schedule, design, entity,
// alignment, schema or output left empty, for a scope the controls cannot say
// (scopeError), for a number that is not one (a comma is not a decimal point
// here, as it is not on the command line), for an entity that is not
// #<whole number from 1>, and for a double quote in any word - the command
// line has no way to write one inside a quoted word. A negative number is
// written as typed: the verb says what it accepts.
[[nodiscard]] katana::core::Result<QString> utilityCommandLine(const UtilityForm& form);

// A scope view of the window's, as the drawing's View scope offers it.
using UtilityScopeView = ScopeFilterView;

// What the dialog asks of whoever shows it; the workbench supplies these, a
// test supplies its own.
struct UtilityDialogContext {
    // Runs `line` as the command line runs a typed one, and returns its reply:
    // the text it logged, or why it failed.
    std::function<katana::core::Result<std::string>(const QString& line)> execute;
    // True in a headless session, which opens no file dialog: Browse and Save
    // As say so instead of waiting on a box nobody can close.
    std::function<bool()> headless;
    // The drawing the scope's layer list, the alignments and Use Selected
    // read. May be null (a test's): the lists are then empty. The dialog may
    // outlive it (it watches it through a DocumentWatcher).
    katana::cad::Document* document = nullptr;
    // The workspace's open views, for the scope's View choice. May be unset:
    // the View choice then names no view a line can carry.
    std::function<std::vector<UtilityScopeView>()> views;
};

class UtilityToolsDialog final : public QDialog {
  public:
    explicit UtilityToolsDialog(UtilityDialogContext context, QWidget* parent = nullptr);
    ~UtilityToolsDialog() override;

    UtilityToolsDialog(const UtilityToolsDialog&) = delete;
    UtilityToolsDialog& operator=(const UtilityToolsDialog&) = delete;

    // Brings `tool`'s tab to the front.
    void showTool(UtilityTool tool);
    [[nodiscard]] UtilityTool tool() const;
    // The source chosen above the tabs (what Report, Verify, Clearance and
    // Check read).
    void setSource(UtilitySource source);
    // The drawing's scope and filter controls.
    [[nodiscard]] ScopeFilterWidget& scopeControls() const { return *scope_; }
    // Refills what comes from the drawing and the window - the scope's
    // layers and views, the alignments - keeping what is chosen. Done when
    // the dialog is shown and when the drawing changes.
    void reload();
    // The fields as they are now.
    [[nodiscard]] UtilityForm form() const;
    // The line Run would run, or why there is none.
    [[nodiscard]] katana::core::Result<QString> command() const;
    // What Run does: the line handed to the executor and its reply shown; a
    // line that cannot be written runs nothing and the status says why.
    void run();
    // What Use Selected does: the one selected entity's id into
    // utilityDesignEntityId, with the design source set to it. False, the
    // status saying why, when not exactly one entity is selected.
    bool useSelectedDesign();
    // Writes the reply to `path` as UTF-8 text: what Save As does once a
    // file is chosen.
    [[nodiscard]] katana::core::Status saveOutputTo(const QString& path);
    // The file name Save As offers, "utility_draw.txt": named for the tool
    // whose reply utilityOutput shows, which stays while another tab is
    // brought forward to prepare the next run. The tab in front before
    // anything has run.
    [[nodiscard]] QString suggestedFileName() const;
    void setStatus(const QString& text, bool isError = false);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refreshCommand();
    void browse(QLineEdit& field, const QString& title, const QString& filter);
    void browseForSave(QLineEdit& field, const QString& title, const QString& filter);
    void saveOutput();
    void showOutput(const QString& text);
    [[nodiscard]] bool documentAlive() const;

    UtilityDialogContext context_;
    // The tool whose reply utilityOutput shows; nullopt before the first run.
    std::optional<UtilityTool> shown_;
    QTabWidget* tabs_ = nullptr;
    QRadioButton* sourceFile_ = nullptr;
    QRadioButton* sourceDrawing_ = nullptr;
    QLineEdit* schedule_ = nullptr;
    QPushButton* scheduleBrowse_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QLineEdit* spacing_ = nullptr;
    QLineEdit* layerPrefix_ = nullptr;
    QLineEdit* minCover_ = nullptr;
    QRadioButton* designFile_ = nullptr;
    QRadioButton* designEntity_ = nullptr;
    QRadioButton* designAlignment_ = nullptr;
    QLineEdit* design_ = nullptr;
    QPushButton* designBrowse_ = nullptr;
    QLineEdit* designEntityId_ = nullptr;
    QPushButton* designUseSelected_ = nullptr;
    QLineEdit* designLevel_ = nullptr;
    QComboBox* alignment_ = nullptr;
    QLineEdit* width_ = nullptr;
    QLineEdit* horizontal_ = nullptr;
    QLineEdit* vertical_ = nullptr;
    QLineEdit* margin_ = nullptr;
    QLineEdit* schema_ = nullptr;
    QLineEdit* scheduleOut_ = nullptr;
    QLineEdit* scheduleSchema_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* output_ = nullptr;
    QPushButton* copy_ = nullptr;
    QPushButton* save_ = nullptr;
    QLabel* status_ = nullptr;
    // Last, so it goes first: no delivery reaches a half-destroyed dialog.
    std::unique_ptr<DocumentWatcher> watcher_;
};

} // namespace katana::qt
