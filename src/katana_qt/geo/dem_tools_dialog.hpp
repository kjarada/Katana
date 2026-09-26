#pragma once

// Terrain > DEM > DEM Tools: the window onto RASTER MOSAIC, CLIP, FILL,
// REPROJECT, FOOTPRINT and DIFFERENCE (docs/terrain.md, "The DEM tools";
// src/katana_app/geo/dem_verbs.cpp).
//
// Non-modal, one tab per tool. Each tab's fields describe its verb's line -
// shown, as it will run, in its Command field - and its Run hands the line to
// the window's one executor (geo_dialog_support.hpp): a background job with
// progress and Cancel, logged and kept in the history like a typed line. The
// dialog computes nothing itself; the line it shows is the line an agent
// types. The rasters come from a binding picker (binding_picker.hpp): a
// reference raster, a surface or a file; the areas CLIP and DIFFERENCE take
// from the drawing from Global Modify's own scope and filter controls.
//
// The lines are made by demToolCommandLine, a pure function of what the
// fields hold, so they are tested without a window. A field left blank is left
// out and the verb's default applies.
//
// Object names (tests/qt_widgets/geo/test_dem_tools_dialog.cpp; the headless
// --dialog terrainDemTools, --fill and --press). Every tab has its <d>Source
// (a picker: <d>Kind, <d>Raster, <d>Surface, <d>Cell, <d>File, <d>Browse),
// <d>Name where it makes a raster, and <d>Command, <d>Preview, <d>Run,
// <d>Status and <d>Reply (GeoRunPanel), <d> being demMosaic, demClip, demFill,
// demReproject, demFootprint or demDifference:
//   demToolsDialog       the dialog; Terrain > DEM > DEM Tools (terrainDemTools)
//   demToolsTabs         the tabs, demToolsMosaic ... demToolsDifference
//   demMosaicTiles       the tiles; demMosaicAdd adds the source picked,
//                        demMosaicRemove the tile selected. With none listed
//                        the source picked is the one (a folder, a pattern)
//   demMosaicResolution  resolution=; demMosaicSave the file SAVE writes and
//                        keeps
//   demClipByArea, demClipArea          clip to a box, x0,y0,x1,y1 ...
//   demClipByBoundaries, demClipBoundaryScope ...  or to the closed
//                        boundaries a scope takes (ScopeFilterWidget,
//                        prefix demClipBoundary)
//   demFillDistance, demFillSmoothing, demFillStrategy
//   demReprojectCrs, demReprojectLike, demReprojectFrom, demReprojectResampling,
//   demReprojectCell
//   demFootprintLayer    the layer the footprint is drawn on (TO LAYER)
//   demDifferenceSecond  the second raster (a picker: demDifferenceSecondKind ...)
//   demDifferenceLimit, demDifferenceWithinScope ...  only within the closed
//                        boundaries a scope takes (prefix demDifferenceWithin)
//   demDifferenceResampling

#include <QDialog>
#include <QString>
#include <QStringList>

#include <array>
#include <memory>
#include <utility>
#include <vector>

#include "geo/binding_picker.hpp"
#include "geo/geo_dialog_support.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QListWidget;
class QRadioButton;
class QShowEvent;
class QTabWidget;

