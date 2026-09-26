#pragma once

// GIS > Check - GDAL > Check Geometry... and Repair Geometry...: the windows
// onto GIS CHECK and GIS REPAIR (docs/geoprocessing.md, "V4"), built on the
// GIS dialog frame (gis_tool_dialog.hpp): each line shown as it will run, and
// Run handing it to the window's one executor.
//
// Their fields, by object name (the frame's are in gis_tool_dialog.hpp):
//   gisCheckDialog     Check Geometry; its scope gisCheckScope, ...
//     gisCheckMarkers  a layer to draw a point at each problem on, replacing
//                      the last check's markers there; blank draws none
//     gisCheckProblems the problems found, a row each
//   gisRepairDialog    Repair Geometry; its scope gisRepairScope, ...
//     gisRepairMethod  linework (GEOS's default, keeps every edge) or
//                      structure (rebuilds from the rings' structure)

#include <QString>

#include "geo/gis_tool_dialog.hpp"

class QComboBox;
class QLineEdit;

namespace katana::qt {

struct GisCheckForm {
    GisScopeWords scope;
    QString markers;
};

// GIS CHECK <scope> [markers=<layer>], exactly as it would be typed.
[[nodiscard]] katana::core::Result<QString> gisCheckLine(const GisCheckForm& form);

struct GisRepairForm {
    GisScopeWords scope;
    QString method = "linework";
};

// GIS REPAIR <scope> [method=structure]; the default method left out.
[[nodiscard]] katana::core::Result<QString> gisRepairLine(const GisRepairForm& form);

class GisCheckDialog final : public GisToolDialog {
  public:
    explicit GisCheckDialog(GisDialogContext context, QWidget* parent = nullptr);
    [[nodiscard]] GisCheckForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;

  protected:
    [[nodiscard]] QString summary(const QString& reply) const override;

  private:
    QLineEdit* markers_ = nullptr;
};

class GisRepairDialog final : public GisToolDialog {
  public:
    explicit GisRepairDialog(GisDialogContext context, QWidget* parent = nullptr);
    [[nodiscard]] GisRepairForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;

  private:
    QComboBox* method_ = nullptr;
};

// A check's reply in a sentence: how many problems, on how many entities.
[[nodiscard]] QString gisProblemsSummary(const QString& reply, const QString& none);

} // namespace katana::qt
