//! The command log: what every line the window runs says, and what a line run through the one
//! executor hands back (main_window.cpp's logMessage, VerbCapture, errorsLogged_, warnUser's
//! capture, and the echo of a typed or executed line with its secrets masked).
//!
//! It is the window's `katana_app::console::Console` (the plan's section 4.2, settling step 6's
//! open door): `out` is a reply, `err` an error, each a logMessage. So a verb of the app layer that
//! prints through a console - IFC's, the update verbs' - prints into the log as the window's own
//! verbs do, and its errors are counted against the line.
//!
//! WHAT A LINE HANDS BACK (runVerbLine). A line run through the executor is bracketed by
//! `begin_verb` and `end_verb`: what is logged between them is captured, the replies and the errors
//! apart, and the line is ok when it logged no error and no box said it failed. The errors are
//! COUNTED, not the lines captured: PLOTSHEETS logs the problems of a plot it carried out and takes
//! them off the count (`set_errors_logged`), and a --command run judges the same line the same way
//! (`errors_logged`). A line run inside another's run - a script's, under the SCRIPT line a dialog
//! ran - adds what it captured to the outer line's too, so the dialog that ran the SCRIPT line gets
//! back everything its lines said.
//!
//! WHAT IS ECHOED. A typed line and an executed one are echoed "> line" - but an ONLINE KEY's value
//! never is, nor what stands where someone hoped UPDATE TOKEN would take a token: those words read
//! "***" (`logged_line`). The echo is the log's alone: it is neither captured nor printed.
//!
//! HEADLESS. A headless run has no window to read the log in, so every logged line is also written
//! to the echo console lent to the log (`set_echo`: the process's stderr in the program, a capture
//! in a test), "! " before an error: what a script - or a ctest check - can see of what a command
//! reported.
//!
//! The masking functions are here, one home: the online workbench's `loggedLine` and the update
//! dialog's `updateLoggedLine` were the C++'s, in gis_online.cpp and update_dialog.cpp, and the
//! log was their only reader of the line; GA's and UP's ports call these (`online_logged_line`,
//! `update_logged_line`, `update_token_line_words`, `is_update_token_verb_word`).

use std::time::{Duration, Instant};

use katana_app::console::Console;

use crate::drive::simplified;
use crate::host::VerbOutcome;

/// How long an error stays in the status bar (statusBar()->showMessage(text, 6000)).
pub const STATUS_MESSAGE: Duration = Duration::from_millis(6000);

/// What a line run through the executor logged (MainWindow::VerbCapture).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VerbCapture {
    pub reply: Vec<String>,
    pub errors: Vec<String>,
    /// A failure warnUser showed in a box, which is not an error logged.
    pub failed: bool,
}

/// A line being run through the executor, from `begin_verb` to `end_verb`.
#[derive(Debug)]
#[must_use = "a line begun is ended by end_verb, which hands back what it did"]
pub struct VerbRun {
    /// The line as it is dispatched: trimmed.
    pub line: String,
    errors_before: usize,
}

/// The command log (see the module comment).
#[derive(Default)]
pub struct CommandLog {
    /// What the log shows, a block each (QPlainTextEdit::appendPlainText): an error with "! "
    /// before it, an echoed line with "> ".
    blocks: Vec<String>,
    /// The lines being run through the executor, innermost last.
    captures: Vec<VerbCapture>,
    errors_logged: usize,
    headless: bool,
    echo: Option<Box<dyn Console>>,
    /// The error the status bar shows, and since when.
    status: Option<(String, Instant)>,
    /// The boxes warnUser asked for, for the window to show: title and text.
    warnings: Vec<(String, String)>,
}

