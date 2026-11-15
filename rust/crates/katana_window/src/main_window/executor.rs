//! The main window's EXECUTOR (the plan's section 4.2): the drawing, the interpreter, the reference
//! data, the geo scratch folder and front end, the workbenches whose verbs the dispatcher runs,
//! the command log, the jobs, the clipboard and the person's settings - everything a line reaches
//! - and the dispatcher, ported as main_window.cpp has it (runTypedLine, runVerbLine,
//! runWorkbenchLine, dispatchLine, runInterpreterLine), in its order:
//!
//! ```text
//! 1. the workbenches: update, survey, IFC, geo (as jobs), online, road, utilities
//! 2. the window's own verbs: GRID, SNAP, EXAGGERATION, QUIT, SCRIPT, PLOTSHEETS, PLOT,
//!    SNAPSHOT, HELP
//! 3. a bare tool word typed by a person, which starts the tool
//! 4. the interpreter
//! ```
//!
//! THE BORROW SPLIT. The executor implements `Host`: a dialog the window draws (its UI state, the
//! other half of `MainWindow`) is lent `&mut Executor` as its host, reads the drawing through it
//! and runs its line through it, one after the other. Rejected, as the plan rejected them:
//! `Rc<RefCell<Document>>` everywhere, and dialogs queueing lines for the next frame.
//!
//! THE WORKBENCHES the dispatcher asks are held here, each in a slot it is taken out of while it is
//! lent the executor as its host (`lend!`), and put back after: the geo, online and road
//! workbenches. A line one of them runs inside its own call - none does today: the online dialog
//! runs its commands through the workbench's core, the geo dialogs are the window's - would not
//! find it, and would go to the interpreter. The update and utility workbenches are reached
//! through their shared line runners (`UpdateLines`, `UtilityLines`), which they made for exactly
//! this re-entry; the workbenches themselves are the UI state's.
//!
//! WHAT ONLY THE WINDOW CAN DO - show a dialog, close itself - a line asks for with a
//! `WindowRequest`, which the window carries out after the call (`take_requests`). A question a
//! person must answer (an unsaved drawing, a project damaged) is asked in the system's own box
//! (rfd), as the C++ asked in a QMessageBox; never in a headless run, which refuses and says so.
//!
//! THE SCENE. The drawing's surfaces as the 3D, section and sheet views draw them, and the meshes
//! an archive brought, are kept in a `SceneStore` behind an `Rc`, replaced whole when they change:
//! a frame lends the views a borrow of the store it cloned the `Rc` of, while the executor is lent
//! to them as their host.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use katana_app::console::Terminal;
use katana_app::geo::references::{record_references, records_references};
use katana_app::geo::{Context, FrontEnd, help_text as geo_help_text, own_scratch};
use katana_app::survey_verbs::{is_survey_line, run_survey_line};
use katana_app::update_verbs::update_help_text;
use katana_cad::bim::bim_object::BimModel;
use katana_cad::bim::bim_view::scene_meshes;
use katana_cad::customisation_host::{
    CustomisationHost, KEPT_CUSTOMISATION_VARIABLE, built_in_advice_for, built_in_customisation,
    built_ins, start_customisation,
};
use katana_cad::customisation_record::typed_save_has_destination;
use katana_cad::customisation_state::CustomisationOrigin;
use katana_cad::layer_overrides::NO_LAYER_OVERRIDES;
use katana_cad::plotting::generators::{LayoutRequest, prepare_for_append, smart_layout};
use katana_cad::plotting::page_setup::parse_sheet_selection;
use katana_cad::plotting::sheet_commands::{field_context_for, frame_date, logo_path};
use katana_cad::plotting::sheet_verbs::parse_plot_sheets;
use katana_cad::plotting::SheetSet;
use katana_cad::scene::{SceneMesh, SceneSurface, scene_bounds};
use katana_cad::section::SectionDesign;
use katana_cad::snapping::{SnapMode, SnapModes};
use katana_cad::terrain::section_cut::cut_drawing_section;
use katana_cad::view_set::ViewId;
use katana_cad::{CommandInterpreter, Document, DocumentChange, ListenerHandle, PlotSettings};
use katana_core::error::make_error;
use katana_core::path_text::{environment_variable, path_from_utf8, path_to_utf8};
use katana_core::text::{format_exact_real, parse_finite_double};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::EntityId;
use katana_entity::EnumName;
use katana_geometry::{Box2, Point2, Polyline2, TriangleMesh};
use katana_interop::ReferenceData;
use katana_storage::ProjectStore;
use katana_terrain::TinSurface;

use crate::command_log::{CommandLog, update_logged_line};
use crate::command_reference_dialog::window_help_text;
use crate::format::grouped;
use crate::geo::geo_workbench::{GeoEffects, GeoWorkbench};
use crate::gis_online::OnlineDataWorkbench;
use crate::host::{
    Clipboard, GeoPhase, Host, JobAwait, JobId, Jobs, MemoryClipboard, UserSettings, VerbOutcome,
    ViewAccess,
};
use crate::jobs::{JobHost, JobRunner, wait_for_all_jobs, wait_for_job};
use crate::bim::road_workbench::RoadWorkbench;
use crate::plotting::plot_drawing_dialog::parse_plot_drawing;
use crate::plotting::plot_output::{PlotReport, PlotRequest, plot_request_for_words, plot_sheets_from};
use crate::plotting::sheet_checks::{check_sheets_for, preflight_log};
use crate::plotting::view_image_export::{
    Background, SnapshotRequest, View as SnapshotView, parse_snapshot, snapshot_size,
    write_snapshot,
};
use crate::plan_painter::{CanvasPdfWriter, plan_drawn_bounds, plan_source_of};
use crate::script_runner::{
    ScriptLine, ScriptOptions, format_script_report, is_script_comment, parse_script_command,
    read_script, run_script_lines, script_lines,
};
use crate::shell::Workbench;
use crate::sheet_painter::{SheetPaintCache, SheetSource};
use crate::survey::utility_workbench::UtilityLines;
use crate::tools::tool_menus::tool_id_for_command;
use crate::update_workbench::UpdateLines;
use crate::view_workspace::{Scenery, ViewWorkspace, WorkspaceLines};
use crate::viewport_widget::{KeyModifiers, ViewParts};

use super::super::main_window_ifc::{IfcBridge, IfcLineFrom};

/// The vertical exaggeration a person may choose, the box's range since View > Vertical
/// Exaggeration had one: a hundredth flattens a section to a line, and beyond a thousand a model
/// of metres is a wall of spikes.
pub const LEAST_EXAGGERATION: f64 = 0.01;
pub const MOST_EXAGGERATION: f64 = 1000.0;

/// What the command line shows while nothing is typed and no tool is asking.
pub const COMMAND_PLACEHOLDER: &str =
    "Command:  LINE  |  CIRCLE 5,5 3  |  SELECT ALL  |  HELP   (Enter repeats the last tool)";

/// The Recent Scripts list, newest first, in the person's settings as the online keys are.
pub const RECENT_SCRIPTS_KEY: &str = "scripts/recent";
/// As many as a File menu lists of recent files in most programs.
pub const RECENT_SCRIPTS: usize = 8;

/// Where File > Open and Save As start (project_folder): never the working folder.
pub const PROJECT_FOLDER_KEY: &str = "files/projectFolder";

/// Who a line came from. Only a person's typed line starts a tool by a bare word; a script's, a
/// paste's or a dialog's is the interpreter's, as katana_cli runs the same line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineSource {
    Typed,
    Executor,
}

/// What a line asked of the window that only the window can do, carried out after the call.
#[derive(Clone, Debug, PartialEq)]
pub enum WindowRequest {
    /// QUIT: the window's ordinary close, its questions asked.
    Close,
    /// Terrain > Alignment Manager.
    ShowAlignmentManager,
    /// File > Project Coordinate System, for a place when there is one.
    ShowProjectCrs(Option<(f64, f64)>),
    /// The import placement dialog for a DXF or an archive (File > Import).
    ImportWithPlacement(String),
    /// The import options dialog for a file chosen in a GIS import's file dialog.
    ImportWithOptions(String),
    /// File > Import IFC with its file.
    ShowIfcImport(String),
    /// File > Export IFC with its file.
    ShowIfcExport(String),
    /// File > Export Drawing's dialog for a file.
    ShowExportOptions(String),
    /// The 3D views' list of what they show changed: the View menu reads it again.
    ViewsChanged,
}

// ---- the scene ----------------------------------------------------------------------------------

/// A surface as the views draw it: how it is shown, and the surface itself, held so an undo that
/// drops the last step holding it cannot free it under a view (the C++'s sceneSurfacesHeld_).
#[derive(Clone)]
pub struct SurfaceItem {
    pub scene: SceneSurface<'static>,
    pub held: Arc<TinSurface>,
}

/// A mesh the session holds beside the drawing: an archive's trimesh.
#[derive(Clone)]
pub struct MeshItem {
    pub scene: SceneMesh<'static>,
    pub held: Arc<TriangleMesh>,
}

