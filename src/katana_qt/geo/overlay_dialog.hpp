#pragma once

// GIS > Analysis - GDAL > Overlay...: the window onto GIS OVERLAY
// (docs/geoprocessing.md, "V2"), built on the GIS dialog frame
// (gis_tool_dialog.hpp).
//
// Two scopes: the subject is the frame's (gisOverlayScope,
// gisOverlayScopeDrawing, ...), and what it is overlaid with is a second set
// of the same controls (gisOverlayWithScope, gisOverlayWithScopeDrawing,
// gisOverlayWithFilterLayer, ...) - or a file. Its fields, by object name:
//   gisOverlayOperation          intersection | difference | union |
//                                symdifference | identity | update | clip
//   gisOverlayWithSourceDrawing  overlay with what the second scope takes
//   gisOverlayWithSourceFile     ... or with a file (radio buttons)
//   gisOverlayWithFile           the file's path
//   gisOverlayWithFileBrowse     choose it with a file dialog
//   gisOverlayWithFileLayer      the file's layer (blank: its first)
//   gisOverlayWithFileWhere      only the file's features this SQL WHERE takes
//   gisOverlayKeep               the subject's properties to carry (a,b | all |
//                                none; blank: all)
//   gisOverlayKeepWith           the overlay's likewise
//   gisOverlayLayer              the layer the pieces go to (default gis/overlay)
//   gisOverlayCsv                a .csv to write the rows to (optional)
//   gisOverlayOverwrite          replace that file when it exists

#include <QString>

#include "geo/gis_tool_dialog.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QPushButton;
class QRadioButton;

namespace katana::qt {

struct GisOverlayForm {
    QString operation = "intersection";
    GisScopeWords subject;
    bool withFile = false;
    GisScopeWords with;
    QString file, fileLayer, fileWhere;
    QString keep, keepWith;
    QString layer;
    QString csv;
    bool overwrite = false;
};

// GIS OVERLAY <operation> <subject> WITH (<scope> | FILE <path> [LAYER <name>]
// [where="<sql>"]) [keep=..] [keepwith=..] [TO LAYER <layer>] [csv=<file>]
// [OVERWRITE], exactly as it would be typed. InvalidArgument naming the field
// for a scope the controls cannot say, a file left empty, and a double quote
// anywhere but a WHERE clause's own single quotes.
[[nodiscard]] katana::core::Result<QString> gisOverlayLine(const GisOverlayForm& form);

class GisOverlayDialog final : public GisToolDialog {
  public:
    explicit GisOverlayDialog(GisDialogContext context, QWidget* parent = nullptr);
    [[nodiscard]] GisOverlayForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;
    void reload() override;
    // The second scope's controls.
    [[nodiscard]] ScopeFilterWidget& withControls() const { return *with_; }

  private:
    void sourceChanged();

    QComboBox* operation_ = nullptr;
    ScopeFilterWidget* with_ = nullptr;
    QRadioButton* fromDrawing_ = nullptr;
    QRadioButton* fromFile_ = nullptr;
    QLineEdit* file_ = nullptr;
    QPushButton* browse_ = nullptr;
    QLineEdit* fileLayer_ = nullptr;
    QLineEdit* fileWhere_ = nullptr;
    QLineEdit* keep_ = nullptr;
    QLineEdit* keepWith_ = nullptr;
    QLineEdit* layer_ = nullptr;
    QLineEdit* csv_ = nullptr;
    QCheckBox* overwrite_ = nullptr;
};

} // namespace katana::qt
