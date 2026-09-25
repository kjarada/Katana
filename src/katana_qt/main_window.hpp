#pragma once

// Desktop shell of the 2D CAD application (PLAN.MD Phase 08).
//
// The window owns the Document and nothing else of substance: every panel is a
// view that is rebuilt from the document when it reports a change, and every
// user action becomes a Command or a line for the CommandInterpreter.

#include <optional>
#include <QMainWindow>
#include <QStringList>

#include <filesystem>
#include <memory>
#include <vector>

#include "icons.hpp"
#include "katana/archive12d/customisation.hpp"
#include "katana/cad/customisation_record.hpp"
#include "katana/cad/plot.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/cad/corridor.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/log.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/reference_data.hpp"
#include "customisation/customisation_workbench.hpp"
#include "survey/survey_workbench.hpp"
#include "tools/tool_menus.hpp"
#include "sheet_editor.hpp"
#include "view_workspace.hpp"

class QAction;
class QActionGroup;
class QCloseEvent;
class QComboBox;
class QDialog;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QTableWidget;
class QTreeWidget;
class QTreeWidgetItem;
class QMenu;

class QToolBar;
class QToolButton;
class QDockWidget;
class QKeySequence;
namespace katana::qt {

class StyleManagerDialog;
class AttributeManagerDialog;
class LayerManagerDialog;
class DatasetInfoDialog;

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

    // Loads a customisation - linestyle and symbol libraries, survey code files -
    // ON TOP OF what is loaded (archive12d::mergeCustomisation; the lead's
    // D1), or in place of the loaded kinds it brings with LoadMode::Replace,
    // and reports what each file added and replaced into the message log.
    // Public because the headless --customise switch drives the same path
    // the menu item does, so what a test exercises is what a person gets.
    void applyCustomisation(
        const std::vector<std::filesystem::path>& paths,
        katana::archive12d::LoadMode mode = katana::archive12d::LoadMode::Merge);
    // The customisation the application ships with or finds beside itself,
    // loaded at startup so a survey drawing is drawn with its linestyles,
    // symbols and survey codes without anyone being asked for them.
    void loadDefaultCustomisation();

    [[nodiscard]] katana::core::Status plotDrawingToPdf(const QString& path,
                                                        katana::cad::PlotSettings settings,
                                                        bool fitToDrawing);
    // Every sheet of the project to one PDF (docs/plotting.md). A project
    // with no sheets plots one sheet fitted to the drawing, laid out for this
    // plot only and not added to the project. Public for --plot-sheets.
    [[nodiscard]] katana::core::Status plotSheetsToPdf(const QString& path);
    // What sheets are drawn from: the drawing, this window's surfaces, meshes
    // and reference layers, the set's logo and the project's fields.
    [[nodiscard]] SheetSource sheetSource() const;
    // File > Sheets: the sheet editor, built on first use and kept.
    void showSheets();

    // A styles and linetypes manager of its own, built but not shown, from
    // the Format menu's context (CustomisationWorkbench). For the headless
    // --style-manager switch, which grabs it; Format > Styles and Linetypes
    // shows the workbench's one, non-modally.
    [[nodiscard]] std::unique_ptr<StyleManagerDialog> makeStyleManager();
    // The attribute manager, built but not shown - as makeStyleManager.
    [[nodiscard]] std::unique_ptr<AttributeManagerDialog> makeAttributeManager();
    [[nodiscard]] std::unique_ptr<LayerManagerDialog> makeLayerManager();
    // GIS > Dataset Information for `path`, built but not shown, as the
    // managers above; nullptr when the file cannot be described, which has
    // been logged. For the headless --dataset-info switch.
    [[nodiscard]] std::unique_ptr<DatasetInfoDialog> makeDatasetInfo(const QString& path);
    // The GIS menu's import dialog for `path`'s kind of data - vector, raster
    // or point cloud - built but not shown. nullptr, logged, for a file that
    // cannot be described or has no such dialog. For --import-options.
    [[nodiscard]] std::unique_ptr<QDialog> makeImportOptions(const QString& path);
    // Triggers the menu item whose object name is `name`, exactly as a click
    // does. For the headless --action switch, so that a menu command is run
    // by a test through the same QAction a person clicks. NotFound for an
    // unknown name, InvalidState for a disabled item.
    [[nodiscard]] katana::core::Status triggerAction(const QString& name);
    // Selects every entity on an unlocked layer, as Edit > Select All does,
    // or just the one with this id. For the headless --attributes switch,
    // whose dialog acts on a selection.
    void selectAll();
    void selectOnly(katana::entity::EntityId id);
    // Runs `line` as if it were typed on the command line and Enter pressed.
    // For the headless --command switch, so a test can set up a drawing -
    // styles, entities, a selection - through the verbs a person types. An
    // empty line is Enter on an empty command line: the running tool's
    // Enter, or the last tool again.
    void runCommand(const QString& line);

