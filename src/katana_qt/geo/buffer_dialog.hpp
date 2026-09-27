#pragma once

// GIS > Analysis - GDAL > Buffer...: the window onto GIS BUFFER
// (docs/geoprocessing.md, "V1"), built on the GIS dialog frame
// (gis_tool_dialog.hpp): its line is shown as it will run and Run hands it
// to the window's one executor.
//
// Its fields, by object name (the frame's are in gis_tool_dialog.hpp; the
// scope's are gisBufferScope, gisBufferScopeDrawing, ...):
//   gisBufferDistance          the distance, metres; negative shrinks an area
//   gisBufferDistanceProperty  or the property holding each entity's own
//                              distance (distance=prop:<key>); it wins when
//                              filled
//   gisBufferSide              both | left | right (of a line, walking along it)
//   gisBufferCaps              round | flat | square line ends
//   gisBufferJoins             round | mitre | bevel corners
//   gisBufferDissolve          merge the results
//   gisBufferDissolveBy        ... by these properties (a,b); blank merges all
//   gisBufferLayer             the layer the result goes to (default gis/buffer)

#include <QString>

#include "geo/gis_tool_dialog.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;

namespace katana::qt {

// What the Buffer dialog's fields hold, as typed.
struct GisBufferForm {
    GisScopeWords scope;
    QString distance;
    QString distanceProperty;
    QString side = "both";
    QString caps = "round";
    QString joins = "round";
    bool dissolve = false;
    QString dissolveBy;
    QString layer;
};

// The line `form` describes, exactly as it would be typed:
//   GIS BUFFER <scope> distance=<m>|distance=prop:<key> [side=left|right]
//              [caps=flat|square] [joins=mitre|bevel] [DISSOLVE|dissolve=a,b]
//              [TO LAYER <layer>]
// A choice left at the verb's default is left out. InvalidArgument naming the
// field for a scope the controls cannot say, a distance that is not a number
// or is 0, neither a distance nor a property, and a double quote anywhere.
[[nodiscard]] katana::core::Result<QString> gisBufferLine(const GisBufferForm& form);

class GisBufferDialog final : public GisToolDialog {
  public:
    explicit GisBufferDialog(GisDialogContext context, QWidget* parent = nullptr);

    [[nodiscard]] GisBufferForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;

  private:
    QLineEdit* distance_ = nullptr;
    QLineEdit* property_ = nullptr;
    QComboBox* side_ = nullptr;
    QComboBox* caps_ = nullptr;
    QComboBox* joins_ = nullptr;
    QCheckBox* dissolve_ = nullptr;
    QLineEdit* dissolveBy_ = nullptr;
    QLineEdit* layer_ = nullptr;
};

} // namespace katana::qt
