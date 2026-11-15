//! The program's view of the window: main.cpp's calls into MainWindow, as a trait, so the program
//! driver (D's port of main.cpp, the switches and the steps) is written over it and the main
//! window (K) implements it - the one interface between them, frozen before either is written.
//!
//! Every call main.cpp makes on the window is here, each with the line it stands for, but those
//! the steps make through the named controls: `findChild` of a dialog, a dock, a field or a button
//! is the registry's (drive::registry), `--fill` and `--press` the mailbox's, `--wheel` an egui
//! event at a control's rectangle, and `grab()` the soft renderer's crop of a target's rectangle.
//! The calls, from main.cpp:
//!
//! ```text
//! setHeadless 1302                  loadDefaultCustomisation 1309     restoreSession 1329
//! show 1330, 1385                   isVisible 782                     startUpdates 1335
//! showStartScreenIfWanted 1340      openProject 1345                  importPath 1347
//! runCommand 1360, 1502             runVerbLine 558, 577, 603         scriptCommandLine 577
//! findChild<QAction*> 165-180, 420  triggerAction 173, 1444, 1492     selectAll 1441, 1708
//! selectOnly 1706                   toggleLayerThroughPanel 1682      makeStyleManager 1722
//! makeAttributeManager 1710         makeLayerManager 1691             makeDatasetInfo 1468, 1665
//! makeImportOptions 1469, 1666      finishImport 1483                 showExportOptions 1568
//! a menu by name 225, 1546          pointerAt 1527                    a plan or 3D view 288-350
//! shortcutClashes 1390              menuGaps 1402                     menuSignClashes 1424
//! checkToolBarFit 616-766           checkSessionRestore 779-838       document 1741
//! sheetsToPlot 1770                 plotSheets 1787                   plotDrawingToPdf 1799
//! plotFormatFrom 1154               takePendingUpdate 1817            updateFailureIsBoxed 1840
//! applicationIcon 1101
//! ```
//!
//! checkToolBarFit and checkSessionRestore are main.cpp's own functions over a dozen QMainWindow
//! calls on the toolbars' layout (removeToolBar, insertToolBar, saveState, restoreLayout,
//! toolBarArea, toolBarBreak, ...); that layout's form is the main window's (K's) to decide, so
//! each check is one call here, printing main.cpp's lines to the console it is lent, and the
//! driver keeps the parsing of the sizes before it (main.cpp 654-672).

use std::path::{Path, PathBuf};

use katana_app::console::Console;
use katana_cad::{Document, PlotColourMode, PlotSettings};
use katana_core::{Result, Status};
use katana_entity::EntityId;
use katana_update::ApplyRequest;

use super::action::{Action, Menu};
use crate::canvas::Image;
use crate::host::VerbOutcome;

/// What `--plot-sheets` asks for beyond the set's page setup (main.cpp 1769-1787): the window
/// makes the plot request of it (plot_output's plotRequestFor) and plots.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetsPlot {
    /// The PDF, or the folder a file a sheet is written into.
    pub path: PathBuf,
    /// `--sheets`: "1,3-5", sheet ids; empty for every sheet.
    pub selection: String,
    /// `--format`: pdf, pdfs, png or tiff (one PDF when none).
    pub format: Option<String>,
    /// `--plot-style`.
    pub colour_mode: Option<PlotColourMode>,
    /// `--dpi`, when it was given.
    pub dpi: Option<f64>,
    /// `--line-weight-scale`.
    pub line_weight_scale: Option<f64>,
}

/// What `--check-toolbars` measured (tools::menuSignClashes): a line for each button measured,
/// clear or not, a line for each clash, and how many buttons with a menu there were.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuSigns {
    pub measured: Vec<String>,
    pub clashes: Vec<String>,
    pub buttons: usize,
}

/// main.cpp's calls into the window (see the module comment). The main window implements it; the
/// program driver, the step engine and the headless and desktop runners call it.
pub trait DrivenWindow {
    // ---- the frame ----

