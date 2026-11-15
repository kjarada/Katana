//! One command-line session: a Document, the CommandInterpreter over it, and
//! the verbs the front ends add above katana_cad - the native IFC
//! IMPORT/EXPORT, SURVEY and the GDAL/PDAL ones of the geoprocessing
//! executor. (CODE and CUSTOMISE are the interpreter's own; a session only
//! hands it the host CUSTOMISE RESET, KEEP and REVERT need.) The port of
//! src/katana_app/session.hpp/.cpp.
//!
//! katana_cli and katana_mcp both drive it, so a line typed at the command
//! line and a line sent by Claude over MCP (docs/mcp.md) do exactly the same
//! thing.
//!
//! Each line is reported on a [`Console`] its caller lends it for the call:
//! `out` (what it did) and `err` (errors and warnings) - the C++'s std::cout
//! and std::cerr. katana_cli lends the process's streams ([`crate::Terminal`]);
//! katana_mcp captures both around each line and hands them back to the
//! client ([`crate::Captured`]).
//!
//! The UPDATE verbs (update_verbs.rs) are taken here too, and act on the
//! PROGRAM that runs the session, not on its drawing (session/update_part.rs).
//!
//! THE RUST SHAPE (docs/rust.md, step 6). The session owns its drawing and its
//! interpreter and lends the rest per call, as the interpreter itself does
//! (it does not hold the Document): the console for each line, an update
//! context made for each UPDATE line. A line is bytes, as the C++'s
//! `run(const std::string&)` takes it; off Windows a line that is not UTF-8 is
//! refused at this door, since the interpreter reads text (where the C++ hands
//! the bytes on and refuses a name at its table's door - see [`Session::run`]).
//! A panic out of a verb is caught and reported as `error: Internal: ...`, as
//! the C++ catches every std::exception, so katana_cli's unsaved drawing
//! survives it.
//!
//! SURVEY, IFC and the geo executor are joined below in the C++'s order, and
//! so are the reference layers (interop's reference data): recorded before a
//! SAVE that has somewhere to go, let go with the drawing on NEW and OPEN,
//! and read again by an OPEN whose project records them.

mod update_part;

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use katana_cad::customisation_host::{
    KEPT_CUSTOMISATION_VARIABLE, built_in_advice_for, built_in_customisation, built_ins,
    start_customisation,
};
use katana_cad::{CommandInterpreter, CustomisationHost, CustomisationOrigin, Document};
use katana_core::Result;
use katana_core::error::make_error;
use katana_core::path_text::{environment_variable, path_from_utf8, read_file_bytes};
use katana_core::text::{split_lines, trimmed};
use katana_core::text_encoding::decode_text;
use katana_interop::ReferenceData;
use katana_update::{ApplyRequest, install_root_of_running_program, recover};

use crate::console::Console;
use crate::update_verbs::{
    UpdateSurface, abandon_apply, is_update_apply_line, is_update_line, update_help_text,
};
use crate::{geo, ifc_verbs, survey_verbs};
use update_part::{UpdatePart, run_update};

pub use update_part::UpdateContextSource;

/// QUIT and EXIT, in any case: a front end's cue to stop, never run. The
/// first word of the line, as the C++ reads it byte by byte.
fn is_quit_line(line: &[u8]) -> bool {
    let verb = upper_verb_of(line);
    verb == b"QUIT" || verb == b"EXIT"
}

// The line's first word in capitals (ASCII only, as std::toupper in the
// classic locale does).
fn upper_verb_of(line: &[u8]) -> Vec<u8> {
    let mut verb: Vec<u8> = Vec::new();
    for &byte in line {
        if byte == b' ' || byte == b'\t' {
            if !verb.is_empty() {
                break;
            }
            continue;
        }
        verb.push(byte.to_ascii_uppercase());
    }
    verb
}

fn upper_verb(line: &str) -> String {
    // Only ASCII letters change, so UTF-8 in stays UTF-8 out.
    String::from_utf8_lossy(&upper_verb_of(line.as_bytes())).into_owned()
}

