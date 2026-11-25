//! The main window (main_window.hpp/.cpp): the desktop shell of the 2D CAD application. It owns
//! the Document and nothing else of substance: every panel is a view drawn from the document at
//! each frame, and every user action becomes a Command or a line for the one executor.
//!
//! THE TWO HALVES (the plan's section 4.2). `MainWindow` is an EXECUTOR (main_window::executor:
//! the drawing, the interpreter, the reference data, the workbenches the dispatcher asks, the log,
//! the jobs, the clipboard and the settings) and a UI STATE (`WindowUi`: the panels, the toolbars'
//! settings, the command line's text, the dialogs and the workbenches only the window draws). A
//! dialog of the UI state is drawn with the executor lent as its host.
//!
//! THE FRAME ORDER (decided here; main_window::panels draws it):
//!
//! ```text
//! 1. the jobs' ends delivered and their effects applied (Executor::deliver_jobs)
//! 2. what the drawing did since the last frame followed (Executor::follow_drawing)
//! 3. the window target begun; the menu bar, the toolbars, the docks, the views and the status
//!    bar drawn in the C++'s geometry (main_window::layout)
//! 4. every workbench's and the window's dialogs drawn, each in its frame
//! 5. what the frame's clicks asked carried out: actions triggered, lines run
//! 6. what the lines asked of the window carried out (WindowRequest); warnings shown
//! ```
//!
//! THE MENUS AND TOOLBARS are rows made again at every frame from the window's state and every
//! workbench's (main_window::actions), in the C++'s order.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use egui::Ui;
use katana_app::console::Console;
use katana_cad::customisation_host::BuiltInEntry;
use katana_cad::interactive_tool::tool_catalog;
use katana_cad::plot::PlotSettings;
use katana_cad::snapping::SnapModes;
use katana_cad::view_set::NO_VIEW;
use katana_cad::viewport_layout::ViewKind;
use katana_cad::Document;
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::{ErrorCode, Result, Status};
use katana_entity::{EntityId, EnumName};
use katana_render::camera::Projection;
use katana_update::ApplyRequest;

use super::actions::{
    Composed, ICON_SIZES, KINDS, LAYOUTS, STANDARD_VIEWS, compose, text_size_action_name,
    tiling_line,
};
use super::executor::{
    Executor, RECENT_SCRIPTS_KEY, SNAP_MENU_MODES, WindowRequest, ask_yes_no, show_warning,
};
use super::layout::{BarSettings, Docks, START_CRS_KEY, START_SHOW_KEY};
use crate::annotation::annotation_workbench::AnnotationWorkbench;
use crate::attribute_manager::AttributeManagerDialog;
use crate::bim::road_object_workbench::RoadObjectWorkbench;
use crate::bim::road_workbench::RoadWorkbench;
use crate::canvas::Image;
use crate::command_reference_dialog::{CommandReferenceDialog, command_reference_sections};
use crate::customisation::customisation_workbench::{
    CustomisationServices, CustomisationWorkbench,
};
use crate::customisation::drawing_summary_dialog::{DrawingSummaryContext, DrawingSummaryDialog};
use crate::dataset_info_dialog::DatasetInfoDialog;
use crate::drawing::drawing_ui::DrawingUi;
use crate::drive::action::{Action, Menu, MenuItem, ToolBar};
use crate::drive::program::{DrivenWindow, MenuSigns, SheetsPlot};
use crate::geo::geo_workbench::GeoMenus;
use crate::geo::menu_table::{GeoMenuDialogs, build_geo_menus};
use crate::gis_export_dialog::VectorExportDialog;
use crate::gis_import_dialogs::{PointCloudImportDialog, RasterImportDialog, VectorImportDialog};
use crate::gis_online::{OnlineDataWorkbench, OnlineServices};
use crate::alignment_manager::AlignmentManagerDialog;
use crate::host::{Host, MemoryClipboard, UserSettings, VerbOutcome};
use crate::ifc_dialogs::{IfcExportDialog, IfcImportDialog, from_native_separators};
use crate::import_placement::ImportPlacementDialog;
use crate::keyboard_shortcuts_dialog::KeyboardShortcutsDialog;
use crate::layer_manager::LayerManagerDialog;
use crate::main_window_ifc::IfcLineFrom;
use crate::plotting::plot_dialog::{PlotDialog, PlotDialogMode, suggested_plot_file};
use crate::plotting::plot_drawing_dialog::PlotDrawingDialog;
use crate::plotting::plot_output::{plot_format_from, plot_request_for};
use crate::plotting::view_image_export::ViewImageDialog;
use crate::project_crs_dialog::ProjectCrsDialog;
use crate::property_panel::PropertyTreePanel;
use crate::script_runner::{ScriptRunDialog, script_command_line};
use crate::select_by_id_dialog::SelectByIdDialog;
use crate::sheet_editor::{SheetEditor, SourceProvider};
use crate::shell::Workbench;
use crate::start_dialog::{StartChoices, StartDialog};
use crate::style_manager::StyleManagerDialog;
use crate::surface_raster_dialog::{SurfaceFromDialog, SurfaceFromKind, SurfaceRasterDialog};
use crate::survey::survey_workbench::{SurveyServices, SurveyWorkbench};
use crate::survey::utility_workbench::{UtilityServices, UtilityWorkbench};
use crate::terrain::surface_edit_ui::SurfaceEditUi;
use crate::terrain::surface_style_manager::SurfaceStyleManagerDialog;
use crate::terrain::surfaces_panel::{SurfacesPanel, SurfacesPanelServices};
use crate::terrain::volumes_dialog::VolumesDialog;
use crate::tools::tool_menus::{BarMenu, ToolActions, ToolMenuTargets, fill_tool_menus};
use crate::update_workbench::{UpdateServices, UpdateWorkbench};
use crate::view_workspace::ViewWorkspace;
use crate::zoom_to_dialog::{ZoomToContext, ZoomToDialog};
use crate::render_view_widget::GpuSession;

/// The tools' menus and toolbars, as the catalogue filled them once (buildToolActions).
#[derive(Clone, Debug)]
pub struct ToolRows {
    pub draw: Menu,
    pub modify: Menu,
    pub annotate: Menu,
    /// Menus made for categories the window gave none (Tools), after Annotate on the bar.
    pub made: Vec<Menu>,
    pub draw_bar: ToolBar,
    pub annotate_bar: ToolBar,
    pub modify_bar: ToolBar,
}

/// What the session's customisation host offers, set once the window has started it
/// (loadDefaultCustomisation) and read by the Format workbench when it asks.
#[derive(Clone, Debug, Default)]
pub struct CustomisationFacts {
    pub kept_file: String,
    pub has_built_in: bool,
}

/// One import options dialog, as makeImportOptions made it for a file's kind.
pub enum ImportOptions {
    Vector(Box<VectorImportDialog>),
    Raster(Box<RasterImportDialog>),
    Cloud(Box<PointCloudImportDialog>),
    Placement(Box<ImportPlacementDialog>),
    Ifc(Box<IfcImportDialog>),
}

/// The window's own dialogs, each made the first time it is asked for and kept (the C++'s
/// children), with whether it is shown where the dialog keeps no flag of its own.
#[derive(Default)]
pub struct Dialogs {
    pub layers: Option<LayerManagerDialog>,
    pub layers_shown: bool,
    pub select_by_id: Option<SelectByIdDialog>,
    pub zoom_to: Option<ZoomToDialog>,
    pub project_crs: Option<ProjectCrsDialog>,
    pub start: Option<StartDialog>,
    pub script: Option<ScriptRunDialog>,
    pub reference: Option<CommandReferenceDialog>,
    pub shortcuts: Option<KeyboardShortcutsDialog>,
    pub summary: Option<DrawingSummaryDialog>,
    pub summary_shown: bool,
    pub plot: Option<PlotDrawingDialog>,
    pub image: Option<ViewImageDialog>,
    pub surface_from: Option<SurfaceFromDialog>,
    pub surface_export: Option<SurfaceRasterDialog>,
    pub alignments: Option<AlignmentManagerDialog>,
    pub sheets: Option<SheetEditor>,
    pub ifc_import: Option<IfcImportDialog>,
    pub ifc_export: Option<IfcExportDialog>,
    pub volumes: Option<VolumesDialog>,
    pub surface_styles: Option<SurfaceStyleManagerDialog>,
    pub dataset_info: Option<DatasetInfoDialog>,
    pub import_options: Option<(String, ImportOptions)>,
    pub export_options: Option<VectorExportDialog>,
    pub attributes: Option<AttributeManagerDialog>,
    /// --style-manager's own manager, from the Format menu's context.
    pub style_manager: Option<StyleManagerDialog>,
    pub style_manager_shown: bool,
    /// File > Plot Sheets' Plot dialog, over the set it was opened on.
    pub plot_sheets: Option<(PlotDialog, katana_cad::plotting::SheetSet)>,
    /// A question the window asks in a small dialog of its own (QInputDialog).
    pub input: Option<super::panels::InputDialog>,
    /// Help > About Katana, in the window's own dialog frame.
    pub about: Option<super::panels::AboutDialog>,
    /// The name the drawing summary's "Show in Styles and Linetypes" asked for, shown by the
    /// window after the dialogs' frame (show_missing_in_styles).
    pub missing_asked: Rc<RefCell<Option<String>>>,
}

/// The window's UI state (see the module comment).
pub struct WindowUi {
    pub visible: bool,
    pub update: UpdateWorkbench,
    pub survey: SurveyWorkbench,
    pub utilities: UtilityWorkbench,
    pub format: CustomisationWorkbench,
    pub annotation: AnnotationWorkbench,
    pub road_objects: RoadObjectWorkbench,
    pub surface_edit: SurfaceEditUi,
    pub drawing: DrawingUi,
    pub geo_menus: GeoMenus,
    pub geo_dialogs: GeoMenuDialogs,
    pub tools: ToolActions,
    pub tool_rows: ToolRows,
    /// What Edit Surface's finish put in the Terrain menu: its submenu and Combine Surfaces.
    pub terrain_edit_items: Vec<MenuItem>,
    pub docks: Docks,
    pub bars: BarSettings,
    /// The torn-off menus, and what the last frame drew of the toolbars and a drag in progress
    /// (main_window::docking).
    pub docking: super::docking::DockingState,
    pub panels: super::panels::Panels,
    pub surfaces: SurfacesPanel,
    pub properties: PropertyTreePanel,
    pub dialogs: Dialogs,
    pub customisation: Rc<RefCell<CustomisationFacts>>,
    /// Set by the update workbench's close: the window is to close once the frame is over.
    pub closing: Rc<Cell<bool>>,
    /// The menu a step opened under its title (--panel of a menu), drawn as a Menu target.
    pub open_menu: Option<String>,
    /// What the frame's clicks asked: actions triggered, after the frame is drawn.
    pub triggered: Vec<String>,
    /// A text size chosen, applied to the context at the next frame.
    pub text_size_asked: Option<crate::theme::TextSize>,
    pub closed: bool,
    /// Whether the theme has been applied to the context the window is drawn in.
    pub themed: bool,
}

