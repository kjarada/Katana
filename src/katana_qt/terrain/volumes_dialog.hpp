#pragma once

// Terrain > Volumes (docs/volumes.md, "In the window"): the earthworks between
// the drawing's surfaces, a page per method - Surface to Surface, Surface to
// Level, Stockpile, Along Alignment, Grid - and the cut/fill map.
//
// The dialog measures nothing itself (CLAUDE.md section 1): each page's fields
// make the VOLUME line a person would type, shown in volumesCommand as it will
// run, and Measure hands that line to the window's one executor. The reply's
// records fill the results table, and on the Along Alignment page the mass
// haul diagram; Copy puts the table on the clipboard, Save CSV runs the same
// line with csv=, Make Cut/Fill Map runs SURFACE DIFFERENCE and Make Envelope
// SURFACE ENVELOPE. So a person
// in the window, katana_cli and an agent through katana_mcp get the same
// numbers from the same code, and the log shows every line.
//
// The boundary is the shared "Apply to" and "Only those that match" controls
// (customisation/scope_filter_widget.hpp), or corners typed as a line takes
// them, or none (the whole of the surfaces).
//
// Object names: volumesDialog; volumesMethods (the pages: volumesSurfacesPage,
// volumesLevelPage, volumesStockpilePage, volumesSectionsPage,
// volumesGridPage); volumesBase, volumesTop; volumesLevelSurface,
// volumesLevel; volumesStockpileSurface, volumesStockpileBase,
// volumesStockpileLevel; volumesSectionsBase, volumesSectionsTop,
// volumesAlignment, volumesInterval, volumesFrom, volumesTo, volumesLeft,
// volumesRight, volumesSectionMethod, volumesMassHaul; volumesGridBase,
// volumesGridTop, volumesCell; volumesBoundaryGroup, volumesBoundaryNone,
// volumesBoundaryScope, volumesBoundaryCorners, volumesCorners, volumesEach
// and the scope's controls with the prefix volumes (volumesScope,
// volumesScopeDrawing, volumesFilterLayer ...); volumesBulk, volumesShrink;
// volumesCommand; volumesRun; volumesResults; volumesCopy; volumesCsvPath,
// volumesCsvBrowse, volumesSaveCsv; volumesMapName, volumesCutFillMap;
// volumesEnvelopeSide, volumesEnvelopeName, volumesEnvelope; volumesStatus;
// volumesReply; volumesClose.

#include <functional>
#include <utility>
#include <vector>

#include <QDialog>
#include <QPointF>
#include <QString>
#include <QStringList>
#include <QWidget>

#include "command_runner.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QGroupBox;
class QLabel;
class QLineEdit;
class QPaintEvent;
class QPlainTextEdit;
class QRadioButton;
class QTabWidget;
class QTableWidget;

namespace katana::qt {

// What the dialog needs of whoever shows it. The window's comes from
// MainWindow; a test gives its own runner, which is how the dialog is tested
// without a window.
struct VolumesDialogContext {
    // The window's one executor.
    CommandRunner run;
    // The drawing: its surfaces, alignments and layers, read when the dialog
    // is reloaded. Null in a test that needs none (the lists are then empty).
    const katana::cad::Document* document = nullptr;
    // The workspace's open views, for the boundary's View scope.
    std::function<std::vector<ScopeFilterView>()> views{};
    // A headless session opens no file dialog: Browse says to fill the field.
    std::function<bool()> headless{};
};

// The reply's records as a table: a column per key in the order the keys are
// first met, the record's kind ("volume", "polygon", "section") first, and a
// row per record. What the results table shows and Copy copies.
struct VolumeResultTable {
    QStringList columns;
    std::vector<QStringList> rows;
};
[[nodiscard]] VolumeResultTable volumeResultTable(const QString& reply);

// The mass haul diagram: the cumulative cut less fill (the `mass` of each
// `section` record) against station, the zero line drawn. Painted, no moc.
class MassHaulChart final : public QWidget {
  public:
    explicit MassHaulChart(QWidget* parent = nullptr);

    // (station, ordinate) pairs, ascending station; none clears it.
    void setPoints(std::vector<QPointF> points);
    [[nodiscard]] const std::vector<QPointF>& points() const { return points_; }

  protected:
    void paintEvent(QPaintEvent* event) override;

  private:
    std::vector<QPointF> points_;
};

// Which page is showing.
enum class VolumeMethod { Surfaces, Level, Stockpile, Sections, Grid };

class VolumesDialog final : public QDialog {
  public:
    explicit VolumesDialog(VolumesDialogContext context, QWidget* parent = nullptr);