// The line's words in capitals with one blank between them, so that a line of
// two words is compared as one text however it was typed.
fn upper_words(line: &str) -> String {
    let mut words = String::new();
    let mut gap = false;
    for ch in line.chars() {
        if ch == ' ' || ch == '\t' {
            gap = !words.is_empty();
            continue;
        }
        if gap {
            words.push(' ');
            gap = false;
        }
        words.push(ch.to_ascii_uppercase());
    }
    words
}

// The desktop window's own verbs (MainWindow::dispatchLine), which this
// session cannot run: why, and what to use here instead. Answered by name,
// not as an unknown command, so a script written for File > Run Script that
// uses one fails in katana_cli and katana_mcp saying where it runs. None for
// any other verb.
fn window_only_verb(verb: &str) -> Option<&'static str> {
    match verb {
        "PLOT" | "PLOTSHEETS" | "SNAPSHOT" => Some(
            "is the desktop window's: its painter is Qt's, which katana_cli and katana_mcp do \
             not have; a headless katana run has --plot and --plot-sheets",
        ),
        "ONLINE" => Some(
            "is the desktop window's (GIS > Online Data), which keeps the provider keys and runs \
             the requests; run katana headless with --command \"ONLINE ...\"",
        ),
        "SCRIPT" => Some(
            "is the desktop window's: katana_cli runs a script given as its argument, and \
             katana_mcp with katana_run_script",
        ),
        // They were unknown commands here, although the help said the window's
        // verbs are refused by name. VIEWS and ZOOM are the interpreter's, which
        // refuses them by name itself when no window answers for the views.
        "GRID" | "EXAGGERATION" => Some(
            "is the desktop window's: it sets how the window's views draw, and katana_cli and \
             katana_mcp have no views; run katana headless with --command",
        ),
        _ => None,
    }
}

// `names`, each in quotes, with commas between.
fn quoted_names(names: &[String]) -> String {
    let mut text = String::new();
    for name in names {
        text.push_str(if text.is_empty() { "\"" } else { ", \"" });
        text.push_str(name);
        text.push('"');
    }
    text
}

// What a program's session is handed for its customisation
// (cad's customisation_host): the built-in of this run - the one compiled
// in, or what the seam KATANA_BUILTIN_CUSTOMISATION says in its place - and
// the kept file, which here is the one KATANA_CUSTOMISATION names and nothing
// else. No per-user place is read: a command line, and a server an agent
// drives, must do the same thing on every machine and for every user.
//
// The variable is read through core, never std::env::var: on Windows that is
// the wide environment, so a file named outside the ANSI code page is found
// (core's path_text).
fn host_of_this_run() -> CustomisationHost {
    let built_in = built_in_customisation();
    let kept = environment_variable(Some(KEPT_CUSTOMISATION_VARIABLE));
    CustomisationHost {
        built_in,
        kept_file: if kept.is_empty() {
            PathBuf::new()
        } else {
            path_from_utf8(&kept)
        },
        ..CustomisationHost::default()
    }
}

// `count` and the noun for it. The start-up line gave every count the plural:
// a customisation of one definition and one rule had "1 linestyles and
// symbols and 1 survey code rules", where the window, for the same start,
// says "1 definition" and "1 survey code rule".
fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