/// The drawing's surfaces as the views draw them, the meshes an import brought and the road
/// model's objects (see the module comment).
#[derive(Clone, Default)]
pub struct SceneStore {
    pub surfaces: Vec<SurfaceItem>,
    pub meshes: Vec<MeshItem>,
    /// The road model whose objects are among the 3D views' meshes (setRoadMeshes); None while
    /// BIM > Show Road Model is off.
    pub road: Option<Arc<BimModel>>,
    pub surfaces_generation: u64,
    pub styles_generation: u64,
}

impl SceneStore {
    /// The surfaces, lent.
    pub fn surfaces(&self) -> Vec<SceneSurface<'_>> {
        self.surfaces
            .iter()
            .map(|item| SceneSurface {
                surface: Some(&*item.held),
                ..item.scene.clone()
            })
            .collect()
    }

    /// The meshes, lent: the imported ones, then the road model's objects shown in `model`.
    pub fn meshes(&self, model: &katana_entity::Model) -> Vec<SceneMesh<'_>> {
        let mut meshes: Vec<SceneMesh<'_>> = self
            .meshes
            .iter()
            .map(|item| SceneMesh {
                mesh: Some(&*item.held),
                ..item.scene.clone()
            })
            .collect();
        if let Some(road) = &self.road {
            meshes.extend(scene_meshes(road, model, &NO_LAYER_OVERRIDES));
        }
        meshes
    }
}

/// The parts of the drawing's notifications heard since they were last taken.
#[derive(Clone, Default)]
pub struct Heard(Arc<AtomicU32>);

impl Heard {
    fn add(&self, parts: u32) {
        self.0.fetch_or(parts, Ordering::AcqRel);
    }

    /// What was heard, and nothing kept.
    pub fn take(&self) -> u32 {
        self.0.swap(0, Ordering::AcqRel)
    }
}

/// `{ let wb = &mut *slot; let host = exec; body }` with the workbench in `exec.$field` taken out
/// for the call and put back after (see the module comment). None when the slot is empty: the
/// workbench is already lent, further up the stack.
macro_rules! lend {
    ($exec:expr, $field:ident, |$wb:ident, $host:ident| $body:expr) => {{
        match $exec.$field.take() {
            Some(mut boxed) => {
                let result = {
                    let $wb = &mut *boxed;
                    let $host: &mut $crate::main_window::executor::Executor = &mut *$exec;
                    $body
                };
                $exec.$field = Some(boxed);
                Some(result)
            }
            None => None,
        }
    }};
}
pub(crate) use lend;

/// The main window's executor (see the module comment).
pub struct Executor {
    // The listeners first, so they go before the Document they are registered with.
    _listener: ListenerHandle,
    pub heard: Heard,
    pub document: Document,
    pub interpreter: CommandInterpreter,
    pub reference: ReferenceData,
    pub scratch: PathBuf,
    pub effects: GeoEffects,
    front_end: FrontEnd,
    pub log: CommandLog,
    pub jobs: JobRunner,
    pub clipboard: Box<dyn Clipboard>,
    pub settings: UserSettings,
    pub headless: bool,
    pub workspace: ViewWorkspace,
    pub lines: WorkspaceLines,
    pub scene: Rc<SceneStore>,
    // The workbenches the dispatcher asks (see the module comment).
    pub geo: Option<Box<GeoWorkbench>>,
    pub online: Option<Box<OnlineDataWorkbench>>,
    pub road: Option<Box<RoadWorkbench>>,
    pub update_lines: Option<UpdateLines>,
    pub utility_lines: Option<UtilityLines>,
    pub ifc: IfcBridge,
    /// The scripts running now, outermost first, by canonical path: a SCRIPT line naming one of
    /// them is refused rather than recursing for ever.
    running_scripts: Vec<String>,
    /// Position while browsing the command history (Up and Down on the command line).
    pub history_cursor: usize,
    /// The kept customisation file - empty when the session keeps none - and whether there is a
    /// built-in, as loadDefaultCustomisation found them.
    pub kept_customisation_file: PathBuf,
    pub has_built_in_customisation: bool,
    requests: Rc<RefCell<Vec<WindowRequest>>>,
    /// A discard of unsaved changes the person agreed to, for the action that asked and is run
    /// again (a box's answer, in the C++ inside the action).
    pub discard_agreed: bool,
}

