#pragma once

// GIS > Check - GDAL > Gaps and Overlaps...: the window onto GIS COVERAGE
// CHECK and CLEAN (docs/geoprocessing.md, "V4"), built on the GIS dialog
// frame (gis_tool_dialog.hpp).
//
// Its fields, by object name (the frame's are in gis_tool_dialog.hpp; the
// scope's are gisCoverageScope, gisCoverageScopeDrawing, ...):
//   gisCoverageMode      check (report the problems) or clean (move the
//                        boundaries so the areas meet)
//   gisCoverageGap       the widest gap to find, or to close, metres; blank
//                        looks for none
//   gisCoverageSnap      clean: snap vertices this close together, metres
//   gisCoverageMerge     clean: which neighbour an overlap goes to
//   gisCoverageReplace   clean: change the areas in place - required, since
//                        surveyed boundaries must not move silently
//   gisCoverageMarkers   check: a layer to draw each bad edge on, replacing
//                        the last check's
//   gisCoverageProblems  the problems found, a row each

#include <QString>

#include "geo/gis_tool_dialog.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;

namespace katana::qt {

struct GisCoverageForm {
    GisScopeWords scope;
    bool clean = false;
    QString gap;
    QString snap;
    QString merge; // empty: GDAL's default
    bool replace = false;
    QString markers;
};

// GIS COVERAGE CHECK <scope> [gap=<m>] [markers=<layer>], or
// GIS COVERAGE CLEAN <scope> [gap=<m>] [snap=<m>] [merge=<strategy>] REPLACE,
// exactly as it would be typed. InvalidArgument naming the field for a scope
// the controls cannot say, a length that does not read, and a clean without
// Replace ticked - which the verb refuses too.
[[nodiscard]] katana::core::Result<QString> gisCoverageLine(const GisCoverageForm& form);

class GisCoverageDialog final : public GisToolDialog {
  public:
    explicit GisCoverageDialog(GisDialogContext context, QWidget* parent = nullptr);
    [[nodiscard]] GisCoverageForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;

  protected:
    [[nodiscard]] QString summary(const QString& reply) const override;

  private:
    void modeChanged();

    QComboBox* mode_ = nullptr;
    QLineEdit* gap_ = nullptr;
    QLineEdit* snap_ = nullptr;
    QComboBox* merge_ = nullptr;
    QCheckBox* replace_ = nullptr;
    QLineEdit* markers_ = nullptr;
};

} // namespace katana::qt