// The customisation a program's session starts with, so that it can draw a
// survey rather than wait to be told where its linestyles are: the kept one
// when there is one and it reads, else the built-in, else none - cad's choice
// (start_customisation), said here in this front end's words. The interpreter
// is handed the host FIRST: it notes the kept file as it is at that moment,
// and CUSTOMISE KEEP refuses to write over one that is another later.
fn start_with_the_hosts_customisation(
    document: &mut Document,
    interpreter: &mut CommandInterpreter,
    console: &mut dyn Console,
) {
    let host = host_of_this_run();
    interpreter.set_customisation_host(host.clone());
    let start = start_customisation(document, &host);
    // What went wrong is said, and the session starts all the same (audit
    // A12-06): unsaid, a built-in or a kept file that did not read drew every
    // drawing's linestyles as plain lines without a word. Said HERE it reaches
    // whoever reads this program's standard error, which a client of
    // katana_mcp does not: for that reader start_customisation has left the
    // same sentences on the Document, and CUSTOMISE JSON, katana_customisation
    // and katana://customisation give them as `start`.
    for problem in &start.problems {
        console.err(&format!("error: {problem}\n"));
    }
    if start.installed == CustomisationOrigin::None {
        return;
    }
    let kept = start.installed == CustomisationOrigin::Kept;
    // One definition is a linestyle OR a symbol; several are linestyles and
    // symbols, whichever they are (CustomisationStart::definitions counts both).
    console.out(&format!(
        "Customisation: {}, {} and {}, {}\n",
        start.name,
        counted(
            start.definitions,
            "linestyle or symbol",
            "linestyles and symbols"
        ),
        counted(start.rules, "survey code rule", "survey code rules"),
        if kept { "kept" } else { "built in" }
    ));
    if start.kept_from_another_built_in {
        // It is the user's, so it is what starts; they may still want to know
        // that the program's own has moved on since they kept theirs.
        console.err(
            "warning: the kept customisation was made from another built-in customisation than \
             this program has; CUSTOMISE RESET gives this program's and CUSTOMISE BUILTIN lists \
             the built-in customisations it has\n",
        );
    }
}

/// The text of a panic's payload, as the C++ says an exception's what();
/// `otherwise` for a payload that holds no text (the session's and the MCP
/// tools' own sentences).
pub(crate) fn panic_text(payload: &(dyn Any + Send), otherwise: &str) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        String::from(*text)
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        String::from(otherwise)
    }
}

/// Why a line that is not UTF-8 is refused off Windows. The C++ hands such a
/// line on, where a path in it reaches a file named in Latin-1 and a NAME is
/// refused at its table's door with "not valid UTF-8"; the Rust interpreter
/// reads text, so the line is refused here, in the same words (docs/rust.md,
/// "Where step 6's Rust differs").
#[cfg(not(windows))]
fn not_utf8_refusal() -> katana_core::Error {
    katana_core::Error::new(
        katana_core::ErrorCode::InvalidArgument,
        "the line is not valid UTF-8 text, and a line is read only as UTF-8 here: save a script \
         as UTF-8, or set the terminal or program that sends the line to UTF-8",
    )
}

/// One command-line session (the module's head).
pub struct Session {
    document: Document,
    interpreter: CommandInterpreter,
    // The reference rasters and point clouds: lent to the geo executor with
    // the drawing for each line.
    reference: ReferenceData,
    updates: UpdatePart,
    // The geo executor's scratch folder (this process's, taken once) and its
    // front-end hooks, of which a session has none: the C++'s geo Context
    // holds them for the session's life; the Rust builds a Context of
    // borrows for each line from these and the drawing.
    geo_scratch: PathBuf,
    geo_front_end: geo::FrontEnd,
    // Whether Session::run has said that it read a line as Windows-1252: once
    // a session, not once a line, or a script saved in the ANSI code page
    // would say it on every line.
    #[cfg(windows)]
    said_windows_1252: bool,
}

