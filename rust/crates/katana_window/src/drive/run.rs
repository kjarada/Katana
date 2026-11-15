//! The program `katana`: the port of src/katana_qt/main.cpp over `DrivenWindow`, the program's
//! view of the window (drive::program). main.cpp's usage comment and docs/headless.md ("The
//! switches", "The steps") are the reference; this keeps their rules in main.cpp's order:
//!
//! ```text
//! katana [project-directory] [data-file...]
//! katana [project-directory] [data-file...] --plot out.pdf [--fit | --scale N]
//!               [--paper A4|A3|A2|A1|A0] [--landscape | --portrait] [--dpi N]
//!               [--plot-style colour|grey|mono] [--line-weight-scale F]
//! katana [project-directory] [data-file...] [--command TEXT...] [--sheets-json out.json|-]
//!               [--plot-sheets out.pdf|folder [--sheets 1,3-5] [--format pdf|pdfs|png|tiff]
//!                [--plot-style colour|grey|mono] [--dpi N] [--line-weight-scale F]]
//! katana [project-directory] [data-file...] --screenshot out.png [switches] [steps...]
//! katana [project-directory] [data-file...] --script FILE... [--command TEXT...]
//! katana --update-health TX
//! ```
//!
//! THE ORDER main() keeps:
//!
//! 1. An update that was interrupted is put right first, before anything else reads the install
//!    (`recover_interrupted_update`); a recovery that fails is said, and the run goes on.
//! 2. The switches are read. A switch given a value it cannot take - a format, a plot style, a
//!    paper, a line weight scale, an output path missing, a health run given no transaction id -
//!    ends the run with exit 2 before the window is made.
//! 3. The window is made, told whether the run is HEADLESS - a screenshot, a plot, the sheets'
//!    JSON, a script batch or the health run: one nobody watches, which opens no box - and given
//!    the customisation it starts with (its start-up line says what was installed).
//! 4. `--update-health TX`: once the window is idle, the marker healthy-TX is written in the
//!    update state folder and the run ends, 0 when it is on the disk, 1 when not. Every other
//!    switch is ignored.
//! 5. `--customise FILE...`: ONE line, CUSTOMISE "<file>" ..., through the one executor. Refused,
//!    it fails a run that only writes or runs a script batch (exit 1), and a screenshot run goes
//!    on.
//! 6. A window a person uses (no screenshot, nothing written, no script batch) restores its last
//!    session, is shown, starts its updates, and shows its start screen when nothing was given to
//!    open.
//! 7. The project and the data files given are opened and imported, in order.
//! 8. Without `--screenshot`, the `--command`, `--enter`, `--run-line` and `--script` steps run
//!    here, in order; one refused fails a run that writes or is a script batch (exit 1).
//! 9. `--screenshot PNG`: the checks, the actions, the steps (drive::steps) and the grab of the
//!    target (drive::headless's soft renderer).
//! 10. `--sheets-json`, then `--plot-sheets`, or `--plot`; a script batch is over.
//! 11. Otherwise the desktop (drive::desktop), and after it the update helper, when the window was
//!     closed by an update.
//!
//! EXIT CODES. 0, the run did what it was asked; 1, it was refused (a step, a line, a write, a
//! check, a plot); 2, a switch's value could not be taken.
//!
//! What a run says goes to the console it is lent: stderr for every sentence, stdout for the
//! sheets' JSON and the files a plot wrote. The window's own log is echoed to stderr by the window
//! in a headless run (MainWindow's, K's).

use std::path::Path;

use katana_app::console::Console;
use katana_cad::PlotSettings;
use katana_cad::plot::plot_colour_mode_from;
use katana_cad::plotting::page_setup::{MAXIMUM_LINE_WEIGHT_SCALE, MINIMUM_LINE_WEIGHT_SCALE};
use katana_cad::plotting::sheet_json::sheet_set_to_json;
use katana_cad::plotting::sheet_verbs::write_sheet_set_file;
use katana_cad::{PaperSize, PlotColourMode};
use katana_core::path_text::{generic_path_utf8, path_from_utf8, path_to_utf8};
use katana_core::text::{format_general, parse_integer};
use katana_update::{
    health_marker_path, install_root_of_running_program, is_valid_transaction_id, recover,
    state_directory, utf8_arguments,
};

