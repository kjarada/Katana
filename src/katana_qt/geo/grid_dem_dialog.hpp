#pragma once

// Terrain > DEM > Grid Points to DEM: the window onto RASTER GRID
// (docs/terrain.md, "Gridding points to a DEM"; src/katana_app/geo/grid_verbs.cpp).
//
// The dialog is non-modal and grids nothing itself. Its fields describe a
// RASTER GRID line - shown, as it will run, in gridCommand - and Run hands the
// line to the window's one executor (geo_dialog_support.hpp), so it runs as a
// background job with progress and Cancel, is logged and kept in the history
// like a typed line, and the DEM it makes is a reference raster whose
// derivation is that line. There is nothing the dialog does that an agent
// cannot do by typing the same line.
//
// The points come from the drawing's scope and filter, Global Modify's own
// controls (ScopeFilterWidget): the points in scope, and the vertices of the
// lines and areas, each with a height at every vertex - or with a number in
// the property gridZ names.
//
// The line is made by gridDemCommandLine, a pure function of what the fields
// hold, so it is tested without a window. A field left blank is left out (the
// verb's default applies: the linear method, a cell suggested for the extent,
// the heights of the geometry, the scope's bounds, the name dem). What the
// verb refuses - a cell of 0, a power for the linear method - it refuses when
// the line runs, in its own words.
//
// Object names (tests/qt_widgets/geo/test_grid_dem_dialog.cpp; the headless
// --dialog terrainGrid, --fill and --press):
//   gridDemDialog   the dialog; Terrain > DEM > Grid Points to DEM
//                   (terrainGrid) opens it
//   gridScope       the points' scope and filter (ScopeFilterWidget: gridScopeDrawing,
//                   gridScopeLayers, gridLayers, gridTypePoint, gridFilterLayer ...)
//   gridMethod      GDAL's gridding method, from its catalogue (method=)
//   gridCell        the cell size, metres (cell=)
//   gridSize        or the grid's columns x rows (size=)
//   gridZ           where the heights come from: geometry, or a property (z=)
//   gridExtent      x0,y0,x1,y1; blank for the scope's bounds (extent=)
//   gridPower       the inverse-distance power (power=; invdist methods only)
//   gridRadius      the search radius, metres (radius=; methods that take one)
//   gridName        the reference raster's name (NAME)
//   gridToSurface   keep the grid as a named surface instead (TO SURFACE),
//                   offered once the surface store takes a raster result
//   gridCommand, gridPreview, gridRun, gridStatus, gridReply   (GeoRunPanel)

#include <QDialog>
#include <QString>

#include <memory>

#include "geo/geo_dialog_support.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QShowEvent;

namespace katana::qt {

class DocumentWatcher;

// What the dialog's fields hold, as typed.
struct GridDemForm {
    // The scope and filter words (ScopeFilterWidget::verbWords), or why the
    // controls say none.
    QString scope;
    QString scopeError;
    QString method;
    QString cell;
    QString size;
    // "geometry" or blank for the drawing's heights; else a property's name.
    QString z;
    QString extent;
    QString power;
    QString radius;
    QString name;
    bool toSurface = false;
};

// The RASTER GRID line `form` describes, exactly as it would be typed:
//   RASTER GRID <scope> [method=<m>] [cell=<m> | size=<c>x<r>] [z=<property>]
//               [extent=x0,y0,x1,y1] [power=<p>] [radius=<m>] [NAME <name>]
//               [TO SURFACE <name>]
// A blank field is left out. InvalidArgument naming the field for a scope the
// controls cannot say, a number that does not read (a comma is not a decimal
// point, as it is not on the command line), a size that is not columns x
// rows, an extent that is not four numbers, a property name or method with a
// blank or an '=' in it (an option's value is one word), and a double quote.
[[nodiscard]] katana::core::Result<QString> gridDemCommandLine(const GridDemForm& form);

class GridDemDialog final : public QDialog {
  public:
    explicit GridDemDialog(GeoDialogContext context, QWidget* parent = nullptr);
    ~GridDemDialog() override;
    GridDemDialog(const GridDemDialog&) = delete;
    GridDemDialog& operator=(const GridDemDialog&) = delete;

    [[nodiscard]] GridDemForm form() const;
    [[nodiscard]] ScopeFilterWidget& scopeControls() const { return *scope_; }
    [[nodiscard]] GeoRunPanel& runPanel() const { return *panel_; }
    // Refills the scope's layers and views and the properties gridZ offers,
    // keeping what is chosen.
    void reload();

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void methodChosen();
    void refresh();

    GeoDialogContext context_;
    ScopeFilterWidget* scope_ = nullptr;
    QComboBox* method_ = nullptr;
    QLineEdit* cell_ = nullptr;
    QLineEdit* size_ = nullptr;
    QComboBox* z_ = nullptr;
    QLineEdit* extent_ = nullptr;
    QLineEdit* power_ = nullptr;
    QLineEdit* radius_ = nullptr;
    QLineEdit* name_ = nullptr;
    QCheckBox* toSurface_ = nullptr;
    GeoRunPanel* panel_ = nullptr;
    // Last, so it goes first: no delivery reaches a half-destroyed dialog.
    std::unique_ptr<DocumentWatcher> watcher_;
};

// Opens (making it the first time) the window's Grid Points to DEM dialog.
GridDemDialog& showGridDemDialog(GeoWorkbench& workbench, QWidget& window);

} // namespace katana::qt