/// The main window (see the module comment).
pub struct MainWindow {
    pub exec: Executor,
    pub ui: WindowUi,
}

impl Default for MainWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl MainWindow {
    /// The window over an empty drawing, its views drawn by the software rasteriser: what a
    /// headless run and a test make.
    pub fn new() -> Self {
        Self::with_gpu(GpuSession::headless())
    }

    /// The window whose 3D views are drawn by `gpu`'s rules (the desktop's).
    pub fn with_gpu(gpu: GpuSession) -> Self {
        // The C++'s MainWindow logs warnings and worse to standard error (a command the stack
        // refused, a storage note), which the headless tests' streams carry.
        let logger = std::sync::Arc::new(katana_core::log::Logger::new());
        logger.set_minimum_level(katana_core::log::LogLevel::Warning);
        logger.add_sink(katana_core::log::make_stderr_sink());
        let document = Document::new(Some(logger));
        let workspace = ViewWorkspace::with_gpu(&document, gpu);
        let mut exec = Executor::new(workspace, document);

        // ---- the workbenches, in the C++'s order -----------------------------------------
        let customisation = Rc::new(RefCell::new(CustomisationFacts::default()));
        let mut surface_edit = SurfaceEditUi::new(&exec);
        // Draw, Modify, Annotate, and the Terrain tools in Edit Surface (buildToolActions).
        let mut targets = ToolMenuTargets::default();
        targets
            .menus
            .insert(String::from("Draw"), Menu::new("drawMenu", "&Draw"));
        targets
            .menus
            .insert(String::from("Modify"), Menu::new("modifyMenu", "&Modify"));
        targets
            .menus
            .insert(String::from("Annotate"), Menu::new("annotateMenu", "&Annotate"));
        targets
            .menus
            .insert(String::from("Terrain"), surface_edit.menu().clone());
        targets
            .tool_bars
            .insert(String::from("Draw"), ToolBar::new("DrawToolBar", "Draw"));
        targets.tool_bars.insert(
            String::from("Annotate"),
            ToolBar::new("AnnotateToolBar", "Annotate"),
        );
        targets
            .tool_bars
            .insert(String::from("Modify"), ToolBar::new("ModifyToolBar", "Modify"));
        targets
            .tool_bars
            .insert(String::from("Terrain"), surface_edit.tool_bar().clone());
        targets.menu_bar = Some(
            [
                ("fileMenu", "&File"),
                ("editMenu", "&Edit"),
                ("viewMenu", "&View"),
                ("drawMenu", "&Draw"),
                ("modifyMenu", "&Modify"),
                ("annotateMenu", "&Annotate"),
                ("formatMenu", "F&ormat"),
                ("surveyMenu", "&Survey"),
                ("terrainMenu", "&Terrain"),
                ("bimMenu", "&BIM"),
                ("gisMenu", "&GIS"),
                // views2: the Window menu (main_window::actions), so no made menu takes its letter.
                ("windowMenu", "&Window"),
                ("helpMenu", "&Help"),
            ]
            .into_iter()
            .map(|(name, title)| BarMenu::new(name, title))
            .collect(),
        );
        let tools = fill_tool_menus(tool_catalog(), &mut targets);
        let take_menu = |targets: &mut ToolMenuTargets, category: &str| {
            targets.menus.remove(category).unwrap_or_else(|| Menu::new("", ""))
        };
        let take_bar = |targets: &mut ToolMenuTargets, category: &str| {
            targets.tool_bars.remove(category).unwrap_or_else(|| ToolBar::new("", ""))
        };
        let mut tool_rows = ToolRows {
            draw: take_menu(&mut targets, "Draw"),
            modify: take_menu(&mut targets, "Modify"),
            annotate: take_menu(&mut targets, "Annotate"),
            made: tools.made_menus().values().cloned().collect(),
            draw_bar: take_bar(&mut targets, "Draw"),
            annotate_bar: take_bar(&mut targets, "Annotate"),
            modify_bar: take_bar(&mut targets, "Modify"),
        };
        // Edit Surface's own items below its tools, its submenu and Combine Surfaces for Terrain.
        let mut terrain_items = Menu::new("terrainMenu", "&Terrain");
        surface_edit.finish(
            take_menu(&mut targets, "Terrain"),
            take_bar(&mut targets, "Terrain"),
            &mut terrain_items,
        );
        // The annotation workbench's Annotate entries, after the tools.
        let mut annotation = AnnotationWorkbench::new(&exec.document);
        let _ = annotation.add_edit_text_action(&tool_rows.annotate);
        annotation.add_label_actions(&tool_rows.annotate);
        annotation.add_leader_actions(&tool_rows.annotate);
        tool_rows.annotate.name = String::from("annotateMenu");

        let facts = Rc::clone(&customisation);
        let built_in = Rc::clone(&customisation);
        let kept = Rc::clone(&customisation);
        let has_kept: Rc<dyn Fn() -> bool> =
            Rc::new(move || !facts.borrow().kept_file.is_empty());
        let format = CustomisationWorkbench::new(CustomisationServices {
            layers: Some(super::actions::layers_action()),
            headless: None,
            has_runner: true,
            has_kept_file: Some(has_kept),
            has_built_in: Some(Box::new(move || built_in.borrow().has_built_in)),
            kept_file: Some(Box::new(move || kept.borrow().kept_file.clone())),
            built_ins: None,
        });
        let survey = SurveyWorkbench::new(SurveyServices {
            code_manager: Some(CustomisationWorkbench::code_manager_action()),
            apply_survey_codes: Some(super::actions::apply_codes_action()),
            survey_linework: Some(super::actions::linework_action()),
            executor: true,
        });
        // UTILITY lines run on an interpreter of their own over the window's drawing and views:
        // the host it is lent cannot lend its own interpreter to be written to.
        let lines = exec.lines.clone();
        let utilities = UtilityWorkbench::new(UtilityServices {
            reads_drawing: true,
            interpret: Some(Box::new(move |host: &mut dyn Host, line: &str| {
                let mut interpreter = katana_cad::CommandInterpreter::new();
                let mut parts = crate::viewport_widget::ViewParts {
                    document: host.document_mut(),
                    reference: None,
                    meshes: &[],
                };
                lines.run_line(&mut interpreter, &mut parts, &[], line)
            })),
        });
        exec.utility_lines = Some(utilities.lines());
        exec.road = Some(Box::new(RoadWorkbench::new(&exec.document)));
        let road_objects = RoadObjectWorkbench::new();
        // GIS > Online.
        let outbox = exec.outbox();
        let online = OnlineDataWorkbench::new(
            OnlineServices {
                version: String::from(katana_update::version::program_version_text()),
                choose_project_crs: Some(Box::new(move |_host: &mut dyn Host, place: Option<(f64, f64)>| {
                    // Nothing waits for an answer here: the dialog shown, and Online Data reads
                    // the system again when it next refreshes.
                    outbox
                        .borrow_mut()
                        .push(WindowRequest::ShowProjectCrs(place));
                    false
                })),
            },
            &mut exec,
        );
        exec.online = Some(Box::new(online));
        // The geoprocessing packages' items (menu_table).
        let mut geo_menus = GeoMenus::new(
            Menu::new("gisMenu", "&GIS"),
            Menu::new("terrainMenu", "&Terrain"),
        );
        let mut geo_dialogs = GeoMenuDialogs::new();
        build_geo_menus(&mut geo_menus, &mut geo_dialogs);
        if let Some(geo) = exec.geo.as_mut() {
            geo.set_effects(Some(exec.effects.clone()));
        }
        // Help > Check for Updates, the status bar's button and the UPDATE verbs.
        let closing = Rc::new(Cell::new(false));
        let close_asked = Rc::clone(&closing);
        let update = UpdateWorkbench::new(UpdateServices {
            close_window: Some(Box::new(move |host: &mut dyn Host| {
                // The window's ordinary close: the unsaved-drawing question asked, as at any
                // other close; a headless run has nobody to ask and keeps the window.
                if host.document().is_modified() {
                    if host.headless() {
                        return false;
                    }
                    if !ask_yes_no(
                        "Unsaved Changes",
                        "The drawing has unsaved changes. Discard them and close?",
                    ) {
                        return false;
                    }
                }
                close_asked.set(true);
                true
            })),
            ..UpdateServices::default()
        });
        exec.update_lines = Some(update.lines());

        let surfaces = SurfacesPanel::new(&exec.document, SurfacesPanelServices::default());
        let drawing = DrawingUi::install(&exec.document);
        let mut window = Self {
            ui: WindowUi {
                visible: false,
                update,
                survey,
                utilities,
                format,
                annotation,
                road_objects,
                surface_edit,
                drawing,
                geo_menus,
                geo_dialogs,
                tools,
                tool_rows,
                terrain_edit_items: terrain_items.items,
                docks: Docks::default(),
                bars: BarSettings::default(),
                docking: super::docking::DockingState::default(),
                panels: super::panels::Panels::default(),
                surfaces,
                properties: PropertyTreePanel::new(),
                dialogs: Dialogs::default(),
                customisation,
                closing,
                open_menu: None,
                triggered: Vec::new(),
                text_size_asked: None,
                closed: false,
                themed: false,
            },
            exec,
        };
        window.wire_sheet_context();
        // A category the catalogue gains that no menu here takes is still reachable by its
        // aliases on the command line; said once the log exists.
        for category in window.ui.tools.unplaced().to_vec() {
            window.exec.log_message(
                &format!(
                    "The tool category {category} has no menu; its tools start from the command \
                     line."
                ),
                false,
            );
        }
        window.with_parts_stop_tool();
        window
            .exec
            .log_message("Katana ready. Type HELP for the command list.", false);
        window
    }