use super::desktop::{Desktop, start_pending_update};
use super::headless::Headless;
use super::program::{DrivenWindow, SheetsPlot};
use super::registry::WINDOW;
use super::steps::{Step, StepEngine, StepKind, qt_double, qt_int, run_line, run_script_file};
use crate::canvas::write_png;
use crate::command_word::command_word;

/// The program's arguments as text: UTF-8 from the command line read wide on Windows
/// (katana_update's utf8_arguments), and anything that is not UTF-8 replaced, as QString takes
/// the local eight-bit text. The program's own name first, as QApplication::arguments has it.
pub fn program_arguments() -> Vec<String> {
    utf8_arguments()
        .into_iter()
        .map(|argument| String::from_utf8_lossy(&argument).into_owned())
        .collect()
}

/// A path as main.cpp prints one: as given, in its platform's form.
fn path_text(path: &Path) -> String {
    String::from_utf8_lossy(&path_to_utf8(path)).into_owned()
}

/// QDir::toNativeSeparators: '/' as '\' on Windows, as it is elsewhere.
fn native_separators(text: &str) -> String {
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        String::from(text)
    }
}

/// An OS error as Qt's errorString says it: the system's own words, without Rust's " (os error N)".
fn os_error_text(error: &std::io::Error) -> String {
    let text = format!("{error}");
    match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => String::from(&text[..at]),
        _ => text,
    }
}

/// main.cpp's recoverInterruptedUpdate, the FIRST thing a run does: an update that a crash or a
/// power cut interrupted is put right before this program does anything else with an install whose
/// files may be half swapped. A program in no install has nothing an update could have touched. A
/// recovery that fails is said, with the command that puts the install right by hand (the Error's
/// own words), and the program goes on.
pub fn recover_interrupted_update(console: &mut dyn Console) {
    let Ok(root) = install_root_of_running_program() else {
        return;
    };
    if let Err(error) = recover(&root) {
        console.err(&format!("{}\n", error.describe()));
    }
}

/// main.cpp's writeHealthMarker: U/healthy-<TX>, "ok" in it, which the update helper looks for once
/// this process has exited. False, said, when it could not be written - the helper then finds no
/// marker and puts the old version back, the right answer to a copy that cannot say it started.
pub fn write_health_marker(transaction: &str, console: &mut dyn Console) -> bool {
    let state = state_directory();
    if state.as_os_str().is_empty() {
        console.err("--update-health: this session has no update state folder\n");
        return false;
    }
    // There already when the helper asks: it runs from a folder inside it.
    let _ = std::fs::create_dir_all(&state);
    let marker = health_marker_path(&state, transaction);
    let written = std::fs::write(&marker, b"ok\n");
    if let Err(error) = written {
        console.err(&format!(
            "--update-health: the marker {} could not be written: {}\n",
            path_text(&marker),
            os_error_text(&error)
        ));
        return false;
    }
    true
}

/// What the switches asked for, read in main.cpp's argument loop.
#[derive(Clone, Debug, PartialEq)]
pub struct Switches {
    /// `--plot PDF`.
    pub plot: Option<String>,
    /// `--plot-sheets PDF|FOLDER`.
    pub plot_sheets: Option<String>,
    /// `--sheets-json FILE|-`.
    pub sheets_json: Option<String>,
    /// `--sheets 1,3-5`.
    pub sheets_selection: String,
    /// `--format`, as given (a format the window's plot output takes).
    pub sheets_format: Option<String>,
    /// `--plot-style`.
    pub plot_style: Option<PlotColourMode>,
    /// `--line-weight-scale`.
    pub line_weight_scale: Option<f64>,
    /// Whether `--dpi` was given.
    pub dpi_given: bool,
    /// `--screenshot PNG`.
    pub screenshot: Option<String>,
    /// `--toggle-layer NAME`.
    pub toggle_layer: Option<String>,
    /// `--customise FILE...`: every path until the next switch.
    pub customisation: Vec<String>,
    pub style_manager: bool,
    pub attribute_manager: bool,
    pub layer_manager: bool,
    /// `--action NAME`, in order.
    pub actions: Vec<String>,
    pub dataset_info: Option<String>,
    pub import_options: Option<String>,
    /// `--select-all`.
    pub select_everything: bool,
    /// The steps, in the order given.
    pub steps: Vec<Step>,
    pub check_shortcuts: bool,
    pub check_menus: bool,
    pub check_tool_bars: bool,
    /// `--check-toolbar-fit`'s sizes, as given.
    pub tool_bar_fit_sizes: String,
    pub session_restore_check: bool,
    /// `--update-health`'s transaction id, as the update helper gave it.
    pub update_health: Option<String>,
    /// `--attributes ID`: the entity the manager shows; 0 for the whole selection.
    pub attribute_entity: i64,
    /// `--fit` (the default) or `--scale N`.
    pub fit: bool,
    /// `--paper`, `--landscape`, `--portrait`, `--dpi`, `--scale`, `--plot-style`,
    /// `--line-weight-scale`, as `--plot` takes them.
    pub settings: PlotSettings,
    /// What is not a switch: a project folder, or a file to import.
    pub inputs: Vec<String>,
}

