//! File > Run Script and the SCRIPT verb (script_runner.hpp; docs/desktop.md, "Run Script"): a file
//! of command lines, run one after another through the window's one executor, as katana_cli runs a
//! .kcs file.
//!
//! A script is what katana_cli reads: a command a line, UTF-8, a Windows line end taken off, and a
//! blank line or one whose first non-blank is '#' skipped - the session's rule too, which once
//! refused an indented note. So a file written for katana_cli or katana_mcp's katana_run_script
//! runs in the window, and one written in the window runs in both. Each line is echoed and run as
//! a dialog's line is - never a running tool's answer, since a script names its points (LINE 0,0
//! 10,0, not LINE and then its points) - and so each is its own undo step, as in katana_cli. The
//! run stops at the first line refused unless it was asked to continue, and QUIT or EXIT ends it,
//! as it ends katana_cli's, without closing the window.
//!
//! The reading and the running are functions of their inputs, tested without a window; the window
//! adds the log and the Recent Scripts list (K's SCRIPT verb, which calls these and says what
//! `script_run_problem` says).
//!
//! The dialog is non-modal and runs nothing itself: it writes the SCRIPT line its fields describe,
//! shown in scriptCommand, and hands it to the executor. Every control has the C++'s object name:
//!
//! ```text
//! fileRunScriptDialog     the dialog (File > Run Script, fileRunScript)
//! scriptPath              the script file
//! scriptBrowse            choose it with a file dialog (not in a headless session, which says so)
//! scriptPreview           the command lines the file holds, numbered as in the file (read-only)
//! scriptContinueOnError   run every line, counting the refused ones, rather than stop at the first
//! scriptCommand           the exact line Run will run (read-only)
//! scriptRun               run it
//! scriptStatus            what happened last
//! scriptClose             close
//! ```

use std::path::Path;

use egui::Ui;
use katana_cad::CommandInterpreter;
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};

use crate::drive::{LineOptions, TextOptions, button, check, label_as, line_with, text_with};
use crate::host::{Host, VerbOutcome};
use crate::shell::DialogFrame;

const USAGE: &str = "usage: SCRIPT <file.kcs> [CONTINUE]";

/// A command line of a script and where it is in the file, counted from 1, so a report can say
/// which line stopped the run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptLine {
    pub number: usize,
    pub text: String,
}

/// QUIT and EXIT as katana_cli reads them (Session::is_quit): the first word, whatever follows.
fn is_quit(line: &str) -> bool {
    let verb = line
        .split(' ')
        .find(|word| !word.is_empty())
        .unwrap_or("")
        .to_uppercase();
    verb == "QUIT" || verb == "EXIT"
}

/// A comment: a line whose first non-blank is '#', as katana_cli skips it. The command line takes
/// a typed one for a note, and runs nothing.
pub fn is_script_comment(line: &str) -> bool {
    line.trim().starts_with('#')
}

/// The command lines of `text`: a line each, "\r\n" and "\r" ends read as "\n", surrounding blanks
/// taken off, and blank lines and '#' comments left out.
pub fn script_lines(text: &str) -> Vec<ScriptLine> {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    unified
        .split('\n')
        .enumerate()
        .filter_map(|(at, raw)| {
            let line = raw.trim();
            if line.is_empty() || is_script_comment(line) {
                return None;
            }
            Some(ScriptLine {
                number: at + 1,
                text: String::from(line),
            })
        })
        .collect()
}

/// The system's sentence for a file that could not be read, without Rust's " (os error N)" (the
/// C++'s QFile::errorString).
fn io_sentence(error: &std::io::Error) -> String {
    let text = format!("{error}");
    if let Some(code) = error.raw_os_error()
        && let Some(sentence) = text.strip_suffix(&format!(" (os error {code})"))
    {
        return String::from(sentence);
    }
    text
}

/// The command lines of the file at `path`, read as UTF-8 (a byte that is not read as U+FFFD, as
/// QString::fromUtf8 reads it). NotFound for a file that does not exist, FileImportFailure for one
/// that cannot be read.
pub fn read_script(path: &str) -> Result<Vec<ScriptLine>> {
    let file = Path::new(path);
    if !file.exists() {
        return Err(make_error(ErrorCode::NotFound, "no such script", path));
    }
    match std::fs::read(file) {
        Ok(bytes) => Ok(script_lines(&String::from_utf8_lossy(&bytes))),
        Err(error) => Err(make_error(
            ErrorCode::FileImportFailure,
            "the script cannot be read",
            format!("{path}: {}", io_sentence(&error)),
        )),
    }
}