namespace katana::qt {

class DocumentWatcher;

// The tools, in the order of the tabs.
enum class DemTool { Mosaic, Clip, Fill, Reproject, Footprint, Difference };
inline constexpr int kDemToolCount = 6;

// The verb's second word: "MOSAIC", "CLIP" ...
[[nodiscard]] const char* demToolWord(DemTool tool);

// What one tab's fields hold, as typed.
struct DemToolForm {
    DemTool tool = DemTool::Mosaic;
    // The source clauses' words, in order: a mosaic's tiles, a difference's
    // first and second; one for the rest.
    QStringList sources;
    // CLIP: AREA x0,y0,x1,y1 or scope words; DIFFERENCE: scope words or
    // empty. regionError says why the controls give none.
    QString region;
    QString regionError;
    // key=value options, as typed; a blank value is left out.
    std::vector<std::pair<QString, QString>> options;
    QString save;  // MOSAIC: SAVE <file>
    QString name;  // NAME
    QString layer; // FOOTPRINT: TO LAYER
};

// The line `form` describes, exactly as it would be typed:
//   RASTER MOSAIC <source>... [resolution=<r>] [SAVE <file>] [NAME <name>]
//   RASTER CLIP <source> <region> [NAME <name>]
//   RASTER FILL <source> [distance=<cells>] [smoothing=<n>] [strategy=<s>] [NAME <name>]
//   RASTER REPROJECT <source> [crs=<crs>] [like=<raster>] [from=<crs>]
//                    [resampling=<m>] [cell=<m>] [NAME <name>]
//   RASTER FOOTPRINT <source> [TO LAYER <path>]
//   RASTER DIFFERENCE <first> <second> [<scope>] [resampling=<m>] [NAME <name>]
// InvalidArgument naming the field for a source or region the controls cannot
// say, an option value with a blank, an '=' or a double quote (an option's
// value is one word), and a number that does not read where one is asked for
// (distance, smoothing, cell).
[[nodiscard]] katana::core::Result<QString> demToolCommandLine(const DemToolForm& form);

class DemToolsDialog final : public QDialog {
  public:
    explicit DemToolsDialog(GeoDialogContext context, QWidget* parent = nullptr);
    ~DemToolsDialog() override;
    DemToolsDialog(const DemToolsDialog&) = delete;
    DemToolsDialog& operator=(const DemToolsDialog&) = delete;

    void showTool(DemTool tool);
    [[nodiscard]] DemTool tool() const;
    [[nodiscard]] DemToolForm form(DemTool tool) const;
    [[nodiscard]] GeoRunPanel& runPanel(DemTool tool) const;
    [[nodiscard]] BindingPicker& source(DemTool tool) const;
    // Refills the pickers' rasters and surfaces, the scopes' layers and
    // views, and the rasters demReprojectLike offers, keeping what is chosen.
    void reload();

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refresh();

    GeoDialogContext context_;
    QTabWidget* tabs_ = nullptr;
    std::array<BindingPicker*, kDemToolCount> sources_{};
    std::array<GeoRunPanel*, kDemToolCount> panels_{};
    std::array<QLineEdit*, kDemToolCount> names_{};
    BindingList* mosaicTiles_ = nullptr;
    QComboBox* resolution_ = nullptr;
    QLineEdit* save_ = nullptr;
    QRadioButton* clipByArea_ = nullptr;
    QRadioButton* clipByBoundaries_ = nullptr;
    QLineEdit* clipArea_ = nullptr;
    ScopeFilterWidget* clipBoundaries_ = nullptr;
    QLineEdit* fillDistance_ = nullptr;
    QLineEdit* fillSmoothing_ = nullptr;
    QComboBox* fillStrategy_ = nullptr;
    QLineEdit* reprojectCrs_ = nullptr;
    QComboBox* reprojectLike_ = nullptr;
    QLineEdit* reprojectFrom_ = nullptr;
    QComboBox* reprojectResampling_ = nullptr;
    QLineEdit* reprojectCell_ = nullptr;
    QLineEdit* footprintLayer_ = nullptr;
    BindingPicker* differenceSecond_ = nullptr;
    QCheckBox* differenceLimit_ = nullptr;
    ScopeFilterWidget* differenceWithin_ = nullptr;
    QComboBox* differenceResampling_ = nullptr;
    // Last, so it goes first: no delivery reaches a half-destroyed dialog.
    std::unique_ptr<DocumentWatcher> watcher_;
};

// Opens (making it the first time) the window's DEM Tools dialog.
DemToolsDialog& showDemToolsDialog(GeoWorkbench& workbench, QWidget& window);

} // namespace katana::qt
