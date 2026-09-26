#pragma once

// GIS > Export Vector Data's options (PLAN.MD Phase 20): which entities, the
// layer's name in the file, the chords arcs become, and the properties.

#include <QDialog>

#include <cstddef>

#include "katana/interop/export.hpp"

class QCheckBox;
class QDoubleSpinBox;
class QLineEdit;

namespace katana::qt {

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

} // namespace katana::qt