/// SCRIPT <file> [CONTINUE], read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptCommand {
    pub path: String,
    pub continue_on_error: bool,
}

/// SCRIPT <file> [CONTINUE], read by the interpreter's own rules (a path with blanks quoted);
/// InvalidArgument, with the usage, for anything else.
pub fn parse_script_command(line: &str) -> Result<ScriptCommand> {
    let tokens = CommandInterpreter::tokenize(line)?;
    let count = tokens.len();
    if !(2..=3).contains(&count) || tokens[1].is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, USAGE));
    }
    let mut command = ScriptCommand {
        path: tokens[1].clone(),
        continue_on_error: false,
    };
    if count == 3 {
        if tokens[2].to_uppercase() != "CONTINUE" {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                USAGE,
                tokens[2].clone(),
            ));
        }
        command.continue_on_error = true;
    }
    Ok(command)
}

/// QDir::fromNativeSeparators: '\' is a separator on Windows alone; on a POSIX system it is an
/// ordinary character of a file name (POSIX.1-2017, 3.170 "Filename").
pub(crate) fn from_native_separators(path: &str) -> String {
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        String::from(path)
    }
}

/// QDir::toNativeSeparators.
pub(crate) fn to_native_separators(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        String::from(path)
    }
}

/// The line parse_script_command reads back: the path always quoted, with '/' separators, as the
/// COPC menu item writes its paths.
pub fn script_command_line(path: &str, continue_on_error: bool) -> String {
    format!(
        "SCRIPT \"{}\"{}",
        from_native_separators(path),
        if continue_on_error { " CONTINUE" } else { "" }
    )
}

/// How a run ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptReport {
    /// The command lines the script holds.
    pub lines: usize,
    /// How many were run.
    pub ran: usize,
    /// How many of those were refused or failed.
    pub failed: usize,
    /// The file line number of the line the run stopped at - refused, QUIT or cancelled - or 0
    /// when it ran to the end.
    pub stopped_at: usize,
    /// Stopped at QUIT or EXIT, which is not a failure.
    pub quit: bool,
    /// Stopped by the progress dialog's Cancel.
    pub cancelled: bool,
}

impl ScriptReport {
    pub fn ok(&self) -> bool {
        self.failed == 0 && !self.cancelled
    }
}

/// How a script runs.
#[derive(Default)]
pub struct ScriptOptions<'a> {
    pub continue_on_error: bool,
    /// Asked before each line with how many have run and how many there are; false cancels the
    /// rest. None: nothing is asked.
    pub progress: Option<&'a mut dyn FnMut(usize, usize) -> bool>,
}

/// Runs `lines` in order through `run`, as `options` says.
pub fn run_script_lines(
    lines: &[ScriptLine],
    run: &mut dyn FnMut(&str) -> VerbOutcome,
    options: ScriptOptions<'_>,
) -> ScriptReport {
    let ScriptOptions {
        continue_on_error,
        mut progress,
    } = options;
    let mut report = ScriptReport {
        lines: lines.len(),
        ..ScriptReport::default()
    };
    for line in lines {
        if let Some(progress) = progress.as_deref_mut()
            && !progress(report.ran, report.lines)
        {
            report.cancelled = true;
            report.stopped_at = line.number;
            return report;
        }
        if is_quit(&line.text) {
            report.quit = true;
            report.stopped_at = line.number;
            return report;
        }
        report.ran += 1;
        if !run(&line.text).ok {
            report.failed += 1;
            if !continue_on_error {
                report.stopped_at = line.number;
                return report;
            }
        }
    }
    report
}

