#pragma once

// The option dialogs of the GIS menu (PLAN.MD Phase 20): what to take from a
// GDAL or PDAL source and how, and how to write data out through them.
//
// Each import dialog is built FROM a description of the file
// (interop::describeSource), so it offers what the file actually holds - the
// layers of this GeoPackage, the point count of this LAS, whether it is COPC -
// and returns the interop options struct the importer takes. No dialog reads
// or writes a file itself; the main window does, so a dialog can be built and
// painted headlessly by a test without a file being touched twice.

#include <QDialog>

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/dataset_info.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/terrain_io.hpp"

class QCheckBox;
class QComboBox;
class QDialogButtonBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QSpinBox;

namespace katana::qt {

class VectorImportDialog final : public QDialog {
  public:
    explicit VectorImportDialog(const katana::interop::SourceDescription& source,
                                QWidget* parent = nullptr);
    [[nodiscard]] katana::interop::VectorImportOptions options() const;

  private:
    QComboBox* sourceLayer_ = nullptr;
    QLineEdit* targetLayer_ = nullptr;
    QCheckBox* attributes_ = nullptr;
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

class VectorExportDialog final : public QDialog {
  public:
    // `format` is the file dialog's choice, shown so the person sees what the
    // options apply to; `selected` is the size of the current selection, and
    // "selected only" is offered only when it is not zero.
    VectorExportDialog(const QString& format, std::size_t selected, QWidget* parent = nullptr);
    [[nodiscard]] bool selectedOnly() const;
    // Everything but the entity list, which belongs to the window.
    void apply(katana::interop::VectorExportOptions& options) const;

  private:
    QCheckBox* selectedOnly_ = nullptr;
    QLineEdit* layerName_ = nullptr;
    QDoubleSpinBox* curveTolerance_ = nullptr;
    QCheckBox* properties_ = nullptr;
};

struct SurfaceChoice {
    QString name;
    katana::geometry::Box2 bounds;
};

class SurfaceRasterDialog final : public QDialog {
  public:
    explicit SurfaceRasterDialog(std::vector<SurfaceChoice> surfaces, QWidget* parent = nullptr);
    [[nodiscard]] int surfaceIndex() const;
    [[nodiscard]] katana::interop::SurfaceRasterOptions options() const;

  private:
    void refreshGrid();

    std::vector<SurfaceChoice> surfaces_;
    QComboBox* surface_ = nullptr;
    QDoubleSpinBox* cellSize_ = nullptr;
    QLabel* grid_ = nullptr;
    QDialogButtonBox* buttons_ = nullptr;
};

// Read-only text in a fixed-width font: GIS > Dataset Information.
class DatasetInfoDialog final : public QDialog {
  public:
    DatasetInfoDialog(const QString& title, const QString& text, QWidget* parent = nullptr);
};

} // namespace katana::qt