    /// Draws the whole window for one frame - its menus, toolbars, docks and views, and every open
    /// dialog in its frame, each a target named as its C++ object - inside the runner's
    /// `egui::Context::run_ui`.
    fn ui(&mut self, ui: &mut egui::Ui);
    /// Whether a frame must still come for work under way: a job running. The runner's idle rule
    /// is this false, no repaint asked, and no act waiting in the mailbox.
    fn busy(&self) -> bool;
    /// The application's icon (icons.hpp's applicationIcon), for the desktop's window.
    fn application_icon(&self) -> Image;

    // ---- start-up ----

    /// MainWindow::setHeadless: a session nobody watches, which opens no box and no file dialog,
    /// and reads and writes no per-user place.
    fn set_headless(&mut self, headless: bool);
    /// MainWindow::loadDefaultCustomisation: the kept customisation, else the built-in one.
    fn load_default_customisation(&mut self);
    /// MainWindow::restoreSession: the window's place, layout and appearance as the last session
    /// left them; never in a headless run.
    fn restore_session(&mut self);
    /// QWidget::show: the window shown (headless: laid out, for the steps and the screenshot).
    fn show(&mut self);
    /// QWidget::isVisible.
    fn is_visible(&self) -> bool;
    /// MainWindow::startUpdates: what a window a person uses does about updates as it starts.
    fn start_updates(&mut self);
    /// MainWindow::showStartScreenIfWanted.
    fn show_start_screen_if_wanted(&mut self);
    /// MainWindow::openProject: a project given on the command line.
    fn open_project(&mut self, directory: &Path);
    /// MainWindow::importPath: a data file given on the command line, imported by its IMPORT line
    /// through the one executor, with no placement asked.
    fn import_path(&mut self, path: &Path);

    // ---- lines ----

    /// MainWindow::runCommand: `line` as if typed on the command line and Enter pressed - a
    /// running tool's answer if one waits; an empty line is Enter on an empty command line. False
    /// when the line logged an error.
    fn run_command(&mut self, line: &str) -> bool;
    /// MainWindow::runVerbLine: the one executor, as a dialog runs a line.
    fn run_verb_line(&mut self, line: &str) -> VerbOutcome;
    /// script_runner's `scriptCommandLine(path, false)`: the SCRIPT line `--script` runs, stopping
    /// at the first line refused.
    fn script_command_line(&self, path: &str) -> String;

    // ---- actions and menus ----

    /// Every action of the window and its workbenches, menus and toolbars alike, each once, in the
    /// window's order: what `findChild<QAction*>` and `findChildren<QAction*>` found.
    fn actions(&self) -> Vec<Action>;
    /// The menu bar's menus, with their submenus (a menu found by its object name; `--report` of a
    /// menu, `--check-menus`).
    fn menus(&self) -> Vec<Menu>;
    /// MainWindow::triggerAction: the action named `name` triggered as a click does. NotFound for
    /// an unknown name, InvalidState for a disabled one.
    fn trigger_action(&mut self, name: &str) -> Status;
    /// MainWindow::selectAll: every entity on an unlocked layer, as Edit > Select All.
    fn select_all(&mut self);
    /// MainWindow::selectOnly.
    fn select_only(&mut self, id: EntityId);
    /// MainWindow::toggleLayerThroughPanel: the layer's visibility box flipped as a click does,
    /// and whether the window came through it properly.
    fn toggle_layer_through_panel(&mut self, layer: &str) -> Status;

    // ---- targets the window makes ----