    // Every key sequence the window's actions and menus answer to that two
    // of them share, one line each ("Ctrl+L: formatLayers, Line"); empty
    // when each is unique. Qt disables an ambiguous shortcut for BOTH
    // actions, so a clash is two keys that silently do nothing; two items of
    // one menu with the same underlined letter make that letter cycle
    // between them instead of choosing, so those are counted too ("Alt+O
    // (Format) > L"). `sequences`, when given, is set to how many distinct
    // sequences there are. For the headless --check-shortcuts switch.
    [[nodiscard]] QStringList shortcutClashes(int* sequences = nullptr) const;

  protected:
    void closeEvent(QCloseEvent* event) override;
    // Up / Down browse the command history; Esc cancels the active tool.
    bool eventFilter(QObject* watched, QEvent* event) override;

  private:
    void buildActions();
    // The GIS menu and toolbar: GDAL vector and raster, PDAL point clouds.
    // `exportAction` is File's Export Vector, shared so the two menus cannot
    // drift.
    void buildGisActions(QMenu& gisMenu, QAction* exportAction);
    // The Survey menu and toolbar; the workbench fills both (PLAN.MD 45).
    void buildSurveyActions(QMenu& surveyMenu, QAction* customiseAction,
                            QAction* replaceCustomisationAction, QAction* codeAction);
    // The Format menu and toolbar; the customisation workbench fills both.
    void buildFormatActions(QMenu& formatMenu, QAction* layersAction, QAction* customiseAction,
                            QAction* replaceCustomisationAction);
    // Draw, Modify and Annotate, menus and toolbars, generated from the tool
    // catalogue (tools/tool_menus.hpp), with Select heading the Draw toolbar.
    void buildToolActions(QMenu& drawMenu, QMenu& modifyMenu, QMenu& annotateMenu);
    // Starts catalogue tool `id` in the active plan view and gives that view
    // the keyboard, or says why it cannot.
    void startTool(const std::string& id);
    // Checks the action of tool `id` in the menus and toolbars, and Select
    // when `id` is "" (nothing runs); the command line's placeholder goes
    // back to its own when nothing runs.
    void showRunningTool(const std::string& id);
    void buildViewMenu(QMenu* viewMenu);
    // `name`, when given, becomes the action's object name: what --action and
    // QMainWindow::saveState know it by.
    [[nodiscard]] QAction* makeAction(Icon icon, const QString& text, const QString& tip,
                                      const QKeySequence& shortcut = {},
                                      const QString& name = {});
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
    // The Properties panel's Style row and the Properties toolbar's current
    // style (decision D9). Part of refreshAll, so it is deferred and
    // coalesced like every panel.
    void refreshStyleChoices();
    // The Style row's Apply: the selection's entities that are not in the
    // chosen style yet get it, in one undoable command.
    void applyPropertyStyle();

    // ---- import / export (PLAN.MD Phase 20) ---------------------------------
    void importFile();
    void loadCustomisation(katana::archive12d::LoadMode mode);
    void applySurveyCodes();
    // Before a save: the project records the names of the customisation
    // files it was drawn with (storage::ProjectMetadata::customisation).
    void recordCustomisation();
    // After an open: says which files the project records that are not
    // loaded. A warning; the project opens all the same.
    void reportMissingCustomisation();
    void reportCustomisationCoverage();
    // The options are the GIS menu's dialogs' choices; File > Import and a
    // path on the command line take the defaults.
    void importVectorFile(const std::filesystem::path& path,
                          katana::interop::VectorImportOptions options = {});
    void importArchive12dFile(const std::filesystem::path& path);
    void importRasterFile(const std::filesystem::path& path,
                          katana::interop::RasterImportOptions options = {});
    void importPointCloudFile(const std::filesystem::path& path,
                              katana::interop::PointCloudImportOptions options = {});
    void exportVectorFile();
    // Writes the drawing, or `options.entities` of it, to `path`: a vector
    // format by extension, or a 12d archive. Reports into the log; false when
    // it failed, which has been reported too.
    bool exportDrawingTo(const std::filesystem::path& path,
                         katana::interop::VectorExportOptions options);
    // A .dxf, read and written natively rather than through GDAL
    // (main_window_dxf.cpp). The export honours the options' entities,
    // layers and origin shift; the rest are GDAL's.
    void importDxfFile(const std::filesystem::path& path);
    bool exportDxfFile(const std::filesystem::path& path,
                       const katana::interop::VectorExportOptions& options);

