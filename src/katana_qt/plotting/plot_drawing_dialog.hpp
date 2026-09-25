#pragma once

// File > Plot to PDF and the PLOT verb: the drawing, as the active plan view
// shows it, on one sheet (docs/headless.md, "--plot"; docs/desktop.md, "Plot
// to PDF and view images").
//
//   PLOT <file.pdf> [paper=A0|A1|A2|A3|A4] [landscape|portrait] [fit|scale=N]
//        [dpi=N] [style=colour|grey|mono] [lineweight=F] [margin=MM]
//
// What is not given takes PlotSettings' default: A3 landscape, fitted at the
// first standard scale the drawing fits, 300 dpi, in colour, line weights as
// drawn, a 10 mm margin. scale= may be written 500 or 1:500. The verb is the
// window's, beside PLOTSHEETS: the painter is Qt's, which katana_cli and
// katana_mcp do not have; a headless run has --plot, whose switches take the
// same settings.
//
// The dialog is non-modal and runs nothing itself: it writes the PLOT line its
// fields describe, shown in plotDrawingCommand, and Plot hands it to the
// window's one executor. Object names:
//   plotDrawingDialog            the dialog (File > Plot to PDF, filePlot)
//   plotDrawingPath              the PDF to write
//   plotDrawingBrowse            choose it with a file dialog (not headless)
//   plotDrawingPaper             A4 .. A0
//   plotDrawingOrientation       Landscape / Portrait
//   plotDrawingFit               fit the drawing at a standard scale
//   plotDrawingScale             1 : N when not fitting
//   plotDrawingDpi               the resolution
//   plotDrawingMargin            the margin all round, mm
//   plotDrawingColourMode        Colour / Greyscale / Monochrome
//   plotDrawingLineWeightScale   every line weight times this, 0.10 to 5.00
//   plotDrawingCommand           the exact line Plot will run (read-only)
//   plotDrawingPlot              run it
//   plotDrawingStatus            what happened last
//   plotDrawingClose             close

#include <QDialog>
#include <QString>

#include <functional>

#include "command_runner.hpp"
#include "katana/cad/plot.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QPushButton;

namespace katana::qt {

struct PlotDrawingRequest {
    QString path;
    // Paper, orientation, scale, dpi, margin, colour mode and line weight
    // scale; the centre is the plot's to set.
    katana::cad::PlotSettings settings{};
    bool fit = true;
};

// A PLOT line read by the interpreter's word rules (a path with blanks
// quoted). InvalidArgument naming the word, with the usage, for anything the
// grammar above does not take: no path, an unknown option, a paper that is
// not A0 to A4, a scale that is not a positive number, a dpi outside 72 to
// 1200, a line weight scale outside 0.1 to 5, a margin outside 0 to 50 mm.
[[nodiscard]] katana::core::Result<PlotDrawingRequest> parsePlotDrawing(const QString& line);
// The line parsePlotDrawing reads back, every setting written out: the path
// quoted with '/' separators, the paper, the orientation, fit or the scale, the
// dpi, the style, the line weight scale and the margin.
[[nodiscard]] QString plotDrawingCommandLine(const PlotDrawingRequest& request);

struct PlotDrawingDialogContext {
    CommandRunner run;
    // True in a headless session, which opens no file dialog.
    std::function<bool()> headless;
    // The file Browse and a new dialog start from.
    QString suggestedPath;
    // Asked before Plot writes over a file that is there: true to write it.
    // Unset, it is written - as the PLOT verb writes, asking nothing. The
    // window asks the person, and never in a headless run.
    std::function<bool(const QString& path)> confirmReplace;
};

class PlotDrawingDialog final : public QDialog {
  public:
    explicit PlotDrawingDialog(PlotDrawingDialogContext context, QWidget* parent = nullptr);

    // The request the fields describe, or why there is none (no file named).
    [[nodiscard]] katana::core::Result<PlotDrawingRequest> request() const;
    // What Plot does: the PLOT line handed to the runner and the outcome
    // said in plotDrawingStatus.
    void plot();
    // The file for the drawing as it is now - another project opened, an
    // untitled one saved: shown in plotDrawingPath unless something else has
    // been typed there since the last suggestion. The dialog is kept while
    // the window lives, and once plotted every drawing into the folder, and
    // under the name, of the project open when it was first shown.
    void suggestPath(const QString& path);

  private:
    void refresh();
    void browse();

    PlotDrawingDialogContext context_;
    // The last path suggested, to tell it from one typed.
    QString suggested_;
    QLineEdit* path_ = nullptr;
    QComboBox* paper_ = nullptr;
    QComboBox* orientation_ = nullptr;
    QCheckBox* fit_ = nullptr;
    QDoubleSpinBox* scale_ = nullptr;
    QDoubleSpinBox* dpi_ = nullptr;
    QDoubleSpinBox* margin_ = nullptr;
    QComboBox* colourMode_ = nullptr;
    QDoubleSpinBox* lineWeight_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* plot_ = nullptr;
    QLabel* status_ = nullptr;
};

} // namespace katana::qt
