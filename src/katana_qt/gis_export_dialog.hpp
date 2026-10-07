#pragma once

// Export Drawing's dialog (File > Export > Export Drawing...; docs/interop.md,
// "Export options"): the window onto EXPORT, built on the GIS dialog frame
// (geo/gis_tool_dialog.hpp). What is written is the shared "Apply to" and
// "Only those that match" controls (CLAUDE.md section 1.1); the rest are
// EXPORT's words. The dialog writes nothing itself: Run hands the EXPORT line
// it shows to the window's one executor, and Preview runs it with PREVIEW,
// which says what the scope takes and writes nothing.
//
// Its fields, by object name (the frame's - vectorExportDialog,
// vectorExportCommand, vectorExportPreview, vectorExportRun, vectorExportReply
// ... - are in gis_tool_dialog.hpp; the scope's are vectorExportScope,
// vectorExportScopeDrawing, ...):
//   vectorExportFile              the file; its extension picks the format
//   vectorExportBrowse            choose it with a file dialog
//   vectorExportLayerName         the layer's name in the file (layername=)
//   vectorExportSplit             one file layer per drawing layer (split=layer)
//   vectorExportAppend            add to the file rather than replace it
//   vectorExportCrs               project | native | a code (crs=)
//   vectorExportCreationOptions   KEY=VALUE ... (co=)
//   vectorExportLayerOptions      KEY=VALUE ... (lco=)
//   vectorExportText              skip | points (text=)
//   vectorExportCurve             the chords' largest stray from an arc (curve=)
//   vectorExportProperties        write entity properties (properties=)
// A .dxf or a .12da takes the scope alone: the GDAL fields are off for one.

#include <QString>

#include "geo/gis_tool_dialog.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QPushButton;

namespace katana::qt {

struct VectorExportForm {
    QString file;
    GisScopeWords scope;
    QString layerName;
    bool split = false;
    bool append = false;
    QString crs;                 // empty: the project's (the verb's default)
    QString creationOptions;     // blank-separated KEY=VALUE
    QString layerOptions;        // blank-separated KEY=VALUE
    bool textAsPoints = false;
    QString curve;               // empty: the verb's 1 mm
    bool properties = true;
};

// Whether EXPORT writes `file` natively (a .dxf, a .12da archive), which
// takes a scope and none of GDAL's options.
[[nodiscard]] bool exportIsNative(const QString& file);

// EXPORT "<file>" <scope> [layername=<n>] [split=layer] [append] [crs=<c>]
// [co=K=V]... [lco=K=V]... [text=points] [curve=<m>] [properties=no],
// exactly as it would be typed; a default is left out. InvalidArgument
// naming the field for no file, a double quote, an option that is not
// KEY=VALUE, a curve that is not a number above 0, a layer name with split,
// and a scope the controls cannot say.
[[nodiscard]] katana::core::Result<QString> vectorExportLine(const VectorExportForm& form);

class VectorExportDialog final : public GisToolDialog {
  public:
    explicit VectorExportDialog(GisDialogContext context, QWidget* parent = nullptr);
    // The file to write, as File > Export > Export Drawing's file dialog chose it.
    void setFile(const QString& file);
    [[nodiscard]] VectorExportForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;

  protected:
    [[nodiscard]] QString summary(const QString& reply) const override;

  private:
    void updateEnabled();

    QLineEdit* file_ = nullptr;
    QPushButton* browse_ = nullptr;
    QLineEdit* layerName_ = nullptr;
    QCheckBox* split_ = nullptr;
    QCheckBox* append_ = nullptr;
    QComboBox* crs_ = nullptr;
    QLineEdit* creationOptions_ = nullptr;
    QLineEdit* layerOptions_ = nullptr;
    QComboBox* text_ = nullptr;
    QLineEdit* curve_ = nullptr;
    QCheckBox* properties_ = nullptr;
};

} // namespace katana::qt