    /// GENERATE on the command line lays out what Generate Sheets would: the same context the
    /// sheet editor's own lines get (sheet_verb_context_for).
    fn wire_sheet_context(&mut self) {
        let provider = sheet_source_provider();
        self.exec.interpreter.set_sheet_context(Some(Box::new(
            move |document: &Document| crate::sheet_editor::sheet_verb_context_for(document, &provider),
        )));
    }

    fn with_parts_stop_tool(&mut self) {
        self.exec
            .with_parts(|workspace, parts| workspace.stop_tool(parts));
    }

    /// The rows of the frame: the menus, the toolbars and the submenus' signs.
    pub fn composed(&self) -> Composed {
        compose(self)
    }

    /// The rows as the window built them, whatever rows a restored layout gave the toolbars.
    pub fn composed_as_built(&self) -> Composed {
        super::actions::compose_with(self, false)
    }

    // ---- the actions --------------------------------------------------------------------

    /// MainWindow::triggerAction (see DrivenWindow::trigger_action).
    pub fn trigger(&mut self, name: &str) -> Status {
        if let Some(rest) = name.strip_prefix("undoStep").or_else(|| name.strip_prefix("redoStep"))
            && let Ok(k) = rest.parse::<usize>()
        {
            let verb = if name.starts_with("undo") { "UNDO" } else { "REDO" };
            let _ = self.exec.run_verb_line(&format!("{verb} {k}"));
            return Ok(());
        }
        // A toolbar docked or floated, a menu torn off and its panel placed (main_window::
        // docking): one door, whatever asked - a menu item, a toolbar's right-click menu, a step.
        if let Some(action) = super::docking::parse_action(name) {
            // A bar that does not dock down a side is refused there with the reason, before the
            // move: its greyed Dock Left and Dock Right items are refused by name as any is.
            let refused = super::docking::side_refused(&action, |key| self.bar_named(key).ok());
            let status = refused.and_then(|()| self.docking_action(action));
            self.after_call();
            return status;
        }
        let composed = self.composed();
        let Some(action) = composed.action(name) else {
            // The sheet editor's, once File > Sheets has made it: it is a child of the C++ window,
            // so findChild<QAction*> found its actions after the window's own.
            if let Some(editor) = self.ui.dialogs.sheets.as_mut()
                && editor.action(&self.exec, name).is_some()
            {
                let status = editor.trigger(&mut self.exec, name);
                self.after_call();
                return status;
            }
            return Err(make_error(ErrorCode::NotFound, "no such menu item", name));
        };
        if !action.enabled {
            return Err(make_error(
                ErrorCode::InvalidState,
                "that menu item is disabled",
                name,
            ));
        }
        self.run_action(&action);
        self.after_call();
        Ok(())
    }

    /// What action `action` does when it is triggered, as a click does.
    fn run_action(&mut self, action: &Action) {
        let exec = &mut self.exec;
        let ui = &mut self.ui;
        let name = action.name.as_str();
        match name {
            // ---- File ----
            "fileNew" => exec.new_document(),
            "fileOpen" => exec.open_document(),
            "fileSave" => {
                let _ = exec.save_document();
            }
            "fileSaveAs" => {
                let _ = exec.save_document_as();
            }
            "fileImport" => self.import_file(),
            "fileExportVector" => self.export_vector_file(),
            "filePlot" => self.show_plot_dialog(),
            "fileExportViewImage" => self.show_view_image_export(),
            "fileRunScript" => {
                ui.dialogs
                    .script
                    .get_or_insert_with(ScriptRunDialog::new)
                    .open = true;
            }
            "fileImportIfc" => self.show_ifc_import(""),
            "fileExportIfc" => self.show_ifc_export(""),
            "fileSheets" => self.show_sheets(),
            "filePlotSheets" => self.show_plot_sheets(),
            "applySurveyCodes" => exec.apply_survey_codes(),
            "surveyLinework" => exec.survey_linework(),
            "fileStartScreen" => self.show_start_screen(),
            "fileProjectCrs" => self.show_project_crs(None),
            "fileDrawingSummary" => self.show_drawing_summary(),
            "fileQuit" => self.close(),
            "recentScriptsClear" => {
                exec.settings.remove(RECENT_SCRIPTS_KEY);
            }
            // ---- Edit ----
            "editUndo" => {
                if let Err(error) = exec.document.undo() {
                    exec.log_message(&error.describe(), true);
                }
            }
            "editRedo" => {
                if let Err(error) = exec.document.redo() {
                    exec.log_message(&error.describe(), true);
                }
            }
            "editSelectAll" => exec.select_all(),
            "editDeselect" => exec.with_parts(|workspace, parts| workspace.cancel(parts)),
            "editErase" => {
                // The selection erased at once, as Delete does in every CAD program: the
                // interpreter's ERASE, not the Erase tool a typed ERASE starts.
                exec.log.append("> ERASE");
                exec.run_interpreter_line("ERASE", "ERASE");
            }
            "editSelectById" => {
                ui.dialogs
                    .select_by_id
                    .get_or_insert_with(|| {
                        let mut dialog = SelectByIdDialog::new();
                        // MainWindow::showSelection: framed in the view the person works in, by
                        // that view's Zoom to Selection line through the one executor, so the log
                        // says what moved the view. Without it the dialog's Zoom to the selection
                        // box framed nothing.
                        dialog.on_selected = Some(Box::new(|zoom: bool, host: &mut dyn Host| {
                            let view = host.views().and_then(|views| views.active_plan_view());
                            if let (true, Some(view)) = (zoom, view) {
                                let _ = host.run(&format!("ZOOM SELECTION view={view}"));
                            }
                        }));
                        dialog
                    })
                    .open = true;
            }
            "editCopyViewImage" => {
                let _ = exec.run_verb_line("SNAPSHOT CLIPBOARD");
            }
            "editAttributes" => {
                // Modal in the C++: a headless run is told how to see it.
                if exec.headless {
                    exec.log_message(
                        "Attributes: the dialog is modal and a headless run opens no modal box; \
                         --attributes grabs it.",
                        true,
                    );
                } else {
                    let _ = self.open_attribute_manager();
                }
            }
            "formatLayers" => self.show_layers(),
            // ---- View ----
            "viewZoomExtents" => {
                let _ = exec.run_verb_line("ZOOM EXTENTS");
            }
            "viewZoomIn" => {
                let _ = exec.run_verb_line("ZOOM IN");
            }
            "viewZoomOut" => {
                let _ = exec.run_verb_line("ZOOM OUT");
            }
            "viewZoomSelection" => {
                let _ = exec.run_verb_line("ZOOM SELECTION");
            }
            "viewZoomTo" => self.show_zoom_to(),
            "viewPaperScale" => {
                if !exec.workspace.show_scale_dialog(exec.workspace.active_id()) {
                    exec.log_message(
                        "Paper Style Scale needs an open plan view. VIEWS lists the views.",
                        true,
                    );
                }
            }
            "viewGrid" => {
                let on = !exec.workspace.grid_visible();
                exec.workspace.set_grid_visible(on);
            }
            "viewSnap" => {
                let on = !ViewWorkspace::snap_enabled(&exec.document);
                exec.with_parts(|workspace, parts| workspace.set_snap_enabled(parts, on));
            }
            "ThinScreenLinesAction" => {
                crate::viewport_widget::set_thin_screen_lines(
                    !crate::viewport_widget::thin_screen_lines(),
                );
                exec.workspace.scenery_changed();
            }
            // meshplan: View > Meshes Seen from Above, through the verb a typed line runs.
            "viewMeshesFaces" => {
                let _ = exec.run_verb_line("VIEWS MESHES FACES");
            }
            "viewMeshesFootprint" => {
                let _ = exec.run_verb_line("VIEWS MESHES FOOTPRINT");
            }
            "viewMeshesHidden" => {
                let _ = exec.run_verb_line("VIEWS MESHES HIDDEN");
            }
            "viewLinkActive" => {
                let active = exec.workspace.state(exec.workspace.active_id());
                match active {
                    Some(state) if katana_cad::view_link::linkable(state.kind) => {
                        let workspace = exec.workspace.clone();
                        workspace.toggle_link(exec, state.id);
                    }
                    _ => exec.log_message(
                        "Only a plan view links, and the active view is not one. Click a plan \
                         view, or VIEWS OPEN plan to open one.",
                        true,
                    ),
                }
            }
            "viewUnlinkAll" => {
                let _ = exec.run_verb_line("VIEWS UNLINK ALL");
            }
            // View > Viewport Layout's tilings, through the verb a typed line runs: one path,
            // logged, with the reply saying how many views were arranged.
            "viewTile" | "viewTileSideBySide" | "viewTileStacked" => {
                if let Some(line) = tiling_line(name) {
                    let _ = exec.run_verb_line(&line);
                }
            }
            // The same way: the verb copies the active view, and refuses, saying how to open
            // one, when no view is open.
            super::actions::DUPLICATE_VIEW_ACTION => {
                let _ = exec.run_verb_line(&crate::view_workspace::duplicate_line(None));
            }
            "viewSelectionGhosts" => {
                let active = exec.workspace.active_id();
                match exec.workspace.state(active) {
                    None => exec.log_message("No view is open. VIEWS OPEN plan opens one.", true),
                    Some(state) => {
                        let workspace = exec.workspace.clone();
                        workspace.set_selection_ghosts(exec, active, !state.selection_ghosts);
                    }
                }
            }
            "viewTogglePerspective" => self.toggle_perspective(),
            "viewVerticalExaggeration" => self.ask_vertical_exaggeration(),
            "viewPanelLayers" => ui.docks.layers = !ui.docks.layers,
            "viewPanelProperties" => ui.docks.properties = !ui.docks.properties,
            "viewPanelCommandLine" => ui.docks.command_line = !ui.docks.command_line,
            "viewPanelReferenceData" => {
                ui.docks.reference = !ui.docks.reference;
                if ui.docks.reference {
                    ui.docks.surfaces_in_front = false;
                }
            }
            "viewSurfacesPanel" => {
                // Shown, it is brought to the front of its tab group, not left behind Reference
                // Data.
                ui.docks.surfaces = !ui.docks.surfaces;
                if ui.docks.surfaces {
                    ui.docks.surfaces_in_front = true;
                    ui.surfaces.refresh(&exec.document);
                }
            }
            "terrainSurfacesPanel" => {
                ui.docks.surfaces = true;
                ui.docks.surfaces_in_front = true;
                ui.surfaces.refresh(&exec.document);
            }
            "viewToolBarNames" => {
                ui.bars.names_shown = !ui.bars.names_shown;
                if !exec.headless {
                    exec.settings
                        .set_flag(super::layout::TOOL_BAR_NAMES_KEY, ui.bars.names_shown);
                }
            }
            "viewResetLayout" => {
                ui.docks = Docks::default();
                ui.bars.hidden.clear();
                ui.bars.places = None;
                ui.docking.panels = super::docking::MenuPanels::default();
                ui.docking.window_panels = super::docking::WindowPanels::default();
                ui.docking.docked_before.clear();
                exec.log_message(
                    "The panels and toolbars are back where a new window has them.",
                    false,
                );
            }
            "toolSelect" => {
                exec.with_parts(|workspace, parts| workspace.stop_tool(parts));
            }
            // ---- Terrain ----
            "terrainAlignmentManager" => self.show_alignment_manager(),
            "surfaceFromPointCloud" => self.show_surface_from(SurfaceFromKind::Cloud),
            "surfaceFromRaster" => self.show_surface_from(SurfaceFromKind::Raster),
            "surfaceFromDrawing" => self.show_surface_from(SurfaceFromKind::Drawing),
            "terrainSectionAlongSelection" => self.cut_section_along_selection(),
            "terrainSectionAlongAlignment" => self.cut_section_along_alignment(),
            "terrainSurfaceStyles" => self.show_surface_style_manager(),
            "terrainVolumes" => {
                let dialog = ui
                    .dialogs
                    .volumes
                    .get_or_insert_with(|| VolumesDialog::new(exec));
                dialog.show(exec);
            }
            // ---- GIS ----
            "importVectorData" => self.import_with_options_dialog(
                "IMPORT <file> [layers=a,b] [where=\"<filter>\"] [target=<layer>]",
                "Import Vector Data",
            ),
            "importRaster" => self.import_with_options_dialog(
                "IMPORT <file> [band=N] [maxpixels=N] [name=<n>]",
                "Import Raster",
            ),
            "importPointCloud" => self.import_with_options_dialog(
                "IMPORT <file> [budget=N] [class=N] [resolution=<m>]",
                "Import Point Cloud",
            ),
            "exportPointCloud" => self.export_point_cloud(),
            "exportSurfaceDem" => {
                let dialog = ui
                    .dialogs
                    .surface_export
                    .get_or_insert_with(|| SurfaceRasterDialog::new(exec));
                dialog.show(exec);
            }
            "convertCopc" => {
                if exec.headless {
                    exec.log_message(
                        "A headless session opens no file dialog: type COPC <source> \
                         <destination.copc.laz> instead.",
                        true,
                    );
                } else {
                    self.convert_point_cloud_to_copc();
                }
            }
            "datasetInformation" => {
                if !exec.refuse_file_dialog("INFO <file>")
                    && let Some(path) = rfd::FileDialog::new()
                        .set_title("Dataset Information")
                        .pick_file()
                {
                    let text = String::from_utf8_lossy(&path_to_utf8(&path)).into_owned();
                    let _ = self.open_dataset_info(Path::new(&text));
                }
            }
            // ---- Help ----
            "helpCommandReference" => self.show_command_reference(""),
            "helpSheetCommands" => self.show_command_reference("Sheets"),
            "helpKeyboardShortcuts" => self.show_keyboard_shortcuts(),
            // A dialog of the window's own frame, not the platform's message box: that box wore
            // a warning sign for what is no warning, and blocked the window while it was up. It
            // asks nothing, so a headless run may show it too.
            "helpAbout" => {
                ui.dialogs
                    .about
                    .get_or_insert_with(super::panels::AboutDialog::new)
                    .show();
            }
            _ => self.run_other_action(action),
        }
    }

