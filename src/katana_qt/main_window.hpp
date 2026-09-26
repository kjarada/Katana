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
#include <span>
#include <vector>

#include "alignment_manager.hpp"
#include "annotation/annotation_workbench.hpp"
#include "command_reference_dialog.hpp"
#include "command_runner.hpp"
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
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/reference_data.hpp"
#include "customisation/customisation_workbench.hpp"
#include "geo/geo_workbench.hpp"
#include "gis_online.hpp"
#include "keyboard_shortcuts_dialog.hpp"
#include "script_runner.hpp"
#include "import_placement.hpp"
#include "survey/survey_workbench.hpp"
#include "survey/utility_workbench.hpp"
#include "tools/tool_menus.hpp"
#include "plotting/plot_drawing_dialog.hpp"
#include "plotting/plot_output.hpp"
#include "plotting/view_image_export.hpp"
#include "project_crs_dialog.hpp"
// Whole, not declared: selectById_ is destroyed wherever the window is.
#include "select_by_id_dialog.hpp"
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
class DrawingSummaryDialog;

class MainWindow final : public QMainWindow {
  public:
    explicit MainWindow(QWidget* parent = nullptr);

    // Opens a project given on the command line.
    void openProject(const QString& directory);
    // Imports a data file given on the command line: the IMPORT line File >
    // Import would make, run through the one executor (runVerbLine), so it is
    // logged, and undone, as if typed. With a `placement` - LOCAL, ALONGSIDE
    // or OFFSET=dE,dN - vector data, a .12da archive or a DXF is moved as one
    // piece (cad/import_placement.hpp), and nobody is asked where to put it;
    // a raster or a point cloud refuses it by name, being reference data
    // drawn at its own coordinates.
    void importPath(const QString& path, const katana::cad::ImportPlacement& placement = {});
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
    // Every sheet of the project to one PDF (docs/plotting.md) in the set's
    // page setup. A project with no sheets plots one sheet fitted to the
    // drawing, laid out for this plot only and not added to the project.
    [[nodiscard]] katana::core::Status plotSheetsToPdf(const QString& path);
    // What File > Plot Sheets and --plot-sheets plot: the project's sheets,
    // or, when it has none, one sheet fitted to the drawing, laid out for
    // this plot only and not added to the project (logged).
    [[nodiscard]] katana::core::Result<katana::cad::plotting::SheetSet> sheetsToPlot();
    // Plots `set` as `request` asks (plotting/plot_output.hpp) from this
    // window's source, logging each problem and the summary. Public for
    // --plot-sheets, whose switches make the request.
    [[nodiscard]] katana::core::Result<PlotReport>
    plotSheets(const katana::cad::plotting::SheetSet& set, const PlotRequest& request);
    // The drawing, read only: for --sheets-json, which writes its sheets.
    [[nodiscard]] const katana::cad::Document& document() const { return document_; }
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
    // or point cloud, or for a DXF or a .12da the placement step File >
    // Import asks (ImportPlacementDialog) - built but not shown. nullptr,
    // logged, for a file that cannot be described or has no such dialog. For
    // --import-options.
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
    // Edit > Select by ID (select_by_id_dialog.hpp), made the first time and
    // kept, as Format > Layers is.
    void showSelectById();
    // File > Project Coordinate System and the status bar's CRS button
    // (project_crs_dialog.hpp): non-modal, so a headless run fills it by its
    // object names instead of hanging on it; made afresh when it is opened
    // from closed, or for a place, so it starts from the project as it is now.
    void showProjectCrs(std::optional<std::pair<double, double>> place = std::nullopt);
    // Runs `line` as if it were typed on the command line and Enter pressed.
    // For the headless --command switch, so a test can set up a drawing -
    // styles, entities, a selection - through the verbs a person types. An
    // empty line is Enter on an empty command line: the running tool's
    // Enter, or the last tool again. False when the line logged an error, so
    // a headless run can stop at a command that was refused.
    bool runCommand(const QString& line);
    // The window's one executor for a dialog (command_runner.hpp): `line`
    // echoed in the command log and run by the dispatcher a typed line goes
    // to - but never offered to a running tool, and what is being typed on
    // the command line is left alone - with what it logged returned. An empty
    // line runs nothing and is not ok.
    VerbOutcome runVerbLine(const QString& line);
    // runVerbLine as a CommandRunner, for the workbenches to hand their
    // dialogs.
    [[nodiscard]] CommandRunner commandRunner();