impl Executor {
    /// The executor of a window over an empty drawing: its views' workspace with one plan view,
    /// the interpreter's hooks over that workspace, the geo front end's effects queue.
    pub fn new(workspace: ViewWorkspace, document: Document) -> Self {
        let heard = Heard::default();
        let parts = heard.clone();
        let listener = document.add_change_listener(move |change: &DocumentChange<'_>| {
            parts.add(change.parts);
        });
        let lines = workspace.lines();
        let mut interpreter = CommandInterpreter::new();
        // VIEW in a verb's scope is a view of this window's, and VIEWS and ZOOM act on its views;
        // set again at each line by the workspace's line runner, and here for the geo phases,
        // which read the interpreter without running a line.
        interpreter.set_view_host(Some(lines.verb_host()));
        interpreter.set_scope_context(Some(lines.scope_provider()));
        let effects = GeoEffects::new();
        let front_end = effects.front_end();
        let mut log = CommandLog::new();
        log.set_echo(Some(Box::new(Terminal)));
        Self {
            _listener: listener,
            heard,
            document,
            interpreter,
            reference: ReferenceData::default(),
            scratch: own_scratch(),
            effects,
            front_end,
            log,
            jobs: JobRunner::new(),
            clipboard: Box::new(MemoryClipboard::new()),
            settings: UserSettings::in_memory(),
            headless: false,
            workspace,
            lines,
            scene: Rc::new(SceneStore::default()),
            geo: Some(Box::new(GeoWorkbench::new())),
            online: None,
            road: None,
            update_lines: None,
            utility_lines: None,
            ifc: IfcBridge::default(),
            running_scripts: Vec::new(),
            history_cursor: 0,
            kept_customisation_file: PathBuf::new(),
            has_built_in_customisation: false,
            requests: Rc::new(RefCell::new(Vec::new())),
            discard_agreed: false,
        }
    }

    /// MainWindow::setHeadless, for the parts of the executor that ask.
    pub fn set_headless(&mut self, headless: bool) {
        self.headless = headless;
        self.log.set_headless(headless);
        self.effects.set_headless(headless);
    }

    /// What the lines asked of the window since the last call.
    pub fn take_requests(&mut self) -> Vec<WindowRequest> {
        std::mem::take(&mut *self.requests.borrow_mut())
    }

    pub fn request(&mut self, request: WindowRequest) {
        self.requests.borrow_mut().push(request);
    }

    /// Where a workbench's callback, lent only the host, puts what it asks of the window.
    pub fn outbox(&self) -> Rc<RefCell<Vec<WindowRequest>>> {
        Rc::clone(&self.requests)
    }

    /// MainWindow::logMessage.
    pub fn log_message(&mut self, text: &str, is_error: bool) {
        self.log.log(text, is_error);
    }

    /// `f` with the workspace and the drawing lent as its views take it.
    pub fn with_parts<R>(
        &mut self,
        f: impl FnOnce(&ViewWorkspace, &mut ViewParts<'_>) -> R,
    ) -> R {
        let scene = Rc::clone(&self.scene);
        let meshes = scene.meshes(self.document.model());
        let workspace = self.workspace.clone();
        let mut parts = ViewParts {
            document: &mut self.document,
            reference: Some(&self.reference),
            meshes: &meshes,
        };
        f(&workspace, &mut parts)
    }

    /// The views' Zoom Extents in every view.
    pub fn zoom_extents_all(&mut self) {
        self.with_parts(|workspace, parts| workspace.zoom_extents_all(parts));
    }

    // ---- the one executor ---------------------------------------------------------------

    /// MainWindow::runVerbLine (command_runner): `line` echoed in the command log and run by the
    /// dispatcher a typed line goes to - but never offered to a running tool - with what it logged
    /// returned. An empty line runs nothing and is not ok.
    pub fn run_verb_line(&mut self, line: &str) -> VerbOutcome {
        // An IFC dialog's line, run as runIfcCommand runs it (main_window_ifc).
        if self.ifc.take_armed() {
            return self.run_ifc_dialog_line(line);
        }
        let Some(run) = self.log.begin_verb(line) else {
            return CommandLog::no_command();
        };
        let trimmed = run.line.clone();
        // Never type_into_tool: that is the whole difference from a typed line.
        if !self.run_workbench_line(&trimmed) {
            self.dispatch_line(&trimmed, LineSource::Executor);
        }
        self.log.end_verb(run)
    }

    /// MainWindow::runCommand: `line` as if typed on the command line and Enter pressed. False
    /// when the line logged an error.
    pub fn run_command(&mut self, line: &str) -> bool {
        let errors = self.log.errors_logged();
        self.run_command_line(line);
        self.log.errors_logged() == errors
    }

    /// MainWindow::runCommandLine: what the field held, one line or several.
    pub fn run_command_line(&mut self, text: &str) {
        let line = text.trim();
        if line.is_empty() {
            // Enter on an empty command line is Enter in the drawing, as in AutoCAD: the running
            // tool's (end a chain, take a default) or, with none running, the last tool again.
            self.with_parts(|workspace, parts| {
                let _ = workspace.press_enter(parts);
            });
            return;
        }
        // Several lines at once - pasted, or a --command holding line breaks - are a script, run
        // a line at a time and stopping at the first refused. But pasted at a prompt for text -
        // a Text's lines - they are that text.
        if line.contains('\n') || line.contains('\r') {
            if self.workspace.tool_takes_text(&self.document) {
                self.type_lines_into_tool(line);
            } else {
                self.run_pasted_lines(line);
            }
            return;
        }
        self.run_typed_line(line);
    }

    /// MainWindow::typeLinesIntoTool: a blank line is left out rather than typed; each other line
    /// goes through the typed path.
    fn type_lines_into_tool(&mut self, text: &str) {
        let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
        for each in normalised.split('\n') {
            let line = each.trim();
            if !line.is_empty() {
                self.run_typed_line(line);
            }
        }
    }

    /// The running tool's answer to a typed line (ViewWorkspace::typeIntoTool), lent the views'
    /// scenery.
    fn type_into_tool(&mut self, line: &str) -> bool {
        let scene = Rc::clone(&self.scene);
        let meshes = scene.meshes(self.document.model());
        let surfaces = scene.surfaces();
        let workspace = self.workspace.clone();
        workspace.type_into_tool(
            self,
            Scenery {
                surfaces: &surfaces,
                meshes: &meshes,
            },
            line,
        )
    }

    /// MainWindow::runTypedLine: one typed line, echoed, then offered to the workbenches' verbs,
    /// to a running tool and to dispatchLine, in that order.
    pub fn run_typed_line(&mut self, line: &str) {
        // A ZOOM typed while a tool runs is echoed by the executor that runs it.
        if !self.workspace.runs_transparently(&self.document, line) {
            self.log.echo_line(line);
        }
        // A tool waiting for typed text takes the whole line before any verb below; otherwise the
        // workbenches' verbs come before a running tool at a point or a pick, and so does a '#'
        // comment.
        if !self.workspace.tool_takes_text(&self.document)
            && (is_script_comment(line) || self.run_workbench_line(line))
        {
            return;
        }
        // While a tool runs, what is typed is its answer before it is anything else.
        if self.type_into_tool(line) {
            return;
        }
        self.dispatch_line(line, LineSource::Typed);
    }

    /// MainWindow::runWorkbenchLine: the UPDATE verbs first, then SURVEY, IFC's lines, the
    /// geoprocessing verbs (as jobs), ONLINE, ROADMODEL SHOW and the UTILITY verbs; false leaves
    /// the line to whoever asked.
    pub fn run_workbench_line(&mut self, line: &str) -> bool {
        if let Some(update) = self.update_lines.clone()
            && update.run_line(self, line)
        {
            return true;
        }
        // SURVEY READ and SURVEY IMPORT: the session's verb, so every front end reads and imports
        // a field file by one function and replies alike.
        if is_survey_line(line) {
            match run_survey_line(&mut self.document, line) {
                Ok(reply) => self.log_message(&reply, false),
                Err(error) => self.log_message(&error.describe(), true),
            }
            return true;
        }
        // A .ifc's IMPORT, EXPORT and INFO, and IFC RULES, are IFC's grammar before anything else.
        let words: Vec<&str> = line.split(' ').filter(|word| !word.is_empty()).collect();
        if let Some(first) = words.first() {
            let verb = first.to_uppercase();
            if matches!(verb.as_str(), "IMPORT" | "EXPORT" | "INFO" | "IFC") {
                let at = line.find(first).unwrap_or(0) + first.len();
                if self
                    .run_ifc_line(&verb, &line[at..], IfcLineFrom::CommandLine)
                    .is_some()
                {
                    return true;
                }
            }
        }
        // GDAL and the geoprocessing families after it, as background jobs.
        if lend!(self, geo, |geo, host| geo.run_line(host, line)).unwrap_or(false) {
            self.apply_geo_effects();
            return true;
        }
        if lend!(self, online, |online, host| online.run_line(host, line)).unwrap_or(false) {
            return true;
        }
        if lend!(self, road, |road, host| road.run_road_line(host, line)).unwrap_or(false) {
            self.take_road_events();
            return true;
        }
        if let Some(utilities) = self.utility_lines.clone() {
            return utilities.run_line(self, line);
        }
        false
    }

    /// MainWindow::dispatchLine: everything a line can be once no tool took it. Shared by the typed
    /// line and the executor's, which differ only where `source` says. The verbs taken here before
    /// the interpreter are listed for people by window_help_text.
    pub fn dispatch_line(&mut self, line: &str, source: LineSource) {
        // A note, as a katana_cli script has them.
        if is_script_comment(line) {
            return;
        }
        let words: Vec<&str> = line.split(' ').filter(|word| !word.is_empty()).collect();
        let Some(first) = words.first() else {
            return;
        };
        let verb = first.to_uppercase();
        let argument = words.get(1).map(|word| word.to_uppercase()).unwrap_or_default();

        // ON, OFF, or nothing to toggle - and anything else refused.
        let on_off = |word: &str, now: bool| -> Option<bool> {
            if word.is_empty() {
                return Some(!now);
            }
            match word {
                "ON" => Some(true),
                "OFF" => Some(false),
                _ => None,
            }
        };
        if verb == "GRID" {
            let on = if words.len() <= 2 {
                on_off(&argument, self.workspace.grid_visible())
            } else {
                None
            };
            let Some(on) = on else {
                self.log_message("usage: GRID [ON|OFF]", true);
                return;
            };
            self.workspace.set_grid_visible(on);
            self.log_message(&format!("grid={}", if on { "on" } else { "off" }), false);
            return;
        }
        // SNAP with the drafting verb's options (modes=, add=, remove=) is the interpreter's.
        if (verb == "SNAP" || verb == "OSNAP") && words[1..].iter().any(|word| word.contains('=')) {
            self.run_interpreter_line(line, &verb);
            return;
        }
        if verb == "SNAP" || verb == "OSNAP" {
            // SNAP <mode> [ON|OFF]: one of View > Snap Modes, by its name there.
            let mode = if argument == "CENTRE" {
                String::from("CENTER")
            } else {
                argument.clone()
            };
            if let Some(found) = SNAP_MENU_MODES
                .iter()
                .find(|each| !mode.is_empty() && each.name().to_uppercase() == mode)
            {
                let bit = *found as SnapModes;
                let now = self.document.drafting().snap_modes & bit != 0;
                let on = if words.len() <= 3 {
                    on_off(
                        &words.get(2).map(|word| word.to_uppercase()).unwrap_or_default(),
                        now,
                    )
                } else {
                    None
                };
                let Some(on) = on else {
                    self.log_message("usage: SNAP <mode> [ON|OFF]", true);
                    return;
                };
                let modes = self.document.drafting().snap_modes;
                ViewWorkspace::set_snap_modes(
                    &mut self.document,
                    if on { modes | bit } else { modes & !bit },
                );
                self.log_message(
                    &format!(
                        "snap_mode={} state={}",
                        found.name().to_lowercase(),
                        if on { "on" } else { "off" }
                    ),
                    false,
                );
                return;
            }
            let on = if words.len() <= 2 {
                on_off(&argument, ViewWorkspace::snap_enabled(&self.document))
            } else {
                None
            };
            let Some(on) = on else {
                self.log_message(
                    "usage: SNAP [ON|OFF] | SNAP <mode> [ON|OFF]   modes: Endpoint, Midpoint, \
                     Center, Intersection, Perpendicular, Tangent, Nearest, Grid | SNAP [on|off] \
                     [modes=a,b|all|none] [add=a,b] [remove=a,b] for every mode (HELP)",
                    true,
                );
                return;
            };
            self.with_parts(|workspace, parts| workspace.set_snap_enabled(parts, on));
            self.log_message(&format!("snap={}", if on { "on" } else { "off" }), false);
            return;
        }
        // EXAGGERATION [factor]: View > Vertical Exaggeration's line, and alone the factor now.
        if verb == "EXAGGERATION" {
            if words.len() == 1 {
                let factor = self.workspace.scene_options().vertical_exaggeration;
                self.log_message(
                    &format!("vertical_exaggeration={}", format_exact_real(factor)),
                    false,
                );
                return;
            }
            let factor = if words.len() == 2 {
                parse_finite_double(words[1])
            } else {
                None
            };
            match factor {
                Some(factor) if (LEAST_EXAGGERATION..=MOST_EXAGGERATION).contains(&factor) => {
                    self.set_vertical_exaggeration(factor);
                }
                _ => self.log_message(
                    "usage: EXAGGERATION [factor]   elevations times 0.01 to 1000",
                    true,
                ),
            }
            return;
        }
        if verb == "QUIT" || verb == "EXIT" {
            self.request(WindowRequest::Close);
            return;
        }
        // SCRIPT <file> [CONTINUE]: a katana_cli script, each line run through the one executor.
        if verb == "SCRIPT" {
            match parse_script_command(line) {
                Ok(command) => self.run_script(&command.path, command.continue_on_error),
                Err(error) => self.log_message(&error.describe(), true),
            }
            return;
        }
        // A .ifc takes the verbs' options exactly as katana_cli reads them.
        if matches!(verb.as_str(), "IMPORT" | "EXPORT" | "INFO" | "IFC")
            && self
                .run_ifc_line(&verb, &line[first.len()..], IfcLineFrom::CommandLine)
                .is_some()
        {
            return;
        }
        // PLOTSHEETS [path] [format=] [style=] [sheets=] [dpi=] [lineweight=] [folder=]
        // [pattern=]: the sheets plotted as the Plot dialog and --plot-sheets plot them.
        if verb == "PLOTSHEETS" {
            self.plot_sheets_line(line);
            return;
        }
        // PLOT <file.pdf> [paper=] [landscape|portrait] [fit|scale=] [dpi=] [style=]
        // [lineweight=] [margin=]: the drawing on one sheet.
        if verb == "PLOT" {
            match parse_plot_drawing(line) {
                Ok(request) => {
                    if let Err(error) =
                        self.plot_drawing_to_pdf(&path_from_utf8(&request.path), request.settings, request.fit)
                    {
                        self.log_message(&error.describe(), true);
                    }
                }
                Err(error) => self.log_message(&error.describe(), true),
            }
            return;
        }
        // SNAPSHOT <file> | CLIPBOARD [width=] [height=] [scale=] [bg=] [view=].
        if verb == "SNAPSHOT" {
            match parse_snapshot(line) {
                Ok(request) => self.snapshot_view(&request),
                Err(error) => self.log_message(&error.describe(), true),
            }
            return;
        }
        // HELP (or ?) alone: the interpreter's commands, then the verbs this front end runs
        // itself, which the interpreter cannot know of.
        if (verb == "HELP" || verb == "?") && words.len() == 1 {
            self.run_interpreter_line(line, &verb);
            self.log_message(&geo_help_text(), false);
            self.log_message(update_help_text(), false);
            self.log_message(window_help_text(), false);
            return;
        }
        if (verb == "HELP" || verb == "?") && words.len() == 2 && argument == "UPDATE" {
            self.log_message(update_help_text(), false);
            return;
        }
        // A bare tool word typed by a person starts the tool, as in any CAD package.
        if source == LineSource::Typed
            && words.len() == 1
            && let Some(id) = tool_id_for_command(&verb)
        {
            self.start_tool(&id);
            return;
        }
        self.run_interpreter_line(line, &verb);
    }

    /// MainWindow::runInterpreterLine: the interpreter's part of the command line, for a line no
    /// tool and no view verb took.
    pub fn run_interpreter_line(&mut self, line: &str, verb: &str) {
        // Not an OPEN of polylines (OPEN #12, OPEN SELECTION): that is an edit.
        let replaces_document = CommandInterpreter::replaces_document(line);
        if replaces_document && !self.confirm_discard() {
            return;
        }
        // As File > Save does - but only for a SAVE that has somewhere to go.
        if verb == "SAVE" && typed_save_has_destination(line, self.document.has_project()) {
            self.record_references();
        }
        let reply = {
            let scene = Rc::clone(&self.scene);
            let meshes = scene.meshes(self.document.model());
            let surfaces = scene.surfaces();
            let mut parts = ViewParts {
                document: &mut self.document,
                reference: Some(&self.reference),
                meshes: &meshes,
            };
            self.lines
                .run_line(&mut self.interpreter, &mut parts, &surfaces, line)
        };
        match reply {
            Err(error) => {
                self.log_message(&error.describe(), true);
                return;
            }
            Ok(reply) => self.log_message(&reply, false),
        }
        if replaces_document {
            // The same as File > New and Open: the backdrop and the meshes went with the drawing.
            self.clear_reference_data();
            self.clear_scene_data();
            self.zoom_extents_all();
        }
        if replaces_document && verb == "OPEN" {
            self.report_missing_customisation();
            self.restore_references();
        }
        self.history_cursor = self.interpreter.history().len();
    }

    /// MainWindow::startTool: catalogue tool `id` in the active plan view, or why it cannot.
    pub fn start_tool(&mut self, id: &str) {
        let started = self.with_parts(|workspace, parts| workspace.start_tool(parts, id));
        if let Err(error) = started {
            self.log_message(&error.describe(), true);
        }
    }

    // ---- scripts -------------------------------------------------------------------------

    /// MainWindow::runScript: SCRIPT <file> [CONTINUE], the file's lines through the executor,
    /// stopping at the first refused unless `continue_on_error`.
    pub fn run_script(&mut self, path: &str, continue_on_error: bool) {
        let canonical = std::fs::canonicalize(path_from_utf8(path))
            .map(|found| String::from_utf8_lossy(&path_to_utf8(&found)).into_owned())
            .unwrap_or_default();
        if !canonical.is_empty() && self.running_scripts.contains(&canonical) {
            let name = Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.log_message(
                &format!("SCRIPT: {name} is already running; a script may not run itself."),
                true,
            );
            return;
        }
        let lines = match read_script(path) {
            Ok(lines) => lines,
            Err(error) => {
                self.log_message(&error.describe(), true);
                return;
            }
        };
        self.running_scripts.push(canonical.clone());
        let shown = crate::ifc_dialogs::from_native_separators(path);
        self.run_lines(&lines, &shown, continue_on_error);
        self.running_scripts.pop();
        // A headless run is a test's or an agent's, and leaves a person's list alone.
        if !self.headless && !canonical.is_empty() {
            let mut recent: Vec<String> = self
                .settings
                .list(RECENT_SCRIPTS_KEY)
                .map(<[String]>::to_vec)
                .unwrap_or_default();
            recent.retain(|each| *each != canonical);
            recent.insert(0, canonical);
            recent.truncate(RECENT_SCRIPTS);
            self.settings.set_list(RECENT_SCRIPTS_KEY, recent);
        }
    }

    /// MainWindow::runPastedLines.
    fn run_pasted_lines(&mut self, text: &str) {
        let lines = script_lines(text);
        if lines.is_empty() {
            return;
        }
        self.run_lines(&lines, "", false);
    }

    /// MainWindow::runLines: the run, and the record and the reason it stopped, logged. The C++'s
    /// progress dialog is not drawn: the run happens inside one call, and no frame comes until it
    /// is over (docs/rust.md records the difference).
    fn run_lines(&mut self, lines: &[ScriptLine], name: &str, continue_on_error: bool) {
        let report = run_script_lines(
            lines,
            &mut |line: &str| self.run_verb_line(line),
            ScriptOptions {
                continue_on_error,
                progress: None,
            },
        );
        self.log_message(&format_script_report(name, &report), false);
        let shown = if name.is_empty() {
            String::from("The pasted lines")
        } else {
            Path::new(name)
                .file_name()
                .map(|file| file.to_string_lossy().into_owned())
                .unwrap_or_else(|| String::from(name))
        };
        let text_at = |number: usize| {
            lines
                .iter()
                .find(|line| line.number == number)
                .map(|line| line.text.clone())
                .unwrap_or_default()
        };
        if report.cancelled {
            self.log_message(
                &format!(
                    "{shown}: cancelled before line {}; the lines from there were not run.",
                    report.stopped_at
                ),
                true,
            );
        } else if report.failed > 0 && !continue_on_error {
            self.log_message(
                &format!(
                    "{shown} stopped at line {}, which was refused: {}",
                    report.stopped_at,
                    text_at(report.stopped_at)
                ),
                true,
            );
        } else if report.failed > 0 {
            self.log_message(
                &format!(
                    "{shown}: {} of the {} lines run were refused.",
                    report.failed, report.ran
                ),
                true,
            );
        }
    }

    // ---- the drawing as a file -----------------------------------------------------------

    /// MainWindow::confirmDiscard: false when the person cancels, or a headless run refuses.
    pub fn confirm_discard(&mut self) -> bool {
        if !self.document.is_modified() {
            return true;
        }
        if std::mem::take(&mut self.discard_agreed) {
            return true;
        }
        // A headless run has nobody to answer the box. Refused and said, never discarded unasked.
        if self.headless {
            self.log_message(
                "Unsaved Changes: the drawing has unsaved changes, and a headless run has nobody \
                 to ask whether to discard them; SAVE or UNDO first.",
                true,
            );
            return false;
        }
        match ask_three(
            "Unsaved Changes",
            "The drawing has unsaved changes. Save them first?\n\nYes saves them, No discards \
             them.",
        ) {
            Answer::Yes => self.save_document(),
            Answer::No => true,
            Answer::Cancel => false,
        }
    }

    /// MainWindow::newDocument.
    pub fn new_document(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        // The views and the interpreter both hold state that belongs to the drawing being
        // discarded: half-picked points, and the last point relative coordinates measure from.
        self.with_parts(|workspace, parts| workspace.reset_interaction(parts));
        self.interpreter.reset_point_state();
        self.document.new_document();
        self.clear_reference_data();
        self.clear_scene_data();
        self.zoom_extents_all();
        self.log_message("New drawing.", false);
    }

    /// MainWindow::refuseFileDialog: in a headless session, says that no file dialog opens and
    /// names `verb`, the line that does the same without one.
    pub fn refuse_file_dialog(&mut self, verb: &str) -> bool {
        if !self.headless {
            return false;
        }
        self.log_message(
            &format!("A headless session opens no file dialog: type {verb} instead."),
            true,
        );
        true
    }

    /// MainWindow::openProject: a project given on the command line, or chosen.
    pub fn open_project(&mut self, directory: &Path) {
        let mut status = self.document.open(directory);
        // Damaged database: offer the newest sound backup. A headless run does not restore.
        if let Err(error) = &status
            && error.code == ErrorCode::DatabaseFailure
            && !self.headless
            && ask_yes_no(
                "Project Damaged",
                &format!(
                    "{}\n\nRestore the most recent intact backup? The damaged file is kept.",
                    error.describe()
                ),
            )
        {
            status = match ProjectStore::recover(directory) {
                Ok(report) => {
                    let name = report
                        .restored_from
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.log_message(&format!("Restored from {name}"), false);
                    self.document.open(directory)
                }
                Err(error) => Err(error),
            };
        }
        if let Err(error) = status {
            self.warn_user("Open Failed", &error.describe());
            return;
        }
        self.with_parts(|workspace, parts| workspace.reset_interaction(parts));
        self.interpreter.reset_point_state();
        self.clear_reference_data();
        self.clear_scene_data();
        self.zoom_extents_all();
        let shown = String::from_utf8_lossy(&path_to_utf8(directory)).into_owned();
        let count = self.document.model().entities.size();
        self.log_message(&format!("Opened {shown} ({count} entities)."), false);
        self.report_missing_customisation();
        self.restore_references();
    }

    /// MainWindow::saveDocument.
    pub fn save_document(&mut self) -> bool {
        if !self.document.has_project() {
            return self.save_document_as();
        }
        self.record_references();
        if let Err(error) = self.document.save() {
            self.warn_user("Save Failed", &error.describe());
            return false;
        }
        self.log_message("Saved.", false);
        true
    }

    /// MainWindow::saveDocumentAs.
    pub fn save_document_as(&mut self) -> bool {
        if self.refuse_file_dialog("SAVE <directory>") {
            return false;
        }
        let start = self.project_folder_for_dialogs();
        let Some(chosen) = rfd::FileDialog::new()
            .set_title("Save Project As")
            .set_directory(&start)
            .set_file_name("untitled.katana")
            .add_filter("Katana project", &["katana"])
            .save_file()
        else {
            return false;
        };
        let mut target = String::from_utf8_lossy(&path_to_utf8(&chosen)).into_owned();
        if !target.to_lowercase().ends_with(".katana") {
            target.push_str(".katana");
        }
        self.record_references();
        if let Err(error) = self.document.save_as(&path_from_utf8(&target)) {
            self.warn_user("Save Failed", &error.describe());
            return false;
        }
        self.remember_project_folder(&target);
        self.log_message(&format!("Saved to {target}"), false);
        true
    }

    /// File > Open's folder dialog, then the project.
    pub fn open_document(&mut self) {
        if !self.confirm_discard() || self.refuse_file_dialog("OPEN <directory>") {
            return;
        }
        let start = self.project_folder_for_dialogs();
        if let Some(directory) = rfd::FileDialog::new()
            .set_title("Open Katana Project (select the .katana folder)")
            .set_directory(&start)
            .pick_folder()
        {
            let text = String::from_utf8_lossy(&path_to_utf8(&directory)).into_owned();
            self.remember_project_folder(&text);
            self.open_project(&directory);
        }
    }

    /// projectFolderForDialogs: where File > Open and Save As start.
    fn project_folder_for_dialogs(&self) -> PathBuf {
        let parent = self
            .document
            .project_directory()
            .and_then(|directory| directory.parent().map(Path::to_path_buf))
            .map(|parent| String::from_utf8_lossy(&path_to_utf8(&parent)).into_owned())
            .unwrap_or_default();
        let remembered = self
            .settings
            .text(PROJECT_FOLDER_KEY)
            .map(String::from)
            .unwrap_or_default();
        let documents = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(|home| PathBuf::from(home).join("Documents"))
            .map(|folder| String::from_utf8_lossy(&path_to_utf8(&folder)).into_owned())
            .unwrap_or_default();
        path_from_utf8(&crate::project_folder::project_dialog_folder(
            &parent,
            &remembered,
            &documents,
        ))
    }

    fn remember_project_folder(&mut self, project: &str) {
        let parent = Path::new(project)
            .parent()
            .map(|parent| String::from_utf8_lossy(&path_to_utf8(parent)).into_owned())
            .unwrap_or_default();
        if !parent.is_empty() {
            self.settings.set_text(PROJECT_FOLDER_KEY, &parent);
        }
    }

    /// Edit > Select All, and --select-all.
    pub fn select_all(&mut self) {
        let reply = self
            .interpreter
            .run(&mut self.document, "SELECT ALL")
            .unwrap_or_default();
        self.log_message(&reply, false);
    }

    /// --attributes ID: just the one entity selected.
    pub fn select_only(&mut self, id: EntityId) {
        let reply = self
            .interpreter
            .run(&mut self.document, &format!("SELECT {id}"))
            .unwrap_or_default();
        self.log_message(&reply, false);
    }

    /// Survey > Apply Survey Codes: CODE SELECTION when something is selected, else CODE DRAWING.
    pub fn apply_survey_codes(&mut self) {
        let line = if self.document.selection().empty() {
            "CODE DRAWING"
        } else {
            "CODE SELECTION"
        };
        let _ = self.run_verb_line(line);
    }

    /// Survey > Process Linework: LINEWORK SELECTION or LINEWORK DRAWING.
    pub fn survey_linework(&mut self) {
        let line = if self.document.selection().empty() {
            "LINEWORK DRAWING"
        } else {
            "LINEWORK SELECTION"
        };
        let _ = self.run_verb_line(line);
    }

    // ---- the customisation the session starts with --------------------------------------

    /// MainWindow::loadDefaultCustomisation: the customisation the person kept, else the one built
    /// into the program, else none; what was installed is logged. After setHeadless: a headless
    /// run with KATANA_CUSTOMISATION unset keeps none.
    pub fn load_default_customisation(&mut self) {
        let mut host = CustomisationHost {
            built_in: built_in_customisation(),
            ..CustomisationHost::default()
        };
        let kept = environment_variable(Some(KEPT_CUSTOMISATION_VARIABLE));
        if !kept.is_empty() {
            host.kept_file = path_from_utf8(&String::from_utf8_lossy(&kept));
        } else if !self.headless {
            // The per-user place, beside the window's own settings file.
            if let Some(folder) = UserSettings::default_path().parent()
                && !folder.as_os_str().is_empty()
            {
                host.kept_file = folder.join("customisation.json");
            }
        }
        self.kept_customisation_file.clone_from(&host.kept_file);
        self.has_built_in_customisation = host.built_in.customisation.is_some();
        // The interpreter first: it notes the kept file as it is NOW.
        self.interpreter.set_customisation_host(host.clone());
        let start = start_customisation(&mut self.document, &host);
        for problem in &start.problems {
            self.log_message(&format!("Customisation: {problem}"), true);
        }
        if start.installed == CustomisationOrigin::None {
            return;
        }
        let count = |count: usize, one: &str, many: &str| {
            format!("{} {}", grouped(count as u64), if count == 1 { one } else { many })
        };
        let message = format!(
            "Customisation: {}, {} ({}) and {}, {}",
            start.name,
            count(start.definitions, "definition", "definitions"),
            count(start.symbols, "symbol", "symbols"),
            count(start.rules, "survey code rule", "survey code rules"),
            if start.installed == CustomisationOrigin::Kept {
                "kept."
            } else {
                "built in."
            }
        );
        self.log_message(&message, false);
        if start.kept_from_another_built_in {
            self.log_message(
                "The kept customisation was made from another built-in customisation than this \
                 program has; CUSTOMISE RESET gives this program's and CUSTOMISE BUILTIN lists the \
                 built-in customisations it has.",
                false,
            );
        }
    }

    /// MainWindow::reportMissingCustomisation: after an open, which customisations the project
    /// records that are not loaded.
    pub fn report_missing_customisation(&mut self) {
        let missing = self.document.customisation_state().missing_at_open.clone();
        if missing.is_empty() {
            return;
        }
        let advice = built_in_advice_for(&missing, built_ins());
        let quoted: Vec<String> = missing.iter().map(|name| format!("\"{name}\"")).collect();
        let mut warning = format!(
            "Warning: this project was drawn with customisations that are not loaded: {}. \
             CUSTOMISE <file> loads a Katana customisation file.",
            quoted.join(", ")
        );
        if !advice.is_empty() {
            warning.push_str(&format!(" {advice}."));
        }
        self.log_message(&warning, false);
    }

    // ---- reference data -----------------------------------------------------------------

    /// Before a save: the project records the reference layers as they are.
    pub fn record_references(&mut self) {
        self.geo(Box::new(|context: &mut Context<'_>| record_references(context)));
    }

    /// After an open: the layers the project records, read again through the one executor.
    pub fn restore_references(&mut self) {
        let mut records = false;
        self.geo(Box::new(|context: &mut Context<'_>| {
            records = records_references(context);
        }));
        if records {
            let _ = self.run_verb_line("REFS RESTORE");
        }
    }

    /// Drops the rasters and point clouds, with the drawing they were loaded beside.
    pub fn clear_reference_data(&mut self) {
        if self.reference.empty() {
            return;
        }
        self.reference.clear();
        self.workspace.invalidate_reference_cache(Some(&self.reference));
    }

    /// Drops the meshes the same way and for the same reason: they are session data beside the
    /// drawing. The surfaces are the drawing's, which New emptied and Open read.
    pub fn clear_scene_data(&mut self) {
        let mut store = (*self.scene).clone();
        store.meshes.clear();
        self.scene = Rc::new(store);
        self.workspace.drawing_replaced();
    }

    /// MainWindow::addMesh: session data like a surface, drawn in 3D with its footprint in plan.
    pub fn add_mesh(&mut self, mesh: MeshItem) {
        let mut store = (*self.scene).clone();
        store.meshes.push(mesh);
        self.scene = Rc::new(store);
        self.workspace.scenery_changed();
    }

    /// What a geo phase asked of the window, applied now that the phase is over (GeoEffects).
    pub fn apply_geo_effects(&mut self) {
        for effect in self.effects.take() {
            match effect {
                crate::geo::GeoEffect::Frame(area) => self.workspace.zoom_to(&area),
                crate::geo::GeoEffect::Changed => {
                    self.workspace.invalidate_reference_cache(Some(&self.reference));
                    self.sync_scene_surfaces(true);
                    self.workspace.scenery_changed();
                }
                crate::geo::GeoEffect::Imported(shown) => self.show_imported(shown),
                crate::geo::GeoEffect::AskFarApart(question) => self.ask_far_apart(&question),
            }
        }
    }

    /// GeoServices::imported: an archive's meshes held and shown, and the 3D view opened once for
    /// a surface or a mesh.
    fn show_imported(&mut self, shown: katana_app::geo::ImportShown) {
        // A colour Katana has no RGB for leaves the mesh its default clay.
        let clay = katana_render::rgba(190, 170, 140);
        let surfaces = shown.surfaces;
        let any_mesh = !shown.meshes.is_empty();
        for mesh in shown.meshes {
            let to_rgba = |colour: &Option<katana_entity::Color>, fallback| match colour {
                Some(colour) => katana_render::rgba(colour.r, colour.g, colour.b),
                None => fallback,
            };
            let base = to_rgba(&mesh.color, clay);
            let faces = mesh
                .face_colors
                .iter()
                .map(|colour| to_rgba(colour, base))
                .collect();
            self.add_mesh(MeshItem {
                scene: SceneMesh {
                    name: mesh.name.clone(),
                    mesh: None,
                    flat_color: base,
                    face_colors: faces,
                    ..SceneMesh::default()
                },
                held: Arc::new(mesh.mesh),
            });
        }
        if surfaces != 0 || any_mesh {
            self.with_parts(|workspace, parts| {
                let _ = workspace.ensure_view(parts, katana_cad::viewport_layout::ViewKind::Model3D);
            });
            self.request(WindowRequest::ViewsChanged);
            self.workspace.scenery_changed();
        }
    }

    /// The far-apart question an import's apply queued: the person's answer is given to the hook,
    /// and the line run again (GeoEffects).
    fn ask_far_apart(&mut self, question: &crate::geo::geo_workbench::FarApartQuestion) {
        if self.headless || question.line.is_empty() {
            return;
        }
        let choice = match ask_three(
            "Far from the current drawing",
            &format!(
                "{}.\n\nYes shifts everything in the file as one piece so it sits beside the \
                 drawing; No keeps the survey coordinates.",
                question.advice
            ),
        ) {
            Answer::Yes => katana_app::geo::FarApartChoice::Alongside,
            Answer::No => katana_app::geo::FarApartChoice::Keep,
            Answer::Cancel => return,
        };
        self.effects.answer_far_apart(choice);
        let _ = self.run_verb_line(&question.line);
    }

    /// The road workbench's events: the road model among the views' meshes, the Alignment
    /// Manager opened, the views repainted.
    pub fn take_road_events(&mut self) {
        let events = match self.road.as_mut() {
            Some(road) => road.take_events(),
            None => return,
        };
        for event in events {
            match event {
                crate::bim::road_workbench::RoadWorkbenchEvent::ShowAlignmentManager => {
                    self.request(WindowRequest::ShowAlignmentManager);
                }
                crate::bim::road_workbench::RoadWorkbenchEvent::RoadMeshes(_) => {
                    let shown = self
                        .road
                        .as_ref()
                        .is_some_and(|road| road.road_model_checked());
                    let mut store = (*self.scene).clone();
                    store.road = shown.then(|| katana_cad::bim::bim_object::bim_model(&self.document));
                    self.scene = Rc::new(store);
                    self.workspace.scenery_changed();
                }
                crate::bim::road_workbench::RoadWorkbenchEvent::RefreshViews => {
                    self.workspace.scenery_changed();
                }
            }
        }
    }

    // ---- 3D views and sections ----------------------------------------------------------

    /// EXAGGERATION <factor>: elevations in the 3D and section views times `factor`, about the
    /// middle of the data.
    pub fn set_vertical_exaggeration(&mut self, factor: f64) {
        let mut options = self.workspace.scene_options();
        options.vertical_exaggeration = factor;
        let mut flat = options;
        flat.vertical_exaggeration = 1.0;
        let scene = Rc::clone(&self.scene);
        let surfaces = scene.surfaces();
        let meshes = scene.meshes(self.document.model());
        let bounds = scene_bounds(&self.document, &surfaces, &flat, &meshes);
        options.exaggeration_datum = if bounds.empty() {
            0.0
        } else {
            bounds.center().z
        };
        self.workspace.set_scene_options(&self.document, options);
        if let Some(id) = self.workspace.active_section_view() {
            let _ = self
                .workspace
                .with_section_view(id, |view, state| view.set_vertical_exaggeration(state, factor));
        }
        if let Some(id) = self.workspace.active_render_view() {
            let scenery = Scenery {
                surfaces: &surfaces,
                meshes: &meshes,
            };
            let _ = self.workspace.with_render_view(
                id,
                &self.document,
                scenery,
                |view, scene, state| view.zoom_extents(scene, state),
            );
        }
        self.log_message(&format!("Vertical exaggeration x{factor:.2}."), false);
    }

    /// Cut a section along `alignment` from the drawing's shown surfaces, and show it.
    pub fn cut_section_along(
        &mut self,
        alignment: Polyline2,
        along: &str,
        design: Option<SectionDesign>,
    ) {
        let cut = cut_drawing_section(&self.document, &alignment, design.as_ref());
        let cut = match cut {
            Ok(cut) => cut,
            Err(error) => {
                self.log_message(&section_refusal(&error), true);
                return;
            }
        };
        let length = cut.section.length;
        let crossings = cut.section.crossings.len();
        let interval = cut.interval;
        let shown = self.with_parts(|workspace, parts| workspace.show_section(parts, cut.section, design));
        if !shown {
            self.log_message("No viewport could show the section.", true);
            return;
        }
        self.request(WindowRequest::ViewsChanged);
        self.log_message(
            &format!(
                "Section cut along {along}: length {length:.3}, interval {interval:.3}, \
                 {crossings} crossings."
            ),
            false,
        );
    }

    /// MainWindow::recutSections: every open section whose drawing's surfaces have changed, cut
    /// again from its own recipe.
    pub fn recut_sections(&mut self) {
        let workspace = self.workspace.clone();
        let document = &self.document;
        let mut cutter = |alignment: &Polyline2, design: Option<&SectionDesign>| {
            cut_drawing_section(document, alignment, design).map(|cut| cut.section)
        };
        let recut = workspace.recut_sections(document, &mut cutter);
        for id in recut.cut {
            self.log_message(
                &format!("Section in view {id} cut again: the drawing's surfaces changed."),
                false,
            );
        }
        for (id, error) in recut.refused {
            self.log_message(
                &format!(
                    "Section in view {id} is of surfaces that have changed and was not cut again: \
                     {}",
                    section_refusal(&error)
                ),
                false,
            );
        }
    }

    // ---- plotting and pictures ------------------------------------------------------------

    /// What sheets are drawn from: the drawing, this window's surfaces, meshes and reference
    /// layers, the set's logo and the project's fields (MainWindow::sheetSource).
    pub fn sheet_source<'s>(
        document: &'s Document,
        reference: &'s ReferenceData,
        surfaces: Vec<SceneSurface<'s>>,
        meshes: &'s [SceneMesh<'s>],
    ) -> SheetSource<'s> {
        let mut plan = plan_source_of(document);
        plan.reference = Some(reference);
        plan.meshes = Some(meshes);
        // The drawing's revision, and which surfaces there are: a section cut before a surface was
        // built is cut again once there is one.
        let mut revision = document.model_revision();
        for surface in &surfaces {
            let address = surface
                .surface
                .map_or(0, |surface| std::ptr::from_ref(surface) as usize as u64);
            revision = revision
                .wrapping_mul(1_000_003)
                .wrapping_add(address)
                .wrapping_add(u64::from(surface.visible));
        }
        let today = katana_core::civil_date::today_utc();
        let logo = logo_path(document)
            .and_then(|path| crate::canvas::read_image(&path).ok())
            .unwrap_or_default();
        SheetSource {
            plan,
            document: Some(document),
            surfaces,
            revision,
            fields: field_context_for(document, frame_date(today)),
            logo,
            assets: document
                .project_directory()
                .map(|directory| directory.join("assets"))
                .unwrap_or_default(),
        }
    }

    /// MainWindow::sheetsToPlot: the project's sheets, or, when it has none, one sheet fitted to
    /// the drawing, laid out for this plot only.
    pub fn sheets_to_plot(&mut self) -> Result<SheetSet> {
        let mut set = (*self.document.sheet_set()).clone();
        if set.sheets.is_empty() {
            let scene = Rc::clone(&self.scene);
            let meshes = scene.meshes(self.document.model());
            let area = {
                let source = Self::sheet_source(&self.document, &self.reference, Vec::new(), &meshes);
                plan_drawn_bounds(&source.plan, &Default::default(), &Default::default())
            };
            let request = LayoutRequest {
                plan_area: Some(area),
                ..LayoutRequest::default()
            };
            let mut fitted = smart_layout(self.document.model(), &request, None)?;
            prepare_for_append(&set, &mut fitted);
            set.sheets = fitted;
            self.log_message(
                "The project has no sheets: plotting one fitted to the drawing.",
                false,
            );
        }
        Ok(set)
    }

    /// MainWindow::plotSheets: the checks logged, then the plot, its problems and summary logged.
    pub fn plot_sheets(&mut self, set: &SheetSet, request: &PlotRequest) -> Result<PlotReport> {
        let mut titled = request.clone();
        if titled.title.is_empty() {
            titled.title.clone_from(&self.document.metadata().name);
        }
        let scene = Rc::clone(&self.scene);
        let meshes = scene.meshes(self.document.model());
        let surfaces: Vec<SceneSurface<'_>> = scene
            .surfaces()
            .into_iter()
            .filter(|surface| surface.visible)
            .collect();
        let (findings, result) = {
            let source = Self::sheet_source(&self.document, &self.reference, surfaces, &meshes);
            let findings = parse_sheet_selection(&titled.sheets, set)
                .map(|selected| preflight_log(&check_sheets_for(set, &source, &selected)))
                .unwrap_or_default();
            let mut cache = SheetPaintCache::new();
            (findings, plot_sheets_from(set, &titled, &source, &mut cache, None))
        };
        for line in findings {
            self.log_message(&line, false);
        }
        let result = result?;
        for problem in &result.problems {
            self.log_message(problem, true);
        }
        self.log_message(&result.summary(&titled), false);
        Ok(result)
    }

    /// PLOTSHEETS: read by the interpreter's own rules against the sheets this plot takes, each
    /// file written logged as file="path" after the summary. The problems a plot carried out with
    /// do not count against the line.
    fn plot_sheets_line(&mut self, line: &str) {
        let tokens = match CommandInterpreter::tokenize(line) {
            Ok(tokens) => tokens,
            Err(error) => {
                self.log_message(&error.describe(), true);
                return;
            }
        };
        let set = match self.sheets_to_plot() {
            Ok(set) => set,
            Err(error) => {
                self.log_message(&error.describe(), true);
                return;
            }
        };
        let parsed = match parse_plot_sheets(&set, &tokens[1..]) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.log_message(&error.describe(), true);
                return;
            }
        };
        let request = plot_request_for_words(&set.page_setup, &parsed);
        let errors_before = self.log.errors_logged();
        match self.plot_sheets(&set, &request) {
            Ok(report) => {
                for file in &report.files {
                    self.log_message(
                        &format!("file=\"{}\"", crate::ifc_dialogs::to_native_separators(file)),
                        false,
                    );
                }
                self.log.set_errors_logged(errors_before);
            }
            Err(error) => self.log_message(&error.describe(), true),
        }
    }

    /// MainWindow::plotDrawingToPdf: the drawing plotted on the active plan view, fitted to what
    /// it draws or about the view's centre.
    pub fn plot_drawing_to_pdf(
        &mut self,
        path: &Path,
        mut settings: PlotSettings,
        fit: bool,
    ) -> Status {
        let Some(view) = self.workspace.active_plan_view() else {
            return Err(make_error(
                ErrorCode::InvalidState,
                "no plan viewport to plot from",
                "",
            ));
        };
        let mut unplotted = Vec::new();
        let plotted = self.with_parts(|workspace, parts| {
            workspace
                .with_plan_view(view, |plan, state| -> Status {
                    if fit {
                        settings = plan.fitted_plot(&*parts, state, settings)?;
                    } else {
                        settings.center = state.plan.center;
                    }
                    let mut writer = CanvasPdfWriter { readable: false };
                    plan.plot_to_pdf(
                        &*parts,
                        state,
                        path,
                        &settings,
                        Some(&mut unplotted),
                        &mut writer,
                    )
                })
                .unwrap_or_else(|| {
                    Err(make_error(
                        ErrorCode::InvalidState,
                        "no plan viewport to plot from",
                        "",
                    ))
                })
        });
        plotted?;
        let shown = String::from_utf8_lossy(&path_to_utf8(path)).into_owned();
        let paper = paper_name(settings.paper);
        let orientation = if settings.landscape {
            "landscape"
        } else {
            "portrait"
        };
        self.log_message(
            &format!(
                "Plotted to {shown}: {paper} {orientation} at 1 : {:.0}, {:.0} dpi. Line widths are \
                 the layers' line weights in millimetres.",
                settings.scale_denominator, settings.dpi
            ),
            false,
        );
        for problem in &unplotted {
            self.log_message(&problem.record(), false);
        }
        self.log_message(
            &format!(
                "file=\"{}\" paper={paper} orientation={orientation} scale={} dpi={} style={} \
                 lineweight={}",
                crate::ifc_dialogs::to_native_separators(&shown),
                qt_general(settings.scale_denominator),
                qt_general(settings.dpi),
                settings.colour_mode.name(),
                qt_general(settings.line_weight_scale)
            ),
            false,
        );
        Ok(())
    }

    /// SNAPSHOT: the active plan view painted afresh at the size asked, or the 3D view drawn and
    /// scaled; written, or put on the clipboard, and a record logged.
    pub fn snapshot_view(&mut self, request: &SnapshotRequest) {
        let (image, view_name) = if request.view == SnapshotView::Plan {
            let Some(id) = self.workspace.active_plan_view() else {
                self.log_message(
                    "SNAPSHOT: no plan view is open; open one, or ask for view=3d.",
                    true,
                );
                return;
            };
            let background = match request.background {
                Background::Theme => crate::theme::palette::viewport(),
                Background::White => crate::canvas::Color::rgb(255, 255, 255),
                Background::None => crate::canvas::Color::rgba(0, 0, 0, 0),
            };
            let as_plot = request.background == Background::White;
            let image = self.with_parts(|workspace, parts| {
                workspace.with_plan_view(id, |plan, state| {
                    let (width, height) = snapshot_size(request, plan.size());
                    plan.render_to_image(
                        &*parts,
                        state,
                        width.max(1) as u32,
                        height.max(1) as u32,
                        background,
                        as_plot,
                    )
                })
            });
            let Some(image) = image else {
                self.log_message(
                    "SNAPSHOT: no plan view is open; open one, or ask for view=3d.",
                    true,
                );
                return;
            };
            (image, "plan")
        } else {
            let Some(id) = self.workspace.active_render_view() else {
                self.log_message(
                    "SNAPSHOT: no 3D view is open; open one, or ask for view=plan.",
                    true,
                );
                return;
            };
            let scene = Rc::clone(&self.scene);
            let surfaces = scene.surfaces();
            let meshes = scene.meshes(self.document.model());
            let image = self.workspace.with_render_view(
                id,
                &self.document,
                Scenery {
                    surfaces: &surfaces,
                    meshes: &meshes,
                },
                |view, scene, state| {
                    // Grabbed as drawn - the renderer's own frame - and scaled.
                    let (width, height) = snapshot_size(request, view.size());
                    let drawn = view.paint(scene, state).clone();
                    scaled(&drawn, width.max(1) as u32, height.max(1) as u32)
                },
            );
            let Some(image) = image else {
                self.log_message(
                    "SNAPSHOT: no 3D view is open; open one, or ask for view=plan.",
                    true,
                );
                return;
            };
            (image, "3d")
        };
        let record = format!(
            "view={view_name} width={} height={}",
            image.width(),
            image.height()
        );
        if request.clipboard {
            let _ = self.clipboard.set_image(&image);
            self.log_message(&format!("clipboard=yes {record}"), false);
            return;
        }
        if let Err(error) = write_snapshot(&image, &path_from_utf8(&request.path)) {
            self.log_message(&error.describe(), true);
            return;
        }
        self.log_message(
            &format!(
                "file=\"{}\" {record}",
                crate::ifc_dialogs::to_native_separators(&request.path)
            ),
            false,
        );
    }

    // ---- the frame's turn ---------------------------------------------------------------

    /// What the drawing did since the last look, carried out now: the scene's surfaces made again
    /// when they changed (at once in the C++, where the views pointed at them), the open sections
    /// cut again, the road model followed. The panels read the drawing as they are drawn.
    pub fn follow_drawing(&mut self) {
        let parts = self.heard.take();
        let change = DocumentChange::of(parts);
        if change.has(DocumentChange::SURFACES | DocumentChange::SURFACE_STYLES) {
            self.sync_scene_surfaces(!change.has(DocumentChange::REPLACED));
        }
        if change.has(DocumentChange::SURFACES) && !change.has(DocumentChange::REPLACED) {
            self.recut_sections();
        }
        if change.has(DocumentChange::SURFACE_STYLES | DocumentChange::REPLACED) {
            self.restyle_scene_surfaces();
        }
        if parts != 0 {
            let document = &self.document;
            if let Some(road) = self.road.as_mut() {
                road.sync_views(document);
            }
            self.take_road_events();
            self.workspace.prune_view_layers(&self.document);
        }
    }

    /// A job's ends delivered and their effects applied: the start of every frame.
    pub fn deliver_jobs(&mut self, context: &egui::Context) {
        let _ = crate::jobs::deliver_jobs(self, context);
        self.apply_geo_effects();
    }
}