    /// The actions named by pattern, and every workbench's.
    fn run_other_action(&mut self, action: &Action) {
        let name = action.name.as_str();
        let exec = &mut self.exec;
        let ui = &mut self.ui;
        if let Some(rest) = name.strip_prefix("recentScript")
            && let Ok(at) = rest.parse::<usize>()
        {
            let path = exec
                .settings
                .list(RECENT_SCRIPTS_KEY)
                .and_then(|list| list.get(at.wrapping_sub(1)).cloned());
            if let Some(path) = path {
                let _ = exec.run_verb_line(&script_command_line(&path, false));
            }
            return;
        }
        if let Some(mode) = name.strip_prefix("viewSnapMode")
            && let Some(found) = SNAP_MENU_MODES.iter().find(|each| each.name() == mode)
        {
            let bit = *found as SnapModes;
            let modes = exec.document.drafting().snap_modes;
            ViewWorkspace::set_snap_modes(&mut exec.document, modes ^ bit);
            return;
        }
        if let Some(layout) = name.strip_prefix("viewLayout")
            && let Some((kind, ..)) = LAYOUTS.iter().find(|(_, each, ..)| *each == layout)
        {
            let kind = *kind;
            exec.with_parts(|workspace, parts| workspace.arrange(parts, kind));
            return;
        }
        // views2: Window > a view's title, through the verb a typed line runs - logged, and the
        // view brought out of the tray or to the front of its tabs as VIEWS ACTIVATE brings it.
        if let Some(id) = super::actions::window_view_id(name) {
            let _ = exec.run_verb_line(&format!("VIEWS ACTIVATE {id}"));
            return;
        }
        if let Some(shown) = name.strip_prefix("viewShows")
            && let Some((kind, ..)) = KINDS.iter().find(|(kind, ..)| kind.name() == shown)
        {
            let kind = *kind;
            let active = exec.workspace.active_id();
            let status = exec.with_parts(|workspace, parts| workspace.set_view_kind(parts, active, kind));
            if let Err(error) = status {
                exec.log_message(&error.describe(), true);
            }
            return;
        }
        if let Some(standard) = name.strip_prefix("viewStandard")
            && let Some((view, ..)) = STANDARD_VIEWS.iter().find(|(_, each, ..)| *each == standard)
        {
            let view = *view;
            match exec.workspace.active_render_view() {
                Some(id) => {
                    let scene = Rc::clone(&exec.scene);
                    let surfaces = scene.surfaces();
                    let meshes = scene.meshes(exec.document.model());
                    let _ = exec.workspace.with_render_view(
                        id,
                        &exec.document,
                        crate::view_workspace::Scenery {
                            surfaces: &surfaces,
                            meshes: &meshes,
                        },
                        |render, _, state| render.set_standard_view(state, view),
                    );
                }
                None => exec.log_message(
                    "No 3D viewport is open. Use View > Viewport Layout.",
                    true,
                ),
            }
            return;
        }
        if let Some(bar) = name.strip_prefix("viewToolBar")
            && !bar.starts_with("Icons")
            && bar != "Names"
        {
            let composed = compose(self);
            if let Some((tool_bar, _)) = composed
                .bars
                .iter()
                .find(|(each, _)| each.title.replace(' ', "") == bar)
            {
                self.ui.bars.toggle(&tool_bar.name);
            }
            return;
        }
        if let Some((pixels, ..)) = ICON_SIZES.iter().find(|(_, _, each)| *each == name) {
            ui.bars.icon_size = *pixels;
            if !exec.headless {
                exec.settings
                    .set_number(super::layout::TOOL_BAR_ICONS_KEY, f64::from(*pixels));
            }
            return;
        }
        if let Some(size) = crate::theme::TEXT_SIZES
            .into_iter()
            .find(|size| text_size_action_name(*size) == name)
        {
            ui.text_size_asked = Some(size);
            if !exec.headless {
                exec.settings.set_text(
                    super::layout::TEXT_SIZE_KEY,
                    crate::theme::to_string(size),
                );
            }
            exec.log_message(
                &format!("Text size: {}.", crate::theme::to_string(size)),
                false,
            );
            return;
        }
        // A catalogue tool: started in the active plan view.
        if let Some(id) = ui.tools.trigger(name) {
            exec.start_tool(&id);
            return;
        }
        // The drawing system's toggles.
        if ui.drawing.trigger(&mut exec.document, name) {
            return;
        }
        // The workbenches', in the order they were built.
        if name == CustomisationWorkbench::settings_action().name {
            let _ = ui.format.trigger(exec, name);
            let updates = ui.update.settings_updates(exec);
            if let Some(settings) = ui.format.settings() {
                settings.set_updates(updates);
            }
            return;
        }
        if ui.format.trigger(exec, name)
            || ui.annotation.trigger(exec, name)
            || ui.survey.trigger(exec, name)
            || ui.utilities.trigger(exec, name)
            || ui.road_objects.trigger(exec, name)
            || ui.surface_edit.trigger(exec, name)
            || ui.update.trigger(exec, name)
            || ui.geo_dialogs.trigger(exec, name)
        {
            return;
        }
        if super::executor::lend!(exec, road, |road, host| Workbench::trigger(road, host, name))
            .unwrap_or(false)
        {
            exec.take_road_events();
            return;
        }
        let _ = super::executor::lend!(exec, online, |online, host| Workbench::trigger(online, host, name));
    }

    // ---- what the window opens --------------------------------------------------------

    /// Format > Layers, made the first time and kept.
    pub fn show_layers(&mut self) {
        let document = &self.exec.document;
        let dialogs = &mut self.ui.dialogs;
        if dialogs.layers.is_none() {
            let mut dialog = LayerManagerDialog::new(document);
            // The action's name plus "Dialog", as the C++ window named it for --dialog.
            dialog.set_object_name(super::actions::FORMAT_LAYERS_DIALOG);
            dialog.show_first_row(document);
            dialogs.layers = Some(dialog);
        }
        dialogs.layers_shown = true;
    }

    /// File > Project Coordinate System and the status bar's CRS button: made afresh when it is
    /// opened from closed, or for a place.
    pub fn show_project_crs(&mut self, place: Option<(f64, f64)>) {
        let fresh = place.is_some()
            || !self
                .ui
                .dialogs
                .project_crs
                .as_ref()
                .is_some_and(ProjectCrsDialog::is_visible);
        if fresh {
            self.ui.dialogs.project_crs = Some(ProjectCrsDialog::new(&self.exec.document, place));
        }
        if let Some(dialog) = self.ui.dialogs.project_crs.as_mut() {
            dialog.show();
        }
    }