impl CommandLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// MainWindow::setHeadless: every logged line is then also written to the echo console.
    pub fn set_headless(&mut self, headless: bool) {
        self.headless = headless;
    }

    pub fn headless(&self) -> bool {
        self.headless
    }

    /// Where a headless run's log is written: the process's stderr (`katana_app::console::
    /// Terminal`) in the program, a capture in a test; None writes it nowhere.
    pub fn set_echo(&mut self, echo: Option<Box<dyn Console>>) {
        self.echo = echo;
    }

    /// MainWindow::logMessage: `text` in the log, captured by the line being run, and - an error -
    /// counted as the line's refusal and shown in the status bar. Nothing for an empty text.
    pub fn log(&mut self, text: &str, is_error: bool) {
        if text.is_empty() {
            return;
        }
        let line = if is_error {
            format!("! {text}")
        } else {
            String::from(text)
        };
        self.blocks.push(line.clone());
        if let Some(capture) = self.captures.last_mut() {
            if is_error {
                capture.errors.push(String::from(text));
            } else {
                capture.reply.push(String::from(text));
            }
        }
        if is_error {
            self.errors_logged += 1;
            self.status = Some((String::from(text), Instant::now()));
        }
        if self.headless
            && let Some(echo) = self.echo.as_mut()
        {
            echo.err(&format!("{line}\n"));
        }
    }

    /// A typed or executed line echoed in the log: "> " and the line with its secrets masked
    /// (`logged_line`). Neither captured nor printed.
    pub fn echo_line(&mut self, line: &str) {
        self.blocks.push(format!("> {}", logged_line(line)));
    }

    /// A text appended to the log as it is, captured by nothing (the window's own notes).
    pub fn append(&mut self, text: &str) {
        self.blocks.push(String::from(text));
    }

    /// The start of MainWindow::runVerbLine: the line trimmed, echoed and captured from here. None
    /// for an empty line, which runs nothing: the caller answers `no_command()`.
    pub fn begin_verb(&mut self, line: &str) -> Option<VerbRun> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        self.echo_line(trimmed);
        self.captures.push(VerbCapture::default());
        Some(VerbRun {
            line: String::from(trimmed),
            errors_before: self.errors_logged,
        })
    }

    /// The end of runVerbLine: what the line logged, handed back; added to the outer line's
    /// capture when it ran inside another.
    pub fn end_verb(&mut self, run: VerbRun) -> VerbOutcome {
        let capture = self.captures.pop().unwrap_or_default();
        if let Some(outer) = self.captures.last_mut() {
            outer.reply.extend(capture.reply.iter().cloned());
            outer.errors.extend(capture.errors.iter().cloned());
            outer.failed |= capture.failed;
        }
        VerbOutcome {
            ok: self.errors_logged == run.errors_before && !capture.failed,
            reply: capture.reply.join("\n"),
            error: capture.errors.join("\n"),
        }
    }

    /// What runVerbLine answers an empty line: an empty TYPED line is Enter in the drawing, but a
    /// dialog has no Enter to press, so for it an empty line is a line it failed to build.
    pub fn no_command() -> VerbOutcome {
        VerbOutcome::new(false, "", "there is no command to run")
    }

    /// MainWindow::warnUser's part of the log: headless, the warning is a logged error (a modal box
    /// there is a hang); otherwise the line being run is told it failed, and why - a box is not a
    /// logged error - and the box is queued for the window to show (`take_warnings`).
    pub fn warn_user(&mut self, title: &str, text: &str) {
        let said = format!("{title}: {}", simplified(text));
        if self.headless {
            self.log(&said, true);
            return;
        }
        if let Some(capture) = self.captures.last_mut() {
            capture.errors.push(said);
            capture.failed = true;
        }
        self.warnings
            .push((String::from(title), String::from(text)));
    }

    /// The boxes warnUser asked for since the last call, title and text.
    pub fn take_warnings(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.warnings)
    }

    /// The errors logged so far (errorsLogged_): MainWindow::runCommand judges a line by them.
    pub fn errors_logged(&self) -> usize {
        self.errors_logged
    }

    /// Takes errors off the count, as PLOTSHEETS does for the problems of a plot it carried out.
    pub fn set_errors_logged(&mut self, errors: usize) {
        self.errors_logged = errors;
    }

    /// The log's blocks, in order.
    pub fn blocks(&self) -> &[String] {
        &self.blocks
    }

    /// The log's whole text (QPlainTextEdit::toPlainText).
    pub fn text(&self) -> String {
        self.blocks.join("\n")
    }

    /// QPlainTextEdit::clear.
    pub fn clear(&mut self) {
        self.blocks.clear();
    }

    /// The error the status bar shows at `now`: the last one logged, for six seconds.
    pub fn status_message(&self, now: Instant) -> Option<&str> {
        let (text, since) = self.status.as_ref()?;
        (now.saturating_duration_since(*since) < STATUS_MESSAGE).then_some(text.as_str())
    }
}