/// The record a run ends with: script="name" lines=N ran=N failed=N, then stopped_at=K (a
/// refusal), quit_at=K or cancelled_at=K when it stopped early. An empty name is lines pasted on
/// the command line: script=pasted, unquoted, so it cannot be read for a file of that name.
pub fn format_script_report(name: &str, report: &ScriptReport) -> String {
    let named = if name.is_empty() {
        String::from("pasted")
    } else {
        format!("\"{name}\"")
    };
    let mut record = format!(
        "script={named} lines={} ran={} failed={}",
        report.lines, report.ran, report.failed
    );
    if report.stopped_at > 0 {
        let key = if report.cancelled {
            "cancelled_at"
        } else if report.quit {
            "quit_at"
        } else {
            "stopped_at"
        };
        record.push_str(&format!(" {key}={}", report.stopped_at));
    }
    record
}

/// What the window logs as an error after a run that did not end well (MainWindow::runLines),
/// beside its record: None when there is nothing to say. `name` is the script's path, empty for
/// pasted lines.
pub fn script_run_problem(
    name: &str,
    report: &ScriptReport,
    lines: &[ScriptLine],
    continue_on_error: bool,
) -> Option<String> {
    let shown = if name.is_empty() {
        String::from("The pasted lines")
    } else {
        Path::new(name)
            .file_name()
            .map(|file| file.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    if report.cancelled {
        return Some(format!(
            "{shown}: cancelled before line {}; the lines from there were not run.",
            report.stopped_at
        ));
    }
    if report.failed > 0 && !continue_on_error {
        let text = lines
            .iter()
            .find(|line| line.number == report.stopped_at)
            .map(|line| line.text.clone())
            .unwrap_or_default();
        return Some(format!(
            "{shown} stopped at line {}, which was refused: {text}",
            report.stopped_at
        ));
    }
    if report.failed > 0 {
        return Some(format!(
            "{shown}: {} of the {} lines run were refused.",
            report.failed, report.ran
        ));
    }
    None
}

// ---- the dialog -------------------------------------------------------------------------------

/// The dialog's fields and what it shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ScriptRunState {
    path: String,
    preview: String,
    continue_on_error: bool,
    command: String,
    ready: bool,
    status: String,
    close: bool,
    /// The path and the check the preview and the command were made for.
    shown_for: Option<(String, bool)>,
}

impl ScriptRunState {
    fn command(&self) -> Result<String> {
        let path = self.path.trim();
        if path.is_empty() {
            return Err(Error::new(ErrorCode::InvalidArgument, "no script is named"));
        }
        Ok(script_command_line(path, self.continue_on_error))
    }

    /// Made again when the path or the check changed (the C++'s textChanged and toggled).
    fn refresh_if_changed(&mut self) {
        let now = (self.path.clone(), self.continue_on_error);
        if self.shown_for.as_ref() != Some(&now) {
            self.shown_for = Some(now);
            self.refresh();
        }
    }

    fn refresh(&mut self) {
        let line = self.command();
        self.ready = line.is_ok();
        self.command = line.unwrap_or_default();
        // What the file holds, as the run will read it; a file that cannot be read says why here,
        // and Run still runs the line, whose refusal is the verb's to give.
        let path = self.path.trim();
        if path.is_empty() {
            self.preview.clear();
            return;
        }
        self.preview = match read_script(path) {
            Err(error) => error.describe(),
            Ok(lines) if lines.is_empty() => {
                String::from("(no commands: every line is blank or a comment)")
            }
            Ok(lines) => lines
                .iter()
                .map(|script| format!("{:>4}  {}", script.number, script.text))
                .collect::<Vec<_>>()
                .join("\n"),
        };
    }

    fn run(&mut self, host: &mut dyn Host) {
        let line = match self.command() {
            Ok(line) => line,
            Err(error) => {
                self.status = error.message;
                return;
            }
        };
        let outcome = host.run(&line);
        // The record the run ended with is the reply's last line; what each line said is in the
        // command log.
        let last = outcome.reply.rsplit('\n').next().unwrap_or("");
        self.status = if outcome.ok {
            format!("Done: {last}")
        } else {
            let error = outcome.error.rsplit('\n').next().unwrap_or("");
            if last.is_empty() {
                format!("Stopped: {error}")
            } else {
                format!("Stopped: {error}\n{last}")
            }
        };
    }

    fn browse(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.status = String::from(
                "A headless session opens no file dialog: fill scriptPath with the path instead.",
            );
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Run Script")
            .add_filter("Katana scripts", &["kcs"])
            .add_filter("Text files", &["txt"])
            .add_filter("All files", &["*"]);
        let typed = self.path.trim();
        if !typed.is_empty() {
            dialog = dialog.set_directory(typed);
        }
        if let Some(path) = dialog.pick_file() {
            self.path = to_native_separators(&path.to_string_lossy());
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.refresh_if_changed();
        ui.add(
            egui::Label::new(
                "A script is a file of commands, one a line, as katana_cli runs it: blank lines \
                 and lines starting with # are skipped. Each line is run as if typed and is its \
                 own undo step; the run stops at the first line refused.",
            )
            .wrap(),
        );
        let mut browse = false;
        ui.horizontal(|ui| {
            ui.label("Script:");
            line_with(
                ui,
                "scriptPath",
                &mut self.path,
                LineOptions {
                    placeholder: "the script: a .kcs file of commands",
                    width: Some(380.0),
                    ..LineOptions::default()
                },
            );
            browse = button(ui, "scriptBrowse", "Browse...").clicked();
        });
        if browse {
            self.browse(host);
        }
        self.refresh_if_changed();
        ui.label("Commands:");
        let mut preview = self.preview.clone();
        text_with(
            ui,
            "scriptPreview",
            &mut preview,
            TextOptions {
                read_only: true,
                rows: 8,
                monospace: true,
            },
        );
        check(
            ui,
            "scriptContinueOnError",
            "Continue after errors",
            &mut self.continue_on_error,
        )
        .on_hover_text(
            "Run every line and count the refused ones (SCRIPT ... CONTINUE), rather than stop \
             at the first",
        );
        self.refresh_if_changed();
        ui.horizontal(|ui| {
            ui.label("Command:");
            let mut command = self.command.clone();
            line_with(
                ui,
                "scriptCommand",
                &mut command,
                LineOptions {
                    read_only: true,
                    width: Some(380.0),
                    ..LineOptions::default()
                },
            )
            .on_hover_text(
                "The line Run hands to the command line - type it there and it does the same",
            );
        });
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let mut run = false;
        ui.horizontal(|ui| {
            label_as(
                ui,
                "scriptStatus",
                &self.status,
                egui::Label::new(self.status.as_str()).wrap(),
            );
            run = ui
                .add_enabled_ui(self.ready, |ui| button(ui, "scriptRun", "Run"))
                .inner
                .clicked();
            if button(ui, "scriptClose", "Close").clicked() {
                self.close = true;
            }
        });
        if run {
            self.run(host);
        }
    }
}

/// File > Run Script (see the module comment).
pub struct ScriptRunDialog {
    frame: DialogFrame,
    state: ScriptRunState,
    /// Whether it is shown.
    pub open: bool,
}

impl Default for ScriptRunDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptRunDialog {
    pub fn new() -> Self {
        let mut state = ScriptRunState::default();
        state.refresh_if_changed();
        Self {
            frame: DialogFrame::new("fileRunScriptDialog", "Run Script"),
            state,
            open: true,
        }
    }

    /// Fills the path, as a pick from a file dialog does.
    pub fn set_script_path(&mut self, path: &str) {
        self.state.path = to_native_separators(path);
        self.state.refresh_if_changed();
    }

    /// The line Run would run, or why there is none (no file named).
    pub fn command(&self) -> Result<String> {
        self.state.command()
    }

    /// What Run does: the SCRIPT line handed to the executor and the outcome said in scriptStatus.
    pub fn run(&mut self, host: &mut dyn Host) {
        self.state.refresh_if_changed();
        self.state.run(host);
    }

    /// What scriptStatus says.
    pub fn status(&self) -> &str {
        &self.state.status
    }

    /// What scriptPreview shows.
    pub fn preview(&self) -> &str {
        &self.state.preview
    }

    /// Draws the dialog while it is open; Close and the title bar's close hide it.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.open {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut parts = (&mut self.state, &mut *host);
        let shown = self.frame.show(
            &ctx,
            &mut parts,
            |ui, parts| parts.0.body(ui, parts.1),
            |ui, parts| parts.0.footer(ui, parts.1),
        );
        if shown.closed || self.state.close {
            self.state.close = false;
            self.open = false;
        }
    }
}