    /// File > Start Screen: made afresh when opened from closed, from the session as it is now
    /// and what was chosen last.
    pub fn show_start_screen(&mut self) {
        if self
            .ui
            .dialogs
            .start
            .as_ref()
            .is_some_and(StartDialog::is_visible)
        {
            return;
        }
        let mut last = StartChoices::default();
        if !self.exec.headless {
            last.coordinate_system = self
                .exec
                .settings
                .text(START_CRS_KEY)
                .map(String::from)
                .unwrap_or_default();
            last.show_at_start = self.exec.settings.flag(START_SHOW_KEY).unwrap_or(true);
        }
        let kept = &self.exec.kept_customisation_file;
        if !kept.as_os_str().is_empty() {
            last.kept_file =
                String::from_utf8_lossy(&katana_core::path_text::generic_path_utf8(kept)).into_owned();
        }
        let built_ins: Vec<BuiltInEntry> = katana_cad::customisation_host::built_ins().to_vec();
        let mut dialog = StartDialog::new(&self.exec.document, last, &built_ins);
        dialog.show();
        self.ui.dialogs.start = Some(dialog);
    }

    /// The start screen's answer kept for the next start, but by a headless run.
    pub(crate) fn remember_start_screen(&mut self) {
        let Some(screen) = self.ui.dialogs.start.as_ref() else {
            return;
        };
        let Some(accepted) = screen.result() else {
            return;
        };
        if self.exec.headless {
            return;
        }
        let show = screen.show_at_start();
        let crs = screen.coordinate_system();
        self.exec.settings.set_flag(START_SHOW_KEY, show);
        if accepted {
            self.exec.settings.set_text(START_CRS_KEY, &crs);
        }
    }

    pub fn show_zoom_to(&mut self) {
        let exec = &mut self.exec;
        if self.ui.dialogs.zoom_to.is_none() {
            let context = ZoomToContext::of(exec);
            self.ui.dialogs.zoom_to = Some(ZoomToDialog::new(context, exec));
        }
        if let Some(dialog) = self.ui.dialogs.zoom_to.as_mut() {
            dialog.refresh(exec);
            dialog.show();
        }
    }

    pub fn show_drawing_summary(&mut self) {
        let document = &self.exec.document;
        let dialogs = &mut self.ui.dialogs;
        if dialogs.summary.is_none() {
            let asked = Rc::clone(&dialogs.missing_asked);
            let context = DrawingSummaryContext {
                has_runner: true,
                show_missing: Some(Rc::new(move |name: &str| {
                    *asked.borrow_mut() = Some(String::from(name));
                })),
                crs: None,
            };
            dialogs.summary = Some(DrawingSummaryDialog::new(document, context));
        }
        if let Some(summary) = dialogs.summary.as_mut() {
            summary.refresh(document);
        }
        dialogs.summary_shown = true;
    }

    /// MainWindow::showMissingInStyles: the style manager shown at its Styles tab, its Missing
    /// chip on and its search the name.
    pub fn show_missing_in_styles(&mut self, name: &str) {
        self.ui
            .format
            .show_style_manager(&mut self.exec)
            .show_missing_named(name);
    }

    pub fn show_command_reference(&mut self, section: &str) {
        let dialog = self
            .ui
            .dialogs
            .reference
            .get_or_insert_with(|| CommandReferenceDialog::new(command_reference_sections()));
        if !section.is_empty() {
            let _ = dialog.show_section(section);
        }
        dialog.open = true;
    }

    pub fn show_keyboard_shortcuts(&mut self) {
        if self.ui.dialogs.shortcuts.is_none() {
            let rows = super::layout::shortcut_rows(&self.composed());
            let (clashes, _) = super::layout::shortcut_clashes(&self.composed());
            self.ui.dialogs.shortcuts = Some(KeyboardShortcutsDialog::new(rows, &clashes));
        }
        if let Some(dialog) = self.ui.dialogs.shortcuts.as_mut() {
            dialog.open = true;
        }
    }

    pub fn show_plot_dialog(&mut self) {
        let suggested = suggested_plot_file(&self.exec.document);
        let dialog = self
            .ui
            .dialogs
            .plot
            .get_or_insert_with(|| PlotDrawingDialog::new(&suggested));
        dialog.show();
    }

    pub fn show_view_image_export(&mut self) {
        let suggested = suggested_image_file(&self.exec.document);
        let dialog = self
            .ui
            .dialogs
            .image
            .get_or_insert_with(|| ViewImageDialog::new(&suggested));
        dialog.show();
    }

    /// MainWindow::suggestPlotFiles: on a new, opened or saved drawing, the Plot dialog's and the
    /// Export View as Image dialog's files follow it, where the person has not chosen another.
    pub fn suggest_plot_files(&mut self) {
        if let Some(dialog) = self.ui.dialogs.plot.as_mut() {
            dialog.suggest_path(&suggested_plot_file(&self.exec.document));
        }
        if let Some(dialog) = self.ui.dialogs.image.as_mut() {
            dialog.suggest_path(&suggested_image_file(&self.exec.document));
        }
    }

    /// File > Plot Sheets: the Plot dialog over every sheet; the page setup is kept only for
    /// sheets the project has.
    pub fn show_plot_sheets(&mut self) {
        let set = match self.exec.sheets_to_plot() {
            Ok(set) => set,
            Err(error) => {
                self.exec.log_message(&error.describe(), true);
                return;
            }
        };
        let suggested = suggested_plot_file(&self.exec.document);
        let dialog = PlotDialog::new(&set, 0, true, &suggested, PlotDialogMode::Plot);
        self.ui.dialogs.plot_sheets = Some((dialog, set));
    }

    /// File > Sheets: the sheet editor, built on first use and kept.
    pub fn show_sheets(&mut self) {
        let exec = &mut self.exec;
        let editor = self.ui.dialogs.sheets.get_or_insert_with(|| {
            let mut editor = SheetEditor::new(sheet_source_provider());
            // Its dialogs' lines run as typed ones do.
            editor.set_command_runner(true);
            editor
        });
        editor.set_headless(exec.headless);
        editor.show(exec);
    }

    pub fn show_alignment_manager(&mut self) {
        let document = &self.exec.document;
        let dialog = self
            .ui
            .dialogs
            .alignments
            .get_or_insert_with(|| AlignmentManagerDialog::new(document));
        dialog.show();
    }

    pub fn show_surface_style_manager(&mut self) {
        let exec = &mut self.exec;
        let dialog = self
            .ui
            .dialogs
            .surface_styles
            .get_or_insert_with(|| SurfaceStyleManagerDialog::new(exec));
        dialog.show();
    }

    /// Terrain > Surface From ...: the dialog on that source, Reference Data's chosen cloud or
    /// raster first.
    pub fn show_surface_from(&mut self, kind: SurfaceFromKind) {
        let exec = &mut self.exec;
        let chosen = match self.ui.panels.reference_selected {
            Some(id) if kind == SurfaceFromKind::Cloud
                && exec.reference.find_point_cloud(id).is_some() =>
            {
                format!("CLOUD {id}")
            }
            Some(id) if kind == SurfaceFromKind::Raster && exec.reference.find_raster(id).is_some() => {
                format!("RASTER {id}")
            }
            _ => String::new(),
        };
        let dialog = self
            .ui
            .dialogs
            .surface_from
            .get_or_insert_with(|| SurfaceFromDialog::new(exec));
        dialog.show(exec);
        dialog.show_kind(exec, kind, &chosen);
    }

    pub fn show_ifc_import(&mut self, file: &str) {
        let context = self.exec.ifc_import_context();
        let dialog = self
            .ui
            .dialogs
            .ifc_import
            .get_or_insert_with(|| IfcImportDialog::new(context));
        if !file.is_empty() {
            dialog.set_file(file);
        }
        dialog.show();
    }

    pub fn show_ifc_export(&mut self, file: &str) {
        let exec = &mut self.exec;
        if self.ui.dialogs.ifc_export.is_none() {
            let context = exec.ifc_export_context();
            self.ui.dialogs.ifc_export = Some(IfcExportDialog::new(context, exec));
        }
        if let Some(dialog) = self.ui.dialogs.ifc_export.as_mut() {
            if !file.is_empty() {
                dialog.set_file(file);
            }
            dialog.show(exec);
            // Already open, a show delivers no show event: the counts are read here.
            dialog.refresh(exec);
        }
    }

    // ---- File > Import and Export ---------------------------------------------------------

    /// File > Import Any File: refused headless; otherwise a file dialog, and the IMPORT line it
    /// makes - a DXF or an archive asking where it lands first.
    pub fn import_file(&mut self) {
        if self
            .exec
            .refuse_file_dialog("IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]")
        {
            return;
        }
        let Some(chosen) = rfd::FileDialog::new().set_title("Import").pick_file() else {
            return;
        };
        let selected = String::from_utf8_lossy(&path_to_utf8(&chosen)).into_owned();
        self.import_chosen(&selected);
    }

    /// What File > Import does with a chosen file.
    fn import_chosen(&mut self, selected: &str) {
        let path = path_from_utf8(selected);
        if katana_ifc::is_ifc_path(&path) {
            let arguments = katana_ifc::ImportArguments {
                path: String::from(selected),
                ..katana_ifc::ImportArguments::default()
            };
            match katana_ifc::format_import_line(&arguments) {
                Ok(line) => {
                    let _ = self.exec.run_ifc_command(&line, IfcLineFrom::Menu);
                }
                Err(error) => self.exec.log_message(&error.describe(), true),
            }
            return;
        }
        // DWG (docs/dwg.md): read natively, where it lands asked as for a DXF - before GDAL is
        // asked what the file is.
        if katana_dwg::is_dwg_path(&path) {
            self.import_with_placement(selected);
            return;
        }
        let kind = katana_interop::import::kind_for_path(&path);
        if katana_dxf::is_dxf_path(&path)
            // DGN: a V7 design asks where it lands, as a DXF does (docs/dgn.md).
            || katana_dgn::is_dgn_path(&path)
            || kind == katana_interop::import::SourceKind::Archive12d
        {
            self.import_with_placement(selected);
            return;
        }
        if kind != katana_interop::import::SourceKind::Unknown {
            let line = crate::import_placement::import_line(
                &from_native_separators(selected),
                &Default::default(),
            );
            let _ = self.exec.run_verb_line(&line);
            return;
        }
        self.exec.warn_user(
            "Import",
            &format!(
                "Neither GDAL nor Katana recognises the data in\n{selected}\n\nGIS > Formats \
                 lists what this build of GDAL reads (FORMATS READ); archives and point clouds \
                 are read too."
            ),
        );
    }