impl Session {
    /// A PROGRAM's session (`program` names it: katana_cli's argv[0]) starts
    /// with a customisation, and says which on `out`: the kept one when the
    /// environment variable KATANA_CUSTOMISATION names a file that reads, else
    /// the program's built-in one - which KATANA_BUILTIN_CUSTOMISATION can
    /// replace for one run, or switch off with `none` - else none (cad's
    /// customisation_host). What could not be read is said on `err` and the
    /// session starts all the same; it also stays in the session's report
    /// (CUSTOMISE JSON, `start`), for a reader that is never shown `err`. Both
    /// variables are read as Unicode on every platform (core's
    /// environment_variable), so a file may sit under a folder named in any
    /// script.
    ///
    /// None is a session of no program - a test's, a tool's (the C++'s
    /// nullptr): it is handed no host, reads neither variable and starts
    /// EMPTY, whatever this build compiled in, so what it does is the same on
    /// every machine. CUSTOMISE RESET, KEEP and REVERT are refused there by
    /// name. Only whether `program` is given is read.
    pub fn new(program: Option<&str>, console: &mut dyn Console) -> Session {
        let mut session = Session {
            document: Document::new(None),
            interpreter: CommandInterpreter::new(),
            reference: ReferenceData::default(),
            updates: UpdatePart {
                // A session of no program has nothing for UPDATE to act on.
                program: program.is_some(),
                ..UpdatePart::default()
            },
            geo_scratch: geo::own_scratch(),
            geo_front_end: geo::FrontEnd::default(),
            #[cfg(windows)]
            said_windows_1252: false,
        };
        // No colour callback is handed over: the interpreter resolves a colour
        // name through the Document - the customisation's own table, then the
        // standard names - which is everything this session could have
        // answered.
        if program.is_some() {
            start_with_the_hosts_customisation(
                &mut session.document,
                &mut session.interpreter,
                console,
            );
        }
        session
    }

    /// Runs one line. False when it failed; either way what happened has been
    /// written to `console`. '#' notes and empty lines succeed.
    ///
    /// The ONE door for the text of a line in the programs with no window.
    /// Text in the model is UTF-8 and a line from outside need not be: a pipe
    /// from a program that writes the console's code page, a script line
    /// (read_script decodes the file, so only a line given some other way gets
    /// here). ON WINDOWS a line that is not UTF-8 is read as Windows-1252
    /// (core's utf8_or_windows1252) and the session says so on `err`, once.
    /// Elsewhere such a line is refused with a sentence that says it is not
    /// valid UTF-8. A panic that still escapes a verb is reported as
    /// `error: Internal: ...` and the line fails; it does not end the
    /// session, which in katana_cli is the person's unsaved drawing.
    pub fn run(&mut self, line: impl AsRef<[u8]>, console: &mut dyn Console) -> bool {
        let line = line.as_ref();
        // Nothing up a verb's call chain catches: the standard library panics
        // for a path it cannot convert, and a panic out of main ends the
        // process - in katana_cli, with the drawing unsaved. The line fails,
        // says why, and the session goes on.
        match catch_unwind(AssertUnwindSafe(|| self.run_bytes(line, console))) {
            Ok(ran) => ran,
            Err(payload) => {
                console.err(&format!(
                    "error: Internal: {}\n",
                    panic_text(&*payload, "a verb stopped without a message")
                ));
                false
            }
        }
    }

    fn run_bytes(&mut self, line: &[u8], console: &mut dyn Console) -> bool {
        match std::str::from_utf8(line) {
            Ok(text) => self.run_line(text, console),
            Err(_) => self.run_not_utf8(line, console),
        }
    }

    // Only here: the bytes a Windows program is handed are the ANSI code
    // page's, and Windows-1252 is the reading of them that decode_text also
    // falls back on.
    #[cfg(windows)]
    fn run_not_utf8(&mut self, line: &[u8], console: &mut dyn Console) -> bool {
        if !self.said_windows_1252 {
            self.said_windows_1252 = true;
            console.err(
                "note: a line that is not UTF-8 text was read as Windows-1252 (the ANSI code \
                 page); Katana's text is UTF-8, so save a script as UTF-8 to say it plainly\n",
            );
        }
        let text = katana_core::text_encoding::utf8_or_windows1252(line);
        self.run_line(&text, console)
    }

    #[cfg(not(windows))]
    fn run_not_utf8(&mut self, _line: &[u8], console: &mut dyn Console) -> bool {
        console.err(&format!("error: {}\n", not_utf8_refusal().describe()));
        false
    }

