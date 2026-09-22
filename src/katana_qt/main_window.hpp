#pragma once

// Desktop shell of the 2D CAD application (PLAN.MD Phase 08).
//
// The window owns the Document and nothing else of substance: every panel is a
// view that is rebuilt from the document when it reports a change, and every
// user action becomes a Command or a line for the CommandInterpreter.

#include <optional>
#include <QMainWindow>

#include <filesystem>
#include <memory>
#include <vector>

#include "icons.hpp"
#include "katana/cad/plot.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/cad/corridor.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/log.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/interop/reference_data.hpp"
#include "viewport_container.hpp"

class QAction;
class QActionGroup;
class QCloseEvent;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QTableWidget;
class QTreeWidget;
class QTreeWidgetItem;
class QMenu;

class QToolBar;
class QDockWidget;
class QKeySequence;
namespace katana::qt {

class StyleManagerDialog;
class AttributeManagerDialog;
class LayerManagerDialog;

class MainWindow final : public QMainWindow {
  public:
    explicit MainWindow(QWidget* parent = nullptr);

    // Opens a project given on the command line.
    void openProject(const QString& directory);
    // Imports a data file given on the command line, routed by its extension
    // exactly as File > Import does.
    void importPath(const QString& path);
    // Plots the drawing to `path` on the active plan viewport. With
    // `fitToDrawing` the scale is the first standard one the drawing fits at
    // and the sheet is centred on it; otherwise `settings.scaleDenominator`
    // is used about the current view centre. Public so that the application
    // can plot headlessly from the command line, which is what lets a test
    // open the result. Fails with InvalidState when there is no plan
    // viewport, and with whatever the fit or the plot refuses.
    // A headless session (--plot, --screenshot) has nobody to answer a
    // question: anything that would open a modal dialog logs its advice and
    // takes the non-destructive default instead - imported survey coordinates
    // are kept, never shifted. Without this an import in a scripted run
    // waits on a box no one can see, which is how the first headless 12da
    // import hung.
    void setHeadless(bool headless) { headless_ = headless; }

    // Refreshes the panels on the next pass of the event loop, once, however
    // many times it is asked before then. See the listener in the constructor
    // for why it must not happen synchronously.
    void scheduleRefresh();
    // A warning box, or in a headless session a line in the log.
    void warnUser(const QString& title, const QString& text);

    // Flips the visibility box of `layer` in the layer panel THROUGH THE
    // WIDGET, as a click does, and reports whether the application survived
    // it properly: the tree must not have been rebuilt while its own signal
    // was on the stack, and the document must show the change afterwards.
    // For the headless --toggle-layer switch, which exists because the first
    // click on that box crashed the application and nothing had tested it.
    [[nodiscard]] katana::core::Status toggleLayerThroughPanel(const QString& layer);

    // Loads a 12d customisation - linestyle and symbol libraries, mapfiles -
    // and reports what it found into the message log. Public because the
    // headless --customise switch drives the same path the menu item does, so
    // what a test exercises is what a person gets.
    void applyCustomisation(const std::vector<std::filesystem::path>& paths);

    [[nodiscard]] katana::core::Status plotDrawingToPdf(const QString& path,
                                                        katana::cad::PlotSettings settings,
                                                        bool fitToDrawing);

    // The styles and linetypes manager, built but not shown. The menu shows
    // it modally; the headless --style-manager switch grabs it instead, so
    // that the dialog is built and painted by a test.
    [[nodiscard]] std::unique_ptr<StyleManagerDialog> makeStyleManager();
    // The attribute manager, built but not shown - as makeStyleManager.
    [[nodiscard]] std::unique_ptr<AttributeManagerDialog> makeAttributeManager();
    [[nodiscard]] std::unique_ptr<LayerManagerDialog> makeLayerManager();
    // Selects every entity on an unlocked layer, as Edit > Select All does,
    // or just the one with this id. For the headless --attributes switch,
    // whose dialog acts on a selection.
    void selectAll();
    void selectOnly(katana::entity::EntityId id);

  protected:
    void closeEvent(QCloseEvent* event) override;
    // Up / Down browse the command history; Esc cancels the active tool.
    bool eventFilter(QObject* watched, QEvent* event) override;

  private:
    void buildActions();
    void buildViewMenu(QMenu* viewMenu);
    [[nodiscard]] QAction* makeAction(Icon icon, const QString& text, const QString& tip,
                                      const QKeySequence& shortcut = {});
    [[nodiscard]] QToolBar* makeToolBar(const QString& title, Qt::ToolBarArea area);
    void refreshViewMenu();

    // ---- terrain, 3D and sections (PLAN.MD Phases 14, 15, 21) -------------
    void buildSurfaceFromPointCloud();
    void buildSurfaceFromRaster();
    void buildSurfaceFromDrawing();
    void addSurface(std::string name, katana::terrain::TinSurface surface);
    // Session data like a surface: not an entity, not undoable, and drawn in
    // 3D with its footprint in plan.
    void addMesh(std::string name, katana::geometry::TriangleMesh mesh,
                 katana::render::Rgba color, std::vector<katana::render::Rgba> faceColors);
    void cutSectionAlongSelection();
    void cutSectionAlongAlignment();
    void corridorQuantities();
    void corridorSurface();
    void plotToPdf();