impl Default for Switches {
    fn default() -> Self {
        Self {
            plot: None,
            plot_sheets: None,
            sheets_json: None,
            sheets_selection: String::new(),
            sheets_format: None,
            plot_style: None,
            line_weight_scale: None,
            dpi_given: false,
            screenshot: None,
            toggle_layer: None,
            customisation: Vec::new(),
            style_manager: false,
            attribute_manager: false,
            layer_manager: false,
            actions: Vec::new(),
            dataset_info: None,
            import_options: None,
            select_everything: false,
            steps: Vec::new(),
            check_shortcuts: false,
            check_menus: false,
            check_tool_bars: false,
            tool_bar_fit_sizes: String::new(),
            session_restore_check: false,
            update_health: None,
            attribute_entity: 0,
            fit: true,
            settings: PlotSettings::default(),
            inputs: Vec::new(),
        }
    }
}

impl Switches {
    /// A run that only writes - a plot, the sheets, their JSON - and shows no window.
    pub fn writes_only(&self) -> bool {
        self.plot.is_some() || self.plot_sheets.is_some() || self.sheets_json.is_some()
    }

    /// A script with nothing to grab: a batch run, as katana_cli's is.
    pub fn script_batch(&self) -> bool {
        self.screenshot.is_none() && self.steps.iter().any(|step| step.kind == StepKind::Script)
    }

    /// A session nobody watches (MainWindow::setHeadless): the health run too, since nobody is
    /// there to answer a question, and it must be the same start on every machine.
    pub fn headless(&self) -> bool {
        self.writes_only()
            || self.script_batch()
            || self.screenshot.is_some()
            || self.update_health.is_some()
    }

    /// The switches of the steps, as main.cpp records them: what decides whether a failed start
    /// of the update helper is boxed.
    pub fn step_kinds(&self) -> Vec<String> {
        self.steps
            .iter()
            .map(|step| String::from(step.switch()))
            .collect()
    }
}

/// The argument after the one at `i`, taken (`i` moved onto it); an empty text when there is none,
/// as main.cpp's `value()` gives a null QString.
fn next_value(arguments: &[String], i: &mut usize) -> String {
    if *i + 1 < arguments.len() {
        *i += 1;
        arguments[*i].clone()
    } else {
        String::new()
    }
}