    /// The import placement dialog for a DXF or an archive.
    pub fn import_with_placement(&mut self, path: &str) {
        let bounds = self.exec.document.model().entities.bounds();
        let dialog = ImportPlacementDialog::new(path, bounds, &self.exec.settings);
        self.ui.dialogs.import_options = Some((
            String::from(path),
            ImportOptions::Placement(Box::new(dialog)),
        ));
    }

    /// GIS > Import Vector, Raster or Point Cloud: refused headless with the line to type; a file
    /// dialog and the options of its kind otherwise.
    fn import_with_options_dialog(&mut self, verb: &str, title: &str) {
        if self.exec.refuse_file_dialog(verb) {
            return;
        }
        if let Some(chosen) = rfd::FileDialog::new().set_title(title).pick_file() {
            let text = String::from_utf8_lossy(&path_to_utf8(&chosen)).into_owned();
            self.import_with_options(&text);
        }
    }

    /// MainWindow::importWithOptions.
    pub fn import_with_options(&mut self, path: &str) {
        let file = path_from_utf8(path);
        // DWG (docs/dwg.md): where it lands is all there is to ask, as for a DXF.
        if katana_dwg::is_dwg_path(&file) {
            self.import_with_placement(path);
            return;
        }
        if katana_dxf::is_dxf_path(&file)
            // DGN
            || katana_dgn::is_dgn_path(&file)
            || katana_interop::import::kind_for_path(&file)
                == katana_interop::import::SourceKind::Archive12d
        {
            self.import_with_placement(path);
            return;
        }
        if katana_ifc::is_ifc_path(&file) {
            self.show_ifc_import(path);
            return;
        }
        let _ = self.open_import_options(&file);
    }

    /// File > Export Drawing: the empty drawing said, refused headless, a file dialog otherwise.
    pub fn export_vector_file(&mut self) {
        let document = &self.exec.document;
        if document.model().entities.empty()
            && document.model().alignments.empty()
            && document.surfaces().all().is_empty()
        {
            if self.exec.headless {
                self.exec.log_message("Export: the drawing is empty.", true);
            } else {
                show_warning("Export", "The drawing is empty.");
            }
            return;
        }
        if self.exec.refuse_file_dialog("EXPORT <file>") {
            return;
        }
        let mut dialog = rfd::FileDialog::new().set_title("Export");
        let formats = katana_interop::export::vector_export_formats();
        for format in &formats {
            dialog = dialog.add_filter(format.description.clone(), &[format.extension.as_str()]);
        }
        // The archive format by the name the rule allows (docs/rust.md, the naming rule).
        dialog = dialog
            .add_filter("Archive", &["12da"])
            .add_filter("Archive, zipped", &["12daz"])
            .add_filter("IFC 4.3", &["ifc"]);
        // DGN: written natively (docs/dgn.md), so offered whether or not this
        // GDAL has a DGN driver to list it above.
        if !formats.iter().any(|format| format.extension == "dgn") {
            dialog = dialog.add_filter(katana_dgn::FILE_TYPE_NAME, &["dgn"]);
        }
        // DWG (docs/dwg.md): written natively, R2018 unless the dialog's release says otherwise.
        dialog = dialog.add_filter("DWG drawing", &["dwg"]);
        let Some(chosen) = dialog.save_file() else {
            return;
        };
        let selected = String::from_utf8_lossy(&path_to_utf8(&chosen)).into_owned();
        if katana_ifc::is_ifc_path(&chosen) {
            self.show_ifc_export(&selected);
            return;
        }
        let _ = self.show_export_options(&selected);
    }

    /// File > Export Drawing's dialog for `selected`, one per window, with the scope set as the
    /// menu sets it.
    pub fn show_export_options(&mut self, selected: &str) -> String {
        let exec = &mut self.exec;
        if self.ui.dialogs.export_options.is_none() {
            let context = crate::geo::gis_tool_dialog::GisDialogContext::of_window(exec);
            self.ui.dialogs.export_options = Some(VectorExportDialog::new(context, exec));
        }
        let dialog = self
            .ui
            .dialogs
            .export_options
            .as_mut()
            .expect("made just above");
        use crate::geo::gis_tool_dialog::GisTool;
        GisTool::reload(dialog, &*exec);
        dialog.set_file(&from_native_separators(selected));
        // What is selected is what is usually meant, when there is a selection.
        let choice = if exec.document.selection().empty() {
            crate::customisation::ScopeChoice::Drawing
        } else {
            crate::customisation::ScopeChoice::Selection
        };
        if let Some(scope) = dialog.frame_mut().scope_controls_mut() {
            scope.set_choice(choice);
        }
        GisTool::show(dialog, &*exec);
        String::from(dialog.object_name())
    }

    /// GIS > Export Point Cloud: the cloud chosen, the decimated sample asked about, a file
    /// dialog, and the EXPORT line it makes.
    fn export_point_cloud(&mut self) {
        let exec = &mut self.exec;
        let clouds = exec.reference.point_clouds();
        let cloud = match self.ui.panels.reference_selected {
            Some(id) if clouds.iter().any(|cloud| cloud.id == id) => {
                clouds.iter().find(|cloud| cloud.id == id)
            }
            _ if clouds.len() == 1 => clouds.first(),
            _ => None,
        };
        let Some(cloud) = cloud else {
            let why = if clouds.is_empty() {
                "No point cloud is loaded. Use GIS > Import first."
            } else {
                "Several point clouds are loaded and nobody can be asked which; select one in the \
                 Reference Data panel."
            };
            exec.log_message(why, true);
            return;
        };
        let (id, name) = (cloud.id, cloud.name.clone());
        if exec.headless {
            exec.log_message(
                &format!(
                    "A headless session opens no file dialog: type EXPORT <file.las|.laz> CLOUD \
                     {id} instead."
                ),
                true,
            );
            return;
        }
        let Some(chosen) = rfd::FileDialog::new()
            .set_title("Export Point Cloud")
            .set_file_name(format!("{name}.laz"))
            .add_filter("LAZ", &["laz"])
            .add_filter("LAS", &["las"])
            .save_file()
        else {
            return;
        };
        let mut text = String::from_utf8_lossy(&path_to_utf8(&chosen)).into_owned();
        let lower = text.to_lowercase();
        if !lower.ends_with(".las") && !lower.ends_with(".laz") {
            text.push_str(".laz");
        }
        let outcome = exec.run_verb_line(&format!(
            "EXPORT \"{}\" CLOUD {id}",
            from_native_separators(&text)
        ));
        if !outcome.ok {
            exec.warn_user("Export failed", &outcome.error);
        }
    }

    /// GIS > Convert Point Cloud to COPC: the two files asked for, and the COPC line.
    fn convert_point_cloud_to_copc(&mut self) {
        let Some(source) = rfd::FileDialog::new()
            .set_title("Convert Point Cloud to COPC")
            .pick_file()
        else {
            return;
        };
        let mut suggested = source.clone();
        suggested.set_extension("copc.laz");
        let Some(destination) = rfd::FileDialog::new()
            .set_title("Save COPC As")
            .set_file_name(suggested.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default())
            .save_file()
        else {
            return;
        };
        let source = String::from_utf8_lossy(&path_to_utf8(&source)).into_owned();
        let mut destination = String::from_utf8_lossy(&path_to_utf8(&destination)).into_owned();
        if !destination.to_lowercase().ends_with(".copc.laz") {
            destination.push_str(".copc.laz");
        }
        let outcome = self.exec.run_verb_line(&format!(
            "COPC \"{}\" \"{}\"",
            from_native_separators(&source),
            from_native_separators(&destination)
        ));
        if !outcome.ok {
            self.exec.warn_user("Conversion failed", &outcome.error);
        }
    }

    // ---- terrain, 3D and sections --------------------------------------------------------

    fn cut_section_along_selection(&mut self) {
        let exec = &mut self.exec;
        let selected = exec.document.selection().ids();
        if selected.len() != 1 {
            exec.log_message("Select exactly one line or polyline to cut along.", true);
            return;
        }
        let Some(entity) = exec.document.model().entities.find(selected[0]) else {
            exec.log_message("The selected entity no longer exists.", true);
            return;
        };
        use katana_entity::Geometry;
        let alignment = match &entity.geometry {
            Geometry::Line(line) => katana_geometry::Polyline2 {
                vertices: vec![line.start, line.end],
                ..Default::default()
            },
            Geometry::Polyline(polyline) => polyline.clone(),
            Geometry::CurvePolyline(_) | Geometry::Spline(_) => katana_geometry::Polyline2 {
                // Along the curve's chords within a millimetre, which a section's chainage cannot
                // tell from the curve.
                vertices: katana_entity::curve_pieces::linework(&entity.geometry, 0.001),
                ..Default::default()
            },
            _ => {
                exec.log_message(
                    "A section must be cut along a line, a polyline or a spline.",
                    true,
                );
                return;
            }
        };
        exec.cut_section_along(alignment, "the selection", None);
    }

    fn cut_section_along_alignment(&mut self) {
        let names = self.exec.document.model().alignments.names();
        if names.is_empty() {
            self.exec.log_message(
                "Define an alignment first: Terrain > Alignment Manager, or ALIGN NEW name x,y \
                 x,y ... on the command line.",
                true,
            );
            if !self.exec.headless {
                self.show_alignment_manager();
            }
            return;
        }
        self.ui.dialogs.input = Some(super::panels::InputDialog::choice(
            "Cut Section Along Alignment",
            "Alignment:",
            names,
            super::panels::InputPurpose::SectionAlignment,
        ));
    }

    /// A section cut down the named alignment, with its design profile when it has one.
    pub(crate) fn cut_section_down(&mut self, chosen: &str) {
        let exec = &mut self.exec;
        let Some(alignment) = exec.document.model().alignments.find(chosen).cloned() else {
            exec.log_message("The alignment no longer exists.", true);
            return;
        };
        let solved = match katana_geometry::alignment::solve_alignment(&alignment.horizontal) {
            Ok(solved) => solved,
            Err(error) => {
                exec.log_message(&error.describe(), true);
                return;
            }
        };
        let design = match &alignment.vertical {
            Some(vertical) => match katana_geometry::profile::solve_profile(vertical) {
                Ok(profile) => Some(katana_cad::section::SectionDesign {
                    profile,
                    name: format!("design {chosen}"),
                }),
                Err(error) => {
                    exec.log_message(&error.describe(), true);
                    return;
                }
            },
            None => None,
        };
        // 10 mm chords: the section samples at most every 100 mm along the line.
        exec.cut_section_along(solved.to_polyline(0.01), &format!("alignment {chosen}"), design);
    }