    // Every key sequence the window's actions and menus answer to that two
    // of them share, one line each ("Ctrl+L: formatLayers, Line"); empty
    // when each is unique. Qt disables an ambiguous shortcut for BOTH
    // actions, so a clash is two keys that silently do nothing; two items of
    // one menu with the same underlined letter make that letter cycle
    // between them instead of choosing, so those are counted too ("Alt+O
    // (Format) > L"). `sequences`, when given, is set to how many distinct
    // sequences there are. For the headless --check-shortcuts switch.
    [[nodiscard]] QStringList shortcutClashes(int* sequences = nullptr) const;
    // Every key those actions and shortcuts answer to, with the menu path
    // its command is under, in the menus' order: Help > Keyboard Shortcuts'
    // table. The keys are the ones shortcutClashes counts.
    [[nodiscard]] std::vector<ShortcutRow> shortcutRows() const;

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
    // A right-click in a plan view with no tool running: the shortcut menu
    // (plan_context_menu.hpp) for the selection, at `globalPos`.
    void showPlanContextMenu(const QPoint& globalPos);
    // After Select by ID: the selection framed in the active plan view when
    // `zoom`, and the Properties panel brought forward to show it.
    void showSelection(bool zoom);
    // A double click on entity `id` with no tool running: it alone selected,
    // and its editor opened - a text's or a label's where the window has
    // one, the Properties panel otherwise.
    void editDoubleClicked(katana::entity::EntityId id);
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
    // sceneSurfaces_ made again from surfaceStore_ when it has changed, each
    // surface keeping how it was shown.
    void syncSceneSurfaces();
    // Session data like a surface: not an entity, not undoable, and drawn in
    // 3D with its footprint in plan.
    void addMesh(std::string name, katana::geometry::TriangleMesh mesh,
                 katana::render::Rgba color, std::vector<katana::render::Rgba> faceColors);
    // Terrain > Alignment Manager: built on first use and kept, non-modal.
    void showAlignmentManager();
    void cutSectionAlongSelection();
    void cutSectionAlongAlignment();
    void corridorQuantities();
    void corridorSurface();
    // File > Plot to PDF: the dialog, made the first time and kept, which
    // runs the PLOT line.
    void plotToPdf();
    // File > Export View as Image, kept likewise; it runs SNAPSHOT.
    void showViewImageExport();
    // The two dialogs' files, suggested afresh for the drawing as it is now:
    // at their first showing, and whenever another drawing is opened or this
    // one saved somewhere (fileListener_).
    void suggestPlotFiles();
    // What the two dialogs ask before writing over a file: the person, in a
    // box; a headless run writes it, as the verbs do.
    bool confirmReplaceFile(const QString& title, const QString& path);
    // SNAPSHOT: the active plan view painted afresh at the size asked, or the
    // 3D view grabbed and scaled; written, or put on the clipboard, and a
    // record logged (plotting/view_image_export.hpp).
    void snapshotView(const SnapshotRequest& request);

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
    // View > Vertical Exaggeration: the factor asked for in a box, then the
    // EXAGGERATION line run through the one executor; headless, refused with
    // the line to type instead.
    void askVerticalExaggeration();
    // EXAGGERATION <factor>: elevations in the 3D and section views times
    // `factor`, about the middle of the data.
    void setVerticalExaggeration(double factor);
    void buildDocks();
    void buildReferenceDock();
    void buildStatusBar();