    // ---- GIS menu: GDAL and PDAL (PLAN.MD Phases 17 and 20) ---------------
    // The imports ask for a file of their kind, describe it, and offer its
    // options before anything is read.
    void importVectorWithOptions();
    void importRasterWithOptions();
    void importPointCloudWithOptions();
    // What the three share once a file is chosen: describe it, offer the
    // dialog for its kind, import with the choices.
    void importWithOptions(const QString& path);
    void exportPointCloud();
    void exportSurfaceAsDem();
    void convertPointCloudToCopc();
    void showDatasetInformation();
    // The reference layer selected in the panel, or the only one of its kind
    // when the panel has no selection; nullptr, having said why, otherwise.
    [[nodiscard]] const katana::interop::PointCloudLayer* chooseReferenceCloud(const QString& title);
    [[nodiscard]] const katana::interop::RasterOverlay* chooseReferenceRaster(const QString& title);
    // Drops the rasters and point clouds, with the drawing they were loaded
    // beside (audit QT-17): File > New and Open must not leave the previous
    // drawing's orthophoto behind the next one.
    void clearReferenceData();
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
    // The part of the command line that is the CommandInterpreter's, for a
    // line no tool and no view verb took; `verb` is its first word, upper
    // case.
    void runInterpreterLine(const QString& line, const QString& verb);
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

    ViewWorkspace* views_ = nullptr;
    // The title bars, the minimised tray and maximise, shared by the panels
    // below and every view (dock_chrome.hpp). Owned by this window.
    DockChrome* chrome_ = nullptr;
    std::unique_ptr<SurveyWorkbench> survey_;
    // The Format menu's managers and what they share. Declared after the
    // Document, so it - and the dialogs it deletes - go first.
    std::unique_ptr<CustomisationWorkbench> format_;
    // Format > Layers, shown beside the drawing and kept between uses. It
    // holds the Document, so it is owned here - declared after document_,
    // destroyed before it - rather than left to Qt, which deletes a window's
    // children only after its members are gone.
    std::unique_ptr<LayerManagerDialog> layers_;
    // File > Sheets, kept between uses and owned here for the reason layers_ is.
    std::unique_ptr<SheetEditor> sheets_;

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
    // The active view's own readout - a section's station and elevation
    // under the cursor, a 3D view's frame time - in a PERMANENT status-bar
    // label, so that it never overwrites a prompt or an error message.
    QLabel* frameStatsLabel_ = nullptr;
    // The Properties panel's Style row, and the Properties toolbar's
    // current style for new work (D9).
    QComboBox* propertyStyle_ = nullptr;
    QToolButton* propertyStyleApply_ = nullptr;
    QComboBox* currentStyle_ = nullptr;
    // The customisation files loaded this session, in load order: what a
    // save records in the project (cad/customisation_record.hpp).
    std::vector<katana::cad::CustomisationSource> customisation_;
    // The files the last open found the project recorded but not loaded, less
    // those loaded since: a save keeps them in the record, since this session
    // cannot judge a file it never had.
    std::vector<std::string> customisationMissingAtOpen_;
    QAction* undoAction_ = nullptr;
    QAction* redoAction_ = nullptr;
    QAction* gridAction_ = nullptr;
    QAction* snapAction_ = nullptr;
    // The catalogue's tools, one action each, by id; and Select, checked
    // while no tool runs.
    tools::ToolActions toolActions_;
    QAction* selectAction_ = nullptr;
    QMenu* viewMenu_ = nullptr;
    // The four panels, by the fixed object names a saved layout keys them on:
    // LayersDock, PropertiesDock, CommandLineDock, ReferenceDataDock.
    QDockWidget* layerDock_ = nullptr;
    QDockWidget* propertyDock_ = nullptr;
    QDockWidget* commandDock_ = nullptr;
    QDockWidget* referenceDock_ = nullptr;
    std::vector<QAction*> layoutActions_;
    std::vector<QAction*> kindActions_;

    // Imported imagery and point clouds. Owned here rather than by the
    // Document so that katana_cad stays free of GDAL and PDAL, which is what
    // lets it build with -DKATANA_BUILD_IO=OFF for the sanitizer job.
    katana::interop::ReferenceData reference_;

    bool refreshingLayers_ = false;     // suppresses cellChanged while rebuilding
    bool refreshingReferences_ = false; // ditto, for the reference table
    bool refreshingStyles_ = false;     // ditto, for the current-style choice
    bool refreshPending_ = false;       // a refreshAll() is queued on the event loop
    bool headless_ = false;
    int historyCursor_ = 0;         // position while browsing command history
};

} // namespace katana::qt
