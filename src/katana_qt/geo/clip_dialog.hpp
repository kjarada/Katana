#pragma once

// GIS > Analysis - GDAL > Clip to Boundary...: the window onto GIS CLIP
// (docs/geoprocessing.md, "V3"), built on the GIS dialog frame
// (gis_tool_dialog.hpp).
//
// Two scopes: what is clipped is the frame's (gisClipScope,
// gisClipScopeDrawing, ...), and the boundary a second set of the same
// controls (gisClipByScope, gisClipByScopeDrawing, ...) - or a file. Its
// fields, by object name:
//   gisClipBySourceDrawing  clip to the areas the second scope takes
//   gisClipBySourceFile     ... or to a file's (radio buttons)
//   gisClipByFile           the file's path
//   gisClipByFileBrowse     choose it with a file dialog
//   gisClipByFileLayer      the file's layer (blank: its first)
//   gisClipByFileWhere      only the file's features this SQL WHERE takes
//   gisClipReplace          cut the entities in place, instead of drawing the
//                           pieces on a layer
//   gisClipLayer            the layer the pieces go to (default gis/clip)

#include <QString>

#include "geo/gis_tool_dialog.hpp"

class QCheckBox;
class QLineEdit;
class QPushButton;
class QRadioButton;

namespace katana::qt {

struct GisClipForm {
    GisScopeWords scope;
    bool byFile = false;
    GisScopeWords by;
    QString file, fileLayer, fileWhere;
    bool replace = false;
    QString layer;
};

// GIS CLIP <scope> BY (<scope> | FILE <path> [LAYER <name>] [where="<sql>"])
// [TO LAYER <layer> | REPLACE], exactly as it would be typed. InvalidArgument
// naming the field for a scope the controls cannot say, a file left empty and
// a double quote anywhere; with Replace the layer is not written.
[[nodiscard]] katana::core::Result<QString> gisClipLine(const GisClipForm& form);

class GisClipDialog final : public GisToolDialog {
  public:
    explicit GisClipDialog(GisDialogContext context, QWidget* parent = nullptr);
    [[nodiscard]] GisClipForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;
    void reload() override;
    [[nodiscard]] ScopeFilterWidget& byControls() const { return *by_; }

  private:
    void sourceChanged();

    ScopeFilterWidget* by_ = nullptr;
    QRadioButton* fromDrawing_ = nullptr;
    QRadioButton* fromFile_ = nullptr;
    QLineEdit* file_ = nullptr;
    QPushButton* browse_ = nullptr;
    QLineEdit* fileLayer_ = nullptr;
    QLineEdit* fileWhere_ = nullptr;
    QCheckBox* replace_ = nullptr;
    QLineEdit* layer_ = nullptr;
};

} // namespace katana::qt
