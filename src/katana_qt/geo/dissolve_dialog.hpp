#pragma once

// GIS > Analysis - GDAL > Dissolve...: the window onto GIS DISSOLVE
// (docs/geoprocessing.md, "V1"), built on the GIS dialog frame
// (gis_tool_dialog.hpp).
//
// Its fields, by object name (the frame's are in gis_tool_dialog.hpp; the
// scope's are gisDissolveScope, gisDissolveScopeDrawing, ...):
//   gisDissolveBy             merge areas whose properties agree (a,b); blank
//                             merges every area in the scope
//   gisDissolveKeepIdentical  keep the other properties the whole group shares
//   gisDissolveReplace        delete the areas merged, in the same undo step
//   gisDissolveLayer          the layer the result goes to (default gis/dissolve)

#include <QString>

#include "geo/gis_tool_dialog.hpp"

class QCheckBox;
class QLineEdit;

namespace katana::qt {

struct GisDissolveForm {
    GisScopeWords scope;
    QString by;
    bool keepIdentical = false;
    bool replace = false;
    QString layer;
};

// GIS DISSOLVE <scope> [by=a,b] [keep=identical] [TO LAYER <layer>] [REPLACE],
// exactly as it would be typed. InvalidArgument naming the field for a scope
// the controls cannot say and a double quote anywhere.
[[nodiscard]] katana::core::Result<QString> gisDissolveLine(const GisDissolveForm& form);

class GisDissolveDialog final : public GisToolDialog {
  public:
    explicit GisDissolveDialog(GisDialogContext context, QWidget* parent = nullptr);

    [[nodiscard]] GisDissolveForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;

  private:
    QLineEdit* by_ = nullptr;
    QCheckBox* keep_ = nullptr;
    QCheckBox* replace_ = nullptr;
    QLineEdit* layer_ = nullptr;
};

} // namespace katana::qt