/// main.cpp's argument loop over `arguments` (the program's name first), and the checks after it:
/// the switches, or the exit code (2) of a value refused, said on `console`. `format_known` is
/// plot_output's plotFormatFrom (`DrivenWindow::plot_format_known`).
pub fn read_switches(
    arguments: &[String],
    format_known: impl Fn(&str) -> bool,
    console: &mut dyn Console,
) -> Result<Switches, i32> {
    let mut switches = Switches::default();
    let mut i = 1;
    while i < arguments.len() {
        let argument = arguments[i].as_str();
        match argument {
            "--plot" => switches.plot = Some(next_value(arguments, &mut i)),
            "--plot-sheets" => switches.plot_sheets = Some(next_value(arguments, &mut i)),
            "--sheets-json" => switches.sheets_json = Some(next_value(arguments, &mut i)),
            "--sheets" => switches.sheets_selection = next_value(arguments, &mut i),
            "--format" => {
                let format = next_value(arguments, &mut i);
                if !format_known(&format) {
                    console.err(&format!(
                        "--format must be one of pdf, pdfs, png, tiff (not '{format}')\n"
                    ));
                    return Err(2);
                }
                switches.sheets_format = Some(format);
            }
            "--plot-style" => {
                let style = next_value(arguments, &mut i);
                let Some(mode) = plot_colour_mode_from(&style) else {
                    console.err(&format!(
                        "--plot-style must be one of colour, grey, mono (not '{style}')\n"
                    ));
                    return Err(2);
                };
                switches.plot_style = Some(mode);
                switches.settings.colour_mode = mode;
            }
            "--line-weight-scale" => {
                let text = next_value(arguments, &mut i);
                // A number in the range, both ends taken; a NaN is no number here (qt_double).
                let Some(factor) = qt_double(&text).filter(|factor| {
                    (MINIMUM_LINE_WEIGHT_SCALE..=MAXIMUM_LINE_WEIGHT_SCALE).contains(factor)
                }) else {
                    console.err(&format!(
                        "--line-weight-scale must be a number from {} to {} (not '{text}')\n",
                        format_general(MINIMUM_LINE_WEIGHT_SCALE, 6),
                        format_general(MAXIMUM_LINE_WEIGHT_SCALE, 6)
                    ));
                    return Err(2);
                };
                switches.line_weight_scale = Some(factor);
                switches.settings.line_weight_scale = factor;
            }
            "--screenshot" => switches.screenshot = Some(next_value(arguments, &mut i)),
            "--toggle-layer" => switches.toggle_layer = Some(next_value(arguments, &mut i)),
            "--customise" => {
                // Every path until the next switch: a customisation may come as several files -
                // one's codes naming another's symbols - and they are loaded together, as one
                // line.
                while i + 1 < arguments.len() && !arguments[i + 1].starts_with("--") {
                    i += 1;
                    switches.customisation.push(arguments[i].clone());
                }
            }
            "--style-manager" => switches.style_manager = true,
            "--layer-manager" => switches.layer_manager = true,
            "--action" => {
                let action = next_value(arguments, &mut i);
                switches.actions.push(action);
            }
            "--dataset-info" => switches.dataset_info = Some(next_value(arguments, &mut i)),
            "--import-options" => switches.import_options = Some(next_value(arguments, &mut i)),
            "--select-all" => switches.select_everything = true,
            "--enter" => {
                // Enter on an empty command line, a step of its own: an empty --command cannot
                // come through a CMake list.
                switches.steps.push(Step::enter());
            }
            "--check-shortcuts" => switches.check_shortcuts = true,
            "--check-menus" => switches.check_menus = true,
            "--check-toolbars" => switches.check_tool_bars = true,
            "--check-toolbar-fit" => switches.tool_bar_fit_sizes = next_value(arguments, &mut i),
            "--check-session-restore" => switches.session_restore_check = true,
            "--update-health" => switches.update_health = Some(next_value(arguments, &mut i)),
            "--attributes" => {
                switches.attribute_manager = true;
                // An optional entity id: with one entity selected the manager shows values, with
                // several it shows where they differ.
                if let Some(id) = arguments
                    .get(i + 1)
                    .and_then(|next| parse_integer(next.trim()))
                    .filter(|&id| id > 0)
                {
                    i += 1;
                    switches.attribute_entity = id;
                }
            }
            "--fit" => switches.fit = true,
            "--scale" => {
                switches.fit = false;
                // 0 on bad input: refused later, by the plot.
                switches.settings.scale_denominator =
                    qt_double(&next_value(arguments, &mut i)).unwrap_or(0.0);
            }
            "--paper" => {
                let paper = next_value(arguments, &mut i).to_uppercase();
                switches.settings.paper = match paper.as_str() {
                    "A0" => PaperSize::A0,
                    "A1" => PaperSize::A1,
                    "A2" => PaperSize::A2,
                    "A3" => PaperSize::A3,
                    "A4" => PaperSize::A4,
                    _ => {
                        console.err("--paper must be one of A0, A1, A2, A3, A4\n");
                        return Err(2);
                    }
                };
            }
            "--landscape" => switches.settings.landscape = true,
            "--portrait" => switches.settings.landscape = false,
            "--dpi" => {
                switches.settings.dpi = qt_double(&next_value(arguments, &mut i)).unwrap_or(0.0);
                switches.dpi_given = true;
            }
            _ => {
                if let Some(kind) = StepKind::from_switch(argument) {
                    let text = next_value(arguments, &mut i);
                    switches.steps.push(Step::new(kind, &text));
                } else {
                    switches.inputs.push(String::from(argument));
                }
            }
        }
        i += 1;
    }
    if switches.plot_sheets.as_deref() == Some("") {
        console.err("--plot-sheets needs an output path\n");
        return Err(2);
    }
    if switches.plot.as_deref() == Some("") {
        console.err("--plot needs an output path\n");
        return Err(2);
    }
    if switches.sheets_json.as_deref() == Some("") {
        console.err("--sheets-json needs an output path, or - for stdout\n");
        return Err(2);
    }
    // The id names a file in the update state folder: only the shape the helper makes is taken,
    // so nothing on a command line can name a file anywhere else.
    if let Some(transaction) = &switches.update_health
        && !is_valid_transaction_id(transaction)
    {
        console.err("--update-health needs the transaction id the update helper gives it\n");
        return Err(2);
    }
    Ok(switches)
}

