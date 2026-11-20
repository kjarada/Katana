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
use katana_core::ErrorCode;

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
            // The status bar says it in a person's words: the box's title as a sentence's start,
            // and the error's sentence without its code. It read "Open Failed: NotFound: no
            // Katana project in this directory [...]" (the sixth look audit's I); the log's record
            // keeps both, as --report and the tests read them.
            let shown = format!(
                "{}: {}",
                title_as_words(title),
                without_error_code(&simplified(text))
            );
            if let Some((status, _)) = self.status.as_mut() {
                *status = shown;
            }
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

    /// The error the status bar shows at `now`: the last one logged, for six seconds, as a person
    /// reads it (`without_error_code`). The log keeps the line whole.
    pub fn status_message(&self, now: Instant) -> Option<&str> {
        let (text, since) = self.status.as_ref()?;
        (now.saturating_duration_since(*since) < STATUS_MESSAGE)
            .then(|| without_error_code(text.as_str()))
    }
}

/// Every code an error's description can start with (katana_core::Error::describe).
const ERROR_CODES: [ErrorCode; 17] = [
    ErrorCode::InvalidArgument,
    ErrorCode::InvalidState,
    ErrorCode::NotFound,
    ErrorCode::AlreadyExists,
    ErrorCode::ParseFailure,
    ErrorCode::Unsupported,
    ErrorCode::InvalidGeometry,
    ErrorCode::InvalidCrs,
    ErrorCode::InvalidSurveyObservation,
    ErrorCode::AdjustmentFailure,
    ErrorCode::TriangulationFailure,
    ErrorCode::FileImportFailure,
    ErrorCode::FileExportFailure,
    ErrorCode::DatabaseFailure,
    ErrorCode::RenderingFailure,
    ErrorCode::CommandRejected,
    ErrorCode::Internal,
];

/// `text` without the code an error's description starts with ("InvalidState: nothing is
/// selected; use SELECT first" is "nothing is selected; use SELECT first"): what the status bar
/// shows a person, who reads the sentence, not the code - the second look audit's N12. The code
/// stays in the log and in what a line hands back, which drivers and tests read. A text that does
/// not start with a code comes back as it is.
pub fn without_error_code(text: &str) -> &str {
    ERROR_CODES
        .iter()
        .find_map(|code| {
            text.strip_prefix(katana_core::error::to_string(*code))
                .and_then(|rest| rest.strip_prefix(": "))
        })
        .unwrap_or(text)
}