/// The modes View > Snap Modes lists, in its order (viewSnapModeEndpoint ... viewSnapModeGrid).
pub const SNAP_MENU_MODES: [SnapMode; 8] = [
    SnapMode::Endpoint,
    SnapMode::Midpoint,
    SnapMode::Center,
    SnapMode::Intersection,
    SnapMode::Perpendicular,
    SnapMode::Tangent,
    SnapMode::Nearest,
    SnapMode::Grid,
];

/// Why a section could not be cut, in the window's words.
pub fn section_refusal(error: &Error) -> String {
    match error.code {
        ErrorCode::NotFound => String::from("Build a surface first (Terrain > Surface From ...)."),
        ErrorCode::InvalidState => String::from(
            "Every surface is hidden: show one first (Terrain > Surfaces Panel, or SURFACE SHOW \
             <name>).",
        ),
        _ => error.describe(),
    }
}

/// A paper's name as the plot record writes it.
pub fn paper_name(size: katana_cad::PaperSize) -> &'static str {
    match size {
        katana_cad::PaperSize::A0 => "A0",
        katana_cad::PaperSize::A1 => "A1",
        katana_cad::PaperSize::A2 => "A2",
        katana_cad::PaperSize::A3 => "A3",
        katana_cad::PaperSize::A4 => "A4",
    }
}

