#pragma once

// The GIS menu's import dialogs (PLAN.MD Phase 20): what to take from a GDAL
// or PDAL source and how - vector data, a raster, a point cloud.
//
// Each is built FROM a description of the file (interop::describeSource), so
// it offers what the file actually holds - the layers of this GeoPackage, the
// point count of this LAS, whether it is COPC - and returns the interop
// options struct the importer takes. No dialog reads or writes a file itself;
// the main window does, so a dialog can be built and painted headlessly by a
// test without a file being touched twice.

#include <QDialog>

#include <cstdint>

#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/dataset_info.hpp"
#include "katana/interop/import.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QSpinBox;

namespace katana::qt {

class ImportPlacementBox;

// GIS > Import Vector Data. Object names: importSourceLayer, importTargetLayer,
// importAttributes, and the Placement group's (import_placement.hpp).
class VectorImportDialog final : public QDialog {
  public:
    // `drawing` is the drawing's extent now, for what the Placement group
    // says a move onto it will do.
    VectorImportDialog(const katana::interop::SourceDescription& source,
                       const katana::geometry::Box2& drawing, QWidget* parent = nullptr);
    [[nodiscard]] katana::interop::VectorImportOptions options() const;
    // Where the data lands; the window reads it with options().
    [[nodiscard]] ImportPlacementBox& placementBox() const { return *placement_; }

  private:
    QComboBox* sourceLayer_ = nullptr;
    QLineEdit* targetLayer_ = nullptr;
    QCheckBox* attributes_ = nullptr;
    ImportPlacementBox* placement_ = nullptr;
};

class RasterImportDialog final : public QDialog {
  public:
    explicit RasterImportDialog(const katana::interop::SourceDescription& source,
                                QWidget* parent = nullptr);
    [[nodiscard]] katana::interop::RasterImportOptions options() const;

  private:
    QLineEdit* name_ = nullptr;
    QComboBox* resolution_ = nullptr;
};

class PointCloudImportDialog final : public QDialog {
  public:
    explicit PointCloudImportDialog(const katana::interop::SourceDescription& source,
                                    QWidget* parent = nullptr);
    [[nodiscard]] katana::interop::PointCloudImportOptions options() const;

  private:
    void refreshEstimate();

    std::uint64_t pointCount_ = 0;
    QLineEdit* name_ = nullptr;
    QSpinBox* budget_ = nullptr;
    QComboBox* classification_ = nullptr;
    QCheckBox* useResolution_ = nullptr;
    QDoubleSpinBox* resolution_ = nullptr;
    QLabel* estimate_ = nullptr;
};

} // namespace katana::qt