/// main.cpp's customise: the line CUSTOMISE "<file>" ... a person would type, through the window's
/// one executor, each file made absolute first (the verb takes its first word for a keyword when
/// it is one, and a file called reset or json in the working folder is still a file). Whether the
/// line ran; a path that cannot be carried on a line is said and loads nothing.
pub fn customise(
    window: &mut dyn DrivenWindow,
    files: &[String],
    console: &mut dyn Console,
) -> bool {
    let mut line = String::from("CUSTOMISE");
    for file in files {
        // QFileInfo::absoluteFilePath, in the generic form Qt writes a path in.
        let given = path_from_utf8(file);
        let absolute = std::path::absolute(&given).unwrap_or(given);
        let text = String::from_utf8_lossy(&generic_path_utf8(&absolute)).into_owned();
        match command_word(&text, "--customise") {
            Ok(word) => {
                line.push(' ');
                line.push_str(&word);
            }
            Err(error) => {
                console.err(&format!("{}\n", error.describe()));
                return false;
            }
        }
    }
    window.run_verb_line(&line).ok
}

/// main.cpp's checkToolBarFit, the sizes read (WIDTHxHEIGHT, a comma between, each at least 200
/// both ways) and the check the window's (`DrivenWindow::check_tool_bar_fit`). Whether all held.
pub fn check_tool_bar_fit(
    window: &mut dyn DrivenWindow,
    sizes: &str,
    console: &mut dyn Console,
) -> bool {
    let mut list = Vec::new();
    for text in sizes.split(',').filter(|text| !text.is_empty()) {
        let parts: Vec<&str> = text.split('x').collect();
        let width = if parts.len() == 2 {
            qt_int(parts[0])
        } else {
            None
        };
        let height = if parts.len() == 2 {
            qt_int(parts[1])
        } else {
            None
        };
        match (width, height) {
            (Some(width), Some(height)) if width >= 200 && height >= 200 => {
                list.push((width as u32, height as u32));
            }
            _ => {
                console.err(&format!(
                    "--check-toolbar-fit needs sizes like 1366x768, a comma between (not '{text}')\n"
                ));
                return false;
            }
        }
    }
    if list.is_empty() {
        console.err("--check-toolbar-fit needs sizes like 1366x768, a comma between\n");
        return false;
    }
    window.check_tool_bar_fit(&list, console)
}

/// main.cpp's toolBarSignsClear: every button with a menu measured is listed, and each clash; a
/// summary when there is none. Whether every sign is clear of its icon.
fn tool_bar_signs_clear(window: &mut dyn DrivenWindow, console: &mut dyn Console) -> bool {
    let signs = window.menu_sign_clashes();
    for line in &signs.measured {
        console.err(&format!("{line}\n"));
    }
    for clash in &signs.clashes {
        console.err(&format!("menu sign clash: {clash}\n"));
    }
    if !signs.clashes.is_empty() {
        return false;
    }
    console.err(&format!(
        "toolbars and title bars: {} buttons with a menu, each drawing its sign clear of its icon\n",
        signs.buttons
    ));
    true
}

/// Frames until the window is idle; false, said, when it is not.
fn settled(
    headless: &mut Headless,
    window: &mut dyn DrivenWindow,
    console: &mut dyn Console,
) -> bool {
    match headless.settle(window) {
        Ok(()) => true,
        Err(problem) => {
            console.err(&format!("{problem}\n"));
            false
        }
    }
}

