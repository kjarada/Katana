#pragma once

#include <QDialog>

#include "command_runner.hpp"
#include "katana/cad/view_set.hpp"

class QComboBox;
class QLabel;
class QLineEdit;

namespace katana::qt {

// A non-modal command builder: the view menu, toolbar and each plan view's
// bar all open this dialog; only the shared VIEWS SCALE command edits state.
class ViewScaleDialog final : public QDialog {
  public:
    ViewScaleDialog(const katana::cad::ViewSet& views, katana::cad::ViewId view,
                    CommandRunner run, QWidget* parent = nullptr);

    void refresh(katana::cad::ViewId view = katana::cad::kNoView);
    [[nodiscard]] katana::core::Result<QString> line() const;
    bool apply();

  private:
    void readView();
    void showLine();
    void setStatus(const QString& text, bool error);

    const katana::cad::ViewSet& views_;
    CommandRunner run_;
    QComboBox* view_ = nullptr;
    QComboBox* mode_ = nullptr;
    QLineEdit* denominator_ = nullptr;
    QLabel* line_ = nullptr;
    QLabel* status_ = nullptr;
};

} // namespace katana::qt