    // Returns false when the command failed.
    fn run_line(&mut self, line: &str, console: &mut dyn Console) -> bool {
        // A note, whatever blanks come before its '#' - the rule the window's
        // scripts and command line keep (src/katana_qt/script_runner.hpp), so
        // one .kcs runs alike in katana_cli, katana_mcp and File > Run Script.
        // An indented "# note" was once a command here and stopped the script
        // the window ran through.
        let body = trimmed(line.as_bytes());
        if body.first() == Some(&b'#') {
            return true;
        }
        // HELP (or ?) alone is the whole session's help - IFC, SURVEY and the
        // geo executor's families too, which the interpreter cannot know of -
        // so katana_cli -c HELP and katana_run_commands say what katana_help
        // and --help say. With a word (HELP SHEETS, HELP UTILITY, HELP
        // CUSTOMISE) it stays the interpreter's.
        if !body.contains(&b' ') && !body.contains(&b'\t') {
            let verb = upper_verb(line);
            if verb == "HELP" || verb == "?" {
                console.out(&format!("{}\n", Self::help_text()));
                return true;
            }
        }
        // HELP UPDATE is the session's as well: the update verbs are taken
        // here, so the interpreter, which answers HELP with a word, has no
        // such word.
        let words = upper_words(line);
        if words == "HELP UPDATE" || words == "? UPDATE" {
            console.out(update_help_text());
            return true;
        }
        if let Some(why) = window_only_verb(&upper_verb(line)) {
            console.err(&format!("error: Unsupported: {} {why}\n", upper_verb(line)));
            return false;
        }
        // UPDATE CHECK, STATUS, DOWNLOAD, APPLY and the rest: the program's
        // own update, through the engine and the network, which the
        // interpreter (cad) sees neither of (update_verbs.rs).
        if is_update_line(line) {
            return run_update(&mut self.updates, line, console);
        }
        // SURVEY READ and SURVEY IMPORT: a survey field file, through
        // surveyio, which the interpreter (cad) may not see (survey_verbs).
        if survey_verbs::is_survey_line(line) {
            return match survey_verbs::run_survey_line(&mut self.document, line) {
                Ok(reply) => {
                    console.out(&format!("{reply}\n"));
                    true
                }
                Err(error) => {
                    console.err(&format!("error: {}\n", error.describe()));
                    false
                }
            };
        }
        // A .ifc is read, written and described natively (ifc_verbs). The
        // rest of the line goes as it is, from the first blank after the
        // first word: the IFC grammar reads its own quotes, and a line may
        // quote more than one path.
        let ifc_verb = upper_verb(line);
        if matches!(ifc_verb.as_str(), "IMPORT" | "EXPORT" | "INFO" | "IFC") {
            let bytes = line.as_bytes();
            let rest = bytes
                .iter()
                .position(|&byte| byte != b' ' && byte != b'\t')
                .and_then(|first| {
                    bytes[first..]
                        .iter()
                        .position(|&byte| byte == b' ' || byte == b'\t')
                        .map(|blank| first + blank)
                })
                .map_or("", |space| &line[space..]);
            if let Some(handled) =
                ifc_verbs::run_ifc_verb(&mut self.document, &ifc_verb, rest, console)
            {
                return handled;
            }
        }
        // IMPORT, EXPORT, INFO <file>, REFS, COPC, GDAL and the families after
        // them run through the one executor the window runs them through,
        // inline: a session has nobody to wait for a job.
        if geo::handles(line) {
            let mut context = geo::Context::new(
                &mut self.document,
                &self.interpreter,
                &mut self.reference,
                &self.geo_scratch,
                &self.geo_front_end,
            );
            return match geo::run_now(&mut context, line, None, None) {
                Ok(reply) => {
                    if !reply.is_empty() {
                        console.out(&format!("{reply}\n"));
                    }
                    true
                }
                Err(error) => {
                    console.err(&format!("error: {}\n", error.describe()));
                    false
                }
            };
        }

        // CUSTOMISE is the interpreter's, like any other verb below: the
        // project's record of what it was drawn with - names, never
        // definitions, which are session data - is written by the save itself
        // (Document::save), so nothing is done for that here.
        let verb = upper_verb(line);
        // The reference layers the drawing is worked on top of are recorded
        // here: their sources and display, read again when it opens
        // (docs/interop.md, "Reference layers"). Only for a SAVE that has
        // somewhere to go, as in the window: writing them marks the drawing
        // modified whether or not the save then happens.
        if verb == "SAVE"
            && katana_cad::customisation_record::typed_save_has_destination(
                line,
                self.document.has_project(),
            )
        {
            geo::references::record_references(&mut geo::Context::new(
                &mut self.document,
                &self.interpreter,
                &mut self.reference,
                &self.geo_scratch,
                &self.geo_front_end,
            ));
        }
        let reply = self.interpreter.run(&mut self.document, line);
        let reply = match reply {
            Ok(reply) => reply,
            Err(mut refusal) => {
                // A refusal's first line is what was refused and why. Lines
                // after it are the report behind it - UTILITY CHECK refuses a
                // schedule with errors and carries the whole check - and go
                // where a report goes, so a script that keeps stdout has the
                // check exactly when it failed.
                let report = match refusal.message.find('\n') {
                    Some(end) => {
                        let report = String::from(&refusal.message[end + 1..]);
                        refusal.message.truncate(end);
                        report
                    }
                    None => String::new(),
                };
                console.err(&format!("error: {}\n", refusal.describe()));
                if !report.is_empty() {
                    console.out(&format!("{report}\n"));
                }
                return false;
            }
        };
        if !reply.is_empty() {
            console.out(&format!("{reply}\n"));
        }
        if verb == "OPEN" && CommandInterpreter::replaces_document(line) {
            // A warning, not a refusal: the drawing opens and draws, but what a
            // missing customisation defined draws as a plain line. The open
            // worked out which (Document::open); the words are this front
            // end's. A project records its customisations by NAME - the name a
            // file declares, not the file's - so they are not called files
            // here.
            let missing = &self.document.customisation_state().missing_at_open;
            if !missing.is_empty() {
                let mut warning = format!(
                    "warning: this project was drawn with customisations that are not loaded: {}",
                    quoted_names(missing)
                );
                // A name that is one of this program's built-ins is no file to
                // load: it is made the session by name.
                let advice = built_in_advice_for(missing, built_ins());
                if !advice.is_empty() {
                    warning.push_str("; ");
                    warning.push_str(&advice);
                }
                warning.push('\n');
                console.err(&warning);
            }
        }
        // Only a line that REPLACES the drawing - NEW, OPEN of a project -
        // goes here. CommandInterpreter::replaces_document tells it from OPEN
        // #12 and OPEN SELECTION, the polyline edits that share the verb:
        // those keep the drawing and so its reference layers, as in the
        // window, which asks the same.
        if CommandInterpreter::replaces_document(line) {
            // The reference layers go with their drawing, as the window's do
            // (audit QT-17); an OPEN then reads again the layers the project
            // records. A layer whose file is gone is warned of by the restore,
            // and the drawing is open all the same. (The surfaces are the
            // drawing's: NEW cleared them, OPEN read the project's.)
            self.reference.clear();
            if CommandInterpreter::verb_of(line) == "OPEN" {
                let mut context = geo::Context::new(
                    &mut self.document,
                    &self.interpreter,
                    &mut self.reference,
                    &self.geo_scratch,
                    &self.geo_front_end,
                );
                if geo::references::records_references(&context) {
                    match geo::run_now(&mut context, "REFS RESTORE", None, None) {
                        Ok(restored) => console.out(&format!("{restored}\n")),
                        Err(error) => console.err(&format!(
                            "warning: the reference layers were not read again: {}\n",
                            error.describe()
                        )),
                    }
                }
            }
        }
        true
    }

