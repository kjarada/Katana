#pragma once

// Terrain > Analysis > Terrain Shading (docs/terrain.md, "Shading"): the
// window onto the RASTER SHADE verb (src/katana_app/geo/shade_verbs.cpp).
//
// The dialog renders nothing itself. It writes the RASTER SHADE line its
// fields describe - shown, as it will run, in shadingCommand - and Run hands
// it to the window's one executor as if it had been typed
// (terrain_dialog_support): the picture is the verb's background job, kept
// as a derived reference raster, and the reply - with the legend - comes
// back into shadingReply. The line is made by a pure function of the fields,
// tested without a window.
//
// terrainShadingDialog:
//   shadingSource     the surface or reference raster
//   shadingStyle      hillshade, relief, relief over hillshade, slope, plain
//   shadingAzimuth    the light's direction (hillshades)
//   shadingAltitude   the light's height (hillshades)
//   shadingZFactor    the vertical exaggeration (hillshades)
//   shadingVariant    regular, combined, multidirectional, igor (hillshades)
//   shadingRamp       terrain, diverging, slope, grey, or a colour-map file
//                     typed in (coloured styles)
//   shadingRangeMin, shadingRangeMax   the values the ramp spans; blank: the
//                     data's
//   shadingName       the reference raster's name; blank: the source's and
//                     the style's
//   shadingSave       also write the picture to this GeoTIFF;
//                     shadingSaveBrowse, shadingOverwrite
//   shadingCommand    the exact line Run will run (read-only)
//   shadingPreview    run it with PREVIEW: what would be read, nothing made
//   shadingRun        run it
//   shadingReply      what it said

#include <QDialog>
#include <QString>

#include <memory>

#include "geo/terrain_dialog_support.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;

namespace katana::qt {

struct ShadingForm {
    // SURFACE <name> or RASTER <id>, as the source picker's data says it.
    QString source;
    // The verb's word: hillshade, relief, relief+hillshade, slope, plain.
    QString style = "hillshade";
    double azimuth = 315.0;
    double altitude = 45.0;
    double zFactor = 1.0;
    QString variant = "regular";
    QString ramp;
    QString rangeMin, rangeMax;
    QString name;
    QString save;
    bool overwrite = false;
};

//   RASTER SHADE <source> style=<style> [azimuth=] [altitude=] [z=] [variant=]
//                [ramp=] [range=<min>,<max>] [NAME <n>] [save=<file>] [OVERWRITE]
// The light is written for a hillshade style only, and only where it is not
// GDAL's default (315, 45, 1, regular) - never an azimuth for
// multidirectional, which lights from every side; a ramp only for a
// coloured style and when it is not the style's own. InvalidArgument naming
// the field: no source, one end of the range without the other or ends that
// are not numbers low before high, a name, ramp or path with a double quote.
[[nodiscard]] katana::core::Result<QString> shadingLine(const ShadingForm& form);

class TerrainShadingDialog final : public QDialog {
  public:
    explicit TerrainShadingDialog(TerrainDialogContext context, QWidget* parent = nullptr);
    ~TerrainShadingDialog() override;

    // Refills the sources, keeping what is chosen: done when shown.
    void reload();
    [[nodiscard]] ShadingForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const;
    void run(bool preview = false);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refresh();
    void browse();

    TerrainDialogContext context_;
    QComboBox* source_ = nullptr;
    QComboBox* style_ = nullptr;
    QDoubleSpinBox* azimuth_ = nullptr;
    QDoubleSpinBox* altitude_ = nullptr;
    QDoubleSpinBox* zFactor_ = nullptr;
    QComboBox* variant_ = nullptr;
    QComboBox* ramp_ = nullptr;
    QLineEdit* rangeMin_ = nullptr;
    QLineEdit* rangeMax_ = nullptr;
    QLineEdit* name_ = nullptr;
    QLineEdit* save_ = nullptr;
    QPushButton* saveBrowse_ = nullptr;
    QCheckBox* overwrite_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    std::unique_ptr<TerrainRun> runner_;
};

} // namespace katana::qt
