#pragma once

// The Plot dialog and the plot's progress (docs/plotting.md, "Plot styles and
// output").
//
// The dialog only collects a PlotRequest - which sheets, the format, the plot
// style, the resolution, where it goes - and plotInteractively hands it to
// plot_output, the same call the command line and an agent make. Every
// control has an object name (listed at PlotDialog), so a test or a scripted
// session fills it the way a person does.
//
// A long plot does not freeze the window: plotWithProgress shows a progress
// dialog that advances a sheet at a time and whose Cancel stops the plot
// before the next sheet. It stays on the GUI thread - the painter reads the
// live document - and lets the event loop run between sheets; the progress
// dialog is APPLICATION-modal, so while it is up nothing in any window can be
// clicked or typed into, and before it is up (a plot quicker than its
// minimum duration shows none) the loop is run without user input. Either
// way the sheets cannot be edited while they are painted. The set is copied
// before the first sheet and the source asked for again before each, so what
// the loop lets through (a background job finishing) cannot leave a sheet
// painted from a surface that has gone.

#include <cstddef>
#include <functional>

#include <QDialog>
#include <QString>

#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"
#include "plot_output.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QFormLayout;
class QLabel;
class QLineEdit;
class QPrinter;
class QPushButton;
class QRadioButton;
class QSpinBox;

namespace katana::cad {
class Document;
}

namespace katana::qt {

// Object names: plotDialog; the sheets plotSheetsAll, plotSheetsCurrent,
// plotSheetsList (radio buttons), plotSheetsText and plotSheetsSummary; the
// output plotFormat (pdf, pdfs, png, tiff, printer - the item data),
// plotColourMode (colour, greyscale, monochrome), plotLineWeightScale,
// plotDpi; the destination plotFile and plotBrowseFile, plotFolder and
// plotBrowseFolder, plotPattern and plotPatternPreview, plotPrinter; then
// plotOpenAfter, plotKeepSetup, plotProblem, and the buttons plotRun and
// plotCancel.
class PlotDialog final : public QDialog {
  public:
    // For `set`, starting from its page setup. `current` is the sheet the
    // editor shows, offered as "Current sheet"; `allSheets` picks All rather
    // than Current to start with. `suggestedFile` is the PDF offered, and
    // its folder the folder offered for a file a sheet.
    PlotDialog(const katana::cad::plotting::SheetSet& set, std::size_t current, bool allSheets,
               const QString& suggestedFile, QWidget* parent = nullptr);

    // The plot the controls describe. For the printer the destination is
    // empty and the format is the page setup's.
    [[nodiscard]] PlotRequest request() const;
    [[nodiscard]] bool toPrinter() const;
    [[nodiscard]] QString printerName() const;
    [[nodiscard]] bool openAfterwards() const;
    // Whether the style, resolution and naming are to become the set's page
    // setup (setPageSetup), so the next plot starts from them.
    [[nodiscard]] bool keepAsPageSetup() const;

    // What stops the choice being plotted - a sheet list that names no
    // sheet, a raster too large, no destination - as validatePlotRequest
    // says it; shown in the dialog, and Plot is disabled while there is one.
    [[nodiscard]] katana::core::Status check() const;

  private:
    void refresh();

    katana::cad::plotting::SheetSet set_;
    std::size_t current_ = 0;
    QRadioButton* all_ = nullptr;
    QRadioButton* currentOnly_ = nullptr;
    QRadioButton* list_ = nullptr;
    QLineEdit* listText_ = nullptr;
    QLabel* sheetsSummary_ = nullptr;
    QComboBox* format_ = nullptr;
    QComboBox* colourMode_ = nullptr;
    QDoubleSpinBox* lineWeightScale_ = nullptr;
    QSpinBox* dpi_ = nullptr;
    QFormLayout* form_ = nullptr;
    QWidget* fileRow_ = nullptr;
    QLineEdit* file_ = nullptr;
    QWidget* folderRow_ = nullptr;
    QLineEdit* folder_ = nullptr;
    QLineEdit* pattern_ = nullptr;
    QLabel* patternPreview_ = nullptr;
    QComboBox* printer_ = nullptr;
    QCheckBox* openAfter_ = nullptr;
    QCheckBox* keepSetup_ = nullptr;
    QLabel* problem_ = nullptr;
    QPushButton* run_ = nullptr;
};

// Plots `request` from a copy of `set` behind an application-modal progress
// dialog (object name plotProgress) that advances a sheet at a time; its
// Cancel stops the plot before the next sheet (PlotReport::cancelled). With
// `printer` the sheets are printed on it instead (printSheets).
[[nodiscard]] katana::core::Result<PlotReport>
plotWithProgress(QWidget* parent, const katana::cad::plotting::SheetSet& set,
                 const PlotRequest& request, const SheetSourceProvider& source,
                 QPrinter* printer = nullptr);

// The PDF a Plot dialog offers first: "<project name>.pdf" in the project's
// folder, or in the user's documents folder for a drawing not saved as a
// project yet ("sheets.pdf" for a project with no name).
[[nodiscard]] QString suggestedPlotFile(const katana::cad::Document& document);

// Plot...: the dialog; then the plot with its progress; then, when asked,
// the choice kept as `document`'s page setup (one undoable step - no step when
// `document` is null or nothing changed); then the result opened when asked.
// The summary, every problem and any error go to `report` (text, whether it
// is an error). Nothing happens when the dialog is cancelled.
void plotInteractively(QWidget* parent, const katana::cad::plotting::SheetSet& set,
                       std::size_t current, bool allSheets, const QString& suggestedFile,
                       const QString& title, const SheetSourceProvider& source,
                       katana::cad::Document* document,
                       const std::function<void(const QString&, bool)>& report);

} // namespace katana::qt
