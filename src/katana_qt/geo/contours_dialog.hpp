#pragma once

// Terrain > Analysis > Contours (docs/terrain.md, "Contours"): the window onto
// the CONTOUR verb (src/katana_app/geo/contour_verbs.cpp).
//
// The dialog traces nothing itself. It writes the CONTOUR line its fields
// describe - shown, as it will run, in contourCommand - and Run hands it to
// the window's one executor as if it had been typed (terrain_dialog_support):
// the contours are the verb's background job, one undo step, and its reply
// comes back into contourReply. The line is made by a pure function of the
// fields, tested without a window.
//
// contoursDialog:
//   contourSource      the surface or reference raster (a surface is traced
//                      exactly on its triangles, a raster by GDAL)
//   contourInterval    the height between contours
//   contourMajorEvery  every how many is a major contour (0: none)
//   contourBase        the level the interval counts from
//   contourLayer       the parent layer: <layer>/major and <layer>/minor
//   contourSmooth      a raster's gaussian smoothing, none, 3 or 5 cells
//                      (off for a surface)
//   contourClip        keep the contours inside closed shapes of the drawing
//   contourScope       which shapes (ScopeFilterWidget: contourScopeDrawing,
//                      contourFilterLayer ...), on with contourClip
//   contourCommand     the exact line Run will run (read-only)
//   contourPreview     run it with PREVIEW: what would be read, nothing drawn
//   contourRun         run it
//   contourReply       what it said

#include <QDialog>
#include <QString>

#include <memory>

#include "geo/terrain_dialog_support.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;
class QSpinBox;

namespace katana::qt {

struct ContourForm {
    // SURFACE <name> or RASTER <id>, as the source picker's data says it.
    QString source;
    QString interval;
    int majorEvery = 5;
    QString base;
    QString layer;
    int smooth = 0;
    bool clip = false;
    // The scope and filter words (ScopeFilterWidget::verbWords), or - in
    // scopeError - why the controls say none.
    QString scope;
    QString scopeError;
};

//   CONTOUR <source> interval=<m> major=<n> [base=<b>] [layer=<l>] [smooth=3|5] [<scope>]
// base is left out when blank, layer when blank, smooth for a surface and
// when none; the scope is written only with clip. InvalidArgument naming the
// field: no source, an interval that is not a positive number, a base that
// is not a number, a layer with a double quote, a scope the controls cannot
// say.
[[nodiscard]] katana::core::Result<QString> contourLine(const ContourForm& form);

class ContoursDialog final : public QDialog {
  public:
    explicit ContoursDialog(TerrainDialogContext context, QWidget* parent = nullptr);
    ~ContoursDialog() override;

    // Refills the sources and the scope's layers and views, keeping what is
    // chosen: done when shown.
    void reload();
    [[nodiscard]] ContourForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const;
    [[nodiscard]] ScopeFilterWidget& scopeControls() const { return *scope_; }
    void run(bool preview = false);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refresh();

    TerrainDialogContext context_;
    QComboBox* source_ = nullptr;
    QLineEdit* interval_ = nullptr;
    QSpinBox* majorEvery_ = nullptr;
    QLineEdit* base_ = nullptr;
    QLineEdit* layer_ = nullptr;
    QComboBox* smooth_ = nullptr;
    QCheckBox* clip_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    std::unique_ptr<TerrainRun> runner_;
};

} // namespace katana::qt