    // What both corridor commands ask for: an alignment with a design
    // profile, a ground surface, the assembly and the interval, from one
    // dialog. nullopt when the user cancels or nothing qualifies, which has
    // already been logged.
    struct CorridorRequest {
        katana::geometry::SolvedAlignment alignment;
        katana::geometry::SolvedProfile profile;
        const katana::terrain::TinSurface* ground = nullptr;
        QString alignmentName;
        QString surfaceName;
        katana::cad::Assembly assembly;
        double crossfallPercent = 0.0;
        double interval = 10.0;
    };
    std::optional<CorridorRequest> askCorridor(const QString& title);
    // The part both section commands share: gather the visible surfaces, cut
    // along `alignment`, show the result. `along` names the source for the
    // log line.
    // `profile`, when given, is added to the section as a design series named
    // `profileName`, so the view shows design against ground.
    void cutSectionAlong(katana::geometry::Polyline2 alignment, const QString& along,
                         const katana::geometry::SolvedProfile* profile = nullptr,
                         const std::string& profileName = {});
    void setVerticalExaggeration();
    void buildDocks();
    void buildReferenceDock();
    void buildStatusBar();

    void refreshAll();
    void refreshTitle();
    void refreshLayers();
    void refreshProperties();
    void refreshReferences();

    // ---- import / export (PLAN.MD Phase 20) ---------------------------------
    void importFile();
    void loadCustomisation();
    void applySurveyCodes();
    void importVectorFile(const std::filesystem::path& path);
    void importArchive12dFile(const std::filesystem::path& path);
    void importRasterFile(const std::filesystem::path& path);
    void importPointCloudFile(const std::filesystem::path& path);
    void exportVectorFile();
    void removeSelectedReference();
    void zoomToSelectedReference();
    void onReferenceCellChanged(int row, int column);

    // Returns false when the user cancels (unsaved changes).
    [[nodiscard]] bool confirmDiscard();
    void newDocument();
    void openDocument();
    bool saveDocument();
    bool saveDocumentAs();

    void runCommandLine();
    void logMessage(const QString& text, bool isError = false);
    void addLayer();
    void addChildLayer();
    void renameSelectedLayer();
    void deleteCurrentLayer();
    void onLayerItemChanged(QTreeWidgetItem* item, int column);
    void onLayerItemDoubleClicked(QTreeWidgetItem* item, int column);
    // Full path of the selected layer, empty when nothing is selected. The
    // path is read from the item rather than rebuilt by walking parents: it is
    // the layer's identity everywhere else and must have one definition.
    [[nodiscard]] std::string selectedLayerPath() const;

    katana::core::Logger logger_;
    katana::cad::Document document_{&logger_};
    katana::cad::Document::ListenerHandle documentListener_;
    katana::cad::CommandInterpreter interpreter_{document_};

    ViewportContainer* views_ = nullptr;

    // Surfaces shown in the 3D and section views. Built on demand from
    // imported point clouds, rasters and drawing geometry, and owned here for
    // the same reason reference data is: katana_cad must stay free of GDAL and
    // PDAL so it still builds with -DKATANA_BUILD_IO=OFF.
    // unique_ptr, NOT a vector of values: sceneSurfaces_ holds raw pointers
    // into this store, and a vector of values would move every surface - and
    // dangle every one of those pointers - the moment it reallocated.
    std::vector<std::unique_ptr<katana::terrain::TinSurface>> surfaceStore_;
    std::vector<katana::cad::SceneSurface> sceneSurfaces_;
    // Meshes (12d trimeshes), held the same way and for the same reasons.
    std::vector<std::unique_ptr<katana::geometry::TriangleMesh>> meshStore_;
    std::vector<katana::cad::SceneMesh> sceneMeshes_;
    QTreeWidget* layerTree_ = nullptr;
    QTableWidget* referenceTable_ = nullptr;
    QTableWidget* propertyTable_ = nullptr;
    QPlainTextEdit* commandLog_ = nullptr;
    QLineEdit* commandInput_ = nullptr;
    QLabel* coordinateLabel_ = nullptr;
    QLabel* snapLabel_ = nullptr;
    QLabel* layerLabel_ = nullptr;
    QAction* undoAction_ = nullptr;
    QAction* redoAction_ = nullptr;
    QAction* gridAction_ = nullptr;
    QAction* snapAction_ = nullptr;
    QActionGroup* toolGroup_ = nullptr;
    QMenu* viewMenu_ = nullptr;
    QDockWidget* referenceDock_ = nullptr;
    std::vector<QAction*> layoutActions_;
    std::vector<QAction*> kindActions_;

    // Imported imagery and point clouds. Owned here rather than by the
    // Document so that katana_cad stays free of GDAL and PDAL, which is what
    // lets it build with -DKATANA_BUILD_IO=OFF for the sanitizer job.
    katana::interop::ReferenceData reference_;

    bool refreshingLayers_ = false;     // suppresses cellChanged while rebuilding
    bool refreshingReferences_ = false; // ditto, for the reference table
    bool refreshPending_ = false;       // a refreshAll() is queued on the event loop
    bool headless_ = false;
    int historyCursor_ = 0;         // position while browsing command history
};

} // namespace katana::qt
