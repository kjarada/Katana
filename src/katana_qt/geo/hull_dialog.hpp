#pragma once

// GIS > Analysis - GDAL > Boundary Around Features...: the window onto GIS
// HULL (docs/geoprocessing.md, "V3"), built on the GIS dialog frame
// (gis_tool_dialog.hpp).
//
// Its fields, by object name (the frame's are in gis_tool_dialog.hpp; the
// scope's are gisHullScope, gisHullScopeDrawing, ...):
//   gisHullKind   convex (the tightest convex boundary) | concave
//   gisHullRatio  concave: 0 (the tightest) to 1 (the convex hull)
//   gisHullHoles  concave: allow holes where the points leave a gap inside
//   gisHullLayer  the layer the boundary goes to (default gis/hull)

#include <QString>

#include "geo/gis_tool_dialog.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;

namespace katana::qt {

struct GisHullForm {
    GisScopeWords scope;
    bool concave = false;
    QString ratio;
    bool holes = false;
    QString layer;
};

// GIS HULL <scope> [concave=<ratio> [holes]] [TO LAYER <layer>], exactly as it
// would be typed; convex, the verb's default, is left out. InvalidArgument
// naming the field for a ratio that is not a number from 0 to 1, and a scope
// the controls cannot say.
[[nodiscard]] katana::core::Result<QString> gisHullLine(const GisHullForm& form);

class GisHullDialog final : public GisToolDialog {
  public:
    explicit GisHullDialog(GisDialogContext context, QWidget* parent = nullptr);
    [[nodiscard]] GisHullForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;

  private:
    QComboBox* kind_ = nullptr;
    QLineEdit* ratio_ = nullptr;
    QCheckBox* holes_ = nullptr;
    QLineEdit* layer_ = nullptr;
};

} // namespace katana::qt