    // The surfaces, alignments, layers and views read again, keeping what is
    // chosen where it is still there: when the dialog is shown, and after a
    // cut/fill map is made.
    void reload();

    void setMethod(VolumeMethod method);
    [[nodiscard]] VolumeMethod method() const;

    // The VOLUME line the showing page's fields say, or why there is none,
    // naming the field.
    [[nodiscard]] katana::core::Result<QString> line() const;
    // SURFACE DIFFERENCE of the page's two surfaces (the Surface to Surface
    // page's when the page has one surface), with the map's name when given.
    [[nodiscard]] katana::core::Result<QString> cutFillMapLine() const;

    // SURFACE ENVELOPE of the same two surfaces: the higher (MAX) or the lower
    // (MIN) of them, as volumesEnvelopeSide says, with the name when given.
    [[nodiscard]] katana::core::Result<QString> envelopeLine() const;

    // Measure: the line run, its records put in the table and, along an
    // alignment, the mass haul diagram. False when there was no line or the
    // line was refused, with the reason in the status.
    bool measure();
    // The line run again with csv= the file field names, replacing it (a
    // file dialog asked first when it chose the file).
    bool saveCsv();
    // SURFACE DIFFERENCE run, and the surface lists reloaded so the map is
    // offered at once.
    bool makeCutFillMap();
    // SURFACE ENVELOPE run, and the surface lists reloaded the same way.
    bool makeEnvelope();
    // The table as tab-separated text with its header: what Copy puts on the
    // clipboard.
    [[nodiscard]] QString resultsText() const;

    [[nodiscard]] const MassHaulChart& chart() const { return *chart_; }

  private:
    void refreshLine();
    void showReply(const VerbOutcome& outcome);
    void setStatus(const QString& text, bool isError);
    void updateEnabled();
    // The two surfaces the showing page has chosen (the Surface to Surface
    // page's when it has one surface), as words of a line.
    [[nodiscard]] katana::core::Result<std::pair<QString, QString>> surfacePair() const;
    // A line that makes a surface run through the executor, reported and the
    // lists reloaded so the new surface is offered at once.
    bool runMaking(const katana::core::Result<QString>& line, const QString& done);
    [[nodiscard]] katana::core::Result<QString> boundaryWords(bool required, bool each) const;
    [[nodiscard]] QString factorWords() const;

    VolumesDialogContext context_;
    QTabWidget* methods_ = nullptr;
    // Surface to Surface
    QComboBox* base_ = nullptr;
    QComboBox* top_ = nullptr;
    // Surface to Level
    QComboBox* levelSurface_ = nullptr;
    QDoubleSpinBox* level_ = nullptr;
    // Stockpile
    QComboBox* stockpileSurface_ = nullptr;
    QComboBox* stockpileBase_ = nullptr;
    QDoubleSpinBox* stockpileLevel_ = nullptr;
    // Along Alignment
    QComboBox* sectionsBase_ = nullptr;
    QComboBox* sectionsTop_ = nullptr;
    QComboBox* alignment_ = nullptr;
    QDoubleSpinBox* interval_ = nullptr;
    QLineEdit* from_ = nullptr;
    QLineEdit* to_ = nullptr;
    QLineEdit* left_ = nullptr;
    QLineEdit* right_ = nullptr;
    QComboBox* sectionMethod_ = nullptr;
    MassHaulChart* chart_ = nullptr;
    // Grid
    QComboBox* gridBase_ = nullptr;
    QComboBox* gridTop_ = nullptr;
    QDoubleSpinBox* cell_ = nullptr;
    // The boundary
    QGroupBox* boundary_ = nullptr;
    QRadioButton* boundaryNone_ = nullptr;
    QRadioButton* boundaryScope_ = nullptr;
    QRadioButton* boundaryCorners_ = nullptr;
    QLineEdit* corners_ = nullptr;
    QCheckBox* each_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    // Factors
    QDoubleSpinBox* bulk_ = nullptr;
    QDoubleSpinBox* shrink_ = nullptr;
    // The line, the results and what happened
    QLineEdit* command_ = nullptr;
    QTableWidget* results_ = nullptr;
    QLineEdit* csvPath_ = nullptr;
    QLineEdit* mapName_ = nullptr;
    QComboBox* envelopeSide_ = nullptr;
    QLineEdit* envelopeName_ = nullptr;
    QLabel* status_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    VolumeResultTable table_;
};

// Terrain > Volumes: the dialog, a child of `window` named volumesDialog (how
// --dialog finds it), made the first time and kept so its fields survive
// between uses; reloaded, shown, raised and activated.
VolumesDialog& showVolumesDialog(QWidget& window, VolumesDialogContext context);

} // namespace katana::qt