    void refreshAll();
    void refreshTitle();
    // The Edit toolbar's Undo and Redo lists, from the history: the k-th
    // entry (undoStepK, redoStepK) runs UNDO k or REDO k.
    void refreshHistoryMenus();
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
    // The GIS menu's import dialogs' choices, which have no IMPORT words yet
    // (docs/interop.md, "Not done"); File > Import, a path on the command line
    // and a typed IMPORT are the executor's (geo/import_verb.cpp).
    // `placement` is where the data lands, as importPath says; Keep asks when
    // it is far from the drawing (decideImportPlacement, import_placement.hpp).
    void importVectorFile(const std::filesystem::path& path,
                          katana::interop::VectorImportOptions options = {},
                          const katana::cad::ImportPlacement& placement = {});
    // File > Import's step for a DXF or a .12da archive, whose only choice
    // is where it lands (ImportPlacementDialog), and then the IMPORT line
    // it makes, through runVerbLine.
    void importWithPlacement(const QString& path);
    void importRasterFile(const std::filesystem::path& path,
                          katana::interop::RasterImportOptions options = {});
    void importPointCloudFile(const std::filesystem::path& path,
                              katana::interop::PointCloudImportOptions options = {});
    void exportVectorFile();
    // Writes the drawing, or `options.entities` of it, to `path`: a vector
    // format by extension, or a 12d archive - the Export Vector dialog's
    // choices, which have no EXPORT words yet; a typed EXPORT is the
    // executor's (geo/export_verb.cpp). Reports into the log; false when it
    // failed, which has been reported too.
    bool exportDrawingTo(const std::filesystem::path& path,
                         katana::interop::VectorExportOptions options);
    // decideImportPlacement for this window: where data read at `incoming`
    // lands in the drawing as it is now, asked or logged.
    [[nodiscard]] PlacementDecision placeImport(const katana::cad::ImportPlacement& placement,
                                                const katana::geometry::Box2& incoming);
    // A .dxf written natively rather than through GDAL (main_window_dxf.cpp),
    // with the dialog's entities, layers and origin shift.
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
    // GIS > Convert Point Cloud to COPC: asks for the two files, then runs the
    // COPC line they make through runVerbLine - a job - and when it has
    // converted, offers to import the result.
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
    // Drops the surfaces and meshes the same way and for the same reason: they
    // are session data beside the drawing, not in it, so replacing the
    // drawing left the previous one's TIN in the 3D view.
    void clearSceneData();
    void removeSelectedReference();
    void zoomToSelectedReference();
    void onReferenceCellChanged(int row, int column);

    // Returns false when the user cancels (unsaved changes).
    [[nodiscard]] bool confirmDiscard();
    // In a headless session, logs that no file dialog opens and names `verb`,
    // the line that does the same without one, and returns true: what File >
    // Open, Save As, Import, Export Vector and Load Customisation do there.
    bool refuseFileDialog(const QString& verb);
    void newDocument();
    void openDocument();
    bool saveDocument();
    bool saveDocumentAs();