impl Console for CommandLog {
    /// A reply, its line end the log's own.
    fn out(&mut self, text: &str) {
        self.log(text.strip_suffix('\n').unwrap_or(text), false);
    }

    /// An error, counted against the line being run.
    fn err(&mut self, text: &str) {
        self.log(text.strip_suffix('\n').unwrap_or(text), true);
    }
}

// ---- the words a log never shows --------------------------------------------------------------

/// One character as Qt::CaseInsensitive compares it: its simple case fold, taken as upper then
/// lower case where each is one character (so 'ſ' is 's' and the Kelvin sign 'k'), else as it is.
fn fold_char(character: char) -> char {
    let mut upper = character.to_uppercase();
    let (Some(single), None) = (upper.next(), upper.next()) else {
        return character;
    };
    let mut lower = single.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(folded), None) => folded,
        _ => character,
    }
}

/// `text` case-folded as Qt::CaseInsensitive compares and searches it: the window's one fold, for
/// its word comparisons and its dialogs' searches.
pub(crate) fn folded(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

/// QString::compare(.., Qt::CaseInsensitive) == 0.
pub(crate) fn same_ignoring_case(a: &str, b: &str) -> bool {
    a.chars().map(fold_char).eq(b.chars().map(fold_char))
}

/// QString::contains(.., Qt::CaseInsensitive).
pub(crate) fn contains_ignoring_case(haystack: &str, needle: &str) -> bool {
    folded(haystack).contains(&folded(needle))
}

/// `line` as the log may show it: an ONLINE KEY's value and an UPDATE TOKEN's words masked
/// (runTypedLine's and runVerbLine's `updateLoggedLine(OnlineDataWorkbench::loggedLine(line))`).
pub fn logged_line(line: &str) -> String {
    update_logged_line(&online_logged_line(line))
}

/// OnlineDataWorkbench::loggedLine: ONLINE KEY <provider> <value> echoed with the value hidden,
/// since a key is a secret a screen share or a log file should not carry. Any other line comes back
/// as it is.
pub fn online_logged_line(line: &str) -> String {
    let words: Vec<&str> = line
        .trim()
        .split(' ')
        .filter(|word| !word.is_empty())
        .collect();
    if words.len() >= 4
        && same_ignoring_case(words[0], "ONLINE")
        && same_ignoring_case(words[1], "KEY")
    {
        return format!("{} {} {} ***", words[0], words[1], words[2]);
    }
    String::from(line)
}

/// The words of `line` when it is UPDATE TOKEN with something after it, and none otherwise: the
/// lines whose later words may be a token somebody typed (update_dialog.cpp's tokenLineWords).
pub fn update_token_line_words(line: &str) -> Vec<String> {
    let words: Vec<String> = simplified(line)
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(String::from)
        .collect();
    if words.len() < 3
        || !same_ignoring_case(&words[0], "UPDATE")
        || !same_ignoring_case(&words[1], "TOKEN")
    {
        return Vec::new();
    }
    words
}

/// The two words the grammar has after UPDATE TOKEN; every other is hidden.
pub fn is_update_token_verb_word(word: &str) -> bool {
    same_ignoring_case(word, "STATUS") || same_ignoring_case(word, "CLEAR")
}

/// updateLoggedLine: there is no UPDATE line that carries an access token, but a person may type
/// one that does (UPDATE TOKEN ghp_..., UPDATE TOKEN SET ghp_...), and a typed line is echoed
/// before any verb sees it. So every word after UPDATE TOKEN that is not STATUS or CLEAR is shown as
/// "***"; the verbs are still handed the line as it was typed, and refuse it. A line with nothing to
/// hide is echoed as it was typed, blanks and all.
pub fn update_logged_line(line: &str) -> String {
    let words = update_token_line_words(line);
    if words.is_empty() {
        return String::from(line);
    }
    let mut shown = vec![words[0].clone(), words[1].clone()];
    let mut hidden = false;
    for word in &words[2..] {
        let known = is_update_token_verb_word(word);
        shown.push(if known {
            word.clone()
        } else {
            String::from("***")
        });
        hidden = hidden || !known;
    }
    if hidden {
        shown.join(" ")
    } else {
        String::from(line)
    }
}
