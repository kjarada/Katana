#pragma once

// GIS > Export Surface as DEM (PLAN.MD Phase 20): which surface, and the cell
// of the grid it is sampled on.

#include <QDialog>
#include <QString>

#include <vector>

#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/terrain_io.hpp"

class QComboBox;
class QDialogButtonBox;
class QDoubleSpinBox;
class QLabel;

namespace katana::qt {

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

} // namespace katana::qt