    // Enter on the command line: what the field holds, one line (runTypedLine)
    // or several - a paste, which a tool waiting for text takes a line at a
    // time (typeLinesIntoTool) and anything else runs as a script
    // (runPastedLines).
    void runCommandLine();
    // One typed line: echoed, then offered to the workbenches' verbs, to a
    // running tool and to dispatchLine, in that order.
    void runTypedLine(const QString& line);
    // Several lines given while a tool waits for text - a text's lines, pasted
    // at its prompt: each typed in turn, blank lines left out, so none of
    // them is run as a command.
    void typeLinesIntoTool(const QString& text);
    // The geoprocessing verbs (GDAL ...), the ONLINE and the UTILITY verbs,
    // run by their workbenches; false leaves the line to whoever asked.
    bool runWorkbenchLine(const QString& line);
    // Who a line came from. Only a person's typed line starts a tool by a
    // bare word; a script's, a paste's or a dialog's is the interpreter's, as
    // katana_cli runs the same line.
    enum class LineSource { Typed, Executor };
    // Everything a line can be once no tool took it: a '#' comment, a view
    // verb (ZOOM, GRID, SNAP, EXAGGERATION), the window's own verbs (SCRIPT,
    // CUSTOMISE, IMPORT, EXPORT, INFO <file>, REFS, COPC, PLOTSHEETS, PLOT,
    // SNAPSHOT, HELP), a tool's alias when typed, or the interpreter's.
    // Shared by the typed line and runVerbLine, so the two differ only where
    // `source` says.
    void dispatchLine(const QString& line, LineSource source);
    // The part of the command line that is the CommandInterpreter's, for a
    // line no tool and no view verb took; `verb` is its first word, upper
    // case.
    void runInterpreterLine(const QString& line, const QString& verb);
    // SCRIPT <file> [CONTINUE] (script_runner.hpp): the file's lines through
    // runVerbLine, stopping at the first refused unless `continueOnError`,
    // and the record the run ends with logged. A script already running is
    // refused, so one cannot run itself. Remembered in Recent Scripts, except
    // in a headless session.
    void runScript(const QString& path, bool continueOnError);
    // Several lines at once on the command line - pasted - run as a script
    // that stops at the first refused.
    void runPastedLines(const QString& text);
    // What the two share: the progress dialog (not headless), the run, and
    // the record and the reason it stopped, logged. `name` is the script's
    // path, empty for pasted lines.
    void runLines(const std::vector<ScriptLine>& lines, const QString& name, bool continueOnError);
    // File > Run Script: the dialog, made the first time and kept.
    void showRunScript();
    // File > Recent Scripts, rebuilt from the settings.
    void refreshRecentScripts();
    // Help > Command Reference, made the first time and kept, brought
    // forward at `section` when one is named (Help > Sheets and Plotting
    // Commands: "Sheets").
    void showCommandReference(const QString& section);
    // Help > Keyboard Shortcuts, made the first time and kept.
    void showKeyboardShortcuts();
    // File > Drawing Summary, made the first time and kept.
    void showDrawingSummary();
    // Format > Styles and Linetypes on its Missing chip, searching for `name`:
    // where the summary sends a name no loaded library defines.
    void showMissingInStyles(const QString& name);
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
    // suggestPlotFiles, on a new, opened or saved drawing.
    katana::cad::Document::ListenerHandle fileListener_;
    katana::cad::CommandInterpreter interpreter_{document_};

    ViewWorkspace* views_ = nullptr;
    // The title bars, the minimised tray and maximise, shared by the panels
    // below and every view (dock_chrome.hpp). Owned by this window.
    DockChrome* chrome_ = nullptr;
    std::unique_ptr<SurveyWorkbench> survey_;
    // The Format menu's managers and what they share. Declared after the
    // Document, so it - and the dialogs it deletes - go first.
    std::unique_ptr<CustomisationWorkbench> format_;
    // Format > Text Styles, Label Styles and Rules, and the annotation scale
    // (docs/annotation.md). After the Document for the reason format_ is.
    std::unique_ptr<AnnotationWorkbench> annotation_;
    // GIS > Online Data and the ONLINE verbs (gis_online.hpp). Declared after
    // the Document, so it and its dialog go first.
    std::unique_ptr<OnlineDataWorkbench> online_;
    // Survey > Subsurface Utilities (AS 5488) and the framing after a UTILITY
    // DRAW (survey/utility_workbench.hpp). After the Document for the reason
    // online_ is.
    std::unique_ptr<UtilityWorkbench> utilities_;
    // Format > Layers, shown beside the drawing and kept between uses. It
    // holds the Document, so it is owned here - declared after document_,
    // destroyed before it - rather than left to Qt, which deletes a window's
    // children only after its members are gone.
    std::unique_ptr<LayerManagerDialog> layers_;
    // File > Sheets, kept between uses and owned here for the reason layers_ is.
    std::unique_ptr<SheetEditor> sheets_;
    // Edit > Select by ID, kept between uses for the reason layers_ is.
    std::unique_ptr<SelectByIdDialog> selectById_;
    // Terrain > Alignment Manager, kept between uses and owned here for the
    // reason layers_ is.
    std::unique_ptr<AlignmentManagerDialog> alignments_;
    // File > Project Coordinate System, owned here for the reason layers_ is.
    std::unique_ptr<ProjectCrsDialog> projectCrs_;