    /// The lines of the script file at `path` (UTF-8 text; the path is made of
    /// it with core's path_from_utf8, so a folder with an accented letter in
    /// it is found). The file is decoded as every text format is (core's
    /// decode_text): a byte order mark, UTF-16 and the Windows-1252 an ANSI
    /// editor writes read as the text they are, and a guess is said on
    /// `console`'s err. NotFound when the file cannot be opened, ParseFailure
    /// for UTF-16 that is not well formed. The path is bytes, as katana_cli's
    /// arguments are.
    pub fn read_script(path: impl AsRef<[u8]>, console: &mut dyn Console) -> Result<Vec<String>> {
        let path = path.as_ref();
        let named = String::from_utf8_lossy(path);
        let bytes = read_file_bytes(&path_from_utf8(path))?;
        let decoded =
            decode_text(bytes).map_err(|error| make_error(error.code, error.message, &*named))?;
        // The fallback is said, never silent (core's text_encoding): a degree
        // sign read under the wrong code page is a wrong character, not an
        // error.
        if decoded.guessed {
            console.err(&format!(
                "note: the script {named} is not UTF-8 text; read as {}\n",
                decoded.encoding
            ));
        }
        Ok(split_lines(decoded.text.as_bytes())
            .into_iter()
            .map(|one| String::from_utf8_lossy(one).into_owned())
            .collect())
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    /// QUIT and EXIT, in any case: a front end's cue to stop, never run.
    pub fn is_quit(line: impl AsRef<[u8]>) -> bool {
        is_quit_line(line.as_ref())
    }

    /// The interpreter's HELP and the verbs a session adds to it.
    pub fn help_text() -> String {
        // The interpreter's help, CUSTOMISE and CODE among it, then a blank
        // line and the verbs this session adds above katana_cad.
        let mut text = CommandInterpreter::help_text();
        text.push('\n');
        text.push_str(ifc_verbs::ifc_help_text());
        text.push_str(survey_verbs::survey_help_text());
        // IMPORT, EXPORT, INFO <file>, REFS and COPC are the executor's, so
        // its table says them, as it does in the window's HELP.
        text.push_str(&geo::help_text());
        // The program's own update, which the window's HELP lists too.
        text.push_str(update_help_text());
        text.push_str(concat!(
            "Window    PLOT, PLOTSHEETS, SNAPSHOT, ONLINE, SCRIPT, GRID and EXAGGERATION are the\n",
            "          desktop window's verbs. ZOOM and VIEWS other than SCALE also need\n",
            "          the window. VIEWS SCALE id AUTO|1:N edits saved plan-view style scale.\n",
            "          katana --plot, --plot-sheets and --command run desktop verbs\n",
            "          headless; a script is katana_cli's argument, or katana_mcp's\n",
            "          katana_run_script\n",
        ));
        text
    }

    /// Which front end runs this session's UPDATE lines (update_verbs.rs):
    /// katana_cli's unless told, which runs them all. katana_mcp's server says
    /// Mcp, and every UPDATE line is then refused naming its tool,
    /// katana_check_update - as the window's own verbs are refused, by name
    /// and saying where they run - so that the tool that runs lines reaches
    /// no network and an agent never downloads or applies. A session of no
    /// program refuses them whatever it is told: there is no program for them
    /// to act on.
    pub fn set_update_surface(&mut self, surface: UpdateSurface) {
        self.updates.surface = surface;
    }

    /// Where this session's UPDATE lines get their context: make_update_context
    /// (update_verbs.rs) of the running program, unless a source is handed
    /// over here. It is asked for every line with the session's surface, as
    /// make_update_context is, so a line sees what the line before it
    /// changed. To what it returns the session adds what is its own to say:
    /// the process the helper waits for (`pid`, this one) and the printing of
    /// a download's progress; `relaunch` it leaves as it comes, which from
    /// make_update_context is empty - katana_cli starts nothing again. A test
    /// hands over a scratch install and a fake feed, and so runs what
    /// katana_cli runs. None puts the running program's back.
    pub fn set_update_context_source(&mut self, source: Option<Box<dyn UpdateContextSource>>) {
        self.updates.source = source;
    }

    /// True for an UPDATE APPLY line, CONFIRM or not: the line a front end
    /// must run LAST, since a session that has run it is over. The verbs' own
    /// reading of the line (update_verbs.rs, is_update_apply_line): a line
    /// that the verbs would read as UPDATE APPLY - its words parted by a
    /// carriage return, say, as a -c argument can hold one - is the line that
    /// must be last.
    pub fn is_update_apply(line: impl AsRef<[u8]>) -> bool {
        // A byte that is not UTF-8 is never a blank nor a letter of UPDATE or
        // APPLY, so the replacement changes no answer.
        is_update_apply_line(&String::from_utf8_lossy(line.as_ref()))
    }

    /// What an UPDATE APPLY CONFIRM line that succeeded left to be done, once:
    /// the staged release is verified and its helper has proved it starts,
    /// and NOTHING in the install has changed. The front end ends the session
    /// first - this value dropped, its drawing closed - and only then calls
    /// katana_update's launch_apply with it; the helper waits for the process
    /// to exit. None when no such line has succeeded since the last call. A
    /// front end that does not take it, or does not launch, leaves the
    /// release downloaded and the install as it was. The helper's copy that
    /// the line made is then nobody's: a front end that took the request and
    /// does not launch it calls abandon_apply (update_verbs.rs), and a
    /// request nobody took is abandoned by the session itself, when a later
    /// one takes its place or when the session ends.
    pub fn take_pending_apply(&mut self) -> Option<ApplyRequest> {
        self.updates.pending_apply.take()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A prepared apply that nobody took before the session ended will not
        // be launched: the helper copy it had made is listed for the sweep
        // (update_verbs.rs, abandon_apply), and the release stays downloaded.
        // Nothing is said if that fails, and nothing may leave a drop: a
        // panic in it would end the program.
        if let Some(pending) = &self.updates.pending_apply {
            let _ = catch_unwind(AssertUnwindSafe(|| abandon_apply(pending)));
        }
    }
}

/// What katana_cli's and katana_mcp's main() do before anything else:
/// katana_update's recover for the install the running program is in, so
/// that an update a crash or a power cut interrupted is put back before the
/// program reads a file of a half-swapped install. With nothing interrupted -
/// every start but that one - it is the listing of one folder, and nothing at
/// all for a program that is in no install (a build tree has no .update
/// folder). A headless program writes nothing per user here: the recovery's
/// only record is the install's own log. A recovery that was needed and did
/// not complete is said on `console`'s err with the command that puts the
/// install right by hand, and the program goes on.
pub fn recover_interrupted_update(console: &mut dyn Console) {
    let Ok(root) = install_root_of_running_program() else {
        return; // in no install: there is nothing an update could have left half done
    };
    if let Err(error) = recover(&root) {
        // Said, and the program goes on: the message names the command that
        // puts the install right by hand (recover.rs).
        console.err(&format!("warning: {}\n", error.describe()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The first word, in any case, after any blanks: QUIT and EXIT alone.
    #[test]
    fn quit_and_exit_are_read_as_the_first_word_in_any_case() {
        for line in ["QUIT", "exit", "  Quit now", "\tEXIT"] {
            assert!(Session::is_quit(line), "{line}");
        }
        for line in ["", "QUITE", "LINE 0,0 1,1", "# QUIT"] {
            assert!(!Session::is_quit(line), "{line}");
        }
    }

    // One blank between the capitalised words, whatever was typed.
    #[test]
    fn the_words_of_a_line_are_compared_as_one_text() {
        assert_eq!(upper_words("  help \t update "), "HELP UPDATE");
        assert_eq!(upper_verb("\t views scale"), "VIEWS");
        assert_eq!(
            quoted_names(&[String::from("A"), String::from("B")]),
            "\"A\", \"B\""
        );
    }
}
