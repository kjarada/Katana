#pragma once

// Terrain > Surface From Point Cloud / Raster / Drawing and GIS > Export
// Surface as DEM (docs/terrain.md, "Surfaces on every front end"): the window
// onto the SURFACE verb (src/katana_app/geo/surface_verbs.cpp).
//
// Neither dialog triangulates or writes anything itself. Each writes the
// SURFACE line its fields describe - shown, as it will run, in its Command
// field - and Run hands it to the window's one executor as if it had been
// typed (terrain_dialog_support.hpp): the triangulation or the export runs as
// a background job, and its reply comes back into the Reply field. So the
// line a dialog shows is the line an agent types to do the same.
//
// The lines are made by pure functions of the fields, tested without a
// window: a word holding a blank is quoted, an option left blank is left out
// (the verb's default applies), and a field that cannot be read refuses the
// whole line, naming the field.
//
// surfaceFromDialog - every Surface From item opens it on its own source:
//   surfaceFromKind       Point cloud / Raster / Drawing
//   surfaceFromSource     the point cloud or raster (Reference Data's
//                         selection is chosen first)
//   surfaceFromClasses    a cloud's ASPRS classes, "2,8"; blank: its ground,
//                         or every return when it has none
//   surfaceFromMax        the most points triangulated (a raster's stride
//                         and a cloud's thinning are chosen to meet it)
//   surfaceFromArea       a raster's window, x0,y0,x1,y1; blank: all of it
//   surfaceFromScope      what of the drawing (ScopeFilterWidget; its
//                         controls surfaceFromScopeDrawing,
//                         surfaceFromDrawnOnly ...); the whole drawing as it
//                         is drawn, at first
//   surfaceFromName       the surface's name; blank: the source's
//   surfaceFromCommand    the exact line Run will run (read-only)
//   surfaceFromPreview    run it with PREVIEW: what would be read, nothing made
//   surfaceFromRun        run it
//   surfaceFromReply      what it said
//
// surfaceRasterDialog - GIS > Export Surface as DEM:
//   surfaceRasterSource   the surface
//   surfaceRasterCell     the cell, suggested for the surface's extent
//   surfaceRasterGrid     the grid that cell makes, and whether it is over
//                         the limit
//   surfaceRasterType     Float32 / Float64
//   surfaceRasterCog      a Cloud Optimised GeoTIFF
//   surfaceRasterFile     where (.tif, .asc, .img), surfaceRasterBrowse
//   surfaceRasterOverwrite  replace a file already there
//   surfaceRasterCommand, surfaceRasterPreview, surfaceRasterRun,
//   surfaceRasterReply    as above

#include <QDialog>
#include <QString>

#include <memory>
#include <vector>

#include "geo/terrain_dialog_support.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;
class QSpinBox;

namespace katana::qt {

// ---- Surface From ---------------------------------------------------------------------------

enum class SurfaceFromKind { Cloud, Raster, Drawing };

struct SurfaceFromForm {
    SurfaceFromKind kind = SurfaceFromKind::Drawing;
    // CLOUD <id> or RASTER <id>, as the source picker's data says it.
    QString source;
    QString classes;
    QString maxPoints;
    QString area;
    // The drawing's scope and filter words (ScopeFilterWidget::verbWords),
    // or - in scopeError - why the controls say none.
    QString scope;
    QString scopeError;
    QString name;
};

//   SURFACE FROM CLOUD <id> [classes=a,b] [max=<n>] [NAME <n>]
//   SURFACE FROM RASTER <id> [max=<n>] [AREA x0,y0,x1,y1] [NAME <n>]
//   SURFACE FROM <scope> [NAME <n>]
// InvalidArgument naming the field: no source chosen, a scope the controls
// cannot say, classes that are not whole numbers, a cap that is not a whole
// number, an area that is not four numbers, a name with a double quote.
[[nodiscard]] katana::core::Result<QString> surfaceFromLine(const SurfaceFromForm& form);

class SurfaceFromDialog final : public QDialog {
  public:
    explicit SurfaceFromDialog(TerrainDialogContext context, QWidget* parent = nullptr);
    ~SurfaceFromDialog() override;

    // Brings `kind` forward, and the source `words` ("RASTER 3") when given.
    void showKind(SurfaceFromKind kind, const QString& words = {});
    // Refills the sources and the scope's layers and views, keeping what is
    // chosen: done when shown.
    void reload();
    [[nodiscard]] SurfaceFromForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const;
    [[nodiscard]] ScopeFilterWidget& scopeControls() const { return *scope_; }
    // What Run and Preview do.
    void run(bool preview = false);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refresh();

    TerrainDialogContext context_;
    QComboBox* kind_ = nullptr;
    QComboBox* source_ = nullptr;
    QLineEdit* classes_ = nullptr;
    QLineEdit* maxPoints_ = nullptr;
    QLineEdit* area_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QLineEdit* name_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    std::unique_ptr<TerrainRun> runner_;
};

// ---- Export Surface as DEM -----------------------------------------------------------------

struct SurfaceExportForm {
    QString surface;
    QString file;
    double cell = 0.0;
    QString type = "Float32";
    bool cog = false;
    bool overwrite = false;
};

//   SURFACE EXPORT <surface> <file> cell=<m> type=<Float32|Float64> [COG] [OVERWRITE]
// InvalidArgument naming the field: no surface, no file, a cell that is not
// positive, a double quote in a name or path.
[[nodiscard]] katana::core::Result<QString> surfaceExportLine(const SurfaceExportForm& form);

class SurfaceRasterDialog final : public QDialog {
  public:
    explicit SurfaceRasterDialog(TerrainDialogContext context, QWidget* parent = nullptr);
    ~SurfaceRasterDialog() override;

    void reload();
    [[nodiscard]] SurfaceExportForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const;
    void run(bool preview = false);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void suggestCell();
    void refresh();
    void browse();

    TerrainDialogContext context_;
    QComboBox* surface_ = nullptr;
    QDoubleSpinBox* cellSize_ = nullptr;
    QLabel* grid_ = nullptr;
    QComboBox* type_ = nullptr;
    QCheckBox* cog_ = nullptr;
    QLineEdit* file_ = nullptr;
    QPushButton* browse_ = nullptr;
    QCheckBox* overwrite_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    bool overLimit_ = false;
    std::unique_ptr<TerrainRun> runner_;
};

} // namespace katana::qt