    // Surfaces shown in the 3D and section views. Built on demand from
    // imported point clouds, rasters and drawing geometry, and owned here for
    // the same reason reference data is: katana_cad must stay free of GDAL and
    // PDAL so it still builds with -DKATANA_BUILD_IO=OFF.
    // The one store of named surfaces (terrain/surface_store.hpp) that the
    // geoprocessing verbs' SURFACE <name> reads too; each surface is shared
    // and immutable, so sceneSurfaces_ may point into it and a background
    // job may read one while the views draw it. sceneSurfaces_ is rebuilt
    // from the store whenever its revision moves (syncSceneSurfaces).
    katana::terrain::SurfaceStore surfaceStore_;
    std::vector<katana::cad::SceneSurface> sceneSurfaces_;
    std::uint64_t sceneSurfacesRevision_ = 0;
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
    QToolButton* crsButton_ = nullptr;
    // The active view's own readout - a section's station and elevation
    // under the cursor, a 3D view's frame time - in a PERMANENT status-bar
    // label, so that it never overwrites a prompt or an error message.
    QLabel* frameStatsLabel_ = nullptr;
    // "3 selected / 120 entities", permanent, refreshed with the panels.
    QLabel* selectionCountLabel_ = nullptr;
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
    // The drop-down lists of the Edit toolbar's Undo and Redo buttons
    // (editUndoButton, editRedoButton): editUndoMenu, editRedoMenu.
    QMenu* undoHistory_ = nullptr;
    QMenu* redoHistory_ = nullptr;
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
    // The geoprocessing verbs (geo/geo_workbench.hpp): GDAL and the families
    // after it, run by the executor katana_cli shares, as background jobs.
    // Declared after the Document, the surfaces and the reference data, which
    // its context holds, so that it goes before them.
    std::unique_ptr<GeoWorkbench> geo_;

    bool refreshingLayers_ = false;     // suppresses cellChanged while rebuilding
    bool refreshingReferences_ = false; // ditto, for the reference table
    bool refreshingStyles_ = false;     // ditto, for the current-style choice
    bool refreshPending_ = false;       // a refreshAll() is queued on the event loop
    bool headless_ = false;
    int historyCursor_ = 0;         // position while browsing command history
    int errorsLogged_ = 0;          // logMessage's errors so far, for runCommand
    // What runVerbLine's line has logged so far, while it runs: logMessage
    // and warnUser add to it. Null otherwise.
    struct VerbCapture {
        QStringList reply;
        QStringList errors;
        // A failure warnUser showed in a box, which is not an error logged.
        bool failed = false;
    };
    VerbCapture* capture_ = nullptr;
    // The scripts running now, outermost first, by canonical path: a SCRIPT
    // line naming one of them is refused rather than recursing for ever.
    QStringList runningScripts_;
    // File > Run Script's dialog, a child of the window; and the Recent
    // Scripts submenu.
    ScriptRunDialog* scriptDialog_ = nullptr;
    QMenu* recentScriptsMenu_ = nullptr;
    // The Help menu's two dialogs, children of the window.
    CommandReferenceDialog* referenceDialog_ = nullptr;
    KeyboardShortcutsDialog* shortcutsDialog_ = nullptr;
    // File > Drawing Summary, a child of the window. It holds the Document
    // through a DocumentWatcher, which is safe when the Document goes first.
    DrawingSummaryDialog* summaryDialog_ = nullptr;
    // File > Plot to PDF and Export View as Image, children of the window.
    PlotDrawingDialog* plotDialog_ = nullptr;
    ViewImageDialog* imageDialog_ = nullptr;
};

} // namespace katana::qt
