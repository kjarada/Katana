#pragma once

// File > Drawing Summary: the drawing at a glance, kept live beside it
// (docs/desktop.md, "File > Drawing Summary").
//
// What STATUS reports (cad/document_status.hpp) and more that a person wants
// in view and the window showed nowhere: the project's full path, what the
// drawing holds (entities, layers, alignments, sheets), what is current
// (layer, style, annotation scale, coordinate system), the selection, the
// history with the next step each way, and the customisation - the loaded
// files in load order, the files the project was drawn with that are not
// loaded, the counts and this drawing's coverage (cad::customisationSummary,
// the words a bare CUSTOMISE prints) - with the names no loaded library
// defines listed: a double-click, or Show in Styles and Linetypes, opens
// Format > Styles and Linetypes on its Missing chip, searching for that name.
//
// It changes nothing. Copy as JSON runs STATUS JSON through the window's one
// executor and copies the reply, so what is copied is exactly what an agent
// reads; Load Customisation is the Format menu's item. The dialog follows the
// Document through a DocumentWatcher, refreshing once per turn of the event
// loop however many commands ran.
//
// Object names:
//   drawingSummaryDialog         the dialog (fileDrawingSummary)
//   drawingSummaryProject        the project's full path and whether it is saved
//   drawingSummaryDrawing        entities, layers, alignments, sheets
//   drawingSummaryCurrent        layer, style, annotation scale, CRS
//   drawingSummarySelection      how many are selected
//   drawingSummaryHistory        undo and redo depth, and the next of each
//   drawingSummaryCustomisation  the customisation report (read-only)
//   drawingSummaryUnresolved     the names no loaded library defines; a
//                                double-click opens the style manager at
//                                Missing on it
//   drawingSummaryShowMissing    the same for the chosen name
//   drawingSummaryCopyJson       copy STATUS JSON
//   drawingSummaryRefresh        read everything again
//   drawingSummaryLoad           Format > Load Customisation
//   drawingSummaryStatus         what happened last
//   drawingSummaryClose          close

#include <QDialog>
#include <QString>

#include <functional>
#include <memory>

#include "command_runner.hpp"
#include "katana/cad/customisation_report.hpp"
#include "katana/cad/document.hpp"

class QLabel;
class QListWidget;
class QPlainTextEdit;

namespace katana::qt {

class DocumentWatcher;

struct DrawingSummaryContext {
    katana::cad::Document* document = nullptr;
    // What is loaded and what the project was drawn with but is not loaded:
    // the window's lists, which the Document does not hold.
    std::function<katana::cad::CustomisationSummary()> customisation;
    // Runs a line through the window's one executor (Copy as JSON).
    CommandRunner run;
    // Format > Load Customisation.
    std::function<void()> load;
    // Opens the style manager at its Missing chip, searching for `name`.
    std::function<void(const QString& name)> showMissing;
    // The project's coordinate system as the status bar says it
    // (projectCrsLabel); unset, the project's own text, or that it has none.
    std::function<QString()> crs;
};

class DrawingSummaryDialog final : public QDialog {
  public:
    explicit DrawingSummaryDialog(DrawingSummaryContext context, QWidget* parent = nullptr);
    ~DrawingSummaryDialog() override;

    // Reads everything again: what the watcher and Refresh do.
    void refresh();
    // What Copy as JSON does: STATUS JSON run, its reply copied and returned;
    // empty, with the status saying why, when it was refused.
    QString copyJson();

  private:
    DrawingSummaryContext context_;
    QLabel* project_ = nullptr;
    QLabel* drawing_ = nullptr;
    QLabel* current_ = nullptr;
    QLabel* selection_ = nullptr;
    QLabel* history_ = nullptr;
    QPlainTextEdit* customisation_ = nullptr;
    QListWidget* unresolved_ = nullptr;
    QLabel* status_ = nullptr;
    // Last, so it goes first and never delivers to a half-destroyed dialog.
    std::unique_ptr<DocumentWatcher> watcher_;
};

} // namespace katana::qt
