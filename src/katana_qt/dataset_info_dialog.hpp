#pragma once

// GIS > Dataset Information: what a GIS file or point cloud holds, in the
// words interop::formatDescription gives it.

#include <QDialog>
#include <QString>

namespace katana::qt {

// Read-only text in a fixed-width font: GIS > Dataset Information.
class DatasetInfoDialog final : public QDialog {
  public:
    DatasetInfoDialog(const QString& title, const QString& text, QWidget* parent = nullptr);
};

} // namespace katana::qt