/// The last frame's picture of target `name` written to `path`: 0, or 1 said.
fn grab(headless: &Headless, name: &str, path: &str, console: &mut dyn Console) -> i32 {
    let written = match headless.screenshot(name) {
        Ok(image) => write_png(&image, &path_from_utf8(path)).is_ok(),
        Err(problem) => {
            console.err(&format!("{problem}\n"));
            false
        }
    };
    if written {
        0
    } else {
        console.err(&format!("could not write {path}\n"));
        1
    }
}

/// A target the window made, drawn until idle and grabbed.
fn grab_made(
    headless: &mut Headless,
    window: &mut dyn DrivenWindow,
    name: &str,
    path: &str,
    console: &mut dyn Console,
) -> i32 {
    if !settled(headless, window, console) {
        return 1;
    }
    grab(headless, name, path, console)
}

/// The two GIS dialogs - `--dataset-info` before `--import-options` - built from `file` and shown:
/// the target's name. A file that cannot be described has no dialog, said, and fails the run.
fn open_options(
    switches: &Switches,
    window: &mut dyn DrivenWindow,
    console: &mut dyn Console,
    file: &str,
) -> Option<String> {
    let made = if switches.dataset_info.is_some() {
        window.open_dataset_info(&path_from_utf8(file))
    } else {
        window.open_import_options(&path_from_utf8(file))
    };
    if made.is_none() {
        console.err(&format!("no dialog for {file}\n"));
    }
    made
}