/// `image` at `width` by `height`, each pixel the area-weighted mean of what it covers in the
/// source: QImage::scaled with Qt::SmoothTransformation, for a 3D view grabbed at its own size and
/// written at another. The same image when the size is its own.
pub fn scaled(image: &crate::canvas::Image, width: u32, height: u32) -> crate::canvas::Image {
    if image.width() == width && image.height() == height || image.is_null() {
        return image.clone();
    }
    let mut out = crate::canvas::Image::new(width, height);
    let sx = f64::from(image.width()) / f64::from(width);
    let sy = f64::from(image.height()) / f64::from(height);
    for y in 0..height {
        let y0 = f64::from(y) * sy;
        let y1 = y0 + sy;
        for x in 0..width {
            let x0 = f64::from(x) * sx;
            let x1 = x0 + sx;
            let mut sum = [0.0f64; 4];
            let mut weight = 0.0;
            let mut row = y0.floor() as u32;
            while f64::from(row) < y1 && row < image.height() {
                let wy = (y1.min(f64::from(row) + 1.0) - y0.max(f64::from(row))).max(0.0);
                let mut column = x0.floor() as u32;
                while f64::from(column) < x1 && column < image.width() {
                    let wx =
                        (x1.min(f64::from(column) + 1.0) - x0.max(f64::from(column))).max(0.0);
                    let pixel = image.pixel(column, row);
                    let w = wx * wy;
                    sum[0] += f64::from(pixel.r) * w;
                    sum[1] += f64::from(pixel.g) * w;
                    sum[2] += f64::from(pixel.b) * w;
                    sum[3] += f64::from(pixel.a) * w;
                    weight += w;
                    column += 1;
                }
                row += 1;
            }
            if weight > 0.0 {
                let channel = |value: f64| (value / weight).round().clamp(0.0, 255.0) as u8;
                out.set_pixel(
                    x,
                    y,
                    crate::canvas::Color::rgba(
                        channel(sum[0]),
                        channel(sum[1]),
                        channel(sum[2]),
                        channel(sum[3]),
                    ),
                );
            }
        }
    }
    out
}