/// A box's title ("Open Failed", "Export IFC Failed") as the start of a sentence: each word after
/// the first that is a capital and small letters made small ("Open failed", "Export IFC failed");
/// an initialism or a name in capitals is left as it is.
fn title_as_words(title: &str) -> String {
    title
        .split(' ')
        .enumerate()
        .map(|(at, word)| {
            let mut letters = word.chars();
            let capitalised = letters.next().is_some_and(char::is_uppercase)
                && letters.clone().next().is_some()
                && letters.all(char::is_lowercase);
            if at > 0 && capitalised {
                word.to_lowercase()
            } else {
                String::from(word)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// An error's words as a person reads them: its sentence without the code it starts with, or -
/// a box's failure, "Open Failed: NotFound: ..." (`CommandLog::warn_user`) - the box's title as
/// words and the sentence after it without its code ("Open failed: ...").
fn error_words(error: &str) -> String {
    let sentence = without_error_code(error);
    if sentence.len() != error.len() {
        return String::from(sentence);
    }
    if let Some((title, rest)) = error.split_once(": ") {
        let said = without_error_code(rest);
        if said.len() != rest.len() {
            return format!("{}: {said}", title_as_words(title));
        }
    }
    String::from(error)
}

/// The command log's text as the log panel shows it, a line at a time (`shown_log_line`).
pub fn shown_log(log: &str) -> String {
    log.split('\n')
        .map(shown_log_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where the newest record - the last block logged - starts in the log as the panel shows it
/// (`shown_log` of the blocks joined by line ends), counted in characters as a text's layout
/// counts them; None for a log that holds nothing. What the panel reads a record from its start
/// by, when the record is taller than the panel (main_window::panels::log_follow_offset).
///
/// Counted in the shown text, not the record: an error is shown without its code and a warning
/// as its sentence, so a line's length differs between the two.
pub fn newest_record_start(blocks: &[String]) -> Option<usize> {
    let (_, earlier) = blocks.split_last()?;
    if earlier.is_empty() {
        return Some(0);
    }
    // The earlier blocks as shown, and the line end that parts the newest from them.
    Some(shown_log(&earlier.join("\n")).chars().count() + 1)
}

/// One line of the log as a person reads it: an error ("! ...") as a sentence - without the code
/// its description starts with (`without_error_code`), its first letter a capital as the status
/// bar shows it ("! unknown command; ..." read as a fragment, the sixth look audit's H) - and a
/// warning record (`warning text="..."`) as the sentence it carries, "Warning: ...", where its
/// quotes were shown escaped. Every other line as it is. What is shown only: the log's record, what
/// --report, the drivers and the tests read, keeps the line as it was logged.
pub fn shown_log_line(line: &str) -> String {
    if let Some(error) = line.strip_prefix("! ") {
        return format!("! {}", crate::drive::controls::sentence_start(&error_words(error)));
    }
    if line.starts_with("warning ") {
        let records = katana_app::geo::replies::parse_records(line);
        if let [record] = records.as_slice()
            && record.kind == "warning"
            && record.body.is_empty()
            && let [(key, text)] = record.fields.as_slice()
            && key == "text"
            && !text.is_empty()
        {
            return format!(
                "Warning: {}",
                crate::drive::controls::sentence_start(text)
            );
        }
    }
    String::from(line)
}

/// What a line of the log is to a person reading it, by its shown words (`shown_log_line`): an
/// error ("! ..."), a warning ("Warning: ..."), or anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogTone {
    Plain,
    Warning,
    Error,
}

/// The tone of `shown`, a line of the log as `shown_log_line` shows it (its line end, if any,
/// kept or not).
pub fn log_line_tone(shown: &str) -> LogTone {
    if shown.starts_with("! ") {
        LogTone::Error
    } else if shown.starts_with("Warning: ") {
        LogTone::Warning
    } else {
        LogTone::Plain
    }
}

/// The colour the log panel draws `shown` in: an error in the error token, a warning in the
/// warning token, every other line in the text colour. The warnings were the text colour, and a
/// far import's advice read as one more reply (the seventh look audit), as a refusal did before
/// errors took their colour (the third's H).
pub fn log_line_colour(shown: &str) -> crate::canvas::Color {
    use crate::theme::palette;
    match log_line_tone(shown) {
        LogTone::Error => palette::error(),
        LogTone::Warning => palette::warning(),
        LogTone::Plain => palette::text(),
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use katana_core::{Error, ErrorCode};

    use super::{
        CommandLog, newest_record_start, shown_log, shown_log_line, without_error_code,
    };

    #[test]
    fn the_newest_record_starts_after_the_earlier_ones_as_they_are_shown() {
        // Counted by hand, in the text the panel shows (shown_log), a line end between records.
        let block = |text: &str| String::from(text);
        // Nothing logged: no record to start anywhere.
        assert_eq!(newest_record_start(&[]), None);
        // One record is the log's start, however many lines it has.
        assert_eq!(newest_record_start(&[block("one\ntwo")]), Some(0));
        // "> A" is 3 characters and its line end 1: the second record starts at 4.
        assert_eq!(newest_record_start(&[block("> A"), block("reply=1")]), Some(4));
        // An earlier record of two lines, "ab" and "cd": 2 + 1 + 2, and its line end.
        assert_eq!(newest_record_start(&[block("ab\ncd"), block("x")]), Some(6));
        // An error is shown without its code: "! NotFound: no file" is shown "! No file", 9
        // characters, where the record has 19 - so the newest starts at 10, not 20.
        let blocks = [block("! NotFound: no file"), block("next")];
        assert_eq!(shown_log(&blocks[0]), "! No file");
        assert_eq!(newest_record_start(&blocks), Some(10));
        // Characters, not bytes, as a text's layout counts them: "é" is one.
        assert_eq!(newest_record_start(&[block("é"), block("x")]), Some(2));
        // An empty newest record starts at the text's end.
        assert_eq!(newest_record_start(&[block("ab"), block("")]), Some(3));
    }

    #[test]
    fn the_status_bar_shows_an_errors_sentence_and_the_log_keeps_its_code() {
        // The second look audit (N12): the status bar read "InvalidState: nothing is selected;
        // use SELECT first", the code a driver reads, where a person reads the sentence.
        let described = Error::new(
            ErrorCode::InvalidState,
            "nothing is selected; use SELECT first",
        )
        .describe();
        let mut log = CommandLog::new();
        log.log(&described, true);
        let now = Instant::now();
        assert_eq!(
            log.status_message(now),
            Some("nothing is selected; use SELECT first")
        );
        assert_eq!(log.blocks(), [format!("! {described}")]);
        // Gone after its six seconds, as before.
        assert_eq!(
            log.status_message(now + super::STATUS_MESSAGE + Duration::from_millis(1)),
            None
        );
    }

    #[test]
    fn a_boxs_failure_is_said_in_the_status_bar_without_its_code_and_logged_whole() {
        // The sixth look audit's I: headless, the status bar read "Open Failed: NotFound: no
        // Katana project in this directory [...]".
        let described = Error::new(ErrorCode::NotFound, "no Katana project in this directory")
            .with_context("/data/site")
            .describe();
        let mut log = CommandLog::new();
        log.set_headless(true);
        log.warn_user("Open Failed", &described);
        assert_eq!(
            log.status_message(Instant::now()),
            Some("Open failed: no Katana project in this directory [/data/site]")
        );
        assert_eq!(log.blocks(), [format!("! Open Failed: {described}")]);
        // A title's initialism stays as it is.
        log.warn_user("Export IFC Failed", "said");
        assert_eq!(
            log.status_message(Instant::now()),
            Some("Export IFC failed: said")
        );
    }

    #[test]
    fn the_log_shows_an_error_as_a_sentence_and_a_warning_record_as_its_words() {
        // The sixth look audit's H: "! unknown command; type HELP [FOO]" began small, and a
        // warning's quotes showed escaped.
        assert_eq!(
            shown_log_line("! unknown command; type HELP [FOO]"),
            "! Unknown command; type HELP [FOO]"
        );
        assert_eq!(
            shown_log_line("! ParseFailure: unknown arrow head [x]"),
            "! Unknown arrow head [x]"
        );
        assert_eq!(
            shown_log_line(r#"warning text="super tin \"Site Surfaces\" ranks the tins""#),
            r#"Warning: Super tin "Site Surfaces" ranks the tins"#
        );
        assert_eq!(shown_log_line("warning text=one"), "Warning: One");
        // A box's failure as the status bar says it.
        assert_eq!(
            shown_log_line("! Open Failed: NotFound: no Katana project in this directory [/p]"),
            "! Open failed: no Katana project in this directory [/p]"
        );
        assert_eq!(shown_log_line("! Style: no style"), "! Style: no style");
        // Only the warning record itself: another record, a warning with other fields, a reply
        // and an echoed line are shown as they are.
        for line in [
            "warning text=\"a\" layer=0",
            "settings_warning text=\"line 3\"",
            "warning text=",
            "warnings=2",
            "> SELECT 99999",
            "imported points=3",
        ] {
            assert_eq!(shown_log_line(line), line);
        }
        assert_eq!(
            shown_log("> FOO\n! unknown command; type HELP [FOO]"),
            "> FOO\n! Unknown command; type HELP [FOO]"
        );
    }

    #[test]
    fn only_a_code_at_the_front_is_taken_off_and_every_code_is_known() {
        assert_eq!(without_error_code("a plain note"), "a plain note");
        assert_eq!(
            without_error_code("Style: InvalidState: no style"),
            "Style: InvalidState: no style"
        );
        assert_eq!(without_error_code("NotFound:no gap"), "NotFound:no gap");
        for code in super::ERROR_CODES {
            let described = Error::new(code, "said").with_context("where").describe();
            assert_eq!(without_error_code(&described), "said [where]", "{code}");
        }
    }
}