/// The run of the program: `arguments` (the program's name first), what it says to `console`,
/// the window `make` makes once the switches are read, and `desktop`, which runs the window a
/// person uses until it is closed and answers the event loop's exit code (the program's eframe
/// call; drive::desktop). The run's exit code.
pub fn run<W, M, D>(arguments: &[String], console: &mut dyn Console, make: M, desktop: D) -> i32
where
    W: DrivenWindow,
    M: FnOnce() -> W,
    D: FnOnce(&mut Desktop<'_, W>) -> i32,
{
    recover_interrupted_update(console);
    let switches = match read_switches(arguments, W::plot_format_known, console) {
        Ok(switches) => switches,
        Err(code) => return code,
    };
    let mut window = make();
    if let Some(code) = start(&switches, &mut window, console) {
        return code;
    }
    if let Some(path) = &switches.screenshot {
        return screenshot(&switches, path, &mut window, console);
    }
    if let Some(code) = write(&switches, &mut window, console) {
        return code;
    }
    if switches.script_batch() {
        return 0;
    }
    let code = {
        let mut shown = Desktop::new(&mut window);
        desktop(&mut shown)
    };
    start_pending_update(&mut window, &switches.step_kinds(), code, console)
}

/// main.cpp from the window's making to its steps without a screenshot: headless or not, the
/// customisation, the health run, a person's start-up, the inputs, and the lines run before
/// anything is written. Some exit code when the run is over.
fn start(
    switches: &Switches,
    window: &mut dyn DrivenWindow,
    console: &mut dyn Console,
) -> Option<i32> {
    let writes_only = switches.writes_only();
    let script_batch = switches.script_batch();
    window.set_headless(switches.headless());
    // Before anything is opened, so the first drawing is drawn with it: the customisation the
    // session starts with (the kept one, else the built-in one), then --customise merged into it
    // as the typed CUSTOMISE line would.
    window.load_default_customisation();
    if let Some(transaction) = &switches.update_health {
        // The window is built and its start-up has run; the marker is written once the window
        // is idle (the C++'s zero timer on the event loop's first idle turn), which a program that
        // hangs on start-up never reaches. Then out, with 0 only when the marker is on the disk.
        let mut headless = Headless::from_environment();
        if !settled(&mut headless, window, console) {
            return Some(1);
        }
        return Some(if write_health_marker(transaction, console) {
            0
        } else {
            1
        });
    }
    if !switches.customisation.is_empty()
        && !customise(window, &switches.customisation, console)
        && (writes_only || script_batch)
    {
        console.err(&format!(
            "--customise {} was refused\n",
            switches.customisation.join(" ")
        ));
        return Some(1);
    }
    if !writes_only && switches.screenshot.is_none() && !script_batch {
        // Where the last session left the window, or fitted to the screen: never in a headless
        // run, whose window is the same everywhere.
        window.restore_session();
        window.show();
        // And what a window a person uses does about updates. A headless run checks nothing by
        // itself and reads no per-user state, so this is not called there.
        window.start_updates();
        // What the session starts with, asked as it starts, unless a project or a file was given
        // to open, which brings its own.
        if switches.inputs.is_empty() {
            window.show_start_screen_if_wanted();
        }
    }
    for input in &switches.inputs {
        let path = path_from_utf8(input);
        if path.is_dir() {
            window.open_project(&path);
        } else {
            window.import_path(&path);
        }
    }
    // Without --screenshot the --command lines and scripts run here, in order, before anything
    // is written. A headless run stops at the first line that is refused. (With --screenshot they
    // run among its steps.) A run nobody watches is drawn until idle after each, where the C++
    // ran its event loop twice; a window a person uses is drawn by the desktop once it starts.
    if switches.screenshot.is_none() {
        let mut headless = (writes_only || script_batch).then(Headless::from_environment);
        for step in &switches.steps {
            let ran = match step.kind {
                StepKind::Command => window.run_command(&step.text),
                StepKind::RunLine => run_line(window, &step.text, console),
                StepKind::Script => run_script_file(window, &step.text, console),
                _ => continue,
            };
            if let Some(headless) = headless.as_mut()
                && !settled(headless, window, console)
            {
                return Some(1);
            }
            if !ran && (writes_only || script_batch) {
                console.err(&format!("{} {} was refused\n", step.switch(), step.text));
                return Some(1);
            }
        }
    }
    None
}

/// main.cpp's `--screenshot` branch: the checks, the actions, the steps, the managers, and the grab
/// of what the run made the target.
fn screenshot(
    switches: &Switches,
    path: &str,
    window: &mut dyn DrivenWindow,
    console: &mut dyn Console,
) -> i32 {
    if path.is_empty() {
        console.err("--screenshot needs an output path\n");
        return 2;
    }
    // Before anything shows the window: what it checks happens only to a window never shown.
    if switches.session_restore_check && !window.check_session_restore(console) {
        return 1;
    }
    let mut headless = Headless::from_environment();
    // Shown so that the layout is real, and drawn until idle: the first frame lays out and the
    // next paint what the layout produced.
    window.show();
    if !settled(&mut headless, window, console) {
        return 1;
    }
    if switches.check_shortcuts {
        let (clashes, sequences) = window.shortcut_clashes();
        for clash in &clashes {
            console.err(&format!("shortcut clash: {clash}\n"));
        }
        if !clashes.is_empty() {
            return 1;
        }
        console.err(&format!(
            "shortcuts: {sequences} key sequences, each reaching one thing\n"
        ));
    }
    if switches.check_menus {
        let (gaps, items) = window.menu_gaps();
        for gap in &gaps {
            console.err(&format!("menu gap: {gap}\n"));
        }
        if !gaps.is_empty() {
            return 1;
        }
        console.err(&format!(
            "menus: {items} items, each with an icon and a status tip\n"
        ));
    }
    if !switches.tool_bar_fit_sizes.is_empty()
        && !check_tool_bar_fit(window, &switches.tool_bar_fit_sizes, console)
    {
        return 1;
    }
    if switches.select_everything {
        window.select_all();
    }
    for action in &switches.actions {
        if let Err(error) = window.trigger_action(action) {
            console.err(&format!("--action {action} failed: {}\n", error.describe()));
            return 1;
        }
        if !settled(&mut headless, window, console) {
            return 1;
        }
    }
    // --check-toolbars measures the toolbars and the docks' title bars as the actions and steps
    // leave them; with steps, after them (below).
    if switches.check_tool_bars
        && switches.steps.is_empty()
        && !tool_bar_signs_clear(window, console)
    {
        return 1;
    }
    let options_file = switches
        .dataset_info
        .as_deref()
        .or(switches.import_options.as_deref());
    if !switches.steps.is_empty() {
        let mut engine = StepEngine::new();
        if let Some(file) = options_file {
            // Held by the window: the steps drive it and the grab reads it. Its Run pressed, the
            // import runs as the menu's does once its dialog is accepted (the window's
            // finishImport).
            let Some(name) = open_options(switches, window, console, file) else {
                return 1;
            };
            engine.start_on(&name);
            if !settled(&mut headless, window, console) {
                return 1;
            }
        }
        for step in &switches.steps {
            if !engine.run(step, window, &mut headless, console) {
                return 1;
            }
        }
        if !settled(&mut headless, window, console) {
            return 1;
        }
        if switches.check_tool_bars && !tool_bar_signs_clear(window, console) {
            return 1;
        }
        let shot = engine.finish(&headless.snapshot(), console);
        return grab(&headless, &shot, path, console);
    }
    if let Some(file) = options_file {
        let Some(name) = open_options(switches, window, console, file) else {
            return 1;
        };
        return grab_made(&mut headless, window, &name, path, console);
    }
    if let Some(layer) = &switches.toggle_layer {
        if let Err(error) = window.toggle_layer_through_panel(layer) {
            console.err(&format!("--toggle-layer failed: {}\n", error.describe()));
            return 1;
        }
        if !settled(&mut headless, window, console) {
            return 1;
        }
    }
    if switches.layer_manager {
        let name = window.open_layer_manager();
        return grab_made(&mut headless, window, &name, path, console);
    }
    if switches.attribute_manager {
        // Everything selected, because the manager acts on the selection and an empty one would
        // show an empty tree.
        if switches.attribute_entity > 0 {
            window.select_only(switches.attribute_entity as u64);
        } else {
            window.select_all();
        }
        let name = window.open_attribute_manager();
        return grab_made(&mut headless, window, &name, path, console);
    }
    if switches.style_manager {
        let name = window.open_style_manager();
        return grab_made(&mut headless, window, &name, path, console);
    }
    grab(&headless, WINDOW, path, console)
}

/// main.cpp's writing without a window: `--sheets-json` first, then `--plot-sheets` or `--plot`.
/// Some exit code when the run wrote, or was refused.
fn write(
    switches: &Switches,
    window: &mut dyn DrivenWindow,
    console: &mut dyn Console,
) -> Option<i32> {
    if let Some(json) = &switches.sheets_json {
        let document = window.document();
        if let Err(error) = document.sheet_set_status() {
            console.err(&format!("--sheets-json: {}\n", error.describe()));
            return Some(1);
        }
        let set = document.sheet_set();
        if json == "-" {
            match sheet_set_to_json(&set) {
                Ok(text) => console.out(&format!("{text}\n")),
                Err(error) => {
                    console.err(&format!("--sheets-json: {}\n", error.describe()));
                    return Some(1);
                }
            }
        } else {
            if let Err(error) = write_sheet_set_file(&set, &path_from_utf8(json)) {
                console.err(&format!("--sheets-json: {}\n", error.describe()));
                return Some(1);
            }
            let count = set.sheets.len();
            console.err(&format!(
                "wrote {count} sheet{} to {json}\n",
                if count == 1 { "" } else { "s" }
            ));
        }
        if switches.plot_sheets.is_none() && switches.plot.is_none() {
            return Some(0);
        }
    }
    if let Some(folder) = &switches.plot_sheets {
        let plot = SheetsPlot {
            path: path_from_utf8(folder),
            selection: switches.sheets_selection.clone(),
            format: switches.sheets_format.clone(),
            colour_mode: switches.plot_style,
            dpi: switches.dpi_given.then_some(switches.settings.dpi),
            line_weight_scale: switches.line_weight_scale,
        };
        return Some(match window.plot_sheets(&plot) {
            Ok(files) => {
                // What was written, for a script to pick up: one path a line.
                for file in &files {
                    console.out(&format!("{}\n", native_separators(&path_text(file))));
                }
                0
            }
            Err(error) => {
                console.err(&format!("plot failed: {}\n", error.describe()));
                1
            }
        });
    }
    if let Some(pdf) = &switches.plot {
        return Some(
            match window.plot_drawing_to_pdf(&path_from_utf8(pdf), switches.settings, switches.fit)
            {
                Ok(()) => 0,
                Err(error) => {
                    console.err(&format!("plot failed: {}\n", error.describe()));
                    1
                }
            },
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{native_separators, os_error_text};

    #[test]
    fn an_os_error_is_said_in_the_systems_words_alone() {
        let error = std::io::Error::from_raw_os_error(2);
        let text = os_error_text(&error);
        assert!(!text.contains("os error"), "{text}");
        assert!(!text.is_empty());
    }

    #[test]
    fn a_path_keeps_its_separators_off_windows() {
        if cfg!(windows) {
            assert_eq!(native_separators("a/b"), "a\\b");
        } else {
            assert_eq!(native_separators("a/b"), "a/b");
        }
    }
}