/// QString::arg(value, 0, 'g', 10): ten significant digits, the shortest form.
fn qt_general(value: f64) -> String {
    katana_core::text::format_general(value, 10)
}

/// What a person answered a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    Cancel,
}

/// A question with Yes, No and Cancel, in the system's box: never asked headless.
pub fn ask_three(title: &str, text: &str) -> Answer {
    match rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(title)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::YesNoCancel)
        .show()
    {
        rfd::MessageDialogResult::Yes => Answer::Yes,
        rfd::MessageDialogResult::No => Answer::No,
        _ => Answer::Cancel,
    }
}

/// A question with Yes and No, in the system's box: never asked headless.
pub fn ask_yes_no(title: &str, text: &str) -> bool {
    rfd::MessageDialog::new()
        .set_title(title)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

/// A warning in the system's box: never shown headless.
pub fn show_warning(title: &str, text: &str) {
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(title)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

// ---- the executor as a dialog sees it -------------------------------------------------------

impl Host for Executor {
    fn run(&mut self, line: &str) -> VerbOutcome {
        self.run_verb_line(line)
    }

    fn document(&self) -> &Document {
        &self.document
    }

    fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    fn interpreter(&self) -> &CommandInterpreter {
        &self.interpreter
    }

    fn reference(&self) -> &ReferenceData {
        &self.reference
    }

    fn reference_mut(&mut self) -> &mut ReferenceData {
        &mut self.reference
    }

    fn geo(&mut self, phase: GeoPhase<'_>) {
        let mut context = Context::new(
            &mut self.document,
            &self.interpreter,
            &mut self.reference,
            &self.scratch,
            &self.front_end,
        );
        phase(&mut context);
    }

    fn headless(&self) -> bool {
        self.headless
    }

    fn log(&mut self, text: &str, is_error: bool) {
        self.log.log(text, is_error);
    }

    fn warn_user(&mut self, title: &str, text: &str) {
        self.log.warn_user(title, text);
    }

    fn views(&mut self) -> Option<&mut dyn ViewAccess> {
        Some(&mut self.workspace)
    }

    fn jobs(&mut self) -> &mut dyn Jobs {
        &mut self.jobs
    }

    fn wait_for(&mut self, id: JobId) {
        wait_for_job(self, id);
        self.apply_geo_effects();
    }

    fn wait_for_all(&mut self) {
        wait_for_all_jobs(self);
        self.apply_geo_effects();
    }

    fn await_job(&mut self, started: &VerbOutcome, done: JobAwait) -> bool {
        match self.geo.as_ref() {
            Some(geo) => geo.await_job(&self.jobs, started, done),
            None => false,
        }
    }

    fn clipboard(&mut self) -> &mut dyn Clipboard {
        self.clipboard.as_mut()
    }

    fn settings(&mut self) -> &mut UserSettings {
        &mut self.settings
    }
}

impl JobHost for Executor {
    fn job_runner(&mut self) -> &mut JobRunner {
        &mut self.jobs
    }
}

/// A plan view's pointer with a key held, as the headless pointer steps give it.
pub fn modifiers_of(key: &str) -> Option<KeyModifiers> {
    match key {
        "" => Some(KeyModifiers::NONE),
        "shift" => Some(KeyModifiers::SHIFT),
        "ctrl" => Some(KeyModifiers::CTRL),
        _ => None,
    }
}

/// The record's text a reply's update masking would show: what the log echoes of `line`.
pub fn echoed(line: &str) -> String {
    format!("> {}", update_logged_line(line))
}

/// A ViewId's text, as the records write it.
pub fn view_text(id: ViewId) -> String {
    id.to_string()
}

/// A point as the status bar's coordinate label shows it (planPoint): four decimals each.
pub fn plan_point(at: Point2) -> String {
    format!("{:.4}, {:.4}", at.x, at.y)
}

/// The box of a plan view's visible area, in ground units.
pub fn visible_area(corners: (Point2, Point2)) -> Box2 {
    let (a, b) = corners;
    Box2::new(
        Point2::new(a.x.min(b.x), a.y.min(b.y)),
        Point2::new(a.x.max(b.x), a.y.max(b.y)),
    )
}

/// A shared handle a closure keeps to reach a thing the window owns.
pub type Shared<T> = Rc<RefCell<T>>;