    fn toggle_perspective(&mut self) {
        let exec = &mut self.exec;
        let Some(id) = exec.workspace.active_render_view() else {
            exec.log_message("No 3D viewport is open.", true);
            return;
        };
        let scene = Rc::clone(&exec.scene);
        let surfaces = scene.surfaces();
        let meshes = scene.meshes(exec.document.model());
        let was = exec.workspace.with_render_view(
            id,
            &exec.document,
            crate::view_workspace::Scenery {
                surfaces: &surfaces,
                meshes: &meshes,
            },
            |render, _, state| {
                let was = state.camera.projection() == Projection::Perspective;
                render.set_projection(
                    state,
                    if was {
                        Projection::Orthographic
                    } else {
                        Projection::Perspective
                    },
                );
                was
            },
        );
        if let Some(was) = was {
            exec.log_message(
                if was {
                    "Orthographic projection."
                } else {
                    "Perspective projection."
                },
                false,
            );
        }
    }

    /// View > Vertical Exaggeration: refused headless with the line to type; the factor asked
    /// for in a small dialog otherwise, then the EXAGGERATION line.
    fn ask_vertical_exaggeration(&mut self) {
        if self.exec.headless {
            self.exec.log_message(
                "A headless session opens no dialog: type EXAGGERATION <factor> instead.",
                true,
            );
            return;
        }
        let current = self.exec.workspace.scene_options().vertical_exaggeration;
        self.ui.dialogs.input = Some(super::panels::InputDialog::text(
            "Vertical Exaggeration",
            "Multiply elevations by:",
            &katana_core::text::format_exact_real(current),
            super::panels::InputPurpose::Exaggeration,
        ));
    }

    /// QWidget::close: the Format managers first, then the unsaved drawing; a window a person
    /// used saves its layout.
    pub fn close(&mut self) {
        if !self.ui.format.confirm_close(&mut self.exec) {
            return;
        }
        if !self.exec.confirm_discard() {
            return;
        }
        if !self.exec.headless {
            self.save_session();
        }
        self.ui.closed = true;
    }

    /// Whether the window has closed (a QUIT, File > Quit, or the update's close).
    pub fn closed(&self) -> bool {
        self.ui.closed || self.ui.closing.get()
    }

    // ---- what --attributes, --layer-manager and --style-manager make --------------------

    pub fn open_attribute_manager(&mut self) -> String {
        let document = &self.exec.document;
        let dialog = self
            .ui
            .dialogs
            .attributes
            .get_or_insert_with(AttributeManagerDialog::new);
        dialog.expand_all(document);
        dialog.open = true;
        String::from(dialog.object_name())
    }

    /// makeImportOptions for `path`, made the window's target: its name; None, logged, for a file
    /// with no such dialog.
    pub fn open_import_options(&mut self, path: &Path) -> Option<String> {
        let exec = &mut self.exec;
        let text = String::from_utf8_lossy(&path_to_utf8(path)).into_owned();
        let shown = from_native_separators(&text);
        if katana_ifc::is_ifc_path(path) {
            // Described first: a file that cannot be read has no dialog, and says why.
            let described = match exec.describe_ifc_file(path) {
                Ok(described) => described,
                Err(error) => {
                    exec.log_message(&error.describe(), true);
                    exec.warn_user("Import", &error.describe());
                    return None;
                }
            };
            let mut dialog = IfcImportDialog::new(exec.ifc_import_context());
            dialog.set_file(&shown);
            dialog.set_summary(&described);
            dialog.show();
            let name = String::from("fileImportIfcDialog");
            self.ui.dialogs.import_options =
                Some((text, ImportOptions::Ifc(Box::new(dialog))));
            return Some(name);
        }
        // DWG and DGN (docs/dwg.md, docs/dgn.md): the placement step a DXF has, before GDAL
        // is asked what it is.
        let dwg = katana_dwg::is_dwg_path(path);
        if dwg
            || katana_dgn::is_dgn_path(path)
            || katana_dxf::is_dxf_path(path)
            || katana_interop::import::kind_for_path(path)
                == katana_interop::import::SourceKind::Archive12d
        {
            let bounds = exec.document.model().entities.bounds();
            let dialog = ImportPlacementDialog::new(&shown, bounds, &exec.settings);
            let name = String::from(dialog.object_name());
            self.ui.dialogs.import_options =
                Some((text, ImportOptions::Placement(Box::new(dialog))));
            return Some(name);
        }
        let description = match katana_interop::dataset_info::describe_source(path, &Default::default()) {
            Ok(description) => description,
            Err(error) => {
                exec.log_message(&error.describe(), true);
                exec.warn_user("Import", &error.describe());
                return None;
            }
        };
        let options = match description.kind {
            katana_interop::import::SourceKind::Vector => {
                let context = crate::geo::gis_tool_dialog::GisDialogContext::of_window(exec);
                let bounds = exec.document.model().entities.bounds();
                ImportOptions::Vector(Box::new(VectorImportDialog::new(
                    &description,
                    bounds,
                    context,
                    exec,
                )))
            }
            katana_interop::import::SourceKind::Raster => {
                ImportOptions::Raster(Box::new(RasterImportDialog::new(&description)))
            }
            katana_interop::import::SourceKind::PointCloud => {
                ImportOptions::Cloud(Box::new(PointCloudImportDialog::new(&description)))
            }
            _ => {
                exec.warn_user(
                    "Import",
                    &format!("Katana has no import options for {text}"),
                );
                return None;
            }
        };
        let name = String::from(match &options {
            ImportOptions::Vector(dialog) => dialog.object_name(),
            ImportOptions::Raster(dialog) => dialog.object_name(),
            ImportOptions::Cloud(dialog) => dialog.object_name(),
            ImportOptions::Placement(dialog) => dialog.object_name(),
            ImportOptions::Ifc(_) => "fileImportIfcDialog",
        });
        self.ui.dialogs.import_options = Some((text, options));
        Some(name)
    }

    /// MainWindow::finishImport: an accepted import dialog's IMPORT line through the one
    /// executor, as if typed.
    pub(crate) fn finish_import(&mut self, options: &ImportOptions) {
        let line: Result<String> = match options {
            ImportOptions::Vector(dialog) => {
                dialog.placement_box().remember(&mut self.exec.settings);
                dialog.command()
            }
            ImportOptions::Raster(dialog) => dialog.command(),
            ImportOptions::Cloud(dialog) => dialog.command(),
            ImportOptions::Placement(dialog) => Ok(dialog.line()),
            ImportOptions::Ifc(_) => return,
        };
        match line {
            Ok(line) => {
                let _ = self.exec.run_verb_line(&line);
            }
            Err(error) => self.exec.log_message(&error.describe(), true),
        }
    }

    /// makeDatasetInfo for `path`, made the window's target: its name; None when the file could
    /// not be described (headless; logged).
    pub fn open_dataset_info(&mut self, path: &Path) -> Option<String> {
        let text = String::from_utf8_lossy(&path_to_utf8(path)).into_owned();
        let dialog = DatasetInfoDialog::new(&from_native_separators(&text), &mut self.exec);
        if self.exec.headless && !dialog.described() {
            return None;
        }
        let name = String::from(dialog.object_name());
        self.ui.dialogs.dataset_info = Some(dialog);
        Some(name)
    }

    // ---- the frame's aftermath --------------------------------------------------------------

    /// What a call asked of the window, carried out: the executor's requests, and the warnings a
    /// person is shown in a box (never headless: the log has them).
    pub(crate) fn after_call(&mut self) {
        // What the views raised while the call ran, as the C++'s hooks delivered it at once: the
        // DIST tool's inverse is in the command log before the next --report reads it.
        self.workspace_events();
        for request in self.exec.take_requests() {
            match request {
                WindowRequest::Close => self.close(),
                WindowRequest::ShowAlignmentManager => {
                    if !self.exec.headless {
                        self.show_alignment_manager();
                    }
                }
                WindowRequest::ShowProjectCrs(place) => self.show_project_crs(place),
                WindowRequest::ImportWithPlacement(path) => self.import_with_placement(&path),
                WindowRequest::ImportWithOptions(path) => self.import_with_options(&path),
                WindowRequest::ShowIfcImport(path) => self.show_ifc_import(&path),
                WindowRequest::ShowIfcExport(path) => self.show_ifc_export(&path),
                WindowRequest::ShowExportOptions(path) => {
                    let _ = self.show_export_options(&path);
                }
                WindowRequest::ViewsChanged => {}
            }
        }
        for (title, text) in self.exec.log.take_warnings() {
            if !self.exec.headless {
                show_warning(&title, &text);
            }
        }
        if self.ui.closing.get() {
            self.ui.closed = true;
        }
    }

    /// The window's session saved: its appearance and which panels are shown (saveSession).
    pub fn save_session(&mut self) {
        super::layout::save_session(self);
    }
}

/// What the sheets are drawn from, for the sheet editor and the interpreter's sheet verbs: the
/// drawing, its surfaces in their styles, the set's logo and the project's fields. The window's
/// reference layers and meshes do not reach it: the provider is handed the drawing alone (seam
/// left for the sheet editor's owner; the window's own plots - PLOTSHEETS, --plot-sheets - draw
/// them).
pub fn sheet_source_provider() -> SourceProvider {
    Rc::new(|document: &Document| {
        let styles = document.surface_styles();
        let surfaces = document
            .surfaces()
            .all()
            .iter()
            .filter(|named| named.visible)
            .filter_map(|named| {
                let surface = named.surface.as_deref()?;
                let mut scene = katana_cad::scene::SceneSurface {
                    name: named.name.clone(),
                    surface: Some(surface),
                    ..Default::default()
                };
                katana_cad::scene::apply_surface_display(
                    &mut scene,
                    katana_cad::terrain::surface_style::style_of(styles, named),
                );
                Some(scene)
            })
            .collect();
        let mut source = Executor::sheet_source(document, &EMPTY_REFERENCE, surfaces, &[]);
        source.plan.reference = None;
        source.plan.meshes = None;
        source
    })
}

