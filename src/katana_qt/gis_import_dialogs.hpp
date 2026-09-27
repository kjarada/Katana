#pragma once

// The GIS menu's import dialogs (PLAN.MD Phase 20; docs/interop.md, "Import
// options"): what to take from a GDAL or PDAL source and how - vector data,
// a raster, a point cloud.
//
// Each is built FROM a description of the file (interop::describeSource), so
// it offers what the file actually holds - the layers of this GeoPackage, the
// point count of this LAS, whether it is COPC - and says its choices as the
// IMPORT line they make, shown read-only in <d>Command. The dialog imports
// nothing itself: the window hands that line to its one executor
// (MainWindow::runVerbLine) when Import is pressed, as if it had been typed,
// so the dialog does nothing katana_cli or katana_mcp cannot do by sending the
// same line. The vector dialog's Preview runs the line with PREVIEW, which
// reads with the filters and says how many features match of how many,
// importing nothing.
//
// Object names:
//   vector (vectorImportDialog): vectorImportLayers, vectorImportWhere,
//     vectorImportSql, vectorImportDialect, vectorImportUseScope,
//     vectorImportScope (the shared ScopeFilterWidget, its parts
//     vectorImportScopeSelection ... vectorImportDrawnOnly), vectorImportClip,
//     vectorImportFields, vectorImportAttributes, vectorImportTarget,
//     vectorImportMax, vectorImportOpenOptions, vectorImportCrs,
//     vectorImportAssumeCrs, vectorImportPreview, vectorImportMatchCount,
//     vectorImportCommand, and the Placement group's (import_placement.hpp);
//   raster (rasterImportDialog): rasterImportName, rasterImportMaxPixels,
//     rasterImportBand, rasterImportSubdataset, rasterImportCrs,
//     rasterImportCommand;
//   point cloud (pointCloudImportDialog): pointCloudImportName,
//     pointCloudImportBudget, pointCloudImportClasses,
//     pointCloudImportUseResolution, pointCloudImportResolution,
//     pointCloudImportCommand.

#include <QDialog>
#include <QString>

#include <cstdint>
#include <filesystem>
#include <functional>
#include <vector>

#include "command_runner.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "geo/gis_tool_dialog.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/dataset_info.hpp"
#include "katana/interop/import.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QListWidget;
class QPushButton;
class QSpinBox;

namespace katana::qt {

class ImportPlacementBox;

// " key=value" as a line takes it: the value double-quoted when it holds a
// blank. InvalidArgument naming `field` for a double quote or a line break,
// which no line can carry.
[[nodiscard]] katana::core::Result<QString> importOptionWord(const QString& key, const QString& value,
                                                             const QString& field);

// GIS > Import Vector Data.
class VectorImportDialog final : public QDialog {
  public:
    // `drawing` is the drawing's extent now, for what the Placement group
    // says a move onto it will do.
    VectorImportDialog(const katana::interop::SourceDescription& source,
                       const katana::geometry::Box2& drawing, GisDialogContext context = {},
                       QWidget* parent = nullptr);
    // The IMPORT line the fields make, or why there is none.
    [[nodiscard]] katana::core::Result<QString> command() const;
    // Where the data lands; the window keeps the choice (remember).
    [[nodiscard]] ImportPlacementBox& placementBox() const { return *placement_; }
    // Runs the line with PREVIEW through the context's runner (`run`, and
    // `await` for the job it starts); the match count goes to
    // vectorImportMatchCount when it answers. The scope offers the context's
    // document's layers and its views.
    void preview();
    [[nodiscard]] QString matchText() const;

  private:
    void refresh();
    void showPreview(const VerbOutcome& outcome);

    std::filesystem::path path_;
    GisDialogContext context_;
    QListWidget* layers_ = nullptr;
    QLineEdit* where_ = nullptr;
    QLineEdit* sql_ = nullptr;
    QComboBox* dialect_ = nullptr;
    QCheckBox* useScope_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QCheckBox* clip_ = nullptr;
    QLineEdit* fields_ = nullptr;
    QCheckBox* attributes_ = nullptr;
    QLineEdit* target_ = nullptr;
    QSpinBox* max_ = nullptr;
    QLineEdit* openOptions_ = nullptr;
    QComboBox* crs_ = nullptr;
    QLineEdit* assumeCrs_ = nullptr;
    ImportPlacementBox* placement_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* import_ = nullptr;
    QLabel* matchCount_ = nullptr;
};

// GIS > Import Raster.
class RasterImportDialog final : public QDialog {
  public:
    explicit RasterImportDialog(const katana::interop::SourceDescription& source,
                                QWidget* parent = nullptr);
    [[nodiscard]] katana::core::Result<QString> command() const;

  private:
    void refresh();

    std::filesystem::path path_;
    QLineEdit* name_ = nullptr;
    QComboBox* resolution_ = nullptr;
    QSpinBox* band_ = nullptr;
    QComboBox* subdataset_ = nullptr;
    QComboBox* crs_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* import_ = nullptr;
};

// GIS > Import Point Cloud.
class PointCloudImportDialog final : public QDialog {
  public:
    explicit PointCloudImportDialog(const katana::interop::SourceDescription& source,
                                    QWidget* parent = nullptr);
    [[nodiscard]] katana::core::Result<QString> command() const;

  private:
    void refreshEstimate();
    void refresh();

    std::filesystem::path path_;
    std::uint64_t pointCount_ = 0;
    QLineEdit* name_ = nullptr;
    QSpinBox* budget_ = nullptr;
    QComboBox* classification_ = nullptr;
    QCheckBox* useResolution_ = nullptr;
    QDoubleSpinBox* resolution_ = nullptr;
    QLabel* estimate_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* import_ = nullptr;
};

} // namespace katana::qt