    /// makeStyleManager, shown, its first rows shown: the target's object name.
    fn open_style_manager(&mut self) -> String;
    /// makeAttributeManager, shown, expanded: the target's object name.
    fn open_attribute_manager(&mut self) -> String;
    /// makeLayerManager, shown, its first row shown: the target's object name.
    fn open_layer_manager(&mut self) -> String;
    /// makeDatasetInfo for `path`, shown: the target's object name; None when the file could not
    /// be described (logged).
    fn open_dataset_info(&mut self, path: &Path) -> Option<String>;
    /// makeImportOptions for `path`, shown: the target's object name; None when the file has no
    /// such dialog (logged). Its Run, accepted, finishes the import as the menu's does
    /// (MainWindow::finishImport, which main.cpp connects to the dialog's accepted).
    fn open_import_options(&mut self, path: &Path) -> Option<String>;
    /// showExportOptions for `path`: File > Export Drawing's dialog, the target's object name.
    fn open_export_options(&mut self, path: &Path) -> Option<String>;
    /// A menu of the window opened under its title, as a click on the menu bar opens it (a
    /// `--panel` of a menu): a Menu target named as the menu. False when the window has no menu of
    /// that name.
    fn open_menu(&mut self, name: &str) -> bool;

    // ---- the pointer and the views ----

    /// MainWindow::pointerAt: `spec` "x,y" in model units, with ",shift" or ",ctrl" for a key
    /// held; the pointer moved there in the plan view Enter goes to - and clicked when `click` -
    /// and the record the step prints answered. ParseFailure for a spec that is not
    /// x,y[,shift|ctrl].
    fn pointer_at(&mut self, spec: &str, click: bool) -> Result<String>;
    /// reportWidget for a view of the window (PlanView2, RenderView2), painted afresh first: what
    /// it drew, by the keys its record uses. None when `name` is no view.
    fn report_view(&mut self, name: &str) -> Option<String>;

    // ---- checks ----

    /// MainWindow::shortcutClashes: one line a key sequence two things share, and how many
    /// distinct sequences there are.
    fn shortcut_clashes(&self) -> (Vec<String>, usize);
    /// MainWindow::menuGaps: one line a menu item with no icon or no status tip, and how many
    /// items there are.
    fn menu_gaps(&self) -> (Vec<String>, usize);
    /// tools::menuSignClashes over the window's toolbars and docks' title bars.
    fn menu_sign_clashes(&mut self) -> MenuSigns;
    /// main.cpp's checkToolBarFit after its sizes are read: at each size the toolbars along the
    /// top show every action; then, at the narrowest, the older builds' layouts restored hold to
    /// the same. Its lines to `err`, as main.cpp prints them. Whether all held.
    fn check_tool_bar_fit(&mut self, sizes: &[(u32, u32)], err: &mut dyn Console) -> bool;
    /// main.cpp's checkSessionRestore: an older build's layout restored into a maximised window
    /// before it is first shown. Its lines to `err`. Whether every toolbar was along the top.
    fn check_session_restore(&mut self, err: &mut dyn Console) -> bool;

    // ---- writing without a window ----

    /// The drawing, read only: for `--sheets-json`.
    fn document(&self) -> &Document;
    /// MainWindow::plotDrawingToPdf.
    fn plot_drawing_to_pdf(&mut self, path: &Path, settings: PlotSettings, fit: bool) -> Status;
    /// sheetsToPlot, the request `plot` makes, and plotSheets: the files written, in order. A
    /// failure of either is the one error ("plot failed: ...").
    fn plot_sheets(&mut self, plot: &SheetsPlot) -> Result<Vec<PathBuf>>;
    /// plot_output's plotFormatFrom: whether `format` is one `--format` takes. Asked as the
    /// switches are read, before the window is made.
    fn plot_format_known(format: &str) -> bool
    where
        Self: Sized;

    // ---- the end ----

    /// MainWindow::takePendingUpdate: the request to start the update helper with, when the
    /// window was closed by an update.
    fn take_pending_update(&mut self) -> Option<ApplyRequest>;
    /// update_dialog's updateFailureIsBoxed: whether a failed start of the update helper is said in
    /// a box too, given the kinds of the steps the run was given.
    fn update_failure_is_boxed(&self, step_kinds: &[String]) -> bool;
}