/// The reference data a source made from the drawing alone is given.
static EMPTY_REFERENCE: std::sync::LazyLock<katana_interop::ReferenceData> =
    std::sync::LazyLock::new(katana_interop::ReferenceData::default);

// ---- the program's view of the window --------------------------------------------------------

impl DrivenWindow for MainWindow {
    fn ui(&mut self, ui: &mut Ui) {
        self.frame(ui);
    }

    fn busy(&self) -> bool {
        use crate::host::Jobs;
        self.exec.jobs.active_count() > 0
    }

    fn application_icon(&self) -> Image {
        crate::icons::application_icon_image(&crate::icons::CanvasRasteriser, 256)
    }

    fn set_headless(&mut self, headless: bool) {
        self.exec.set_headless(headless);
        if headless {
            self.exec.clipboard = Box::new(MemoryClipboard::new());
            self.exec.settings = UserSettings::in_memory();
        }
    }

    fn load_default_customisation(&mut self) {
        self.exec.load_default_customisation();
        let mut facts = self.ui.customisation.borrow_mut();
        facts.kept_file = String::from_utf8_lossy(&katana_core::path_text::generic_path_utf8(
            &self.exec.kept_customisation_file,
        ))
        .into_owned();
        if self.exec.kept_customisation_file.as_os_str().is_empty() {
            facts.kept_file.clear();
        }
        facts.has_built_in = self.exec.has_built_in_customisation;
    }

    fn restore_session(&mut self) {
        super::layout::restore_session(self);
    }

    fn show(&mut self) {
        self.ui.visible = true;
    }

    fn is_visible(&self) -> bool {
        self.ui.visible && !self.closed()
    }

    fn start_updates(&mut self) {
        self.ui.update.start(&mut self.exec);
        self.after_call();
    }

    fn show_start_screen_if_wanted(&mut self) {
        if self.exec.headless || !self.exec.settings.flag(START_SHOW_KEY).unwrap_or(true) {
            return;
        }
        self.show_start_screen();
    }

    fn open_project(&mut self, directory: &Path) {
        self.exec.open_project(directory);
        self.after_call();
    }

    fn import_path(&mut self, path: &Path) {
        let text = String::from_utf8_lossy(&path_to_utf8(path)).into_owned();
        if katana_ifc::is_ifc_path(path) {
            let arguments = katana_ifc::ImportArguments {
                path: text,
                ..katana_ifc::ImportArguments::default()
            };
            match katana_ifc::format_import_line(&arguments) {
                Ok(line) => {
                    let _ = self.exec.run_ifc_command(&line, IfcLineFrom::Menu);
                }
                Err(error) => self.exec.log_message(&error.describe(), true),
            }
        } else {
            let line = crate::import_placement::import_line(
                &from_native_separators(&text),
                &Default::default(),
            );
            let _ = self.exec.run_verb_line(&line);
        }
        self.after_call();
    }

    fn run_command(&mut self, line: &str) -> bool {
        let ran = self.exec.run_command(line);
        self.after_call();
        ran
    }

    fn run_verb_line(&mut self, line: &str) -> VerbOutcome {
        let outcome = self.exec.run_verb_line(line);
        self.after_call();
        outcome
    }

    fn script_command_line(&self, path: &str) -> String {
        script_command_line(path, false)
    }

    fn actions(&self) -> Vec<Action> {
        let mut actions = self.composed().actions();
        // findChildren<QAction*> found the sheet editor's too, once it was made (see trigger).
        if let Some(editor) = self.ui.dialogs.sheets.as_ref() {
            for action in editor.actions(&self.exec) {
                if !actions.iter().any(|known| known.name == action.name) {
                    actions.push(action);
                }
            }
        }
        actions
    }

    fn menus(&self) -> Vec<Menu> {
        // The menu bar's, then the Edit toolbar's history lists: reportWidget's
        // findChild<QMenu*> found any of the window's menus by its name.
        let mut menus = self.composed().menus;
        menus.extend(super::actions::history_menus(&self.exec.document));
        menus
    }

    fn trigger_action(&mut self, name: &str) -> Status {
        self.trigger(name)
    }

    fn select_all(&mut self) {
        self.exec.select_all();
    }

    fn select_only(&mut self, id: EntityId) {
        self.exec.select_only(id);
    }

    fn toggle_layer_through_panel(&mut self, layer: &str) -> Status {
        super::panels::toggle_layer_through_panel(self, layer)
    }

    fn open_style_manager(&mut self) -> String {
        let context = self.ui.format.context();
        let dialog = StyleManagerDialog::new(&context, &self.exec.document);
        self.ui.dialogs.style_manager = Some(dialog);
        self.ui.dialogs.style_manager_shown = true;
        String::from(crate::style_manager::STYLE_MANAGER)
    }

    fn open_attribute_manager(&mut self) -> String {
        MainWindow::open_attribute_manager(self)
    }

    fn open_layer_manager(&mut self) -> String {
        let document = &self.exec.document;
        let mut dialog = LayerManagerDialog::new(document);
        dialog.show_first_row(document);
        self.ui.dialogs.layers = Some(dialog);
        self.ui.dialogs.layers_shown = true;
        String::from(crate::layer_manager::LAYER_MANAGER)
    }

    fn open_dataset_info(&mut self, path: &Path) -> Option<String> {
        MainWindow::open_dataset_info(self, path)
    }

    fn open_import_options(&mut self, path: &Path) -> Option<String> {
        MainWindow::open_import_options(self, path)
    }

    fn open_export_options(&mut self, path: &Path) -> Option<String> {
        let text = String::from_utf8_lossy(&path_to_utf8(path)).into_owned();
        Some(self.show_export_options(&text))
    }

    fn open_menu(&mut self, name: &str) -> bool {
        let found = crate::drive::steps::find_menu(&self.composed().menus, name).is_some();
        if found {
            self.ui.open_menu = Some(String::from(name));
        }
        found
    }

    fn pointer_at(&mut self, spec: &str, click: bool) -> Result<String> {
        let parts: Vec<&str> = spec.split(',').collect();
        let coordinate = |at: usize| {
            parts
                .get(at)
                .and_then(|part| katana_core::text::parse_finite_double(part.trim()))
        };
        let modifiers = match parts.len() {
            2 => super::executor::modifiers_of(""),
            3 => super::executor::modifiers_of(&parts[2].trim().to_lowercase()),
            _ => None,
        };
        let (Some(x), Some(y), Some(modifiers)) = (coordinate(0), coordinate(1), modifiers) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "a pointer step is x,y or x,y,shift or x,y,ctrl",
                spec,
            ));
        };
        let at = katana_geometry::Point2::new(x, y);
        let record = self
            .exec
            .with_parts(|workspace, parts| workspace.pointer_at(parts, at, click, modifiers));
        // What the click said (the tool's reply, a refusal) is logged now, before the step prints
        // the record: the C++ view's signals reach the log during the click, so its reply comes
        // first. Left to the next frame, it was printed after the record.
        self.workspace_events();
        self.after_call();
        record
    }

    fn report_view(&mut self, name: &str) -> Option<String> {
        super::panels::report_view(self, name)
    }

    fn shortcut_clashes(&self) -> (Vec<String>, usize) {
        super::layout::shortcut_clashes(&self.composed())
    }

    fn menu_gaps(&self) -> (Vec<String>, usize) {
        super::layout::menu_gaps(&self.composed())
    }

    fn menu_sign_clashes(&mut self) -> MenuSigns {
        super::layout::menu_sign_clashes(self)
    }

    fn check_tool_bar_fit(&mut self, sizes: &[(u32, u32)], err: &mut dyn Console) -> bool {
        super::layout::check_tool_bar_fit(self, sizes, err)
    }

    fn check_session_restore(&mut self, err: &mut dyn Console) -> bool {
        super::layout::check_session_restore(self, err)
    }

    fn document(&self) -> &Document {
        &self.exec.document
    }

    fn plot_drawing_to_pdf(&mut self, path: &Path, settings: PlotSettings, fit: bool) -> Status {
        let plotted = self.exec.plot_drawing_to_pdf(path, settings, fit);
        self.after_call();
        plotted
    }

    fn plot_sheets(&mut self, plot: &SheetsPlot) -> Result<Vec<PathBuf>> {
        let set = self.exec.sheets_to_plot()?;
        let destination = String::from_utf8_lossy(&path_to_utf8(&plot.path)).into_owned();
        let mut request = plot_request_for(&set.page_setup, &destination, &plot.selection);
        // --format names the format; one PDF when it is not given.
        request.format = plot
            .format
            .as_deref()
            .and_then(plot_format_from)
            .unwrap_or(crate::plotting::plot_output::PlotFormat::Pdf);
        if let Some(mode) = plot.colour_mode {
            request.colour_mode = mode;
        }
        if let Some(dpi) = plot.dpi {
            request.dpi = dpi;
        }
        if let Some(scale) = plot.line_weight_scale {
            request.line_weight_scale = scale;
        }
        let report = self.exec.plot_sheets(&set, &request)?;
        Ok(report.files.iter().map(path_from_utf8).collect())
    }

    fn plot_format_known(format: &str) -> bool {
        plot_format_from(format).is_some()
    }

    fn take_pending_update(&mut self) -> Option<ApplyRequest> {
        self.ui.update.take_pending_apply()
    }

    fn update_failure_is_boxed(&self, step_kinds: &[String]) -> bool {
        crate::update_dialog::update_failure_is_boxed::<String>(step_kinds)
    }
}

/// suggestedImageFile: the plot's file with a .png for its suffix, in the system's separators.
fn suggested_image_file(document: &Document) -> String {
    let plot = suggested_plot_file(document);
    let image = Path::new(&plot)
        .with_extension("png")
        .to_string_lossy()
        .into_owned();
    crate::ifc_dialogs::to_native_separators(&image)
}

/// The views' kinds by the names the View menu's items carry.
pub fn kind_named(name: &str) -> Option<ViewKind> {
    KINDS
        .iter()
        .find(|(kind, ..)| kind.name() == name)
        .map(|(kind, ..)| *kind)
}

/// No view is open.
pub const NONE: katana_cad::view_set::ViewId = NO_VIEW;

/// The kinds of the windows' docks the window keeps by name.
pub type DockNames = BTreeSet<String>;
